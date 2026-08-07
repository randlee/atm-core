//! Composition-owned receiver-hook implementations for the replacement daemon.
//!
//! This module is deliberately outside `atm-http-runtime`: the runtime sees
//! only the sealed selector/emitter boundary. Graft remains an independently
//! running receiver reached through its already-published endpoint; no daemon
//! crate imports `atm-graft`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use atm_core::LocalServiceRuntime;
use atm_core::RequestDeadline;
use atm_core::boundary::{
    self, AsyncMessageReceivedHookEmitter, BuiltInPostSendDispatch, MessageReceivedHookSelector,
    PostSendBuiltInTarget, PostSendEmissionPath,
};
use atm_core::error::{AtmError, AtmErrorCode};

const TMUX_DOUBLE_ENTER_DELAY: Duration = Duration::from_millis(275);

/// Environment variable accepted only by the isolated capacity harness to
/// select whether the post-commit received hook is measured.
pub const RECEIVED_HOOK_MODE_ENV: &str = "ATM_HTTP_RECEIVED_HOOK_MODE";

/// Explicit acknowledgement required before the benchmark can suppress a
/// real receiver notification.  This prevents an operator from accidentally
/// disabling notification on a normal daemon startup.
pub const BENCHMARK_MODE_ENV: &str = "ATM_HTTP_BENCHMARK_MODE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceivedHookMode {
    Active,
    DisabledForBenchmark,
}

impl ReceivedHookMode {
    /// Parses the replacement daemon's hook configuration before listeners
    /// are bound. The default is always the normal active hook behavior.
    pub fn from_environment() -> Result<Self, AtmError> {
        let mode = std::env::var(RECEIVED_HOOK_MODE_ENV).unwrap_or_else(|_| "active".to_owned());
        Self::parse(
            &mode,
            std::env::var_os(BENCHMARK_MODE_ENV).is_some_and(|value| value == "1"),
        )
    }

    fn parse(mode: &str, benchmark_mode: bool) -> Result<Self, AtmError> {
        match mode {
            "active" => Ok(Self::Active),
            "disabled" if benchmark_mode => Ok(Self::DisabledForBenchmark),
            "disabled" => Err(AtmError::config(
                "ATM_HTTP_RECEIVED_HOOK_MODE=disabled requires ATM_HTTP_BENCHMARK_MODE=1",
            )),
            _ => Err(AtmError::config(
                "ATM_HTTP_RECEIVED_HOOK_MODE must be `active` or benchmark-authorized `disabled`",
            )),
        }
    }
}

/// Builds the injected selector selected before replacement runtime binding.
///
/// The runtime owns no notification implementation.  Benchmark-only disabled
/// mode returns an empty selector, preserving the normal durable-write route
/// while intentionally measuring its hook-free variant.
pub fn received_hook_selector_from_environment(
    service_runtime: LocalServiceRuntime,
) -> Result<Arc<dyn MessageReceivedHookSelector>, AtmError> {
    match ReceivedHookMode::from_environment()? {
        ReceivedHookMode::Active => Ok(Arc::new(ReplacementReceivedHookSelector::new(
            service_runtime,
        ))),
        ReceivedHookMode::DisabledForBenchmark => Ok(Arc::new(DisabledReceivedHookSelector)),
    }
}

/// Selects the receiver implementation from the post-persistence dispatch
/// target already planned by core. It owns no application routing or storage.
#[derive(Clone)]
pub struct ReplacementReceivedHookSelector {
    tmux: TokioTmuxReceivedHook,
    graft: PublishedGraftReceivedHook,
}

impl ReplacementReceivedHookSelector {
    #[must_use]
    pub fn new(service_runtime: LocalServiceRuntime) -> Self {
        Self {
            tmux: TokioTmuxReceivedHook,
            graft: PublishedGraftReceivedHook { service_runtime },
        }
    }
}

impl boundary::sealed::Sealed for ReplacementReceivedHookSelector {}

impl MessageReceivedHookSelector for ReplacementReceivedHookSelector {
    fn select_emitter(
        &self,
        dispatch: &BuiltInPostSendDispatch,
    ) -> Option<&dyn AsyncMessageReceivedHookEmitter> {
        match dispatch.target {
            PostSendBuiltInTarget::LocalTmux(_) => Some(&self.tmux),
            PostSendBuiltInTarget::Graft(_) => Some(&self.graft),
        }
    }
}

/// Benchmark-only selector which leaves post-commit hook dispatch empty.
/// It is private to bootstrap composition and cannot be selected by a normal
/// runtime startup without the explicit benchmark acknowledgement.
#[derive(Clone, Copy)]
struct DisabledReceivedHookSelector;

impl boundary::sealed::Sealed for DisabledReceivedHookSelector {}

impl MessageReceivedHookSelector for DisabledReceivedHookSelector {
    fn select_emitter(
        &self,
        _dispatch: &BuiltInPostSendDispatch,
    ) -> Option<&dyn AsyncMessageReceivedHookEmitter> {
        None
    }
}

/// Tokio-native tmux receiver emitter. It never polls a child or blocks a
/// runtime worker: each command and the retained inter-key delay are awaited.
#[derive(Clone, Copy)]
struct TokioTmuxReceivedHook;

impl boundary::sealed::Sealed for TokioTmuxReceivedHook {}

impl AsyncMessageReceivedHookEmitter for TokioTmuxReceivedHook {
    fn emit_received_message(
        &self,
        dispatch: BuiltInPostSendDispatch,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<PostSendEmissionPath, AtmError>> + Send + '_>> {
        Box::pin(async move {
            let PostSendBuiltInTarget::LocalTmux(target) = dispatch.target else {
                return Err(AtmError::validation(
                    "tmux receiver hook received a non-tmux dispatch",
                ));
            };
            run_tmux(
                [
                    "send-keys",
                    "-t",
                    target.pane_id.as_str(),
                    "-l",
                    &target.rendered_nudge,
                ],
                deadline,
            )
            .await?;
            run_tmux(
                ["send-keys", "-t", target.pane_id.as_str(), "Enter"],
                deadline,
            )
            .await?;
            let delay = deadline
                .remaining()
                .ok_or_else(|| hook_deadline_error("before tmux's second Enter"))?
                .min(TMUX_DOUBLE_ENTER_DELAY);
            tokio::time::sleep(delay).await;
            run_tmux(
                ["send-keys", "-t", target.pane_id.as_str(), "Enter"],
                deadline,
            )
            .await?;
            Ok(PostSendEmissionPath::LocalTmux)
        })
    }
}

async fn run_tmux<const N: usize>(
    arguments: [&str; N],
    deadline: RequestDeadline,
) -> Result<(), AtmError> {
    let remaining = deadline
        .remaining()
        .ok_or_else(|| hook_deadline_error("before tmux command start"))?;
    let output = tokio::time::timeout(
        remaining,
        tokio::process::Command::new("tmux")
            .args(arguments)
            .output(),
    )
    .await
    .map_err(|_| hook_deadline_error("while executing tmux"))?
    .map_err(|source| {
        AtmError::new(
            AtmErrorCode::PostSendTmuxSendFailed,
            "failed to start tmux received-message hook command",
        )
        .with_cause(source)
    })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(AtmError::new(
            AtmErrorCode::PostSendTmuxSendFailed,
            "tmux received-message hook command failed",
        ))
    }
}

fn hook_deadline_error(stage: &'static str) -> AtmError {
    AtmError::new(
        AtmErrorCode::PostSendTmuxSendFailed,
        format!("received-message hook deadline expired {stage}"),
    )
}

/// Graft remains receiver-owned. The existing endpoint operation has bounded
/// socket timeouts derived from the inherited absolute deadline, so the only
/// blocking seam is awaited rather than detached.
#[derive(Clone)]
struct PublishedGraftReceivedHook {
    service_runtime: LocalServiceRuntime,
}

impl boundary::sealed::Sealed for PublishedGraftReceivedHook {}

impl AsyncMessageReceivedHookEmitter for PublishedGraftReceivedHook {
    fn emit_received_message(
        &self,
        dispatch: BuiltInPostSendDispatch,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<PostSendEmissionPath, AtmError>> + Send + '_>> {
        let service_runtime = self.service_runtime.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                atm_core::graft::deliver_published_receiver_hook_from_local_runtime(
                    &service_runtime,
                    &dispatch,
                    deadline,
                )
            })
            .await
            .map_err(|source| {
                AtmError::new(
                    AtmErrorCode::InternalError,
                    "published Graft receiver hook task ended unexpectedly",
                )
                .with_cause(source)
            })?
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use atm_core::RequestDeadline;
    use atm_core::boundary::{
        AsyncMessageReceivedHookEmitter, BuiltInPostSendDispatch, LocalTmuxNudgeTarget,
        MessageReceivedHookSelector, PostSendBuiltInTarget, PostSendHookEvent,
    };
    use atm_core::types::{AgentName, PaneId, TeamName};

    use super::{DisabledReceivedHookSelector, ReceivedHookMode, TokioTmuxReceivedHook};

    fn tmux_dispatch() -> BuiltInPostSendDispatch {
        BuiltInPostSendDispatch {
            event: PostSendHookEvent {
                sender: "sender".parse::<AgentName>().expect("agent"),
                sender_chat_id: None,
                sender_team: "team".parse::<TeamName>().expect("team"),
                recipient: "receiver".parse::<AgentName>().expect("agent"),
                recipient_team: "team".parse::<TeamName>().expect("team"),
                message_id: "01KZ0000000000000000000000".parse().expect("message"),
                description: "test".to_owned(),
                requires_ack: false,
                is_ack: false,
                task_id: None,
                recipient_pane_id: Some(PaneId::from_cli("%1").expect("pane")),
            },
            target: PostSendBuiltInTarget::LocalTmux(LocalTmuxNudgeTarget {
                pane_id: PaneId::from_cli("%1").expect("pane"),
                rendered_nudge: "test".to_owned(),
            }),
        }
    }

    #[tokio::test]
    async fn expired_hook_budget_does_not_start_a_tmux_process() {
        let error = TokioTmuxReceivedHook
            .emit_received_message(tmux_dispatch(), RequestDeadline::after(Duration::ZERO))
            .await
            .expect_err("expired deadline must fail before command spawn");

        assert!(error.message().contains("deadline expired"));
    }

    #[test]
    fn hook_mode_defaults_or_requires_explicit_benchmark_authority() {
        assert_eq!(
            ReceivedHookMode::parse("active", false).expect("active mode"),
            ReceivedHookMode::Active
        );
        assert_eq!(
            ReceivedHookMode::parse("disabled", true).expect("authorized disabled mode"),
            ReceivedHookMode::DisabledForBenchmark
        );
        assert!(ReceivedHookMode::parse("disabled", false).is_err());
        assert!(ReceivedHookMode::parse("unexpected", true).is_err());
    }

    #[test]
    fn disabled_selector_never_selects_an_emitter() {
        let selector = DisabledReceivedHookSelector;
        assert!(selector.select_emitter(&tmux_dispatch()).is_none());
    }
}
