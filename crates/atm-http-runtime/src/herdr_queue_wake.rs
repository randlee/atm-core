//! Tokio-owned polling pump for deferred Herdr queue nudges.

mod release_guard;
mod task_pass;

use release_guard::ReleasePendingOnDrop;
use task_pass::{queue_drain_eligible, runtime_state_with_absence};

use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use atm_core::LocalServiceRuntime;
use atm_core::api::RequestDeadline;
use atm_core::boundary::{
    DurableRosterStore, MemberKey, MessageReceivedHookSelector, NudgeKind, PendingNudgeStore,
};
use atm_core::delivery_channel::{HerdrAgentName, HerdrSession, local_message_received_backend};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::nudge_dispatch::{
    load_received_hook_dispatch_message, rebuild_received_hook_dispatch,
};
use atm_core::protocol::{
    RosterRuntimeObservation, RosterRuntimeObservationUpdate, RuntimeMemberState,
    RuntimeObservationSource,
};
use atm_core::types::IsoTimestamp;
use atm_herdr::{AgentSnapshot, HerdrProcessAdapter};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::herdr_escalation::EscalationState;
use crate::router_support::BoundedBlockingBridge;
use crate::runtime_health::RuntimeHealth;

/// Poll cadence required by AQ2.7.
pub const HERDR_POLL_INTERVAL_MS: u64 = 5_000;
/// Maximum number of prompts admitted by one poll tick.
pub const HERDR_MAX_PROMPTS_PER_TICK: usize = 16;
/// Consecutive no-input releases before one retry-budget attempt is spent.
pub const HERDR_MAX_CONSECUTIVE_RELEASES: u32 = 10;
const HERDR_REQUEST_BUDGET: Duration = Duration::from_secs(5);

pub(crate) fn herdr_request_deadline() -> RequestDeadline {
    RequestDeadline::after(HERDR_REQUEST_BUDGET)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct HerdrQueueWakeStats {
    pub listed_sessions: usize,
    pub pending_members: usize,
    pub idle_members: usize,
    pub prompted: usize,
    pub released: usize,
    pub breaker_open: usize,
    pub not_present: usize,
    pub task_reminders: usize,
    pub task_reminders_failed: usize,
    pub task_reminders_unrenderable: usize,
    pub lead_notifications: usize,
    pub blocked_escalations: usize,
    pub escalation_writes_failed: usize,
    pub task_step_skipped: bool,
    pub last_tick_at: Option<IsoTimestamp>,
}

#[derive(Clone)]
pub struct HerdrQueueWakePump {
    pub(crate) service_runtime: LocalServiceRuntime,
    selector: Arc<dyn MessageReceivedHookSelector>,
    runtime_health: RuntimeHealth,
    pub(crate) herdr_process: Arc<dyn HerdrProcessAdapter>,
    cursor: Arc<Mutex<usize>>,
    release_streaks: Arc<Mutex<HashMap<MemberKey, u32>>>,
    absence_started_at: Arc<Mutex<HashMap<MemberKey, IsoTimestamp>>>,
    clock: Arc<dyn Fn() -> IsoTimestamp + Send + Sync>,
    pub(crate) escalation_state: EscalationState,
    pub(crate) daemon_home: PathBuf,
    task_step_available: Arc<Mutex<Option<bool>>>,
    last_stats: Arc<Mutex<HerdrQueueWakeStats>>,
    release_handles: Arc<Mutex<Vec<JoinHandle<()>>>>,
    pub(crate) blocking_bridge: BoundedBlockingBridge,
    #[cfg(test)]
    pub(crate) handoff_cleanup_test_gate:
        Arc<Mutex<Option<crate::herdr_queue_wake_test_gates::Gate>>>,
    #[cfg(test)]
    pub(crate) prompt_started_test_gate: Arc<Mutex<Option<Arc<tokio::sync::Notify>>>>,
}

impl HerdrQueueWakePump {
    #[must_use]
    pub fn new(
        service_runtime: LocalServiceRuntime,
        selector: Arc<dyn MessageReceivedHookSelector>,
        runtime_health: RuntimeHealth,
        herdr_process: Arc<dyn HerdrProcessAdapter>,
    ) -> Self {
        Self {
            service_runtime,
            selector,
            runtime_health: runtime_health.clone(),
            herdr_process,
            cursor: Arc::new(Mutex::new(0)),
            release_streaks: Arc::new(Mutex::new(HashMap::new())),
            absence_started_at: Arc::new(Mutex::new(HashMap::new())),
            clock: Arc::new(IsoTimestamp::now),
            escalation_state: EscalationState::default(),
            daemon_home: PathBuf::new(),
            task_step_available: Arc::new(Mutex::new(None)),
            last_stats: Arc::new(Mutex::new(HerdrQueueWakeStats::default())),
            release_handles: Arc::new(Mutex::new(Vec::new())),
            blocking_bridge: BoundedBlockingBridge::new(
                NonZeroUsize::new(HERDR_MAX_PROMPTS_PER_TICK)
                    .expect("Herdr blocking capacity is non-zero"),
                runtime_health,
            ),
            #[cfg(test)]
            handoff_cleanup_test_gate: Arc::new(Mutex::new(None)),
            #[cfg(test)]
            prompt_started_test_gate: Arc::new(Mutex::new(None)),
        }
    }

    /// Supplies the daemon home used by daemon-originated escalation mail.
    #[must_use]
    pub fn with_daemon_home(mut self, daemon_home: PathBuf) -> Self {
        self.daemon_home = daemon_home;
        self
    }

    #[cfg(test)]
    #[must_use]
    fn with_clock(mut self, clock: Arc<dyn Fn() -> IsoTimestamp + Send + Sync>) -> Self {
        self.clock = clock;
        self
    }

    /// Starts the single polling task. The task owns no per-member workers.
    pub fn start(self: Arc<Self>, mut shutdown: watch::Receiver<()>) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(HERDR_POLL_INTERVAL_MS));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    changed = shutdown.changed() => {
                        let _ = changed;
                        break;
                    }
                    _ = interval.tick() => {
                        tokio::select! {
                            changed = shutdown.changed() => {
                                let _ = changed;
                                break;
                            }
                            _ = self.tick_once() => {}
                        }
                    }
                }
            }
            self.await_release_handles(herdr_request_deadline()).await;
        })
    }

    async fn await_release_handles(&self, deadline: RequestDeadline) {
        loop {
            let handles = std::mem::take(
                &mut *self
                    .release_handles
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
            );
            if handles.is_empty() {
                return;
            }
            for mut handle in handles {
                let Some(remaining) = deadline.remaining() else {
                    handle.abort();
                    tracing::warn!(
                        subsystem = "herdr_queue_wake",
                        action = "queue_claim_release_shutdown",
                        outcome = "deadline_exceeded",
                        "Herdr queue release drain exceeded its shutdown deadline"
                    );
                    continue;
                };
                if tokio::time::timeout(remaining, &mut handle).await.is_err() {
                    handle.abort();
                    tracing::warn!(
                        subsystem = "herdr_queue_wake",
                        action = "queue_claim_release_shutdown",
                        outcome = "deadline_exceeded",
                        "Herdr queue release drain exceeded its shutdown deadline"
                    );
                }
            }
        }
    }

    fn prune_finished_release_handles(&self) {
        self.release_handles
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .retain(|handle| !handle.is_finished());
    }

    /// Runs one complete roster/list/claim/dispatch pass.
    pub async fn tick_once(&self) {
        self.prune_finished_release_handles();
        let mut stats = HerdrQueueWakeStats {
            last_tick_at: Some((self.clock)()),
            ..HerdrQueueWakeStats::default()
        };
        let pending_store = match self.service_runtime.pending_nudge_store() {
            Ok(store) => store,
            Err(error) => {
                tracing::warn!(
                    subsystem = "herdr_queue_wake",
                    action = "pending_store_resolve",
                    outcome = "unavailable",
                    error = %error,
                    "Herdr queue wake skipped: pending store unavailable"
                );
                self.save_stats(stats);
                return;
            }
        };
        let roster_store = self.service_runtime.shared_roster_store_arc();
        let pending_members = match self.list_pending_members(&pending_store).await {
            Ok(members) => members,
            Err(error) => {
                tracing::warn!(
                    subsystem = "herdr_queue_wake",
                    action = "pending_member_list",
                    outcome = "failed",
                    error = %error,
                    "Herdr queue wake skipped: pending roster unavailable"
                );
                self.save_stats(stats);
                return;
            }
        };
        stats.pending_members = pending_members.len();
        let pending_set: HashSet<_> = pending_members.into_iter().collect();
        let candidates = match self
            .load_candidates(roster_store, pending_set.clone())
            .await
        {
            Ok(candidates) => candidates,
            Err(error) => {
                tracing::warn!(
                    subsystem = "herdr_queue_wake",
                    action = "roster_candidates",
                    outcome = "failed",
                    error = %error,
                    "Herdr queue wake skipped: roster unavailable"
                );
                self.save_stats(stats);
                return;
            }
        };
        self.prune_member_state(&candidates);

        let (mut eligible, task_candidates) = self.list_eligible(candidates, &mut stats).await;
        let prepared_task_pass = self.prepare_task_pass(&mut stats, &task_candidates).await;
        if let Some(prepared) = prepared_task_pass.as_ref() {
            eligible.retain(|candidate| prepared.queue_drain_allowed(&candidate.key));
        }
        self.drain_eligible(pending_store, eligible, &mut stats)
            .await;
        if let Some(prepared_task_pass) = prepared_task_pass {
            self.remind_open_tasks(
                prepared_task_pass,
                task_candidates,
                &pending_set,
                &mut stats,
            )
            .await;
        }
        self.finish_tick(stats);
    }

    async fn list_pending_members(
        &self,
        pending_store: &Arc<dyn PendingNudgeStore + Send + Sync>,
    ) -> Result<Vec<MemberKey>, AtmError> {
        self.blocking_bridge
            .run(herdr_request_deadline(), {
                let pending_store = Arc::clone(pending_store);
                move || pending_store.list_pending_members()
            })
            .await
    }

    async fn load_candidates(
        &self,
        roster_store: Arc<dyn DurableRosterStore + Send + Sync>,
        pending_set: HashSet<MemberKey>,
    ) -> Result<Vec<HerdrCandidate>, AtmError> {
        self.blocking_bridge
            .run(herdr_request_deadline(), move || {
                herdr_candidates(roster_store.as_ref(), &pending_set)
            })
            .await
    }

    fn finish_tick(&self, stats: HerdrQueueWakeStats) {
        self.save_stats(stats.clone());
        tracing::info!(
            event = "herdr_queue_poll_tick",
            listed_sessions = stats.listed_sessions,
            pending_members = stats.pending_members,
            idle_members = stats.idle_members,
            prompted = stats.prompted,
            released = stats.released,
            breaker_open = stats.breaker_open,
            task_reminders = stats.task_reminders,
            task_reminders_failed = stats.task_reminders_failed,
            task_reminders_unrenderable = stats.task_reminders_unrenderable,
            lead_notifications = stats.lead_notifications,
            blocked_escalations = stats.blocked_escalations,
            escalation_writes_failed = stats.escalation_writes_failed,
            task_step_skipped = stats.task_step_skipped,
            "Herdr queue wake poll tick"
        );
    }

    async fn list_eligible(
        &self,
        candidates: Vec<HerdrCandidate>,
        stats: &mut HerdrQueueWakeStats,
    ) -> (Vec<HerdrCandidate>, Vec<MemberObservation>) {
        let mut by_session: HashMap<Option<HerdrSession>, Vec<HerdrCandidate>> = HashMap::new();
        let mut eligible = Vec::new();
        let mut task_candidates = Vec::new();
        for candidate in candidates {
            match &candidate.target {
                CandidateTarget::Herdr(target) => {
                    by_session
                        .entry(target.session.clone())
                        .or_default()
                        .push(candidate);
                }
                CandidateTarget::RosterOnly => {
                    if let Some(record) = self
                        .service_runtime
                        .roster_ephemeral_state(candidate.key.team(), candidate.key.agent())
                    {
                        task_candidates.push(MemberObservation {
                            member: candidate.key.clone(),
                            state: record.runtime.state,
                            state_changed_at: record.runtime.state_changed_at,
                        });
                        if candidate.pending
                            && queue_drain_eligible(&MemberObservation {
                                member: candidate.key.clone(),
                                state: record.runtime.state,
                                state_changed_at: record.runtime.state_changed_at,
                            })
                        {
                            eligible.push(candidate);
                        }
                    }
                }
            }
        }
        for (session, members) in by_session {
            stats.listed_sessions += 1;
            match self
                .herdr_process
                .list(session.as_ref(), herdr_request_deadline())
                .await
            {
                Ok(outcome) => {
                    self.collect_idle_members(
                        outcome.agents,
                        members,
                        (self.clock)(),
                        stats,
                        &mut eligible,
                        &mut task_candidates,
                    );
                }
                Err(error) => {
                    self.record_unavailable_members(&members, (self.clock)());
                    if error.is_infrastructure() {
                        stats.breaker_open += 1;
                    }
                    log_herdr_list_failure(&session, &error);
                }
            }
        }
        eligible.sort_by(|left, right| member_order(&left.key, &right.key));
        task_candidates.sort_by(|left, right| member_order(&left.member, &right.member));
        (eligible, task_candidates)
    }

    fn apply_herdr_observations(
        &self,
        snapshots: &HashMap<&str, &AgentSnapshot>,
        members: &[HerdrCandidate],
        observed_at: IsoTimestamp,
    ) -> HashMap<MemberKey, RosterRuntimeObservation> {
        let mut updates_by_team = HashMap::new();
        for member in members {
            let CandidateTarget::Herdr(target) = &member.target else {
                continue;
            };
            let mut absences = self
                .absence_started_at
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let state = runtime_state_with_absence(
                &mut absences,
                &member.key,
                snapshots
                    .get(target.agent.as_str())
                    .map(|snapshot| snapshot.status),
                observed_at,
            );
            updates_by_team
                .entry(member.key.team().clone())
                .or_insert_with(Vec::new)
                .push(RosterRuntimeObservationUpdate::observed(
                    member.key.agent().clone(),
                    state,
                    RuntimeObservationSource::HerdrPoll,
                    observed_at,
                    None,
                ));
        }
        let mut accepted = HashMap::new();
        for (team, updates) in updates_by_team {
            for outcome in self
                .service_runtime
                .apply_roster_runtime_observations(&team, &updates)
            {
                accepted.insert(
                    MemberKey::new(team.clone(), outcome.agent.clone()),
                    outcome.current,
                );
            }
        }
        accepted
    }

    fn record_unavailable_members(&self, members: &[HerdrCandidate], observed_at: IsoTimestamp) {
        let mut updates_by_team = HashMap::new();
        for member in members {
            updates_by_team
                .entry(member.key.team().clone())
                .or_insert_with(Vec::new)
                .push(RosterRuntimeObservationUpdate::unavailable(
                    member.key.agent().clone(),
                    RuntimeObservationSource::HerdrPoll,
                    observed_at,
                ));
        }
        for (team, updates) in updates_by_team {
            let _ = self
                .service_runtime
                .apply_roster_runtime_observations(&team, &updates);
        }
    }

    async fn drain_eligible(
        &self,
        pending_store: Arc<dyn PendingNudgeStore + Send + Sync>,
        eligible: Vec<HerdrCandidate>,
        stats: &mut HerdrQueueWakeStats,
    ) {
        if eligible.is_empty() {
            return;
        }
        let start = *self
            .cursor
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            % eligible.len();
        let mut visited = 0;
        for offset in 0..eligible.len() {
            if stats.prompted >= HERDR_MAX_PROMPTS_PER_TICK {
                break;
            }
            visited += 1;
            let _ = self
                .process_candidate(
                    &pending_store,
                    &eligible[(start + offset) % eligible.len()],
                    stats,
                )
                .await;
        }
        *self
            .cursor
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = (start + visited) % eligible.len();
    }

    fn prune_member_state(&self, candidates: &[HerdrCandidate]) {
        let members: HashSet<_> = candidates
            .iter()
            .map(|candidate| candidate.key.clone())
            .collect();
        self.release_streaks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .retain(|member, _| members.contains(member));
    }

    async fn process_candidate(
        &self,
        pending_store: &Arc<dyn PendingNudgeStore + Send + Sync>,
        member: &HerdrCandidate,
        stats: &mut HerdrQueueWakeStats,
    ) -> Option<atm_core::schema::AtmMessageId> {
        let claim = self.claim_next_pending(pending_store, member).await?;
        // A claimed nudge is now a real, in-flight Herdr wake attempt for this
        // member: mark the ephemeral roster state pending. `ReleasePendingOnDrop`
        // clears it unconditionally when the attempt concludes below.
        self.service_runtime.set_roster_herdr_wake_pending(
            member.key.team(),
            member.key.agent(),
            true,
        );
        let mut release = ReleasePendingOnDrop::new(
            Arc::clone(pending_store),
            member.key.clone(),
            claim.clone(),
            Arc::clone(&self.release_streaks),
            self.service_runtime.clone(),
            Arc::clone(&self.release_handles),
            self.blocking_bridge.clone(),
        );
        let dispatch = match self.rebuild_dispatch(member, claim.msg).await {
            Ok(Some(dispatch)) => dispatch,
            Ok(None) | Err(_) => {
                release.release_without_input().await;
                stats.released += 1;
                tracing::info!(
                    event = "herdr_queue_poll_outcome",
                    member = %member.key,
                    msg_id = %claim.msg,
                    queue_kind = NudgeKind::Queue.as_str(),
                    outcome = "dispatch_failed_released",
                    "Herdr queue dispatch could not be rebuilt"
                );
                return None;
            }
        };
        if !still_idle(&self.service_runtime, &member.key) {
            release.release_without_input().await;
            stats.released += 1;
            tracing::info!(
                event = "herdr_queue_poll_outcome",
                member = %member.key,
                msg_id = %claim.msg,
                queue_kind = NudgeKind::Queue.as_str(),
                outcome = "held_not_idle",
                "Herdr queue prompt skipped after the live idle recheck"
            );
            return None;
        }
        let Some(emitter) = self.selector.select_emitter(&dispatch) else {
            release.release_without_input().await;
            stats.released += 1;
            tracing::info!(
                event = "herdr_queue_poll_outcome",
                member = %member.key,
                msg_id = %claim.msg,
                queue_kind = NudgeKind::Queue.as_str(),
                outcome = "held_target_not_present",
                "Herdr queue selector returned no emitter"
            );
            return None;
        };
        self.emit_claim(emitter, dispatch, member, claim, &mut release, stats)
            .await
    }

    async fn claim_next_pending(
        &self,
        pending_store: &Arc<dyn PendingNudgeStore + Send + Sync>,
        member: &HerdrCandidate,
    ) -> Option<atm_core::boundary::NudgeClaim> {
        self.blocking_bridge
            .run(herdr_request_deadline(), {
                let pending_store = Arc::clone(pending_store);
                let member = member.key.clone();
                move || pending_store.claim_next_pending(&member)
            })
            .await
            .ok()
            .flatten()
    }

    async fn rebuild_dispatch(
        &self,
        member: &HerdrCandidate,
        message_id: atm_core::schema::AtmMessageId,
    ) -> Result<Option<atm_core::boundary::BuiltInPostSendDispatch>, AtmError> {
        let runtime = self.service_runtime.clone();
        let member_key = member.key.clone();
        let message = self
            .blocking_bridge
            .run(herdr_request_deadline(), move || {
                load_received_hook_dispatch_message(&runtime, &member_key, message_id)
            })
            .await?;
        let Some(message) = message else {
            return Ok(None);
        };
        rebuild_received_hook_dispatch(
            &self.service_runtime,
            &member.key,
            message_id,
            NudgeKind::Queue,
            &message,
        )
    }

    async fn emit_claim(
        &self,
        emitter: &dyn atm_core::boundary::AsyncMessageReceivedHookEmitter,
        dispatch: atm_core::boundary::BuiltInPostSendDispatch,
        member: &HerdrCandidate,
        claim: atm_core::boundary::NudgeClaim,
        release: &mut ReleasePendingOnDrop,
        stats: &mut HerdrQueueWakeStats,
    ) -> Option<atm_core::schema::AtmMessageId> {
        #[cfg(test)]
        self.notify_prompt_started_test_gate();
        match emitter
            .emit_received_message(dispatch, herdr_request_deadline())
            .await
        {
            Ok(_) => {
                let message_id = claim.msg;
                self.complete_successful_claim(member, claim, release, stats)
                    .await;
                Some(message_id)
            }
            Err(error) => {
                if error.code() == AtmErrorCode::HerdrUnavailable {
                    stats.breaker_open += 1;
                }
                let outcome = match error.code() {
                    AtmErrorCode::HerdrPromptFailed => {
                        release.requeue().await;
                        "dispatch_failed_requeued"
                    }
                    AtmErrorCode::HerdrAgentNotVisible => {
                        release.release_without_input().await;
                        "held_target_not_present"
                    }
                    AtmErrorCode::PostSendHerdrPromptFailed => {
                        release.release_without_input().await;
                        "blocked_before_input_released"
                    }
                    _ => {
                        release.release_without_input().await;
                        "dispatch_failed_released"
                    }
                };
                stats.released += 1;
                tracing::info!(
                    event = "herdr_queue_poll_outcome",
                    member = %member.key,
                    msg_id = %claim.msg,
                    queue_kind = NudgeKind::Queue.as_str(),
                    outcome,
                    error_code = ?error.code(),
                    "Herdr queue prompt failed"
                );
                None
            }
        }
    }

    async fn complete_successful_claim(
        &self,
        member: &HerdrCandidate,
        claim: atm_core::boundary::NudgeClaim,
        release: &mut ReleasePendingOnDrop,
        stats: &mut HerdrQueueWakeStats,
    ) {
        // Herdr has accepted the prompt. Disarm before any cleanup await so
        // cancellation cannot re-release an already delivered claim.
        release.disarm();
        let runtime = self.service_runtime.clone();
        let member_key = member.key.clone();
        let message_id = claim.msg;
        let now = (self.clock)();
        let next_due = atm_core::boundary::next_reminder_due(now);
        let health = self.runtime_health.clone();
        let _ = self
            .blocking_bridge
            .run(herdr_request_deadline(), move || {
                atm_core::nudge_dispatch::rearm_queue_marker_after_handoff(
                    &runtime,
                    &member_key,
                    &message_id,
                    next_due,
                    || health.record_graft_queue_marker_clear_failure(),
                );
                Ok(())
            })
            .await;
        #[cfg(test)]
        self.await_handoff_cleanup_test_gate().await;
        self.reset_release_streak(&member.key);
        stats.prompted += 1;
        tracing::info!(
            event = "herdr_queue_poll_outcome",
            member = %member.key,
            msg_id = %claim.msg,
            queue_kind = NudgeKind::Queue.as_str(),
            outcome = "prompted",
            "Herdr queue prompt accepted"
        );
    }

    #[must_use]
    #[cfg(test)]
    fn stats(&self) -> HerdrQueueWakeStats {
        self.last_stats
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    #[cfg(test)]
    fn cursor_position(&self) -> usize {
        *self
            .cursor
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[cfg(test)]
    fn release_streak_for(&self, member: &MemberKey) -> u32 {
        self.release_streaks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(member)
            .copied()
            .unwrap_or_default()
    }

    fn save_stats(&self, stats: HerdrQueueWakeStats) {
        self.runtime_health
            .record_herdr_queue_tick(stats.last_tick_at);
        *self
            .last_stats
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = stats;
    }

    fn reset_release_streak(&self, member: &MemberKey) {
        self.release_streaks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(member);
    }
}

fn log_herdr_list_failure(session: &Option<HerdrSession>, error: &atm_herdr::HerdrError) {
    let code = AtmError::from(error.clone()).code();
    tracing::warn!(
        subsystem = "herdr_queue_wake",
        action = "herdr_list",
        outcome = "failed",
        session = ?session,
        code = %code,
        failure_class = error.diagnostic_name(),
        error = ?error,
        "Herdr queue wake list failed: {code}"
    );
}

impl crate::RuntimeMaintenance for HerdrQueueWakePump {
    fn start(&self, shutdown: watch::Receiver<()>) -> JoinHandle<()> {
        Arc::new(self.clone()).start(shutdown)
    }
}

#[derive(Clone)]
struct HerdrCandidate {
    key: MemberKey,
    pending: bool,
    target: CandidateTarget,
}

#[derive(Clone)]
enum CandidateTarget {
    Herdr(HerdrTarget),
    RosterOnly,
}

#[derive(Clone)]
struct HerdrTarget {
    agent: HerdrAgentName,
    session: Option<HerdrSession>,
}

/// One accepted runtime observation. The task-disposition pass consumes this
/// rather than making eligibility decisions from a poll snapshot.
#[derive(Clone)]
struct MemberObservation {
    member: MemberKey,
    state: RuntimeMemberState,
    state_changed_at: Option<IsoTimestamp>,
}

fn herdr_candidates(
    roster_store: &dyn DurableRosterStore,
    pending: &HashSet<MemberKey>,
) -> Result<Vec<HerdrCandidate>, AtmError> {
    let mut candidates = Vec::new();
    let mut teams = roster_store.list_teams()?;
    teams.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    for team in teams {
        let roster = roster_store.load_roster(&team)?;
        for member in roster.members {
            let key = MemberKey::new(member.team_name.clone(), member.agent_name.clone());
            let target = match local_message_received_backend(&member) {
                Some(atm_core::delivery_channel::LocalMessageReceivedBackend::Herdr {
                    session,
                    agent,
                }) => {
                    let Some(agent) = atm_core::delivery_channel::resolve_herdr_agent_target(
                        key.agent(),
                        agent,
                        "herdr_queue_wake",
                    ) else {
                        continue;
                    };
                    CandidateTarget::Herdr(HerdrTarget { agent, session })
                }
                Some(atm_core::delivery_channel::LocalMessageReceivedBackend::Tmux { .. })
                | None => CandidateTarget::RosterOnly,
            };
            candidates.push(HerdrCandidate {
                pending: pending.contains(&key),
                key,
                target,
            });
        }
    }
    candidates.sort_by(|left, right| member_order(&left.key, &right.key));
    Ok(candidates)
}

fn member_order(left: &MemberKey, right: &MemberKey) -> std::cmp::Ordering {
    left.team()
        .as_str()
        .cmp(right.team().as_str())
        .then_with(|| left.agent().as_str().cmp(right.agent().as_str()))
}

fn still_idle(runtime: &LocalServiceRuntime, member: &MemberKey) -> bool {
    runtime
        .roster_ephemeral_state(member.team(), member.agent())
        .is_some_and(|state| state.runtime.state == RuntimeMemberState::Idle)
}

#[cfg(test)]
mod tests;
