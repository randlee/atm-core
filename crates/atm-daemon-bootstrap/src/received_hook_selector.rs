//! Composition-owned receiver-hook implementations for the replacement daemon.
//!
//! This module is deliberately outside `atm-http-runtime`: the runtime sees
//! only the sealed selector/emitter boundary. Graft remains an independently
//! running receiver reached through its already-published endpoint; no daemon
//! crate imports `atm-graft`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use atm_core::LocalServiceRuntime;
use atm_core::RequestDeadline;
use atm_core::boundary::{
    self, AsyncMessageReceivedHookEmitter, BuiltInPostSendDispatch, MessageReceivedHookSelector,
    NudgeKind, PostSendBuiltInTarget, PostSendEmissionPath, TMUX_DOUBLE_ENTER_DELAY,
    TMUX_NUDGE_CONFIRM_KEY,
};
use atm_core::error::{AtmError, AtmErrorCode};

/// Builds the selector injected into every production replacement daemon.
///
/// Production startup has no hook-disable configuration surface: every
/// durable new write gets the selected receiver hook. The benchmark harness
/// is compiled as a separate feature-gated binary below.
pub fn active_received_hook_selector(
    service_runtime: LocalServiceRuntime,
) -> Arc<dyn MessageReceivedHookSelector> {
    Arc::new(ReplacementReceivedHookSelector::new(service_runtime))
}

/// Mode accepted exclusively by the separately compiled benchmark binary.
#[cfg(feature = "benchmark-harness")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BenchmarkHookMode {
    Active,
    Disabled,
}

#[cfg(feature = "benchmark-harness")]
impl BenchmarkHookMode {
    /// Parses only the benchmark binary's explicit `--hook-mode` argument.
    pub fn parse(value: &str) -> Result<Self, AtmError> {
        match value {
            "active" => Ok(Self::Active),
            "disabled" => Ok(Self::Disabled),
            _ => Err(AtmError::config(
                "benchmark hook mode must be `active` or `disabled`",
            )),
        }
    }
}

/// Builds a selector solely for the feature-gated benchmark executable.
#[cfg(feature = "benchmark-harness")]
pub fn benchmark_received_hook_selector(
    service_runtime: LocalServiceRuntime,
    mode: BenchmarkHookMode,
) -> Arc<dyn MessageReceivedHookSelector> {
    match mode {
        BenchmarkHookMode::Active => active_received_hook_selector(service_runtime),
        BenchmarkHookMode::Disabled => Arc::new(DisabledReceivedHookSelector),
    }
}

/// Selects the receiver implementation from the post-persistence dispatch
/// target already planned by core. It owns no application routing or storage.
#[derive(Clone)]
struct ReplacementReceivedHookSelector {
    tmux: TokioTmuxReceivedHook,
    graft: PublishedGraftReceivedHook,
}

impl ReplacementReceivedHookSelector {
    #[must_use]
    fn new(service_runtime: LocalServiceRuntime) -> Self {
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
        match (dispatch.kind, &dispatch.target) {
            (NudgeKind::Steer, PostSendBuiltInTarget::LocalSteer(_)) => Some(&self.tmux),
            (NudgeKind::Steer, PostSendBuiltInTarget::Graft(_)) => Some(&self.graft),
            (NudgeKind::Queue, _) => None, // AQ2/AQ3 own queue-kind emitters
        }
    }
}

/// Benchmark-only selector which leaves post-commit hook dispatch empty.
/// It is compiled only into the dedicated benchmark executable, never the
/// normal replacement daemon binary.
#[cfg(feature = "benchmark-harness")]
#[derive(Clone, Copy)]
struct DisabledReceivedHookSelector;

#[cfg(feature = "benchmark-harness")]
impl boundary::sealed::Sealed for DisabledReceivedHookSelector {}

#[cfg(feature = "benchmark-harness")]
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
            let PostSendBuiltInTarget::LocalSteer(target) = dispatch.target else {
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
                [
                    "send-keys",
                    "-t",
                    target.pane_id.as_str(),
                    TMUX_NUDGE_CONFIRM_KEY,
                ],
                deadline,
            )
            .await?;
            let delay = deadline
                .remaining()
                .ok_or_else(|| hook_deadline_error("before tmux's second Enter"))?
                .min(TMUX_DOUBLE_ENTER_DELAY);
            tokio::time::sleep(delay).await;
            run_tmux(
                [
                    "send-keys",
                    "-t",
                    target.pane_id.as_str(),
                    TMUX_NUDGE_CONFIRM_KEY,
                ],
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
    use atm_core::boundary::MessageReceivedHookSelector;
    use atm_core::boundary::{
        AsyncMessageReceivedHookEmitter, BuiltInPostSendDispatch, LocalTmuxNudgeTarget, NudgeKind,
        PostSendBuiltInTarget, PostSendHookEvent,
    };
    use atm_core::types::{AgentName, PaneId, TeamName};

    #[cfg(feature = "benchmark-harness")]
    use super::{BenchmarkHookMode, DisabledReceivedHookSelector};
    use super::{ReplacementReceivedHookSelector, TokioTmuxReceivedHook};

    fn tmux_dispatch() -> BuiltInPostSendDispatch {
        BuiltInPostSendDispatch {
            event: PostSendHookEvent {
                sender: "sender".parse::<AgentName>().expect("agent"),
                sender_chat_id: None,
                sender_team: "team".parse::<TeamName>().expect("team"),
                sender_host: None,
                recipient: "receiver".parse::<AgentName>().expect("agent"),
                recipient_team: "team".parse::<TeamName>().expect("team"),
                message_id: "01KZ0000000000000000000000".parse().expect("message"),
                description: "test".to_owned(),
                requires_ack: false,
                is_ack: false,
                task_id: None,
                recipient_pane_id: Some(PaneId::from_cli("%1").expect("pane")),
            },
            target: PostSendBuiltInTarget::LocalSteer(LocalTmuxNudgeTarget {
                pane_id: PaneId::from_cli("%1").expect("pane"),
                rendered_nudge: "test".to_owned(),
            }),
            kind: NudgeKind::Steer,
        }
    }

    fn queue_dispatch() -> BuiltInPostSendDispatch {
        let mut dispatch = tmux_dispatch();
        dispatch.kind = NudgeKind::Queue;
        dispatch
    }

    #[test]
    fn selector_routes_tmux_only_for_steer() {
        let temporary_root = tempfile::tempdir().expect("temporary selector runtime root");
        let assembly =
            atm_runtime_test_support::open_isolated_sqlite_boundary(temporary_root.path())
                .expect("assemble isolated selector runtime");
        let selector = ReplacementReceivedHookSelector::new(assembly.service_runtime);

        assert!(selector.select_emitter(&tmux_dispatch()).is_some());
        assert!(selector.select_emitter(&queue_dispatch()).is_none());
    }

    #[tokio::test]
    async fn expired_hook_budget_does_not_start_a_tmux_process() {
        let error = TokioTmuxReceivedHook
            .emit_received_message(tmux_dispatch(), RequestDeadline::after(Duration::ZERO))
            .await
            .expect_err("expired deadline must fail before command spawn");

        assert!(error.message().contains("deadline expired"));
    }

    #[cfg(feature = "benchmark-harness")]
    #[test]
    fn benchmark_mode_is_explicit_and_disabled_selector_is_empty() {
        assert_eq!(
            BenchmarkHookMode::parse("active").expect("active mode"),
            BenchmarkHookMode::Active
        );
        assert_eq!(
            BenchmarkHookMode::parse("disabled").expect("disabled mode"),
            BenchmarkHookMode::Disabled
        );
        assert!(BenchmarkHookMode::parse("unexpected").is_err());
        assert!(
            DisabledReceivedHookSelector
                .select_emitter(&tmux_dispatch())
                .is_none()
        );
    }
}
