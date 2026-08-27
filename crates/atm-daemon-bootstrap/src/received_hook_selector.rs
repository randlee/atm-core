//! Composition-owned receiver-hook implementations for the replacement daemon.
//!
//! This module is deliberately outside `atm-http-runtime`: the runtime sees
//! only the sealed selector/emitter boundary. Graft remains an independently
//! running receiver reached through its already-published endpoint; no daemon
//! crate imports `atm-graft`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

#[cfg(feature = "benchmark-harness")]
use atm_core::HerdrSession;
use atm_core::LocalServiceRuntime;
use atm_core::RequestDeadline;
use atm_core::boundary::{
    self, AsyncMessageReceivedHookEmitter, BuiltInPostSendDispatch, MessageReceivedHookSelector,
    NudgeKind, PostSendBuiltInTarget, PostSendEmissionPath, TMUX_DOUBLE_ENTER_DELAY,
    TMUX_NUDGE_CONFIRM_KEY,
};
use atm_core::error::{AtmError, AtmErrorCode};
#[cfg(feature = "benchmark-harness")]
use atm_herdr::{AgentSnapshot, HerdrAgentStatus, HerdrError};
use atm_herdr::{HerdrProcessAdapter, HerdrPromptOutcome};
use atm_http_runtime::{
    BareCliFifo, BareCliQueueFullDrops, RuntimeHealth, append_bare_cli_message,
};

/// Builds the selector injected into every production replacement daemon.
///
/// Production startup has no hook-disable configuration surface: every
/// durable new write gets the selected receiver hook. The benchmark harness
/// is compiled as a separate feature-gated binary below.
pub fn active_received_hook_selector(
    service_runtime: LocalServiceRuntime,
    herdr_process: Arc<dyn HerdrProcessAdapter>,
) -> Arc<dyn MessageReceivedHookSelector> {
    active_received_hook_selector_with_health(
        service_runtime,
        herdr_process,
        RuntimeHealth::default(),
    )
}

/// Builds the production selector with the daemon's shared health projection.
pub fn active_received_hook_selector_with_health(
    service_runtime: LocalServiceRuntime,
    herdr_process: Arc<dyn HerdrProcessAdapter>,
    runtime_health: RuntimeHealth,
) -> Arc<dyn MessageReceivedHookSelector> {
    Arc::new(ReplacementReceivedHookSelector::with_herdr_process(
        service_runtime,
        herdr_process,
        runtime_health,
    ))
}

/// Builds the production selector with the composition-root-owned bare-CLI
/// FIFO and overflow counter.
pub fn active_received_hook_selector_with_health_and_fifo(
    service_runtime: LocalServiceRuntime,
    herdr_process: Arc<dyn HerdrProcessAdapter>,
    runtime_health: RuntimeHealth,
    bare_cli_fifo: BareCliFifo,
    bare_cli_queue_full_drops: BareCliQueueFullDrops,
) -> Arc<dyn MessageReceivedHookSelector> {
    Arc::new(
        ReplacementReceivedHookSelector::with_herdr_process_and_fifo(
            service_runtime,
            herdr_process,
            runtime_health,
            bare_cli_fifo,
            bare_cli_queue_full_drops,
        ),
    )
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
    herdr_process: Arc<dyn HerdrProcessAdapter>,
) -> Arc<dyn MessageReceivedHookSelector> {
    benchmark_received_hook_selector_with_health(
        service_runtime,
        mode,
        herdr_process,
        RuntimeHealth::default(),
    )
}

#[cfg(feature = "benchmark-harness")]
pub fn benchmark_received_hook_selector_with_health(
    service_runtime: LocalServiceRuntime,
    mode: BenchmarkHookMode,
    herdr_process: Arc<dyn HerdrProcessAdapter>,
    runtime_health: RuntimeHealth,
) -> Arc<dyn MessageReceivedHookSelector> {
    match mode {
        BenchmarkHookMode::Active => active_received_hook_selector_with_health(
            service_runtime,
            herdr_process,
            runtime_health,
        ),
        BenchmarkHookMode::Disabled => Arc::new(DisabledReceivedHookSelector),
    }
}

#[cfg(feature = "benchmark-harness")]
pub fn benchmark_received_hook_selector_with_health_and_fifo(
    service_runtime: LocalServiceRuntime,
    mode: BenchmarkHookMode,
    herdr_process: Arc<dyn HerdrProcessAdapter>,
    runtime_health: RuntimeHealth,
    bare_cli_fifo: BareCliFifo,
    bare_cli_queue_full_drops: BareCliQueueFullDrops,
) -> Arc<dyn MessageReceivedHookSelector> {
    match mode {
        BenchmarkHookMode::Active => active_received_hook_selector_with_health_and_fifo(
            service_runtime,
            herdr_process,
            runtime_health,
            bare_cli_fifo,
            bare_cli_queue_full_drops,
        ),
        BenchmarkHookMode::Disabled => Arc::new(DisabledReceivedHookSelector),
    }
}

/// Benchmark-only adapter: active-hook capacity runs must not invoke Herdr or
/// construct the real process invoker. It accepts the wake locally so the
/// benchmark measures ATM admission and routing rather than an external CLI.
#[cfg(feature = "benchmark-harness")]
pub struct BenchmarkNoopHerdrProcessAdapter;

#[cfg(feature = "benchmark-harness")]
impl HerdrProcessAdapter for BenchmarkNoopHerdrProcessAdapter {
    fn prompt<'a>(
        &'a self,
        agent: &'a atm_core::types::AgentName,
        _session: Option<&'a HerdrSession>,
        _deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<HerdrPromptOutcome, HerdrError>> + Send + 'a>> {
        let snapshot = AgentSnapshot {
            name: Some(agent.to_string()),
            status: HerdrAgentStatus::Idle,
            workspace_id: None,
        };
        Box::pin(async move { Ok(HerdrPromptOutcome::Accepted(snapshot)) })
    }

    fn wait<'a>(
        &'a self,
        agent: &'a atm_core::types::AgentName,
        _session: Option<&'a HerdrSession>,
        _until: &'a [HerdrAgentStatus],
        _timeout: std::time::Duration,
        _deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<atm_herdr::HerdrWaitOutcome, HerdrError>> + Send + 'a>>
    {
        let snapshot = AgentSnapshot {
            name: Some(agent.to_string()),
            status: HerdrAgentStatus::Idle,
            workspace_id: None,
        };
        Box::pin(async move { Ok(atm_herdr::HerdrWaitOutcome { snapshot }) })
    }

    fn get<'a>(
        &'a self,
        agent: &'a atm_core::types::AgentName,
        _session: Option<&'a HerdrSession>,
        _deadline: RequestDeadline,
        _breaker_policy: atm_herdr::BreakerPolicy,
    ) -> Pin<Box<dyn Future<Output = Result<atm_herdr::HerdrGetOutcome, HerdrError>> + Send + 'a>>
    {
        let snapshot = AgentSnapshot {
            name: Some(agent.to_string()),
            status: HerdrAgentStatus::Idle,
            workspace_id: None,
        };
        Box::pin(async move { Ok(atm_herdr::HerdrGetOutcome { snapshot }) })
    }

    fn list<'a>(
        &'a self,
        _session: Option<&'a HerdrSession>,
        _deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<atm_herdr::HerdrListOutcome, HerdrError>> + Send + 'a>>
    {
        Box::pin(async { Ok(atm_herdr::HerdrListOutcome { agents: Vec::new() }) })
    }
}

/// Selects the receiver implementation from the post-persistence dispatch
/// target already planned by core. It owns no application routing or storage.
#[derive(Clone)]
struct ReplacementReceivedHookSelector {
    tmux: TokioTmuxReceivedHook,
    herdr: HerdrReceivedHook,
    graft: PublishedGraftReceivedHook,
    queue_pull: PullPendingReceivedHook,
}

impl ReplacementReceivedHookSelector {
    #[cfg(test)]
    #[must_use]
    fn new(service_runtime: LocalServiceRuntime) -> Self {
        Self::with_herdr_process(
            service_runtime,
            Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default()),
            RuntimeHealth::default(),
        )
    }

    #[must_use]
    fn with_herdr_process(
        service_runtime: LocalServiceRuntime,
        herdr_process: Arc<dyn HerdrProcessAdapter>,
        runtime_health: RuntimeHealth,
    ) -> Self {
        Self::with_herdr_process_and_fifo(
            service_runtime,
            herdr_process,
            runtime_health,
            Default::default(),
            Default::default(),
        )
    }

    #[must_use]
    fn with_herdr_process_and_fifo(
        service_runtime: LocalServiceRuntime,
        herdr_process: Arc<dyn HerdrProcessAdapter>,
        runtime_health: RuntimeHealth,
        bare_cli_fifo: BareCliFifo,
        bare_cli_queue_full_drops: BareCliQueueFullDrops,
    ) -> Self {
        Self {
            tmux: TokioTmuxReceivedHook,
            herdr: HerdrReceivedHook {
                process: herdr_process,
            },
            graft: PublishedGraftReceivedHook {
                service_runtime: service_runtime.clone(),
                runtime_health: runtime_health.clone(),
            },
            queue_pull: PullPendingReceivedHook {
                service_runtime,
                bare_cli_fifo,
                bare_cli_queue_full_drops,
            },
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
            (
                NudgeKind::Steer,
                PostSendBuiltInTarget::LocalSteer(boundary::LocalSteerTarget::Tmux(_)),
            ) => Some(&self.tmux),
            (
                NudgeKind::Steer,
                PostSendBuiltInTarget::LocalSteer(boundary::LocalSteerTarget::Herdr(_)),
            ) => Some(&self.herdr),
            (NudgeKind::Steer, PostSendBuiltInTarget::Graft(_)) => Some(&self.graft),
            (NudgeKind::Queue, PostSendBuiltInTarget::Graft(_)) => Some(&self.graft),
            (NudgeKind::Queue, PostSendBuiltInTarget::QueuePull(_)) => Some(&self.queue_pull),
            (NudgeKind::Steer, PostSendBuiltInTarget::QueuePull(_)) => None,
            (
                NudgeKind::Queue,
                PostSendBuiltInTarget::LocalSteer(boundary::LocalSteerTarget::Tmux(_)),
            ) => Some(&self.tmux),
            (
                NudgeKind::Queue,
                PostSendBuiltInTarget::LocalSteer(boundary::LocalSteerTarget::Herdr(_)),
            ) => Some(&self.herdr),
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
            let PostSendBuiltInTarget::LocalSteer(boundary::LocalSteerTarget::Tmux(target)) =
                dispatch.target
            else {
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

/// Tokio-native Herdr receiver. Herdr performs live agent resolution and its
/// own pre-input blocked-dialog guard; this emitter never falls back to tmux.
#[derive(Clone)]
struct HerdrReceivedHook {
    process: Arc<dyn HerdrProcessAdapter>,
}

impl boundary::sealed::Sealed for HerdrReceivedHook {}

impl AsyncMessageReceivedHookEmitter for HerdrReceivedHook {
    fn emit_received_message(
        &self,
        dispatch: BuiltInPostSendDispatch,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<PostSendEmissionPath, AtmError>> + Send + '_>> {
        let process = Arc::clone(&self.process);
        Box::pin(async move {
            let PostSendBuiltInTarget::LocalSteer(boundary::LocalSteerTarget::Herdr(target)) =
                dispatch.target
            else {
                return Err(AtmError::validation(
                    "Herdr receiver hook received a non-Herdr dispatch",
                ));
            };
            let result = process
                .prompt(&dispatch.event.recipient, target.session.as_ref(), deadline)
                .await;
            match result {
                Ok(HerdrPromptOutcome::Accepted(_)) => {
                    tracing::info!(backend = "herdr", member = %dispatch.event.recipient, outcome = "accepted", "Herdr wake-up submitted");
                }
                Err(error) => {
                    let outcome = error.emission_outcome();
                    tracing::warn!(backend = "herdr", member = %dispatch.event.recipient, error = ?error, outcome, "Herdr wake-up was not accepted");
                }
            }
            Ok(PostSendEmissionPath::LocalHerdr)
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
    let mut child = tokio::process::Command::new("tmux")
        .args(arguments)
        .kill_on_drop(true)
        .spawn()
        .map_err(|source| {
            AtmError::new(
                AtmErrorCode::PostSendTmuxSendFailed,
                "failed to start tmux received-message hook command",
            )
            .with_cause(source)
        })?;
    let status = match tokio::time::timeout(remaining, child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(source)) => {
            return Err(AtmError::new(
                AtmErrorCode::PostSendTmuxSendFailed,
                "tmux received-message hook command failed",
            )
            .with_cause(source));
        }
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(hook_deadline_error("while executing tmux"));
        }
    };
    status.success().then_some(()).ok_or_else(|| {
        AtmError::new(
            AtmErrorCode::PostSendTmuxSendFailed,
            "tmux received-message hook command failed",
        )
    })
}

fn hook_deadline_error(stage: &'static str) -> AtmError {
    AtmError::new(
        AtmErrorCode::PostSendTmuxSendFailed,
        format!("received-message hook deadline expired {stage}"),
    )
}

fn clear_queue_marker_after_handoff(
    service_runtime: &LocalServiceRuntime,
    runtime_health: &RuntimeHealth,
    member: &atm_core::boundary::MemberKey,
    message_id: &atm_core::schema::AtmMessageId,
) {
    let store = match service_runtime.pending_nudge_store() {
        Ok(store) => store,
        Err(error) => {
            runtime_health.record_graft_queue_marker_clear_failure();
            tracing::warn!(
                subsystem = "atm_core.queue",
                action = "handoff_marker_clear",
                outcome = "failed",
                %error,
                msg_id = %message_id,
                "queue delivery succeeded but pending marker store was unavailable"
            );
            return;
        }
    };
    if let Err(error) = store.clear_pending_on_handoff(member, message_id) {
        runtime_health.record_graft_queue_marker_clear_failure();
        tracing::warn!(
            subsystem = "atm_core.queue",
            action = "handoff_marker_clear",
            outcome = "failed",
            %error,
            msg_id = %message_id,
            "queue delivery succeeded but pending marker clear failed; retrying"
        );
        if let Err(retry_error) = store.clear_pending_on_handoff(member, message_id) {
            runtime_health.record_graft_queue_marker_clear_failure();
            tracing::warn!(
                subsystem = "atm_core.queue",
                action = "handoff_marker_clear",
                outcome = "failed",
                %retry_error,
                msg_id = %message_id,
                "pending marker clear retry failed after successful queue delivery"
            );
        }
    }
}

/// Graft remains receiver-owned. The existing endpoint operation has bounded
/// socket timeouts derived from the inherited absolute deadline, so the only
/// blocking seam is awaited rather than detached.
#[derive(Clone)]
struct PublishedGraftReceivedHook {
    service_runtime: LocalServiceRuntime,
    runtime_health: RuntimeHealth,
}

impl boundary::sealed::Sealed for PublishedGraftReceivedHook {}

impl AsyncMessageReceivedHookEmitter for PublishedGraftReceivedHook {
    fn emit_received_message(
        &self,
        dispatch: BuiltInPostSendDispatch,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<PostSendEmissionPath, AtmError>> + Send + '_>> {
        let service_runtime = self.service_runtime.clone();
        let runtime_health = self.runtime_health.clone();
        let kind = dispatch.kind;
        let member = atm_core::boundary::MemberKey::new(
            dispatch.event.recipient_team.clone(),
            dispatch.event.recipient.clone(),
        );
        let member_for_handoff = member.clone();
        let message_id = dispatch.event.message_id;
        let runtime_health_for_clear = runtime_health.clone();
        Box::pin(async move {
            let result = tokio::task::spawn_blocking(move || {
                let result = atm_core::graft::deliver_published_receiver_hook_from_local_runtime(
                    &service_runtime,
                    &dispatch,
                    deadline,
                );
                if result.is_ok() && kind == NudgeKind::Queue {
                    clear_queue_marker_after_handoff(
                        &service_runtime,
                        &runtime_health_for_clear,
                        &member_for_handoff,
                        &message_id,
                    );
                }
                result
            })
            .await
            .map_err(|source| {
                AtmError::new(
                    AtmErrorCode::InternalError,
                    "published Graft receiver hook task ended unexpectedly",
                )
                .with_cause(source)
            })?;
            if let Err(error) = &result
                && kind == NudgeKind::Queue
            {
                runtime_health.record_graft_queue_handoff_failure();
                tracing::warn!(
                    subsystem = "atm_graft.queue",
                    action = "handoff",
                    outcome = "failed",
                    member = %member,
                    msg_id = %message_id,
                    error_code = %error.code(),
                    error_message = %error.message(),
                    "queue-kind graft handoff failed"
                );
            }
            result
        })
    }
}

/// Hands a bare-CLI delivery to the daemon-lifetime FIFO and immediately
/// clears only the exact durable pending marker that was handed off.
#[derive(Clone)]
struct PullPendingReceivedHook {
    service_runtime: LocalServiceRuntime,
    bare_cli_fifo: BareCliFifo,
    bare_cli_queue_full_drops: BareCliQueueFullDrops,
}

impl boundary::sealed::Sealed for PullPendingReceivedHook {}

impl AsyncMessageReceivedHookEmitter for PullPendingReceivedHook {
    fn emit_received_message(
        &self,
        dispatch: BuiltInPostSendDispatch,
        _deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<PostSendEmissionPath, AtmError>> + Send + '_>> {
        let service_runtime = self.service_runtime.clone();
        let bare_cli_fifo = self.bare_cli_fifo.clone();
        let bare_cli_queue_full_drops = self.bare_cli_queue_full_drops.clone();
        let target = match dispatch.target {
            PostSendBuiltInTarget::QueuePull(target) => target,
            _ => {
                return Box::pin(async {
                    Err(AtmError::new(
                        AtmErrorCode::InternalError,
                        "queue-pull emitter received a non-queue-pull target",
                    ))
                });
            }
        };
        Box::pin(async move {
            let member =
                atm_core::boundary::MemberKey::new(target.team.clone(), target.agent.clone());
            let message = atm_core::protocol::QueuedNudgeMessage {
                kind: target.kind,
                msg_id: target.msg_id,
                body: target.body,
            };
            tokio::task::spawn_blocking(move || {
                append_bare_cli_message(
                    &bare_cli_fifo,
                    &bare_cli_queue_full_drops,
                    member.clone(),
                    message,
                )?;
                let store = service_runtime.pending_nudge_store()?;
                store.clear_pending_on_handoff(&member, &target.msg_id)?;
                Ok(PostSendEmissionPath::QueuePull)
            })
            .await
            .map_err(|source| {
                AtmError::new(
                    AtmErrorCode::InternalError,
                    "bare-CLI queue-pull handoff task ended unexpectedly",
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
        AsyncMessageReceivedHookEmitter, BuiltInPostSendDispatch, HerdrNudgeTarget,
        LocalSteerTarget, LocalTmuxNudgeTarget, MemberKey, NudgeClaim, NudgeKind,
        PendingNudgeStore, PostSendBuiltInTarget, PostSendEmissionPath, PostSendHookEvent,
        QueuePullTarget, RosterEntry, RosterHarness, RosterMemberKind,
    };
    use atm_core::error::{AtmError, AtmErrorCode};
    use atm_core::graft::GraftReceiverListener;
    use atm_core::nudge_dispatch::rebuild_received_hook_dispatch;
    use atm_core::observability::NullObservability;
    use atm_core::protocol::{GraftReceiverRegistration, OwnerGeneration};
    use atm_core::schema::{AgentType, AtmMessageId};
    use atm_core::send::{NudgeMode, SendMessageSource, WriteRequest, write_mail_with_runtime};
    use atm_core::types::{AgentName, IsoTimestamp, PaneId, TeamName};
    use atm_http_runtime::RuntimeHealth;
    use atm_runtime_test_support::{
        open_graft_receiver_endpoint_store, open_isolated_sqlite_boundary,
    };
    use atm_storage::RosterSnapshot;
    use std::fs;
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

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
            target: PostSendBuiltInTarget::LocalSteer(LocalSteerTarget::Tmux(
                LocalTmuxNudgeTarget {
                    pane_id: PaneId::from_cli("%1").expect("pane"),
                    rendered_nudge: "test".to_owned(),
                },
            )),
            kind: NudgeKind::Steer,
        }
    }

    fn queue_dispatch() -> BuiltInPostSendDispatch {
        let mut dispatch = tmux_dispatch();
        dispatch.kind = NudgeKind::Queue;
        dispatch
    }

    fn herdr_dispatch(kind: NudgeKind) -> BuiltInPostSendDispatch {
        let mut dispatch = tmux_dispatch();
        dispatch.kind = kind;
        dispatch.target =
            PostSendBuiltInTarget::LocalSteer(LocalSteerTarget::Herdr(HerdrNudgeTarget {
                session: Some(atm_core::HerdrSession::new("team-a").expect("session")),
            }));
        dispatch
    }

    #[test]
    fn selector_routes_tmux_for_steer_and_queue_replay() {
        let temporary_root = tempfile::tempdir().expect("temporary selector runtime root");
        let assembly =
            atm_runtime_test_support::open_isolated_sqlite_boundary(temporary_root.path())
                .expect("assemble isolated selector runtime");
        let selector = ReplacementReceivedHookSelector::new(assembly.service_runtime);

        assert!(selector.select_emitter(&tmux_dispatch()).is_some());
        assert!(selector.select_emitter(&queue_dispatch()).is_some());
    }

    #[test]
    fn selector_routes_queue_kind_graft_to_the_graft_emitter() {
        let temporary_root = tempfile::tempdir().expect("temporary selector runtime root");
        let assembly =
            atm_runtime_test_support::open_isolated_sqlite_boundary(temporary_root.path())
                .expect("assemble isolated selector runtime");
        let selector = ReplacementReceivedHookSelector::new(assembly.service_runtime);
        let mut dispatch = queue_dispatch();
        dispatch.target = PostSendBuiltInTarget::Graft(atm_core::boundary::GraftNudgeTarget {
            recipient: dispatch.event.recipient.clone(),
            recipient_team: dispatch.event.recipient_team.clone(),
            rendered_nudge: "<atm>queue</atm>".to_owned(),
            message_body: "queue body".to_owned(),
        });
        assert!(selector.select_emitter(&dispatch).is_some());
    }

    fn graft_roster_entry(team: &TeamName, agent: &str) -> RosterEntry {
        RosterEntry {
            team_name: team.clone(),
            agent_name: agent.parse().expect("agent"),
            member_kind: RosterMemberKind::Permanent,
            harness: RosterHarness::PythonGraft,
            agent_type: AgentType::default(),
            model: atm_core::types::ModelName::default(),
            recipient_pane_id: None,
            metadata_json: serde_json::Map::new(),
        }
    }

    fn queue_graft_runtime(
        root: &std::path::Path,
    ) -> (
        atm_core::LocalServiceRuntime,
        std::sync::Arc<dyn atm_core::GraftReceiverEndpointStore + Send + Sync>,
        TeamName,
        AgentName,
    ) {
        let assembly = open_isolated_sqlite_boundary(root).expect("assemble isolated runtime");
        let team: TeamName = "test-team".parse().expect("team");
        let recipient: AgentName = "recipient".parse().expect("recipient");
        assembly
            .service_runtime
            .shared_roster_store_arc()
            .save_roster(&RosterSnapshot {
                team_name: team.clone(),
                members: vec![
                    graft_roster_entry(&team, "sender"),
                    graft_roster_entry(&team, "recipient"),
                ],
                refreshed_at: None,
            })
            .expect("seed roster");
        let endpoint_store =
            open_graft_receiver_endpoint_store(root.join("runtime").join("mail.sqlite3"))
                .expect("open endpoint store");
        let runtime = assembly
            .service_runtime
            .with_graft_receiver_endpoint_store(endpoint_store.clone());
        (runtime, endpoint_store, team, recipient)
    }

    fn queue_write(
        root: &std::path::Path,
        runtime: &atm_core::LocalServiceRuntime,
        team: &TeamName,
    ) -> atm_core::schema::AtmMessageId {
        let home = root.join("home");
        fs::create_dir_all(&home).expect("home");
        let request = WriteRequest::new(
            home.clone(),
            home,
            "sender".parse().expect("sender"),
            "recipient@test-team",
            team.clone(),
            SendMessageSource::Inline("queued graft body".to_owned()),
            None,
            false,
            None,
            false,
        )
        .expect("write request")
        .with_nudge_mode(NudgeMode::Deferred);
        write_mail_with_runtime(request, &NullObservability, runtime)
            .expect("queue write")
            .persisted_message_id()
    }

    struct FailingClearPendingStore {
        inner: Arc<dyn PendingNudgeStore + Send + Sync>,
        clear_calls: AtomicUsize,
    }

    impl atm_storage::contract::sealed::Sealed for FailingClearPendingStore {}

    impl PendingNudgeStore for FailingClearPendingStore {
        fn mark_pending(
            &self,
            member: &MemberKey,
            msg: &AtmMessageId,
            at: IsoTimestamp,
        ) -> Result<bool, AtmError> {
            self.inner.mark_pending(member, msg, at)
        }

        fn claim_next_pending(&self, member: &MemberKey) -> Result<Option<NudgeClaim>, AtmError> {
            self.inner.claim_next_pending(member)
        }

        fn requeue_pending(&self, member: &MemberKey, claim: &NudgeClaim) -> Result<(), AtmError> {
            self.inner.requeue_pending(member, claim)
        }

        fn release_pending(&self, member: &MemberKey, claim: &NudgeClaim) -> Result<(), AtmError> {
            self.inner.release_pending(member, claim)
        }

        fn clear_pending_on_read(
            &self,
            member: &MemberKey,
            msg: &AtmMessageId,
        ) -> Result<(), AtmError> {
            self.inner.clear_pending_on_read(member, msg)
        }

        fn clear_pending_on_handoff(
            &self,
            _member: &MemberKey,
            _msg: &AtmMessageId,
        ) -> Result<(), AtmError> {
            self.clear_calls.fetch_add(1, Ordering::SeqCst);
            Err(AtmError::mailbox_write("clear marker test failure"))
        }

        fn list_pending_members(&self) -> Result<Vec<MemberKey>, AtmError> {
            self.inner.list_pending_members()
        }
    }

    #[tokio::test]
    async fn bare_cli_queue_pull_appends_and_clears_the_exact_pending_marker() {
        let root = tempfile::tempdir().expect("temporary runtime root");
        let (runtime, _endpoint_store, team, recipient) = queue_graft_runtime(root.path());
        let message_id = queue_write(root.path(), &runtime, &team);
        let member = MemberKey::new(team.clone(), recipient.clone());
        let mut dispatch = tmux_dispatch();
        dispatch.kind = NudgeKind::Queue;
        dispatch.target = PostSendBuiltInTarget::QueuePull(QueuePullTarget {
            team: team.clone(),
            agent: recipient.clone(),
            kind: NudgeKind::Queue,
            msg_id: message_id,
            body: "bare CLI body".to_owned(),
        });
        let fifo: atm_http_runtime::BareCliFifo = Default::default();
        let drops: atm_http_runtime::BareCliQueueFullDrops = Default::default();
        let selector = ReplacementReceivedHookSelector::with_herdr_process_and_fifo(
            runtime.clone(),
            Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default()),
            RuntimeHealth::default(),
            fifo.clone(),
            drops,
        );

        let path = selector
            .select_emitter(&dispatch)
            .expect("bare-CLI queue-pull emitter")
            .emit_received_message(dispatch, RequestDeadline::after(Duration::from_secs(1)))
            .await
            .expect("queue-pull handoff");
        assert_eq!(path, PostSendEmissionPath::QueuePull);
        let drained =
            atm_http_runtime::drain_bare_cli_messages(&fifo, &member).expect("drain FIFO");
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].msg_id, message_id);
        assert!(
            runtime
                .pending_nudge_store()
                .expect("pending store")
                .claim_next_pending(&member)
                .expect("claim query")
                .is_none()
        );
    }

    #[tokio::test]
    async fn queue_graft_handoff_clears_only_the_handed_message_marker() {
        let root = tempfile::tempdir().expect("temporary runtime root");
        let (runtime, endpoint_store, team, recipient) = queue_graft_runtime(root.path());
        let listener = GraftReceiverListener::bind(root.path(), &team, &recipient, None)
            .expect("bind graft receiver");
        endpoint_store
            .register(
                &GraftReceiverRegistration {
                    team: team.clone(),
                    agent: recipient.clone(),
                    endpoint: listener.local_addr().expect("endpoint"),
                    capability: listener.capability().clone(),
                    owner_generation: OwnerGeneration::new(listener.owner_generation())
                        .expect("generation"),
                },
                atm_core::types::IsoTimestamp::now().into_inner(),
            )
            .expect("register receiver");
        let server = thread::Builder::new()
            .name("graft-test-server".to_string())
            .spawn(move || {
                let mut stream = loop {
                    if let Some(stream) = listener.poll_accept().expect("accept") {
                        break stream;
                    }
                    thread::yield_now();
                };
                let request = listener
                    .read_request(&mut stream, std::time::Duration::from_secs(3))
                    .expect("read request");
                assert_eq!(request.kind, NudgeKind::Queue);
                listener
                    .write_response(
                        &mut stream,
                        &atm_core::graft::GraftPostSendResponse::Delivered,
                    )
                    .expect("write response");
            })
            .expect("spawn graft test server");

        let message_id = queue_write(root.path(), &runtime, &team);
        let other_message_id = queue_write(root.path(), &runtime, &team);
        let member = MemberKey::new(team.clone(), recipient.clone());
        let dispatch =
            rebuild_received_hook_dispatch(&runtime, &member, message_id, NudgeKind::Queue)
                .expect("rebuild dispatch")
                .expect("graft dispatch");
        let selector = ReplacementReceivedHookSelector::with_herdr_process(
            runtime.clone(),
            Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default()),
            RuntimeHealth::default(),
        );
        selector
            .select_emitter(&dispatch)
            .expect("graft emitter")
            .emit_received_message(
                dispatch,
                RequestDeadline::after(std::time::Duration::from_secs(3)),
            )
            .await
            .expect("queue handoff");
        server.join().expect("server join");

        let claim = runtime
            .pending_nudge_store()
            .expect("pending store")
            .claim_next_pending(&member)
            .expect("claim query");
        assert_eq!(
            claim.expect("the other message remains pending").msg,
            other_message_id,
            "successful handoff clears only the exact marker"
        );
    }

    #[tokio::test]
    async fn aq2_crit_001_successful_handoff_retries_marker_clear_failure_without_failing_delivery()
    {
        let root = tempfile::tempdir().expect("temporary runtime root");
        let (base_runtime, endpoint_store, team, recipient) = queue_graft_runtime(root.path());
        let base_pending_store = base_runtime.pending_nudge_store().expect("pending store");
        let failing_store = Arc::new(FailingClearPendingStore {
            inner: base_pending_store,
            clear_calls: AtomicUsize::new(0),
        });
        let runtime = base_runtime.with_pending_nudge_store(failing_store.clone());
        let listener = GraftReceiverListener::bind(root.path(), &team, &recipient, None)
            .expect("bind graft receiver");
        endpoint_store
            .register(
                &GraftReceiverRegistration {
                    team: team.clone(),
                    agent: recipient.clone(),
                    endpoint: listener.local_addr().expect("endpoint"),
                    capability: listener.capability().clone(),
                    owner_generation: OwnerGeneration::new(listener.owner_generation())
                        .expect("generation"),
                },
                atm_core::types::IsoTimestamp::now().into_inner(),
            )
            .expect("register receiver");
        let server = thread::Builder::new()
            .name("graft-clear-failure-test-server".to_owned())
            .spawn(move || {
                let mut stream = loop {
                    if let Some(stream) = listener.poll_accept().expect("accept") {
                        break stream;
                    }
                    thread::yield_now();
                };
                listener
                    .read_request(&mut stream, std::time::Duration::from_secs(3))
                    .expect("read request");
                listener
                    .write_response(
                        &mut stream,
                        &atm_core::graft::GraftPostSendResponse::Delivered,
                    )
                    .expect("write response");
            })
            .expect("spawn graft test server");

        let message_id = queue_write(root.path(), &runtime, &team);
        let member = MemberKey::new(team.clone(), recipient.clone());
        let dispatch =
            rebuild_received_hook_dispatch(&runtime, &member, message_id, NudgeKind::Queue)
                .expect("rebuild dispatch")
                .expect("graft dispatch");
        let health = RuntimeHealth::default();
        let selector = ReplacementReceivedHookSelector::with_herdr_process(
            runtime.clone(),
            Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default()),
            health.clone(),
        );
        let result = selector
            .select_emitter(&dispatch)
            .expect("graft emitter")
            .emit_received_message(
                dispatch,
                RequestDeadline::after(std::time::Duration::from_secs(3)),
            )
            .await;
        server.join().expect("server join");

        assert!(
            result.is_ok(),
            "marker-clear failure must not fail delivery"
        );
        assert_eq!(failing_store.clear_calls.load(Ordering::SeqCst), 2);
        assert_eq!(health.snapshot().graft_queue_handoff_failures_total, 0);
        assert_eq!(health.snapshot().graft_queue_marker_clear_failures_total, 2);
        let claim = runtime
            .pending_nudge_store()
            .expect("pending store")
            .claim_next_pending(&member)
            .expect("claim query")
            .expect("failed marker clear leaves marker set");
        assert_eq!(claim.msg, message_id);
    }

    #[tokio::test]
    async fn failed_queue_graft_handoff_retains_marker_at_attempt_zero() {
        let root = tempfile::tempdir().expect("temporary runtime root");
        let (runtime, endpoint_store, team, recipient) = queue_graft_runtime(root.path());
        let unused = TcpListener::bind(("127.0.0.1", 0)).expect("reserve endpoint");
        let endpoint = unused.local_addr().expect("endpoint");
        drop(unused);
        endpoint_store
            .register(
                &GraftReceiverRegistration {
                    team: team.clone(),
                    agent: recipient.clone(),
                    endpoint,
                    capability: atm_core::local_http::LocalCapability::generate()
                        .expect("capability"),
                    owner_generation: OwnerGeneration::new("01J00000000000000000000000")
                        .expect("generation"),
                },
                atm_core::types::IsoTimestamp::now().into_inner(),
            )
            .expect("register unavailable receiver");

        let message_id = queue_write(root.path(), &runtime, &team);
        let member = MemberKey::new(team.clone(), recipient.clone());
        let dispatch =
            rebuild_received_hook_dispatch(&runtime, &member, message_id, NudgeKind::Queue)
                .expect("rebuild dispatch")
                .expect("graft dispatch");
        let health = RuntimeHealth::default();
        let selector = ReplacementReceivedHookSelector::with_herdr_process(
            runtime.clone(),
            Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default()),
            health.clone(),
        );
        let error = selector
            .select_emitter(&dispatch)
            .expect("graft emitter")
            .emit_received_message(
                dispatch,
                RequestDeadline::after(std::time::Duration::from_secs(1)),
            )
            .await
            .expect_err("unavailable graft must fail");
        assert_eq!(error.code(), AtmErrorCode::PostSendGraftUnavailable);
        assert_eq!(health.snapshot().graft_queue_handoff_failures_total, 1);
        let store = runtime.pending_nudge_store().expect("pending store");
        assert!(
            store
                .list_pending_members()
                .expect("pending members")
                .contains(&member)
        );
        let claim = store
            .claim_next_pending(&member)
            .expect("claim query")
            .expect("failed handoff remains claimable");
        assert_eq!(claim.msg, message_id);
        assert_eq!(
            claim.attempt, 0,
            "write-time failure does not increment attempts"
        );
        store
            .release_pending(&member, &claim)
            .expect("restore test claim");
    }

    #[tokio::test]
    async fn sweep_dispatched_queue_graft_failure_reports_for_caller_requeue() {
        let root = tempfile::tempdir().expect("temporary runtime root");
        let (runtime, endpoint_store, team, recipient) = queue_graft_runtime(root.path());
        let unused = TcpListener::bind(("127.0.0.1", 0)).expect("reserve endpoint");
        let endpoint = unused.local_addr().expect("endpoint");
        drop(unused);
        endpoint_store
            .register(
                &GraftReceiverRegistration {
                    team: team.clone(),
                    agent: recipient.clone(),
                    endpoint,
                    capability: atm_core::local_http::LocalCapability::generate()
                        .expect("capability"),
                    owner_generation: OwnerGeneration::new("01J00000000000000000000000")
                        .expect("generation"),
                },
                atm_core::types::IsoTimestamp::now().into_inner(),
            )
            .expect("register unavailable receiver");

        let message_id = queue_write(root.path(), &runtime, &team);
        let member = MemberKey::new(team.clone(), recipient.clone());
        let dispatch =
            rebuild_received_hook_dispatch(&runtime, &member, message_id, NudgeKind::Queue)
                .expect("rebuild dispatch")
                .expect("graft dispatch");
        let claim = runtime
            .pending_nudge_store()
            .expect("pending store")
            .claim_next_pending(&member)
            .expect("claim query")
            .expect("sweep claim");
        let health = RuntimeHealth::default();
        let selector = ReplacementReceivedHookSelector::with_herdr_process(
            runtime.clone(),
            Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default()),
            health.clone(),
        );
        let error = selector
            .select_emitter(&dispatch)
            .expect("graft emitter")
            .emit_received_message(dispatch, RequestDeadline::after(Duration::from_secs(1)))
            .await
            .expect_err("unavailable graft must be reported to sweep caller");
        assert_eq!(error.code(), AtmErrorCode::PostSendGraftUnavailable);
        assert_eq!(health.snapshot().graft_queue_handoff_failures_total, 1);
        runtime
            .pending_nudge_store()
            .expect("pending store")
            .requeue_pending(&member, &claim)
            .expect("AQ3 caller owns requeue");
    }

    #[tokio::test]
    async fn selector_routes_both_herdr_kinds_through_the_injected_adapter() {
        let temporary_root = tempfile::tempdir().expect("temporary selector runtime root");
        let assembly =
            atm_runtime_test_support::open_isolated_sqlite_boundary(temporary_root.path())
                .expect("assemble isolated selector runtime");
        let fake = std::sync::Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
        let selector = ReplacementReceivedHookSelector::with_herdr_process(
            assembly.service_runtime,
            fake.clone(),
            RuntimeHealth::default(),
        );
        for kind in [NudgeKind::Steer, NudgeKind::Queue] {
            let emitter = selector
                .select_emitter(&herdr_dispatch(kind))
                .expect("Herdr local steer must be selected");
            assert_eq!(
                emitter
                    .emit_received_message(
                        herdr_dispatch(kind),
                        RequestDeadline::after(Duration::from_secs(1))
                    )
                    .await
                    .expect("fake Herdr accepts prompt"),
                atm_core::boundary::PostSendEmissionPath::LocalHerdr
            );
        }
        let calls = fake.calls();
        assert_eq!(calls.len(), 2);
        assert!(calls.iter().all(|call| matches!(
            call,
            atm_herdr::testing::FakeHerdrCall::Prompt {
                session: Some(_),
                ..
            }
        )));
    }

    #[test]
    fn structured_herdr_outcomes_preserve_each_d8_condition() {
        use atm_herdr::HerdrError;
        assert_eq!(
            HerdrError::AgentBlocked.emission_outcome(),
            "blocked_before_input"
        );
        assert_eq!(
            HerdrError::AgentNotFound.emission_outcome(),
            "target_not_present"
        );
        assert_eq!(HerdrError::AgentNotReady.emission_outcome(), "not_ready");
        assert_ne!(
            HerdrError::ProtocolMismatch.emission_outcome(),
            "advisory_failure"
        );
        assert_eq!(HerdrError::TimedOut.emission_outcome(), "timed_out");
        assert_eq!(
            HerdrError::Unavailable {
                retry_after: Duration::from_secs(1),
            }
            .emission_outcome(),
            "breaker_unavailable"
        );
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
