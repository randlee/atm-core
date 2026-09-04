//! AQ3's durable queue drain and restart-recovery sweep.
//!
//! This is composition-owned code. The runtime reports a lifecycle transition
//! through its sealed boundary; this module performs the storage claim and
//! routes the rebuilt dispatch through the ordinary receiver selector.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use atm_core::LocalServiceRuntime;
use atm_core::api::RequestDeadline;
use atm_core::boundary::{MemberKey, MessageReceivedHookSelector, NudgeClaim, NudgeKind};
use atm_core::delivery_channel::{
    DeliveryChannel, classify_delivery_channel, graft_lease_state, local_message_received_backend,
};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::nudge_dispatch::rebuild_received_hook_dispatch;
use atm_core::protocol::RuntimeMemberState;
use atm_http_runtime::{MemberStateTransitionSink, RuntimeHealth};
use tokio::sync::watch;
use tokio::task::JoinHandle;

const RECOVERY_SWEEP_INTERVAL: Duration = Duration::from_secs(30);
const QUEUE_DRAIN_DEADLINE: Duration = Duration::from_secs(3);

/// The one classifier guard shared by transition drains and recovery sweeps.
///
/// In particular, Herdr remains AQ2.7's lifecycle-gated claimant and bare CLI
/// handoff remains AQ2.5's write-time FIFO path.
pub(crate) fn queue_drain_channel_allowed(
    runtime: &LocalServiceRuntime,
    member: &MemberKey,
) -> Result<bool, AtmError> {
    let Some(roster) = runtime.load_roster_member(member.team(), member.agent())? else {
        return Ok(false);
    };
    let local_backend = local_message_received_backend(&roster);
    let graft_lease = match runtime.graft_receiver_endpoint_store() {
        Ok(store) => store
            .lookup(member.team(), member.agent())
            .map_err(atm_core::graft_store_error)?,
        Err(_) => None,
    };
    Ok(matches!(
        classify_delivery_channel(
            local_backend.as_ref(),
            graft_lease_state(graft_lease.as_ref()),
        ),
        DeliveryChannel::TmuxSteer | DeliveryChannel::Graft
    ))
}

#[derive(Clone)]
pub(crate) struct DrainOnTransitionSink {
    runtime: LocalServiceRuntime,
    selector: Arc<dyn MessageReceivedHookSelector>,
    runtime_health: RuntimeHealth,
    tracker: TransitionDrainTracker,
}

impl DrainOnTransitionSink {
    pub(crate) fn new(
        runtime: LocalServiceRuntime,
        selector: Arc<dyn MessageReceivedHookSelector>,
        runtime_health: RuntimeHealth,
        tracker: TransitionDrainTracker,
    ) -> Self {
        Self {
            runtime,
            selector,
            runtime_health,
            tracker,
        }
    }
}

pub(crate) fn transition_sink(
    runtime: LocalServiceRuntime,
    selector: Arc<dyn MessageReceivedHookSelector>,
    runtime_health: RuntimeHealth,
    tracker: TransitionDrainTracker,
) -> Arc<dyn MemberStateTransitionSink> {
    Arc::new(DrainOnTransitionSink::new(
        runtime,
        selector,
        runtime_health,
        tracker,
    ))
}

impl atm_core::boundary::sealed::Sealed for DrainOnTransitionSink {}

impl MemberStateTransitionSink for DrainOnTransitionSink {
    fn on_transition(&self, member: &MemberKey, from: RuntimeMemberState, to: RuntimeMemberState) {
        if to != RuntimeMemberState::Idle || from == RuntimeMemberState::Idle {
            return;
        }
        let runtime = self.runtime.clone();
        let selector = Arc::clone(&self.selector);
        let health = self.runtime_health.clone();
        let member = member.clone();
        let mut cancelled = self.tracker.subscribe();
        let join = tokio::spawn(async move {
            tokio::select! {
                _ = cancelled.changed() => {}
                result = drain_one(&runtime, &selector, &health, &member) => {
                    if let Err(error) = result {
                        health.record_queue_drain_failure();
                        tracing::warn!(
                            subsystem = "atm_core.queue",
                            action = "idle_transition_drain",
                            outcome = "failed",
                            member = %member,
                            %error,
                            "idle-transition queue drain failed"
                        );
                    }
                }
            }
        });
        self.tracker.track(join);
    }
}

#[derive(Clone)]
pub(crate) struct TransitionDrainTracker {
    cancel: watch::Sender<bool>,
    joins: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

impl TransitionDrainTracker {
    fn new() -> Self {
        let (cancel, _) = watch::channel(false);
        Self {
            cancel,
            joins: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn subscribe(&self) -> watch::Receiver<bool> {
        self.cancel.subscribe()
    }

    fn track(&self, join: JoinHandle<()>) {
        self.joins
            .lock()
            .expect("transition tracker lock")
            .push(join);
    }

    fn abort_all(&self) {
        let _ = self.cancel.send(true);
        for join in self.joins.lock().expect("transition tracker lock").iter() {
            join.abort();
        }
    }

    async fn shutdown(&self, deadline: Duration) {
        let _ = self.cancel.send(true);
        let deadline_at = tokio::time::Instant::now() + deadline;
        let joins = std::mem::take(&mut *self.joins.lock().expect("transition tracker lock"));
        for mut join in joins {
            let remaining = deadline_at.saturating_duration_since(tokio::time::Instant::now());
            if tokio::time::timeout(remaining, &mut join).await.is_err() {
                join.abort();
                let _ = join.await;
            }
        }
    }
}

pub(crate) struct RecoverySweepHandle {
    join: Option<JoinHandle<()>>,
    tracker: TransitionDrainTracker,
}

impl Drop for RecoverySweepHandle {
    fn drop(&mut self) {
        self.tracker.abort_all();
        if let Some(join) = self.join.take() {
            join.abort();
        }
    }
}

pub(crate) fn spawn_recovery_sweep(
    runtime: LocalServiceRuntime,
    selector: Arc<dyn MessageReceivedHookSelector>,
    runtime_health: RuntimeHealth,
) -> RecoverySweepHandle {
    let tracker = TransitionDrainTracker::new();
    let mut cancelled = tracker.subscribe();
    let join = tokio::spawn(async move {
        let mut interval = tokio::time::interval_at(
            tokio::time::Instant::now() + RECOVERY_SWEEP_INTERVAL,
            RECOVERY_SWEEP_INTERVAL,
        );
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(error) = run_recovery_sweep_once(&runtime, &selector, &runtime_health).await {
                        runtime_health.record_queue_drain_failure();
                        tracing::warn!(
                            subsystem = "atm_core.queue",
                            action = "recovery_sweep",
                            outcome = "failed",
                            %error,
                            "queue recovery sweep failed"
                        );
                    }
                }
                changed = cancelled.changed() => {
                    if changed.is_err() || *cancelled.borrow() {
                        break;
                    }
                }
            }
        }
    });
    RecoverySweepHandle {
        join: Some(join),
        tracker,
    }
}

impl RecoverySweepHandle {
    pub(crate) fn transition_tracker(&self) -> TransitionDrainTracker {
        self.tracker.clone()
    }
}

pub(crate) async fn run_recovery_sweep_once(
    runtime: &LocalServiceRuntime,
    selector: &Arc<dyn MessageReceivedHookSelector>,
    runtime_health: &RuntimeHealth,
) -> Result<(), AtmError> {
    let runtime_for_list = runtime.clone();
    let members = run_blocking("list pending queue members", move || {
        runtime_for_list
            .pending_nudge_store()?
            .list_pending_members()
    })
    .await?;
    let candidate_count = members.len();
    let mut drained = 0usize;
    for member in members {
        match drain_one(runtime, selector, runtime_health, &member).await {
            Ok(true) => drained += 1,
            Ok(false) => {}
            Err(error) => {
                runtime_health.record_queue_drain_failure();
                tracing::warn!(
                    subsystem = "atm_core.queue",
                    action = "recovery_sweep_member",
                    outcome = "failed",
                    member = %member,
                    %error,
                    "queue recovery sweep member failed; continuing"
                );
            }
        }
    }
    tracing::info!(
        subsystem = "atm_core.queue",
        action = "recovery_sweep",
        outcome = "complete",
        candidate_count,
        drained,
        "queue recovery sweep completed"
    );
    Ok(())
}

async fn drain_one(
    runtime: &LocalServiceRuntime,
    selector: &Arc<dyn MessageReceivedHookSelector>,
    runtime_health: &RuntimeHealth,
    member: &MemberKey,
) -> Result<bool, AtmError> {
    let Some((mut claim_guard, dispatch)) = claim_next_dispatch(runtime, member).await? else {
        return Ok(false);
    };
    let message_id = claim_guard.message_id();
    let tmux_marker = matches!(
        &dispatch.target,
        atm_core::boundary::PostSendBuiltInTarget::LocalSteer(
            atm_core::boundary::LocalSteerTarget::Tmux(_)
        )
    );
    let emit_result = match selector.select_emitter(&dispatch) {
        Some(emitter) => emitter
            .emit_received_message(dispatch, RequestDeadline::after(QUEUE_DRAIN_DEADLINE))
            .await
            .map(|_| ()),
        None => Err(AtmError::new(
            AtmErrorCode::InternalError,
            "queue claim rebuilt without a selected receiver emitter",
        )),
    };
    if let Err(error) = emit_result {
        requeue_claim(runtime, member, &claim_guard).await?;
        claim_guard.disarm();
        tracing::warn!(
            subsystem = "atm_core.queue",
            action = "drain",
            outcome = "requeued",
            member = %member,
            msg_id = %message_id,
            %error,
            "queue dispatch failed and was requeued"
        );
        return Err(error);
    }
    if tmux_marker {
        clear_delivered_marker(runtime, runtime_health, member, message_id).await;
    }
    claim_guard.disarm();
    runtime_health.record_queue_message_drained();
    tracing::info!(
        subsystem = "atm_core.queue",
        action = "drain",
        outcome = "delivered",
        member = %member,
        msg_id = %message_id,
        "one pending queue message drained"
    );
    Ok(true)
}

async fn claim_next_dispatch(
    runtime: &LocalServiceRuntime,
    member: &MemberKey,
) -> Result<
    Option<(
        ClaimedPendingMessage,
        atm_core::boundary::BuiltInPostSendDispatch,
    )>,
    AtmError,
> {
    let runtime_for_claim = runtime.clone();
    let member_for_claim = member.clone();
    run_blocking("claim pending queue message", move || {
        if !queue_drain_channel_allowed(&runtime_for_claim, &member_for_claim)? {
            return Ok(None);
        }
        let store = runtime_for_claim.pending_nudge_store()?;
        let Some(claim) = store.claim_next_pending(&member_for_claim)? else {
            return Ok(None);
        };
        let mut claim_guard =
            ClaimedPendingMessage::new(runtime_for_claim.clone(), member_for_claim.clone(), claim);
        let message_id = claim_guard.message_id();
        let dispatch = match rebuild_received_hook_dispatch(
            &runtime_for_claim,
            &member_for_claim,
            message_id,
            NudgeKind::Queue,
        ) {
            Ok(Some(dispatch)) => dispatch,
            Ok(None) => {
                let result = store.requeue_pending(&member_for_claim, claim_guard.claim());
                if result.is_ok() {
                    claim_guard.disarm();
                }
                result?;
                return Ok(None);
            }
            Err(error) => {
                if store
                    .requeue_pending(&member_for_claim, claim_guard.claim())
                    .is_ok()
                {
                    claim_guard.disarm();
                }
                return Err(error);
            }
        };
        Ok(Some((claim_guard, dispatch)))
    })
    .await
}

async fn requeue_claim(
    runtime: &LocalServiceRuntime,
    member: &MemberKey,
    claim_guard: &ClaimedPendingMessage,
) -> Result<(), AtmError> {
    let runtime_for_requeue = runtime.clone();
    let member_for_requeue = member.clone();
    let claim_for_requeue = claim_guard.claim().clone();
    run_blocking("requeue failed queue message", move || {
        runtime_for_requeue
            .pending_nudge_store()?
            .requeue_pending(&member_for_requeue, &claim_for_requeue)
    })
    .await
}

fn clear_delivered_marker<'a>(
    runtime: &'a LocalServiceRuntime,
    runtime_health: &'a RuntimeHealth,
    member: &'a MemberKey,
    message_id: atm_core::schema::AtmMessageId,
) -> impl std::future::Future<Output = ()> + 'a {
    let runtime_for_clear = runtime.clone();
    let member_for_clear = member.clone();
    let health_for_clear = runtime_health.clone();
    async move {
        if let Err(error) = run_blocking("clear delivered queue marker", move || {
            atm_core::nudge_dispatch::clear_queue_marker_after_handoff(
                &runtime_for_clear,
                &member_for_clear,
                &message_id,
                || health_for_clear.record_graft_queue_marker_clear_failure(),
            );
            Ok(())
        })
        .await
        {
            runtime_health.record_queue_drain_failure();
            tracing::warn!(
                subsystem = "atm_core.queue",
                action = "handoff_marker_clear",
                outcome = "failed",
                member = %member,
                msg_id = %message_id,
                %error,
                "tmux queue delivery succeeded but marker clear task failed"
            );
        }
    }
}

struct ClaimedPendingMessage {
    runtime: LocalServiceRuntime,
    member: MemberKey,
    claim: Option<NudgeClaim>,
}

impl ClaimedPendingMessage {
    fn new(runtime: LocalServiceRuntime, member: MemberKey, claim: NudgeClaim) -> Self {
        Self {
            runtime,
            member,
            claim: Some(claim),
        }
    }

    fn claim(&self) -> &NudgeClaim {
        self.claim.as_ref().expect("claim guard is armed")
    }

    fn message_id(&self) -> atm_core::schema::AtmMessageId {
        self.claim().msg
    }

    fn disarm(&mut self) {
        self.claim = None;
    }
}

impl Drop for ClaimedPendingMessage {
    fn drop(&mut self) {
        let Some(claim) = self.claim.take() else {
            return;
        };
        if let Ok(store) = self.runtime.pending_nudge_store() {
            let _ = store.release_pending(&self.member, &claim);
        }
    }
}

async fn run_blocking<T, F>(description: &'static str, operation: F) -> Result<T, AtmError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, AtmError> + Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|source| {
            AtmError::new(
                AtmErrorCode::InternalError,
                format!("{description} task ended unexpectedly"),
            )
            .with_cause(source)
        })?
}

impl RecoverySweepHandle {
    pub(crate) async fn shutdown(mut self, deadline: Duration) {
        let deadline_at = tokio::time::Instant::now() + deadline;
        let _ = self.tracker.cancel.send(true);
        if let Some(mut join) = self.join.take()
            && tokio::time::timeout(
                deadline_at.saturating_duration_since(tokio::time::Instant::now()),
                &mut join,
            )
            .await
            .is_err()
        {
            join.abort();
            let _ = join.await;
        }
        self.tracker
            .shutdown(deadline_at.saturating_duration_since(tokio::time::Instant::now()))
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DrainOnTransitionSink, TransitionDrainTracker, drain_one, queue_drain_channel_allowed,
        run_recovery_sweep_once,
    };
    use atm_core::RequestDeadline;
    use atm_core::boundary::{
        AsyncMessageReceivedHookEmitter, BuiltInPostSendDispatch, MemberKey,
        MessageReceivedHookSelector, PostSendEmissionPath, RosterEntry, RosterHarness,
        RosterMemberKind, sealed,
    };
    use atm_core::error::{AtmError, AtmErrorCode};
    use atm_core::observability::NullObservability;
    use atm_core::protocol::RuntimeMemberState;
    use atm_core::schema::AgentType;
    use atm_core::send::{NudgeMode, SendMessageSource, WriteRequest, write_mail_with_runtime};
    use atm_core::types::{ModelName, PaneId, TeamName};
    use atm_http_runtime::{MemberStateTransitionSink, RuntimeHealth};
    use atm_runtime_test_support::open_isolated_sqlite_boundary;
    use atm_storage::RosterSnapshot;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::sync::Notify;

    #[test]
    fn shared_channel_precheck_skips_herdr_and_bare_cli_members() {
        let root = tempfile::tempdir().expect("queue drain fixture root");
        let assembly = open_isolated_sqlite_boundary(root.path()).expect("runtime");
        let team: TeamName = "queue-team".parse().expect("team");
        let tmux = "tmux-agent".parse().expect("agent");
        let herdr = "herdr-agent".parse().expect("agent");
        let bare = "cli-agent".parse().expect("agent");
        assembly
            .service_runtime
            .shared_roster_store_arc()
            .save_roster(&RosterSnapshot {
                team_name: team.clone(),
                members: vec![
                    entry(
                        &team,
                        &tmux,
                        Some(PaneId::from_cli("%7").expect("pane")),
                        None,
                    ),
                    entry(&team, &herdr, None, Some("herdr")),
                    entry(&team, &bare, None, None),
                ],
                refreshed_at: None,
            })
            .expect("save roster");

        let runtime = assembly.service_runtime;
        assert!(
            queue_drain_channel_allowed(&runtime, &MemberKey::new(team.clone(), tmux))
                .expect("tmux check")
        );
        assert!(
            !queue_drain_channel_allowed(&runtime, &MemberKey::new(team.clone(), herdr))
                .expect("Herdr check")
        );
        assert!(
            !queue_drain_channel_allowed(&runtime, &MemberKey::new(team, bare))
                .expect("bare CLI check")
        );
    }

    #[tokio::test]
    async fn idle_drain_delivers_oldest_then_next_transition_drains_next() {
        let root = tempfile::tempdir().expect("queue drain fixture root");
        let assembly = open_isolated_sqlite_boundary(root.path()).expect("runtime");
        let team: TeamName = "queue-team".parse().expect("team");
        let sender = "sender".parse().expect("agent");
        let recipient = "recipient".parse().expect("agent");
        assembly
            .service_runtime
            .shared_roster_store_arc()
            .save_roster(&RosterSnapshot {
                team_name: team.clone(),
                members: vec![
                    entry(&team, &sender, None, None),
                    entry(
                        &team,
                        &recipient,
                        Some(PaneId::from_cli("%7").expect("pane")),
                        None,
                    ),
                ],
                refreshed_at: None,
            })
            .expect("save roster");
        let runtime = assembly.service_runtime;
        let first = queue_write(root.path(), &runtime, &team, "first");
        let second = queue_write(root.path(), &runtime, &team, "second");
        let member = MemberKey::new(team, recipient);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let selector: Arc<dyn MessageReceivedHookSelector> = Arc::new(RecordingSelector {
            emitter: RecordingEmitter {
                seen: Arc::clone(&seen),
            },
        });
        let health = RuntimeHealth::default();

        assert!(
            drain_one(&runtime, &selector, &health, &member)
                .await
                .expect("first drain")
        );
        assert!(
            drain_one(&runtime, &selector, &health, &member)
                .await
                .expect("second drain")
        );
        assert_eq!(*seen.lock().expect("recording lock"), vec![first, second]);
        assert!(
            runtime
                .pending_nudge_store()
                .expect("pending store")
                .list_pending_members()
                .expect("pending members")
                .is_empty()
        );
        assert_eq!(health.snapshot().queue_messages_drained_total, 2);
    }

    #[tokio::test]
    async fn concurrent_transition_and_sweep_claim_once() {
        let root = tempfile::tempdir().expect("queue drain fixture root");
        let assembly = open_isolated_sqlite_boundary(root.path()).expect("runtime");
        let team: TeamName = "queue-team".parse().expect("team");
        let sender = "sender".parse().expect("agent");
        let recipient = "recipient".parse().expect("agent");
        assembly
            .service_runtime
            .shared_roster_store_arc()
            .save_roster(&RosterSnapshot {
                team_name: team.clone(),
                members: vec![
                    entry(&team, &sender, None, None),
                    entry(
                        &team,
                        &recipient,
                        Some(PaneId::from_cli("%7").expect("pane")),
                        None,
                    ),
                ],
                refreshed_at: None,
            })
            .expect("save roster");
        let runtime = assembly.service_runtime;
        let message_id = queue_write(root.path(), &runtime, &team, "concurrent");
        let member = MemberKey::new(team, recipient);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let selector: Arc<dyn MessageReceivedHookSelector> = Arc::new(RecordingSelector {
            emitter: RecordingEmitter {
                seen: Arc::clone(&seen),
            },
        });
        let health = RuntimeHealth::default();
        let (left, right) = tokio::join!(
            drain_one(&runtime, &selector, &health, &member),
            drain_one(&runtime, &selector, &health, &member),
        );
        assert_eq!(
            [left.expect("transition drain"), right.expect("sweep drain")]
                .into_iter()
                .filter(|drained| *drained)
                .count(),
            1
        );
        assert_eq!(*seen.lock().expect("recording lock"), vec![message_id]);
        assert!(
            runtime
                .pending_nudge_store()
                .expect("pending store")
                .list_pending_members()
                .expect("pending members")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn shutdown_cancels_tracked_transition_and_releases_in_flight_claim() {
        let root = tempfile::tempdir().expect("queue drain fixture root");
        let assembly = open_isolated_sqlite_boundary(root.path()).expect("runtime");
        let team: TeamName = "queue-team".parse().expect("team");
        let sender = "sender".parse().expect("agent");
        let recipient = "recipient".parse().expect("agent");
        assembly
            .service_runtime
            .shared_roster_store_arc()
            .save_roster(&RosterSnapshot {
                team_name: team.clone(),
                members: vec![
                    entry(&team, &sender, None, None),
                    entry(
                        &team,
                        &recipient,
                        Some(PaneId::from_cli("%7").expect("pane")),
                        None,
                    ),
                ],
                refreshed_at: None,
            })
            .expect("save roster");
        let runtime = assembly.service_runtime;
        queue_write(root.path(), &runtime, &team, "shutdown");
        let member = MemberKey::new(team, recipient);
        let started = Arc::new(Notify::new());
        let selector: Arc<dyn MessageReceivedHookSelector> = Arc::new(BlockingSelector {
            emitter: BlockingEmitter {
                started: Arc::clone(&started),
            },
        });
        let health = RuntimeHealth::default();
        let tracker = TransitionDrainTracker::new();
        let sink = DrainOnTransitionSink::new(runtime.clone(), selector, health, tracker.clone());

        sink.on_transition(
            &member,
            RuntimeMemberState::Active,
            RuntimeMemberState::Idle,
        );
        started.notified().await;
        tracker.shutdown(Duration::from_secs(1)).await;

        assert!(
            runtime
                .pending_nudge_store()
                .expect("pending store")
                .claim_next_pending(&member)
                .expect("claim after shutdown")
                .is_some(),
            "shutdown must release a claim held by an interrupted transition drain"
        );
    }

    #[tokio::test]
    async fn recovery_sweep_replays_pending_after_restart() {
        let root = tempfile::tempdir().expect("queue drain fixture root");
        let assembly = open_isolated_sqlite_boundary(root.path()).expect("runtime");
        let team: TeamName = "queue-team".parse().expect("team");
        let sender = "sender".parse().expect("agent");
        let recipient = "recipient".parse().expect("agent");
        assembly
            .service_runtime
            .shared_roster_store_arc()
            .save_roster(&RosterSnapshot {
                team_name: team.clone(),
                members: vec![
                    entry(&team, &sender, None, None),
                    entry(
                        &team,
                        &recipient,
                        Some(PaneId::from_cli("%7").expect("pane")),
                        None,
                    ),
                ],
                refreshed_at: None,
            })
            .expect("save roster");
        let runtime = assembly.service_runtime;
        let message_id = queue_write(root.path(), &runtime, &team, "restart");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let selector: Arc<dyn MessageReceivedHookSelector> = Arc::new(RecordingSelector {
            emitter: RecordingEmitter {
                seen: Arc::clone(&seen),
            },
        });
        run_recovery_sweep_once(&runtime, &selector, &RuntimeHealth::default())
            .await
            .expect("recovery sweep");
        assert_eq!(*seen.lock().expect("recording lock"), vec![message_id]);
    }

    #[tokio::test]
    async fn recovery_sweep_isolates_one_member_failure_and_continues() {
        let root = tempfile::tempdir().expect("queue drain fixture root");
        let assembly = open_isolated_sqlite_boundary(root.path()).expect("runtime");
        let team: TeamName = "queue-team".parse().expect("team");
        let sender = "sender".parse().expect("agent");
        let first = "first".parse().expect("agent");
        let failed = "failed".parse().expect("agent");
        let third = "third".parse().expect("agent");
        assembly
            .service_runtime
            .shared_roster_store_arc()
            .save_roster(&RosterSnapshot {
                team_name: team.clone(),
                members: vec![
                    entry(&team, &sender, None, None),
                    entry(
                        &team,
                        &first,
                        Some(PaneId::from_cli("%7").expect("pane")),
                        None,
                    ),
                    entry(
                        &team,
                        &failed,
                        Some(PaneId::from_cli("%8").expect("pane")),
                        None,
                    ),
                    entry(
                        &team,
                        &third,
                        Some(PaneId::from_cli("%9").expect("pane")),
                        None,
                    ),
                ],
                refreshed_at: None,
            })
            .expect("save roster");
        let runtime = assembly.service_runtime;
        let first_id = queue_write_to(root.path(), &runtime, &team, "first", "first");
        let failed_id = queue_write_to(root.path(), &runtime, &team, "failed", "failed");
        let third_id = queue_write_to(root.path(), &runtime, &team, "third", "third");
        let failed_member = MemberKey::new(team.clone(), failed);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let selector: Arc<dyn MessageReceivedHookSelector> = Arc::new(MemberOutcomeSelector {
            emitter: MemberOutcomeEmitter {
                seen: Arc::clone(&seen),
                failed_member: failed_member.clone(),
            },
        });
        let health = RuntimeHealth::default();

        run_recovery_sweep_once(&runtime, &selector, &health)
            .await
            .expect("member-isolated recovery sweep");

        assert_eq!(
            *seen.lock().expect("recording lock"),
            vec![first_id, third_id]
        );
        assert_eq!(health.snapshot().queue_drain_failures_total, 1);
        assert_eq!(
            runtime
                .pending_nudge_store()
                .expect("pending store")
                .list_pending_members()
                .expect("pending members"),
            vec![failed_member]
        );
        assert_ne!(failed_id, first_id);
    }

    struct RecordingSelector {
        emitter: RecordingEmitter,
    }

    impl sealed::Sealed for RecordingSelector {}

    impl MessageReceivedHookSelector for RecordingSelector {
        fn select_emitter(
            &self,
            _dispatch: &BuiltInPostSendDispatch,
        ) -> Option<&dyn AsyncMessageReceivedHookEmitter> {
            Some(&self.emitter)
        }
    }

    struct RecordingEmitter {
        seen: Arc<Mutex<Vec<atm_core::schema::AtmMessageId>>>,
    }

    impl sealed::Sealed for RecordingEmitter {}

    impl AsyncMessageReceivedHookEmitter for RecordingEmitter {
        fn emit_received_message(
            &self,
            dispatch: BuiltInPostSendDispatch,
            _deadline: RequestDeadline,
        ) -> Pin<Box<dyn Future<Output = Result<PostSendEmissionPath, AtmError>> + Send + '_>>
        {
            self.seen
                .lock()
                .expect("recording lock")
                .push(dispatch.event.message_id);
            Box::pin(async { Ok(PostSendEmissionPath::LocalTmux) })
        }
    }

    struct MemberOutcomeSelector {
        emitter: MemberOutcomeEmitter,
    }

    impl sealed::Sealed for MemberOutcomeSelector {}

    impl MessageReceivedHookSelector for MemberOutcomeSelector {
        fn select_emitter(
            &self,
            _dispatch: &BuiltInPostSendDispatch,
        ) -> Option<&dyn AsyncMessageReceivedHookEmitter> {
            Some(&self.emitter)
        }
    }

    struct MemberOutcomeEmitter {
        seen: Arc<Mutex<Vec<atm_core::schema::AtmMessageId>>>,
        failed_member: MemberKey,
    }

    impl sealed::Sealed for MemberOutcomeEmitter {}

    impl AsyncMessageReceivedHookEmitter for MemberOutcomeEmitter {
        fn emit_received_message(
            &self,
            dispatch: BuiltInPostSendDispatch,
            _deadline: RequestDeadline,
        ) -> Pin<Box<dyn Future<Output = Result<PostSendEmissionPath, AtmError>> + Send + '_>>
        {
            if MemberKey::new(
                dispatch.event.recipient_team.clone(),
                dispatch.event.recipient.clone(),
            ) == self.failed_member
            {
                return Box::pin(async {
                    Err(AtmError::new(
                        AtmErrorCode::InternalError,
                        "induced member dispatch failure",
                    ))
                });
            }
            self.seen
                .lock()
                .expect("recording lock")
                .push(dispatch.event.message_id);
            Box::pin(async { Ok(PostSendEmissionPath::LocalTmux) })
        }
    }

    struct BlockingSelector {
        emitter: BlockingEmitter,
    }

    impl sealed::Sealed for BlockingSelector {}

    impl MessageReceivedHookSelector for BlockingSelector {
        fn select_emitter(
            &self,
            _dispatch: &BuiltInPostSendDispatch,
        ) -> Option<&dyn AsyncMessageReceivedHookEmitter> {
            Some(&self.emitter)
        }
    }

    struct BlockingEmitter {
        started: Arc<Notify>,
    }

    impl sealed::Sealed for BlockingEmitter {}

    impl AsyncMessageReceivedHookEmitter for BlockingEmitter {
        fn emit_received_message(
            &self,
            _dispatch: BuiltInPostSendDispatch,
            _deadline: RequestDeadline,
        ) -> Pin<Box<dyn Future<Output = Result<PostSendEmissionPath, AtmError>> + Send + '_>>
        {
            self.started.notify_one();
            Box::pin(std::future::pending())
        }
    }

    fn queue_write(
        root: &std::path::Path,
        runtime: &atm_core::LocalServiceRuntime,
        team: &TeamName,
        body: &str,
    ) -> atm_core::schema::AtmMessageId {
        queue_write_to(root, runtime, team, "recipient", body)
    }

    fn queue_write_to(
        root: &std::path::Path,
        runtime: &atm_core::LocalServiceRuntime,
        team: &TeamName,
        recipient: &str,
        body: &str,
    ) -> atm_core::schema::AtmMessageId {
        let home = root.join("home");
        std::fs::create_dir_all(&home).expect("home");
        let recipient_address = format!("{recipient}@{team}");
        let request = WriteRequest::new(
            home.clone(),
            home,
            "sender".parse().expect("sender"),
            &recipient_address,
            team.clone(),
            SendMessageSource::Inline(body.to_owned()),
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

    fn entry(
        team: &TeamName,
        agent: &atm_core::types::AgentName,
        pane: Option<PaneId>,
        backend: Option<&str>,
    ) -> RosterEntry {
        let metadata_json = backend.map_or_else(
            Default::default,
            atm_core::delivery_channel::test_backend_type_metadata,
        );
        RosterEntry {
            team_name: team.clone(),
            agent_name: agent.clone(),
            member_kind: RosterMemberKind::Permanent,
            harness: RosterHarness::ClaudeCode,
            agent_type: AgentType::default(),
            model: ModelName::default(),
            recipient_pane_id: pane,
            metadata_json,
        }
    }
}
