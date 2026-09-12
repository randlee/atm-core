//! Tokio-owned polling pump for deferred Herdr queue nudges.

#[path = "herdr_queue_wake_reminders.rs"]
mod reminders;

use std::collections::{HashMap, HashSet};
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
    RosterRuntimeObservationUpdate, RuntimeMemberState, RuntimeObservationSource,
};
use atm_core::types::IsoTimestamp;
use atm_herdr::{AgentSnapshot, HerdrAgentStatus, HerdrProcessAdapter};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::herdr_escalation::EscalationState;
use crate::runtime_health::RuntimeHealth;

/// Poll cadence required by AQ2.7.
pub const HERDR_POLL_INTERVAL_MS: u64 = 5_000;
/// Maximum number of prompts admitted by one poll tick.
pub const HERDR_MAX_PROMPTS_PER_TICK: usize = 16;
/// Consecutive no-input releases before one retry-budget attempt is spent.
pub const HERDR_MAX_CONSECUTIVE_RELEASES: u32 = 10;
const HERDR_REQUEST_DEADLINE: Duration = Duration::from_secs(5);

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
    pub task_reminders_blocked: usize,
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
    clock: Arc<dyn Fn() -> IsoTimestamp + Send + Sync>,
    pub(crate) escalation_state: EscalationState,
    pub(crate) daemon_home: PathBuf,
    task_step_available: Arc<Mutex<Option<bool>>>,
    last_stats: Arc<Mutex<HerdrQueueWakeStats>>,
    release_handles: Arc<Mutex<Vec<JoinHandle<()>>>>,
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
            runtime_health,
            herdr_process,
            cursor: Arc::new(Mutex::new(0)),
            release_streaks: Arc::new(Mutex::new(HashMap::new())),
            clock: Arc::new(IsoTimestamp::now),
            escalation_state: EscalationState::default(),
            daemon_home: PathBuf::new(),
            task_step_available: Arc::new(Mutex::new(None)),
            last_stats: Arc::new(Mutex::new(HerdrQueueWakeStats::default())),
            release_handles: Arc::new(Mutex::new(Vec::new())),
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
            self.await_release_handles().await;
        })
    }

    async fn await_release_handles(&self) {
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
            for handle in handles {
                let _ = handle.await;
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
        let pending_members = match run_blocking({
            let pending_store = Arc::clone(&pending_store);
            move || pending_store.list_pending_members()
        })
        .await
        {
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
        let candidates = match run_blocking({
            let pending_set = pending_set.clone();
            move || herdr_candidates(roster_store.as_ref(), &pending_set)
        })
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

        let (eligible, task_candidates, list_complete) =
            self.list_eligible(candidates, &mut stats).await;
        self.drain_eligible(pending_store, eligible, &mut stats)
            .await;
        self.remind_open_tasks(task_candidates, &pending_set, list_complete, &mut stats)
            .await;
        self.finish_tick(stats);
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
            task_reminders_blocked = stats.task_reminders_blocked,
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
    ) -> (Vec<HerdrCandidate>, Vec<MemberObservation>, bool) {
        let mut by_session: HashMap<Option<HerdrSession>, Vec<HerdrCandidate>> = HashMap::new();
        let mut eligible = Vec::new();
        let mut task_candidates = Vec::new();
        let mut complete = true;
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
                        if candidate.pending && record.runtime.state == RuntimeMemberState::Idle {
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
                .list(
                    session.as_ref(),
                    RequestDeadline::after(HERDR_REQUEST_DEADLINE),
                )
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
                    complete = false;
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
        (eligible, task_candidates, complete)
    }

    fn collect_idle_members(
        &self,
        agents: Vec<AgentSnapshot>,
        members: Vec<HerdrCandidate>,
        observed_at: IsoTimestamp,
        stats: &mut HerdrQueueWakeStats,
        eligible: &mut Vec<HerdrCandidate>,
        task_candidates: &mut Vec<MemberObservation>,
    ) {
        let snapshots: HashMap<&str, &AgentSnapshot> = agents
            .iter()
            .filter_map(|snapshot| snapshot.name.as_deref().map(|name| (name, snapshot)))
            .collect();
        let mut updates_by_team = HashMap::new();
        for member in &members {
            let CandidateTarget::Herdr(target) = &member.target else {
                continue;
            };
            let state = runtime_state(
                snapshots
                    .get(target.agent.as_str())
                    .map(|snapshot| snapshot.status),
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
        for member in members {
            let CandidateTarget::Herdr(target) = &member.target else {
                continue;
            };
            if !snapshots.contains_key(target.agent.as_str()) {
                if member.pending {
                    stats.not_present += 1;
                    tracing::info!(
                        event = "herdr_queue_poll_outcome",
                        member = %member.key,
                        herdr_agent = %target.agent,
                        queue_kind = NudgeKind::Queue.as_str(),
                        outcome = "held_target_not_present",
                        "Herdr queue target was absent from the poll result"
                    );
                }
                continue;
            }
            let Some(observation) = accepted.get(&member.key) else {
                continue;
            };
            task_candidates.push(MemberObservation {
                member: member.key.clone(),
                state: observation.state,
                state_changed_at: observation.state_changed_at,
            });
            if member.pending && observation.state == RuntimeMemberState::Idle {
                stats.idle_members += 1;
                eligible.push(member);
            }
        }
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
    ) -> HashSet<MemberKey> {
        let mut prompted = HashSet::new();
        if eligible.is_empty() {
            return prompted;
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
            if self
                .process_candidate(
                    &pending_store,
                    &eligible[(start + offset) % eligible.len()],
                    stats,
                )
                .await
            {
                prompted.insert(eligible[(start + offset) % eligible.len()].key.clone());
            }
        }
        *self
            .cursor
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = (start + visited) % eligible.len();
        prompted
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
    ) -> bool {
        let claim = match run_blocking({
            let pending_store = Arc::clone(pending_store);
            let member = member.key.clone();
            move || pending_store.claim_next_pending(&member)
        })
        .await
        {
            Ok(Some(claim)) => claim,
            Ok(None) | Err(_) => return false,
        };
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
                return false;
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
            return false;
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
            return false;
        };
        self.emit_claim(emitter, dispatch, member, claim, &mut release, stats)
            .await
    }

    async fn rebuild_dispatch(
        &self,
        member: &HerdrCandidate,
        message_id: atm_core::schema::AtmMessageId,
    ) -> Result<Option<atm_core::boundary::BuiltInPostSendDispatch>, AtmError> {
        let runtime = self.service_runtime.clone();
        let member_key = member.key.clone();
        let message = run_blocking(move || {
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
    ) -> bool {
        #[cfg(test)]
        self.notify_prompt_started_test_gate();
        match emitter
            .emit_received_message(dispatch, RequestDeadline::after(HERDR_REQUEST_DEADLINE))
            .await
        {
            Ok(_) => {
                self.complete_successful_claim(member, claim, release, stats)
                    .await;
                true
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
                false
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
        let health = self.runtime_health.clone();
        let _ = run_blocking(move || {
            atm_core::nudge_dispatch::clear_queue_marker_after_handoff(
                &runtime,
                &member_key,
                &message_id,
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

pub(crate) async fn run_blocking<T, F>(job: F) -> Result<T, AtmError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, AtmError> + Send + 'static,
{
    tokio::task::spawn_blocking(job).await.map_err(|source| {
        AtmError::new(
            AtmErrorCode::InternalError,
            "Herdr queue wake blocking operation ended unexpectedly",
        )
        .with_cause(source)
    })?
}

fn member_order(left: &MemberKey, right: &MemberKey) -> std::cmp::Ordering {
    left.team()
        .as_str()
        .cmp(right.team().as_str())
        .then_with(|| left.agent().as_str().cmp(right.agent().as_str()))
}

fn runtime_state(status: Option<HerdrAgentStatus>) -> RuntimeMemberState {
    match status {
        None => RuntimeMemberState::Unknown,
        Some(status) => match status {
            HerdrAgentStatus::Idle | HerdrAgentStatus::Done => RuntimeMemberState::Idle,
            HerdrAgentStatus::Working => RuntimeMemberState::Active,
            HerdrAgentStatus::Blocked => RuntimeMemberState::Blocked,
            HerdrAgentStatus::Unknown => RuntimeMemberState::Unknown,
        },
    }
}

fn still_idle(runtime: &LocalServiceRuntime, member: &MemberKey) -> bool {
    runtime
        .roster_ephemeral_state(member.team(), member.agent())
        .is_some_and(|state| state.runtime.state == RuntimeMemberState::Idle)
}

struct ReleasePendingOnDrop {
    store: Arc<dyn PendingNudgeStore + Send + Sync>,
    member: MemberKey,
    claim: atm_core::boundary::NudgeClaim,
    release_streaks: Arc<Mutex<HashMap<MemberKey, u32>>>,
    release_handles: Arc<Mutex<Vec<JoinHandle<()>>>>,
    /// Clears the member's ephemeral Herdr wake-pending roster flag when this
    /// claim attempt concludes (success, requeue, or release), regardless of
    /// which exit path was taken. Set alongside claiming a pending nudge in
    /// [`HerdrQueueWakePump::process_candidate`]; this is the real production
    /// transition the ephemeral roster state exists to track (FTQ-AW finding
    /// 4 on PR #1240 -- previously this state had no production caller).
    service_runtime: LocalServiceRuntime,
    armed: bool,
}

impl ReleasePendingOnDrop {
    fn new(
        store: Arc<dyn PendingNudgeStore + Send + Sync>,
        member: MemberKey,
        claim: atm_core::boundary::NudgeClaim,
        release_streaks: Arc<Mutex<HashMap<MemberKey, u32>>>,
        service_runtime: LocalServiceRuntime,
        release_handles: Arc<Mutex<Vec<JoinHandle<()>>>>,
    ) -> Self {
        Self {
            store,
            member,
            claim,
            release_streaks,
            release_handles,
            service_runtime,
            armed: true,
        }
    }

    async fn release_without_input(&mut self) {
        if !self.armed {
            return;
        }
        let should_requeue = self.claim_release_action();
        self.armed = false;
        let store = Arc::clone(&self.store);
        let member = self.member.clone();
        let claim = self.claim.clone();
        if let Err(error) = run_blocking(move || {
            if should_requeue {
                store.requeue_pending(&member, &claim)
            } else {
                store.release_pending(&member, &claim)
            }
        })
        .await
        {
            tracing::warn!(
                subsystem = "herdr_queue_wake",
                action = "queue_claim_release",
                outcome = "failed",
                error = %error,
                member = %self.member,
                "failed to resolve Herdr queue claim"
            );
        }
    }

    async fn requeue(&mut self) {
        if !self.armed {
            return;
        }
        self.release_streaks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&self.member);
        self.armed = false;
        let store = Arc::clone(&self.store);
        let member = self.member.clone();
        let claim = self.claim.clone();
        if let Err(error) = run_blocking(move || store.requeue_pending(&member, &claim)).await {
            tracing::warn!(
                subsystem = "herdr_queue_wake",
                action = "queue_claim_requeue",
                outcome = "failed",
                error = %error,
                member = %self.member,
                "failed to requeue Herdr queue claim"
            );
        }
    }

    fn claim_release_action(&self) -> bool {
        let mut streaks = self
            .release_streaks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let streak = streaks.entry(self.member.clone()).or_default();
        if *streak >= HERDR_MAX_CONSECUTIVE_RELEASES {
            streaks.remove(&self.member);
            true
        } else {
            *streak = streak.saturating_add(1);
            false
        }
    }

    fn release_in_drop(&mut self) {
        if !self.armed {
            return;
        }
        let should_requeue = self.claim_release_action();
        self.armed = false;
        let store = Arc::clone(&self.store);
        let member = self.member.clone();
        let claim = self.claim.clone();
        let release = move || {
            let result = if should_requeue {
                store.requeue_pending(&member, &claim)
            } else {
                store.release_pending(&member, &claim)
            };
            if let Err(error) = result {
                tracing::warn!(
                    subsystem = "herdr_queue_wake",
                    action = "queue_claim_release",
                    outcome = "failed",
                    error = %error,
                    member = %member,
                    "failed to resolve Herdr queue claim during drop"
                );
            }
        };
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let release_handle = handle.spawn_blocking(release);
            let mut release_handles = self
                .release_handles
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            release_handles.retain(|handle| !handle.is_finished());
            release_handles.push(release_handle);
        } else {
            release();
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ReleasePendingOnDrop {
    fn drop(&mut self) {
        self.release_in_drop();
        self.service_runtime.set_roster_herdr_wake_pending(
            self.member.team(),
            self.member.agent(),
            false,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{
        HERDR_MAX_CONSECUTIVE_RELEASES, HERDR_MAX_PROMPTS_PER_TICK, HERDR_POLL_INTERVAL_MS,
        HerdrQueueWakePump, ReleasePendingOnDrop, log_herdr_list_failure, runtime_state,
    };
    use atm_core::LocalServiceRuntime;
    use atm_core::ack::{AckRequest, ack_mail_with_runtime};
    use atm_core::api::RequestDeadline;
    use atm_core::boundary::{
        AsyncMessageReceivedHookEmitter, BuiltInPostSendDispatch, MessageReceivedHookSelector,
        PostSendEmissionPath, RosterEntry, RosterHarness, RosterMemberKind,
    };
    use atm_core::error::{AtmError, AtmErrorCode};
    use atm_core::observability::NullObservability;
    use atm_core::protocol::{RuntimeMemberState, RuntimeObservationAvailability};
    use atm_core::schema::AtmMessageId;
    use atm_core::send::{NudgeMode, SendMessageSource, WriteRequest, write_mail_with_runtime};
    use atm_core::test_support as atm_storage;
    use atm_core::types::{IsoTimestamp, ModelName, TaskId, TeamName};
    use atm_herdr::{
        AgentSnapshot, HerdrAgentStatus, HerdrListOutcome, HerdrProcessAdapter, HerdrPromptOutcome,
    };
    use atm_observability::{
        DiagnosticSink, RetainedEvent, RetainedLogPolicy, SinkOffer, TracingBridgeLayer,
        build_retained_logger,
    };
    use atm_runtime_test_support::open_isolated_sqlite_boundary;
    use atm_storage::{RosterSnapshot, TaskRow, TaskState};
    use serde_json::json;
    use std::collections::HashMap;
    use std::future::Future;
    use std::pin::Pin;
    use std::str::FromStr;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::sync::watch;

    use tracing_subscriber::prelude::*;

    #[derive(Default)]
    struct RecordingDiagnosticSink {
        codes: Mutex<Vec<String>>,
    }

    impl DiagnosticSink for RecordingDiagnosticSink {
        fn offer(&self, event: &RetainedEvent<'_>) -> SinkOffer {
            if let Some(code) = event.code {
                self.codes.lock().expect("codes").push(code.to_owned());
            }
            SinkOffer::Accepted
        }
    }

    #[test]
    fn herdr_list_failure_reaches_the_tracing_bridge_with_its_error_code() {
        let root = tempfile::tempdir().expect("tempdir");
        let logger = Arc::new(
            build_retained_logger(
                "atm",
                &root.path().join("logs"),
                RetainedLogPolicy {
                    rotation_max_bytes: 1_024 * 1_024,
                    rotation_max_files: 2,
                    retention_max_age: Duration::from_secs(60),
                    maintenance_cadence: Duration::from_secs(60),
                    writer_shutdown_timeout: Duration::from_secs(1),
                    maintenance_max_work_per_pass: Some(2),
                },
                None,
            )
            .expect("logger"),
        );
        let bridge = TracingBridgeLayer::new(logger);
        let sink = Arc::new(RecordingDiagnosticSink::default());
        bridge.set_diagnostic_sink(sink.clone());

        tracing::subscriber::with_default(
            tracing_subscriber::Registry::default().with(bridge),
            || {
                log_herdr_list_failure(&None, &atm_herdr::HerdrError::ServerNotRunning);
            },
        );

        assert_eq!(
            sink.codes.lock().expect("codes").as_slice(),
            ["ATM_HERDR_UNAVAILABLE"]
        );
    }

    struct FakeSelector {
        emitter: FakeEmitter,
    }

    struct FakeEmitter {
        process: Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
    }

    struct NoEmitterSelector;

    impl atm_core::boundary::sealed::Sealed for FakeSelector {}
    impl atm_core::boundary::sealed::Sealed for FakeEmitter {}
    impl atm_core::boundary::sealed::Sealed for NoEmitterSelector {}

    impl MessageReceivedHookSelector for FakeSelector {
        fn select_emitter(
            &self,
            _dispatch: &BuiltInPostSendDispatch,
        ) -> Option<&dyn AsyncMessageReceivedHookEmitter> {
            Some(&self.emitter)
        }
    }

    impl MessageReceivedHookSelector for NoEmitterSelector {
        fn select_emitter(
            &self,
            _dispatch: &BuiltInPostSendDispatch,
        ) -> Option<&dyn AsyncMessageReceivedHookEmitter> {
            None
        }
    }

    impl AsyncMessageReceivedHookEmitter for FakeEmitter {
        fn emit_received_message(
            &self,
            dispatch: BuiltInPostSendDispatch,
            deadline: RequestDeadline,
        ) -> Pin<Box<dyn Future<Output = Result<PostSendEmissionPath, AtmError>> + Send + '_>>
        {
            let process = Arc::clone(&self.process);
            Box::pin(async move {
                let target = match dispatch.target {
                    atm_core::boundary::PostSendBuiltInTarget::LocalSteer(
                        atm_core::boundary::LocalSteerTarget::Herdr(target),
                    ) => target,
                    _ => {
                        return Err(AtmError::new(
                            AtmErrorCode::InternalError,
                            "test dispatch was not Herdr",
                        ));
                    }
                };
                process
                    .prompt(
                        &target.agent,
                        target.session.as_ref(),
                        &target.rendered_nudge,
                        deadline,
                    )
                    .await
                    .map(|HerdrPromptOutcome::Accepted(_)| PostSendEmissionPath::LocalHerdr)
                    .map_err(Into::into)
            })
        }
    }

    fn herdr_member_with_session(team: &TeamName, agent: &str, session: &str) -> RosterEntry {
        RosterEntry {
            team_name: team.clone(),
            agent_name: agent.parse().expect("agent"),
            member_kind: RosterMemberKind::Permanent,
            harness: RosterHarness::CodexCli,
            agent_type: atm_storage::AgentType::default(),
            model: ModelName::default(),
            recipient_pane_id: None,
            metadata_json: {
                let mut metadata = atm_core::delivery_channel::test_backend_type_metadata("herdr");
                metadata.insert("herdrSession".to_owned(), json!(session));
                metadata
            },
        }
    }

    fn herdr_member(team: &TeamName, agent: &str) -> RosterEntry {
        herdr_member_with_session(team, agent, "aq27-test")
    }

    fn herdr_member_with_alias(team: &TeamName, agent: &str, alias: &str) -> RosterEntry {
        let mut member = herdr_member(team, agent);
        member
            .metadata_json
            .insert("alias".to_string(), json!(alias));
        member
    }

    fn queue_message(
        root: &std::path::Path,
        runtime: &LocalServiceRuntime,
        team: &TeamName,
        agent: &str,
    ) -> AtmMessageId {
        let home = root.join("home");
        std::fs::create_dir_all(&home).expect("home");
        let recipient = format!("{agent}@{team}");
        write_mail_with_runtime(
            WriteRequest::new(
                home.clone(),
                home,
                "sender".parse().expect("sender"),
                &recipient,
                team.clone(),
                SendMessageSource::Inline("AQ2.7 test message".to_owned()),
                None,
                false,
                None,
                false,
            )
            .expect("write request")
            .with_nudge_mode(NudgeMode::Deferred),
            &NullObservability,
            runtime,
        )
        .expect("queue write")
        .persisted_message_id()
    }

    fn queue_task_message(
        root: &std::path::Path,
        runtime: &LocalServiceRuntime,
        team: &TeamName,
        agent: &str,
        task_id: TaskId,
    ) -> AtmMessageId {
        let home = root.join("home");
        std::fs::create_dir_all(&home).expect("home");
        let recipient = format!("{agent}@{team}");
        let mut request = WriteRequest::new(
            home.clone(),
            home,
            "sender".parse().expect("sender"),
            &recipient,
            team.clone(),
            SendMessageSource::Inline("AX5 reminder task".to_owned()),
            None,
            true,
            None,
            false,
        )
        .expect("task write request")
        .with_nudge_mode(NudgeMode::Deferred);
        request.task_id = Some(task_id);
        write_mail_with_runtime(request, &NullObservability, runtime)
            .expect("queue task write")
            .persisted_message_id()
    }

    fn pump_with_clock(
        runtime: LocalServiceRuntime,
        fake: Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
        health: super::RuntimeHealth,
        now: Arc<Mutex<IsoTimestamp>>,
    ) -> HerdrQueueWakePump {
        let selector = Arc::new(FakeSelector {
            emitter: FakeEmitter {
                process: Arc::clone(&fake),
            },
        });
        pump_with_selector(runtime, fake, health, now, selector)
    }

    fn pump_with_selector(
        runtime: LocalServiceRuntime,
        fake: Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
        health: super::RuntimeHealth,
        now: Arc<Mutex<IsoTimestamp>>,
        selector: Arc<dyn MessageReceivedHookSelector>,
    ) -> HerdrQueueWakePump {
        let process: Arc<dyn HerdrProcessAdapter> = fake;
        let clock_now = Arc::clone(&now);
        HerdrQueueWakePump::new(runtime, selector, health, process).with_clock(Arc::new(
            move || *clock_now.lock().expect("test clock lock"),
        ))
    }

    fn queue_idle_result(
        fake: &atm_herdr::testing::FakeHerdrProcessAdapter,
        key: &atm_core::boundary::MemberKey,
    ) {
        fake.queue_list_result(Ok(HerdrListOutcome {
            agents: vec![AgentSnapshot {
                name: Some(key.agent().to_string()),
                pane_id: None,
                status: HerdrAgentStatus::Idle,
                workspace_id: None,
            }],
        }));
    }

    fn queue_status_result(
        fake: &atm_herdr::testing::FakeHerdrProcessAdapter,
        keys: &[atm_storage::MemberKey],
        status: HerdrAgentStatus,
    ) {
        fake.queue_list_result(Ok(HerdrListOutcome {
            agents: keys
                .iter()
                .map(|key| AgentSnapshot {
                    name: Some(key.agent().to_string()),
                    pane_id: None,
                    status,
                    workspace_id: None,
                })
                .collect(),
        }));
    }

    fn prompt_texts(fake: &atm_herdr::testing::FakeHerdrProcessAdapter) -> Vec<String> {
        fake.calls()
            .into_iter()
            .filter_map(|call| match call {
                atm_herdr::testing::FakeHerdrCall::Prompt { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    }

    fn clear_pending_markers(runtime: &LocalServiceRuntime, key: &atm_core::boundary::MemberKey) {
        let store = runtime.pending_nudge_store().expect("pending store");
        while let Some(claim) = store.claim_next_pending(key).expect("claim pending marker") {
            store
                .clear_pending_on_handoff(key, &claim.msg)
                .expect("clear pending marker");
        }
    }

    fn ack_task_assignment(
        root: &std::path::Path,
        runtime: &LocalServiceRuntime,
        team: &TeamName,
        message_id: AtmMessageId,
    ) {
        let home = root.join("home");
        ack_mail_with_runtime(
            AckRequest {
                home_dir: home.clone(),
                current_dir: home,
                caller_identity: "aq27-agent".parse().expect("agent"),
                caller_chat_id: None,
                caller_team: team.clone(),
                activity_observation: None,
                message_id,
                reply_body: "acknowledged".to_owned(),
            },
            &NullObservability,
            runtime,
        )
        .expect("task acknowledgement");
    }

    fn complete_task(
        root: &std::path::Path,
        runtime: &LocalServiceRuntime,
        team: &TeamName,
        task_id: TaskId,
    ) {
        let home = root.join("home");
        let request = WriteRequest::new(
            home.clone(),
            home,
            "sender".parse().expect("sender"),
            &format!("aq27-agent@{team}"),
            team.clone(),
            SendMessageSource::Inline("task completed".to_owned()),
            None,
            false,
            None,
            false,
        )
        .expect("completion request")
        .with_nudge_mode(NudgeMode::Immediate)
        .with_task_complete(task_id);
        write_mail_with_runtime(request, &NullObservability, runtime).expect("task completion");
    }

    fn build_test_pump_with_agents(
        agents: Vec<AgentSnapshot>,
    ) -> (
        tempfile::TempDir,
        LocalServiceRuntime,
        Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
        Arc<HerdrQueueWakePump>,
        super::RuntimeHealth,
        atm_core::boundary::MemberKey,
    ) {
        let root = tempfile::tempdir().expect("temporary root");
        let assembly = open_isolated_sqlite_boundary(root.path()).expect("runtime");
        let team: TeamName = "aq27-team".parse().expect("team");
        let roster_agents: Vec<String> = if agents.is_empty() {
            vec!["aq27-agent".to_owned()]
        } else {
            agents
                .iter()
                .filter_map(|snapshot| snapshot.name.clone())
                .collect()
        };
        let members = roster_agents
            .iter()
            .map(|agent| herdr_member(&team, agent))
            .collect();
        assembly
            .service_runtime
            .shared_roster_store_arc()
            .save_roster(&RosterSnapshot {
                team_name: team.clone(),
                members,
                refreshed_at: None,
            })
            .expect("roster");
        for agent in &roster_agents {
            queue_message(root.path(), &assembly.service_runtime, &team, agent);
        }
        let key =
            atm_core::boundary::MemberKey::new(team, roster_agents[0].parse().expect("agent"));
        let fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
        fake.queue_list_result(Ok(HerdrListOutcome { agents }));
        let selector = Arc::new(FakeSelector {
            emitter: FakeEmitter {
                process: Arc::clone(&fake),
            },
        });
        let process: Arc<dyn HerdrProcessAdapter> = fake.clone();
        let health = super::RuntimeHealth::default();
        let pump = Arc::new(HerdrQueueWakePump::new(
            assembly.service_runtime.clone(),
            selector,
            health.clone(),
            process,
        ));
        (root, assembly.service_runtime, fake, pump, health, key)
    }

    fn build_test_pump() -> (
        tempfile::TempDir,
        LocalServiceRuntime,
        Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
        Arc<HerdrQueueWakePump>,
        super::RuntimeHealth,
        atm_core::boundary::MemberKey,
    ) {
        build_test_pump_with_agents(vec![AgentSnapshot {
            name: Some("aq27-agent".to_owned()),
            pane_id: None,
            status: HerdrAgentStatus::Idle,
            workspace_id: None,
        }])
    }

    #[tokio::test]
    async fn shared_herdr_server_prompts_each_team_by_its_roster_alias() {
        let root = tempfile::tempdir().expect("temporary root");
        let assembly = open_isolated_sqlite_boundary(root.path()).expect("runtime");
        let team_a: TeamName = "a-team".parse().expect("team");
        let team_b: TeamName = "b-team".parse().expect("team");
        assembly
            .service_runtime
            .shared_roster_store_arc()
            .save_roster(&RosterSnapshot {
                team_name: team_a.clone(),
                members: vec![herdr_member_with_alias(
                    &team_a,
                    atm_core::roles::ROLE_TEAM_LEAD,
                    "team-lead_a-team",
                )],
                refreshed_at: None,
            })
            .expect("team a roster");
        assembly
            .service_runtime
            .shared_roster_store_arc()
            .save_roster(&RosterSnapshot {
                team_name: team_b.clone(),
                members: vec![herdr_member_with_alias(
                    &team_b,
                    atm_core::roles::ROLE_TEAM_LEAD,
                    "team-lead_b-team",
                )],
                refreshed_at: None,
            })
            .expect("team b roster");
        queue_message(
            root.path(),
            &assembly.service_runtime,
            &team_a,
            atm_core::roles::ROLE_TEAM_LEAD,
        );
        queue_message(
            root.path(),
            &assembly.service_runtime,
            &team_b,
            atm_core::roles::ROLE_TEAM_LEAD,
        );

        let fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
        fake.queue_list_result(Ok(HerdrListOutcome {
            agents: vec![
                AgentSnapshot {
                    name: Some("team-lead_a-team".to_owned()),
                    pane_id: None,
                    status: HerdrAgentStatus::Idle,
                    workspace_id: None,
                },
                AgentSnapshot {
                    name: Some("team-lead_b-team".to_owned()),
                    pane_id: None,
                    status: HerdrAgentStatus::Idle,
                    workspace_id: None,
                },
            ],
        }));
        let selector = Arc::new(FakeSelector {
            emitter: FakeEmitter {
                process: Arc::clone(&fake),
            },
        });
        let process: Arc<dyn HerdrProcessAdapter> = fake.clone();
        let runtime = assembly.service_runtime.clone();
        let pump = HerdrQueueWakePump::new(
            runtime.clone(),
            selector,
            super::RuntimeHealth::default(),
            process,
        );

        pump.tick_once().await;

        let prompted: Vec<String> = fake
            .calls()
            .into_iter()
            .filter_map(|call| match call {
                atm_herdr::testing::FakeHerdrCall::Prompt { agent, .. } => Some(agent),
                _ => None,
            })
            .collect();
        assert_eq!(prompted.len(), 2);
        assert!(prompted.contains(&"team-lead_a-team".to_owned()));
        assert!(prompted.contains(&"team-lead_b-team".to_owned()));
    }

    type TaskOnlyPumpFixture = (
        tempfile::TempDir,
        LocalServiceRuntime,
        Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
        HerdrQueueWakePump,
        Arc<atm_storage::DummyTaskStore>,
        Vec<atm_storage::MemberKey>,
        Arc<Mutex<IsoTimestamp>>,
    );

    fn build_task_only_pump(
        statuses: Vec<HerdrAgentStatus>,
        fail_reminders: bool,
    ) -> TaskOnlyPumpFixture {
        build_task_only_pump_with_template(statuses, fail_reminders, None)
    }

    fn build_task_only_pump_with_template(
        statuses: Vec<HerdrAgentStatus>,
        fail_reminders: bool,
        task_template: Option<&str>,
    ) -> TaskOnlyPumpFixture {
        let root = tempfile::tempdir().expect("temporary root");
        let assembly = open_isolated_sqlite_boundary(root.path()).expect("runtime");
        let team: TeamName = "ax5-task-only".parse().expect("team");
        let agents: Vec<String> = (0..statuses.len())
            .map(|index| format!("ax5-agent-{index:02}"))
            .collect();
        let members: Vec<RosterEntry> = agents
            .iter()
            .map(|agent| herdr_member(&team, agent))
            .collect();
        assembly
            .service_runtime
            .shared_roster_store_arc()
            .save_roster(&RosterSnapshot {
                team_name: team.clone(),
                members,
                refreshed_at: None,
            })
            .expect("roster");
        let assigned_at = IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("timestamp");
        let rows = task_rows(&team, &agents, assigned_at);
        if let Some(template) = task_template {
            assembly
                .nudge_template_override_store
                .save_template_override(
                    &team,
                    atm_storage::BuiltInNudgeTemplateKind::Task,
                    template,
                )
                .expect("task template override");
        }
        let task_store = Arc::new(atm_storage::DummyTaskStore::with_rows(
            rows.clone(),
            fail_reminders,
        ));
        let reader: Arc<dyn atm_core::boundary::AsyncTaskLedgerReader + Send + Sync> =
            task_store.clone();
        let runtime = assembly
            .service_runtime
            .with_task_store(task_store.clone())
            .with_async_task_ledger_reader(reader);
        let fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
        fake.queue_list_result(Ok(HerdrListOutcome {
            agents: agents
                .iter()
                .zip(statuses)
                .map(|(name, status)| AgentSnapshot {
                    name: Some(name.clone()),
                    pane_id: None,
                    status,
                    workspace_id: None,
                })
                .collect(),
        }));
        let now = Arc::new(Mutex::new(assigned_at));
        let health = super::RuntimeHealth::default();
        let pump = pump_with_clock(runtime.clone(), fake.clone(), health, Arc::clone(&now));
        let keys = agents
            .into_iter()
            .map(|agent| atm_storage::MemberKey::new(team.clone(), agent.parse().expect("agent")))
            .collect();
        (root, runtime, fake, pump, task_store, keys, now)
    }

    fn task_rows(team: &TeamName, agents: &[String], assigned_at: IsoTimestamp) -> Vec<TaskRow> {
        agents
            .iter()
            .enumerate()
            .map(|(index, agent)| TaskRow {
                position: None,
                team: team.clone(),
                task_id: format!("AX5-TASK-{index:02}").parse().expect("task id"),
                assignee: agent.parse().expect("agent"),
                assigner: "sender".parse().expect("assigner"),
                state: TaskState::Assigned,
                assignment_message_id: AtmMessageId::new(),
                description: format!("reminder body {index}"),
                assigned_at,
                updated_at: assigned_at,
                last_reminded_at: None,
                reminder_count: 0,
                lead_notified_count: 0,
            })
            .collect()
    }

    fn build_test_pump_with_two_sessions() -> (
        tempfile::TempDir,
        LocalServiceRuntime,
        Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
        Arc<HerdrQueueWakePump>,
        atm_core::boundary::MemberKey,
    ) {
        let root = tempfile::tempdir().expect("temporary root");
        let assembly = open_isolated_sqlite_boundary(root.path()).expect("runtime");
        let team: TeamName = "aq27-team".parse().expect("team");
        let members = vec![
            herdr_member_with_session(&team, "aq27-agent", "aq27-session-a"),
            herdr_member_with_session(&team, "aq27-agent-b", "aq27-session-b"),
        ];
        assembly
            .service_runtime
            .shared_roster_store_arc()
            .save_roster(&RosterSnapshot {
                team_name: team.clone(),
                members,
                refreshed_at: None,
            })
            .expect("roster");
        queue_message(root.path(), &assembly.service_runtime, &team, "aq27-agent");
        queue_message(
            root.path(),
            &assembly.service_runtime,
            &team,
            "aq27-agent-b",
        );
        let agents = vec![
            AgentSnapshot {
                name: Some("aq27-agent".to_owned()),
                pane_id: None,
                status: HerdrAgentStatus::Idle,
                workspace_id: None,
            },
            AgentSnapshot {
                name: Some("aq27-agent-b".to_owned()),
                pane_id: None,
                status: HerdrAgentStatus::Idle,
                workspace_id: None,
            },
        ];
        let fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
        for _ in 0..2 {
            fake.queue_list_result(Ok(HerdrListOutcome {
                agents: agents.clone(),
            }));
        }
        let selector = Arc::new(FakeSelector {
            emitter: FakeEmitter {
                process: Arc::clone(&fake),
            },
        });
        let process: Arc<dyn HerdrProcessAdapter> = fake.clone();
        let pump = Arc::new(HerdrQueueWakePump::new(
            assembly.service_runtime.clone(),
            selector,
            super::RuntimeHealth::default(),
            process,
        ));
        let key = atm_core::boundary::MemberKey::new(team, "aq27-agent".parse().expect("agent"));
        (root, assembly.service_runtime, fake, pump, key)
    }

    async fn cancel_inflight_prompt() -> (
        tempfile::TempDir,
        LocalServiceRuntime,
        Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
        atm_core::boundary::MemberKey,
    ) {
        let (root, runtime, fake, pump, _health, key) = build_test_pump();
        let prompt_gate = fake.block_next_prompt();
        let prompt_started = pump.install_prompt_started_test_gate();
        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let sender_clone = shutdown_tx.clone();
        let task = pump.clone().start(shutdown_rx);
        tokio::time::timeout(Duration::from_secs(1), prompt_started.notified())
            .await
            .expect("the fake prompt is in flight before shutdown");
        shutdown_tx.send(()).expect("shutdown notification");
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("pump joins after shutdown notification")
            .expect("poll task join");
        drop(prompt_gate);
        drop(sender_clone);
        (root, runtime, fake, key)
    }

    async fn test_pump() -> (
        tempfile::TempDir,
        LocalServiceRuntime,
        Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
        atm_core::boundary::MemberKey,
    ) {
        let (root, runtime, fake, pump, _health, key) = build_test_pump();
        pump.tick_once().await;
        assert_eq!(pump.stats().prompted, 1, "one queued message prompted");
        (root, runtime, fake, key)
    }

    #[test]
    fn poll_contract_uses_fixed_cadence_and_cap() {
        assert_eq!(HERDR_POLL_INTERVAL_MS, 5_000);
        assert_eq!(HERDR_MAX_PROMPTS_PER_TICK, 16);
        assert_eq!(atm_core::boundary::TASK_REMINDER_INTERVAL_MS, 60_000);
    }

    #[tokio::test]
    async fn ax5_01_assigned_task_is_reminded_without_a_state_transition() {
        let (root, runtime, fake, _old_pump, health, key) = build_test_pump();
        let task_id: TaskId = "AX5-ASSIGNED".parse().expect("task id");
        queue_task_message(
            root.path(),
            &runtime,
            key.team(),
            key.agent().as_str(),
            task_id.clone(),
        );
        let now = Arc::new(Mutex::new(
            IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("test timestamp"),
        ));
        let pump = pump_with_clock(runtime.clone(), fake.clone(), health, Arc::clone(&now));

        pump.tick_once().await;
        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str("2030-01-01T00:01:00Z").expect("test timestamp");
        queue_idle_result(&fake, &key);
        pump.tick_once().await;
        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str("2030-01-01T00:02:00Z").expect("test timestamp");
        queue_idle_result(&fake, &key);
        pump.tick_once().await;

        assert_eq!(
            runtime
                .task_store()
                .expect("task store")
                .load_task(key.team(), &task_id)
                .expect("load task")
                .expect("task row")
                .state,
            atm_storage::TaskState::Assigned,
            "a reminder never acknowledges the assignment"
        );
        assert_eq!(pump.stats().task_reminders, 1);
        assert!(
            prompt_texts(&fake)
                .iter()
                .any(|text| text.contains("<task id=\"AX5-ASSIGNED\">"))
        );
    }

    #[tokio::test]
    async fn ac01_ack_and_completion_advance_to_the_next_task_reminder() {
        let (root, runtime, fake, _old_pump, health, key) = build_test_pump();
        let first: TaskId = "AX5-AC1-FIRST".parse().expect("task id");
        let second: TaskId = "AX5-AC1-SECOND".parse().expect("task id");
        let first_message = queue_task_message(
            root.path(),
            &runtime,
            key.team(),
            key.agent().as_str(),
            first.clone(),
        );
        queue_task_message(
            root.path(),
            &runtime,
            key.team(),
            key.agent().as_str(),
            second.clone(),
        );
        let mut roster = runtime
            .shared_roster_store_arc()
            .load_roster(key.team())
            .expect("load roster");
        roster.members.push(herdr_member(key.team(), "sender"));
        // `shared_roster_store_arc()` is the write-through roster boundary:
        // this save already updated the RAM roster mirror in the same
        // operation, so no separate cache invalidation is needed here.
        runtime
            .shared_roster_store_arc()
            .save_roster(&roster)
            .expect("add task sender to roster");
        clear_pending_markers(&runtime, &key);
        let now = Arc::new(Mutex::new(
            IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("test timestamp"),
        ));
        let pump = pump_with_clock(runtime.clone(), fake.clone(), health, Arc::clone(&now));

        pump.tick_once().await;
        let first_reminder = prompt_texts(&fake).pop().expect("first reminder");
        assert!(first_reminder.contains("AX5-AC1-FIRST"));
        queue_idle_result(&fake, &key);
        pump.tick_once().await;
        assert_eq!(
            prompt_texts(&fake).len(),
            1,
            "second tick is inside cadence"
        );

        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str("2030-01-01T00:01:05Z").expect("test timestamp");
        queue_idle_result(&fake, &key);
        pump.tick_once().await;
        assert_eq!(prompt_texts(&fake).len(), 2);
        assert!(prompt_texts(&fake)[1].contains("AX5-AC1-FIRST"));

        ack_task_assignment(root.path(), &runtime, key.team(), first_message);
        assert_eq!(
            runtime
                .task_store()
                .expect("task store")
                .load_task(key.team(), &first)
                .expect("load first task")
                .expect("first task")
                .state,
            TaskState::Active
        );
        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str("2030-01-01T00:02:10Z").expect("test timestamp");
        queue_idle_result(&fake, &key);
        pump.tick_once().await;
        assert!(prompt_texts(&fake)[2].contains("AX5-AC1-FIRST"));

        complete_task(root.path(), &runtime, key.team(), first);
        assert_eq!(
            runtime
                .task_store()
                .expect("task store")
                .load_task(key.team(), &"AX5-AC1-FIRST".parse().expect("task id"))
                .expect("load completed task")
                .expect("completed task")
                .state,
            TaskState::Complete(atm_core::test_support::TaskCloseOutcome::Completed)
        );
        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str("2030-01-01T00:03:15Z").expect("test timestamp");
        queue_idle_result(&fake, &key);
        pump.tick_once().await;
        assert!(prompt_texts(&fake)[3].contains("AX5-AC1-SECOND"));
        assert!(!prompt_texts(&fake)[3].contains("AX5-AC1-FIRST"));
        assert_eq!(
            runtime
                .task_store()
                .expect("task store")
                .load_task(key.team(), &second)
                .expect("load second task")
                .expect("second task")
                .state,
            TaskState::Active
        );
    }

    #[tokio::test]
    async fn ax5_02_drain_prompt_consumes_the_shared_reminder_budget() {
        let (root, runtime, fake, _old_pump, health, key) = build_test_pump();
        let task_id: TaskId = "AX5-BUDGET".parse().expect("task id");
        queue_task_message(
            root.path(),
            &runtime,
            key.team(),
            key.agent().as_str(),
            task_id,
        );
        let now = Arc::new(Mutex::new(
            IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("test timestamp"),
        ));
        let pump = pump_with_clock(runtime.clone(), fake.clone(), health, Arc::clone(&now));
        pump.tick_once().await;
        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str("2030-01-01T00:01:00Z").expect("test timestamp");
        queue_idle_result(&fake, &key);
        pump.tick_once().await;

        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str("2030-01-01T00:02:00Z").expect("test timestamp");
        let _ = queue_message(root.path(), &runtime, key.team(), key.agent().as_str());
        queue_idle_result(&fake, &key);
        pump.tick_once().await;
        assert_eq!(
            pump.stats().prompted,
            1,
            "only the fresh queue nudge is emitted"
        );
        assert_eq!(pump.stats().task_reminders, 0);

        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str("2030-01-01T00:03:00Z").expect("test timestamp");
        queue_idle_result(&fake, &key);
        pump.tick_once().await;
        assert_eq!(pump.stats().task_reminders, 1);
        assert_eq!(
            prompt_texts(&fake).len(),
            4,
            "two drains, queue, then reminder"
        );
    }

    #[tokio::test]
    async fn ac02_seventeen_due_reminders_split_across_ticks_at_sixteen() {
        let statuses = vec![HerdrAgentStatus::Idle; 17];
        let (_root, _runtime, fake, pump, store, keys, _now) =
            build_task_only_pump(statuses.clone(), false);
        pump.tick_once().await;
        assert_eq!(pump.stats().prompted, 16);
        assert_eq!(pump.stats().task_reminders, 16);
        assert_eq!(prompt_texts(&fake).len(), 16);

        queue_status_result(&fake, &keys, HerdrAgentStatus::Idle);
        pump.tick_once().await;
        assert_eq!(pump.stats().prompted, 1);
        assert_eq!(pump.stats().task_reminders, 1);
        assert_eq!(prompt_texts(&fake).len(), 17);
        assert_eq!(
            store
                .row(&keys[16], &"AX5-TASK-16".parse().expect("task id"))
                .reminder_count,
            1
        );
    }

    #[tokio::test]
    async fn ax5_05_emitted_prompts_consume_budget_when_audit_writes_fail() {
        let statuses = vec![HerdrAgentStatus::Idle; 17];
        let (_root, _runtime, fake, pump, store, _keys, _now) =
            build_task_only_pump(statuses, true);
        pump.tick_once().await;

        assert_eq!(pump.stats().prompted, 16);
        assert_eq!(pump.stats().task_reminders, 16);
        assert_eq!(prompt_texts(&fake).len(), 16);
        assert_eq!(
            store
                .row(
                    &atm_storage::MemberKey::new(
                        "ax5-task-only".parse().expect("team"),
                        "ax5-agent-00".parse().expect("agent"),
                    ),
                    &"AX5-TASK-00".parse().expect("task id"),
                )
                .reminder_count,
            0
        );
    }

    #[tokio::test]
    async fn ax5_09_generic_emit_failure_counts_and_respects_cooldown() {
        let (_root, _runtime, fake, pump, store, keys, now) =
            build_task_only_pump(vec![HerdrAgentStatus::Idle], false);
        fake.queue_prompt_result(Err(atm_herdr::HerdrError::AgentPromptStalled));
        pump.tick_once().await;

        let task_id = "AX5-TASK-00".parse().expect("task id");
        assert_eq!(pump.stats().task_reminders_failed, 1);
        assert_eq!(pump.stats().task_reminders, 0);
        assert_eq!(store.row(&keys[0], &task_id).reminder_count, 0);
        assert_eq!(prompt_texts(&fake).len(), 1);

        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str("2030-01-01T00:00:05Z").expect("test timestamp");
        fake.queue_prompt_result(Err(atm_herdr::HerdrError::AgentPromptStalled));
        queue_status_result(&fake, &keys, HerdrAgentStatus::Idle);
        pump.tick_once().await;
        assert_eq!(
            prompt_texts(&fake).len(),
            2,
            "failed emit retries next tick"
        );
        assert_eq!(pump.stats().task_reminders_failed, 1);

        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str("2030-01-01T00:01:00Z").expect("test timestamp");
        queue_status_result(&fake, &keys, HerdrAgentStatus::Idle);
        pump.tick_once().await;
        assert_eq!(pump.stats().task_reminders, 1);
        assert_eq!(
            prompt_texts(&fake).len(),
            3,
            "the next successful emit is recorded"
        );
        assert_eq!(store.row(&keys[0], &task_id).reminder_count, 1);

        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str("2030-01-01T00:01:05Z").expect("test timestamp");
        queue_status_result(&fake, &keys, HerdrAgentStatus::Idle);
        pump.tick_once().await;
        assert_eq!(prompt_texts(&fake).len(), 3, "durable reminder rate-limits");
    }

    #[tokio::test]
    async fn ax5_03_active_task_wins_over_a_newer_assigned_task() {
        let (root, runtime, fake, _old_pump, health, key) = build_test_pump();
        let first: TaskId = "AX5-ACTIVE".parse().expect("task id");
        let second: TaskId = "AX5-ASSIGNED-2".parse().expect("task id");
        let first_message = queue_task_message(
            root.path(),
            &runtime,
            key.team(),
            key.agent().as_str(),
            first.clone(),
        );
        queue_task_message(
            root.path(),
            &runtime,
            key.team(),
            key.agent().as_str(),
            second.clone(),
        );
        let mut roster = runtime
            .shared_roster_store_arc()
            .load_roster(key.team())
            .expect("load roster");
        roster.members.push(herdr_member(key.team(), "sender"));
        // `shared_roster_store_arc()` is the write-through roster boundary:
        // this save already updated the RAM roster mirror in the same
        // operation, so no separate cache invalidation is needed here.
        runtime
            .shared_roster_store_arc()
            .save_roster(&roster)
            .expect("add task sender to roster");
        ack_task_assignment(root.path(), &runtime, key.team(), first_message);
        let now = Arc::new(Mutex::new(
            IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("test timestamp"),
        ));
        let pump = pump_with_clock(runtime.clone(), fake.clone(), health, Arc::clone(&now));

        pump.tick_once().await;
        queue_idle_result(&fake, &key);
        pump.tick_once().await;
        queue_idle_result(&fake, &key);
        pump.tick_once().await;
        queue_idle_result(&fake, &key);
        pump.tick_once().await;
        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str("2030-01-01T00:04:00Z").expect("test timestamp");
        queue_idle_result(&fake, &key);
        pump.tick_once().await;

        let reminder = prompt_texts(&fake)
            .into_iter()
            .last()
            .expect("active task reminder");
        assert!(reminder.contains("<task id=\"AX5-ACTIVE\">"));
        assert!(!reminder.contains("AX5-ASSIGNED-2"));
        assert_eq!(
            runtime
                .task_store()
                .expect("task store")
                .load_task(key.team(), &second)
                .expect("load task")
                .expect("second task")
                .state,
            atm_storage::TaskState::Assigned
        );
    }

    #[tokio::test]
    async fn ax5_04_emit_failure_retries_until_durable_reminder_rate_limits() {
        let (root, runtime, fake, _old_pump, health, key) = build_test_pump();
        let task_id: TaskId = "AX5-RETRY".parse().expect("task id");
        queue_task_message(
            root.path(),
            &runtime,
            key.team(),
            key.agent().as_str(),
            task_id.clone(),
        );
        let now = Arc::new(Mutex::new(
            IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("test timestamp"),
        ));
        let pump = pump_with_clock(runtime.clone(), fake.clone(), health, Arc::clone(&now));
        pump.tick_once().await;

        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str("2030-01-01T00:01:00Z").expect("test timestamp");
        queue_idle_result(&fake, &key);
        pump.tick_once().await;

        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str("2030-01-01T00:02:00Z").expect("test timestamp");
        fake.queue_prompt_result(Err(atm_herdr::HerdrError::AgentNotReady));
        queue_idle_result(&fake, &key);
        pump.tick_once().await;
        assert_eq!(
            runtime
                .task_store()
                .expect("task store")
                .load_task(key.team(), &task_id)
                .expect("load task")
                .expect("task row")
                .reminder_count,
            0
        );
        assert_eq!(pump.stats().task_reminders_failed, 1);

        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str("2030-01-01T00:02:05Z").expect("test timestamp");
        queue_idle_result(&fake, &key);
        pump.tick_once().await;
        assert_eq!(
            pump.stats().task_reminders,
            1,
            "a failed emit retries on the next tick"
        );

        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str("2030-01-01T00:03:00Z").expect("test timestamp");
        queue_idle_result(&fake, &key);
        pump.tick_once().await;
        assert_eq!(
            pump.stats().task_reminders,
            0,
            "the durable reminder timestamp rate-limits the later tick"
        );
    }

    #[tokio::test]
    async fn ac04_breaker_and_absent_emitter_leave_no_reminder_audit() {
        let (_root, _runtime, fake, pump, store, keys, _now) =
            build_task_only_pump(vec![HerdrAgentStatus::Idle], false);
        fake.queue_prompt_result(Err(atm_herdr::HerdrError::ServerUnavailable {
            message: String::new(),
            retry_after: None,
            io_error_kind: None,
        }));
        pump.tick_once().await;
        let task_id = "AX5-TASK-00".parse().expect("task id");
        assert_eq!(pump.stats().prompted, 0);
        assert_eq!(pump.stats().breaker_open, 1);
        assert_eq!(store.row(&keys[0], &task_id).reminder_count, 0);

        queue_status_result(&fake, &keys, HerdrAgentStatus::Idle);
        pump.tick_once().await;
        assert_eq!(
            pump.stats().task_reminders,
            1,
            "closed breaker resumes next tick"
        );
        assert_eq!(store.row(&keys[0], &task_id).reminder_count, 1);

        let (_root, runtime, fake, _pump, store, keys, now) =
            build_task_only_pump(vec![HerdrAgentStatus::Idle], false);
        let no_emitter = pump_with_selector(
            runtime.clone(),
            fake.clone(),
            super::RuntimeHealth::default(),
            now,
            Arc::new(NoEmitterSelector),
        );
        no_emitter.tick_once().await;
        assert_eq!(no_emitter.stats().prompted, 0);
        assert_eq!(no_emitter.stats().task_reminders, 0);
        assert_eq!(store.row(&keys[0], &task_id).reminder_count, 0);
    }

    #[tokio::test]
    async fn ax5_06_task_reminder_only_appends_audit_bookkeeping() {
        let (root, runtime, fake, _old_pump, health, key) = build_test_pump();
        let task_id: TaskId = "AX5-AUDIT-ONLY".parse().expect("task id");
        queue_task_message(
            root.path(),
            &runtime,
            key.team(),
            key.agent().as_str(),
            task_id.clone(),
        );
        let now = Arc::new(Mutex::new(
            IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("test timestamp"),
        ));
        let pump = pump_with_clock(runtime.clone(), fake.clone(), health, Arc::clone(&now));
        pump.tick_once().await;
        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str("2030-01-01T00:01:00Z").expect("test timestamp");
        queue_idle_result(&fake, &key);
        pump.tick_once().await;
        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str("2030-01-01T00:02:00Z").expect("test timestamp");
        queue_idle_result(&fake, &key);
        pump.tick_once().await;

        let row = runtime
            .task_store()
            .expect("task store")
            .load_task(key.team(), &task_id)
            .expect("load task")
            .expect("task row");
        let events = runtime
            .task_store()
            .expect("task store")
            .list_task_events(key.team(), &task_id, Some(key.agent()))
            .expect("task events");
        assert_eq!(row.state, atm_storage::TaskState::Assigned);
        assert_eq!(row.reminder_count, 1);
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event == atm_storage::TaskEventKind::Assigned)
                .count(),
            1
        );
        assert!(
            events
                .iter()
                .all(|event| event.event != atm_storage::TaskEventKind::Acked)
        );
    }

    #[tokio::test]
    async fn ax5_05_drain_precedes_task_reminder_and_clock_controls_cadence() {
        let (root, runtime, fake, _old_pump, health, key) = build_test_pump();
        let task_id: TaskId = "AX5-REMINDER".parse().expect("task id");
        queue_task_message(
            root.path(),
            &runtime,
            key.team(),
            key.agent().as_str(),
            task_id.clone(),
        );
        let now = Arc::new(Mutex::new(IsoTimestamp::now()));
        let clock_now = Arc::clone(&now);
        let selector = Arc::new(FakeSelector {
            emitter: FakeEmitter {
                process: Arc::clone(&fake),
            },
        });
        let process: Arc<dyn HerdrProcessAdapter> = fake.clone();
        let pump = HerdrQueueWakePump::new(runtime.clone(), selector, health, process).with_clock(
            Arc::new(move || *clock_now.lock().expect("test clock lock")),
        );

        // The pre-existing queue entry and then the task's own deferred
        // marker consume the first two ticks. Neither may produce a second
        // prompt from the reminder step in the same tick.
        pump.tick_once().await;
        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("future timestamp");
        fake.queue_list_result(Ok(HerdrListOutcome {
            agents: vec![AgentSnapshot {
                name: Some(key.agent().to_string()),
                pane_id: None,
                status: HerdrAgentStatus::Idle,
                workspace_id: None,
            }],
        }));
        pump.tick_once().await;
        assert_eq!(pump.stats().task_reminders, 0, "drain consumes this tick");

        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str("2030-01-01T00:01:05Z").expect("future timestamp");
        fake.queue_list_result(Ok(HerdrListOutcome {
            agents: vec![AgentSnapshot {
                name: Some(key.agent().to_string()),
                pane_id: None,
                status: HerdrAgentStatus::Idle,
                workspace_id: None,
            }],
        }));
        pump.tick_once().await;

        let row = runtime
            .task_store()
            .expect("task store")
            .load_task(key.team(), &task_id)
            .expect("load task")
            .expect("task row");
        assert_eq!(row.reminder_count, 1);
        assert_eq!(pump.stats().task_reminders, 1);
        assert_eq!(
            fake.calls()
                .iter()
                .filter(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::Prompt { .. }))
                .count(),
            3,
            "two queue drains plus exactly one cadence-controlled reminder"
        );
    }

    #[tokio::test]
    async fn ax5_08_missing_task_store_skips_only_the_task_step() {
        let root = tempfile::tempdir().expect("temporary root");
        let assembly = open_isolated_sqlite_boundary(root.path()).expect("runtime");
        let team: TeamName = "aq27-team".parse().expect("team");
        assembly
            .service_runtime
            .shared_roster_store_arc()
            .save_roster(&RosterSnapshot {
                team_name: team.clone(),
                members: vec![herdr_member(&team, "aq27-agent")],
                refreshed_at: None,
            })
            .expect("roster");
        let _message = queue_message(root.path(), &assembly.service_runtime, &team, "aq27-agent");
        let pending = assembly
            .service_runtime
            .pending_nudge_store()
            .expect("pending store");
        let async_reader = assembly
            .service_runtime
            .async_task_ledger_reader()
            .expect("task reader");
        let roster = atm_runtime_test_support::build_write_through_roster_for_test(
            assembly.shared_roster_store_arc(),
        )
        .expect("write-through roster fixture hydrates from the isolated sqlite assembly");
        let runtime = LocalServiceRuntime::new_with_delivery_boundaries(
            assembly.message_store_arc(),
            roster,
            assembly.nudge_template_override_store.clone(),
            Arc::new(atm_core::LocalFileNonClaudeOutbound::new()),
        )
        .with_pending_nudge_store(pending)
        .with_async_task_ledger_reader(async_reader);
        let fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
        fake.queue_list_result(Ok(HerdrListOutcome {
            agents: vec![AgentSnapshot {
                name: Some("aq27-agent".to_owned()),
                pane_id: None,
                status: HerdrAgentStatus::Idle,
                workspace_id: None,
            }],
        }));
        let health = super::RuntimeHealth::default();
        let selector = Arc::new(FakeSelector {
            emitter: FakeEmitter {
                process: Arc::clone(&fake),
            },
        });
        let process: Arc<dyn HerdrProcessAdapter> = fake.clone();
        let pump = HerdrQueueWakePump::new(runtime, selector, health, process);

        pump.tick_once().await;
        assert_eq!(pump.stats().prompted, 1, "queue drain remains active");
        assert_eq!(pump.stats().task_reminders, 0);
        assert!(pump.stats().task_step_skipped);
    }

    #[tokio::test]
    async fn ac01_fifo_per_member_via_claim() {
        let (root, runtime, fake, pump, _health, key) = build_test_pump();
        queue_message(root.path(), &runtime, key.team(), key.agent().as_str());
        queue_message(root.path(), &runtime, key.team(), key.agent().as_str());
        pump.tick_once().await;
        assert_eq!(pump.stats().prompted, 1, "FIFO claims one message per tick");
        let prompt_text = fake
            .calls()
            .into_iter()
            .find_map(|call| match call {
                atm_herdr::testing::FakeHerdrCall::Prompt { text, .. } => Some(text),
                _ => None,
            })
            .expect("queue tick prompt");
        let prompted_message_id = prompt_text
            .split("message-id=\"")
            .nth(1)
            .and_then(|value| value.split('"').next())
            .expect("message id in rendered prompt");
        assert_eq!(
            prompt_text,
            format!(
                "<atm from=\"sender@aq27-team\" message-id=\"{prompted_message_id}\">\n  <action>atm read --message-id {prompted_message_id}</action>\n  <description>AQ2.7 test message</description>\n  <action>execute the assigned task</action>\n  <console announce=\"concise\" pause=\"false\"/>\n</atm>"
            )
        );
        assert!(
            runtime
                .pending_nudge_store()
                .expect("pending store")
                .list_pending_members()
                .expect("pending members")
                .contains(&key)
        );
    }

    /// Proves the ephemeral roster wake-pending flag actually mutates on a
    /// real Herdr wake attempt (FTQ-AW finding 4 on PR #1240): it is unset
    /// before any attempt, set while a claim's prompt is in flight, and
    /// clears again once the attempt concludes -- regardless of the claim
    /// eventually succeeding.
    #[tokio::test]
    async fn ac13_herdr_wake_pending_ephemeral_state_tracks_the_in_flight_claim() {
        let (root, runtime, fake, pump, _health, key) = build_test_pump();
        assert_eq!(
            runtime.roster_ephemeral_state(key.team(), key.agent()),
            Some(atm_storage::RosterMemberEphemeralState::default()),
            "no wake attempt is in flight before the first tick"
        );

        let prompt_gate = fake.block_next_prompt();
        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let task = pump.start(shutdown_rx);
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if runtime
                    .roster_ephemeral_state(key.team(), key.agent())
                    .expect("member present in RAM roster")
                    .herdr_wake_pending
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("wake-pending ephemeral state must be set while a claim is in flight");

        drop(prompt_gate);
        shutdown_tx.send(()).expect("shutdown notification");
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("pump joins after shutdown notification")
            .expect("poll task join");

        assert!(
            !runtime
                .roster_ephemeral_state(key.team(), key.agent())
                .expect("member present in RAM roster")
                .herdr_wake_pending,
            "wake-pending must clear once the claim attempt concludes"
        );
        let _ = root;
    }

    #[tokio::test]
    async fn ac02_burst_cap_is_sixteen_successful_prompts() {
        let agents: Vec<AgentSnapshot> = (0..17)
            .map(|index| AgentSnapshot {
                name: Some(if index == 0 {
                    "aq27-agent".to_owned()
                } else {
                    format!("aq27-agent-{index:02}")
                }),
                pane_id: None,
                status: HerdrAgentStatus::Idle,
                workspace_id: None,
            })
            .collect();
        let (_root, runtime, fake, pump, _health, key) = build_test_pump_with_agents(agents);
        pump.tick_once().await;
        assert_eq!(pump.stats().prompted, HERDR_MAX_PROMPTS_PER_TICK);
        assert_eq!(
            fake.calls()
                .iter()
                .filter(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::Prompt { .. }))
                .count(),
            HERDR_MAX_PROMPTS_PER_TICK
        );
        assert_eq!(
            runtime
                .pending_nudge_store()
                .expect("pending store")
                .list_pending_members()
                .expect("pending members")
                .len(),
            1
        );
        let remaining = atm_core::boundary::MemberKey::new(
            key.team().clone(),
            "aq27-agent-16".parse().expect("agent"),
        );
        let store = runtime.pending_nudge_store().expect("pending store");
        let claim = store
            .claim_next_pending(&remaining)
            .expect("remaining claim")
            .expect("cap leaves remaining marker");
        assert_eq!(claim.attempt, 0, "the capped member was never claimed");
        store
            .release_pending(&remaining, &claim)
            .expect("restore cap assertion claim");
    }

    #[tokio::test]
    async fn ac03_session_grouping_is_part_of_the_poll_contract() {
        let (_root, _runtime, fake, pump, _key) = build_test_pump_with_two_sessions();
        pump.tick_once().await;
        let list_sessions: Vec<_> = fake
            .calls()
            .into_iter()
            .filter_map(|call| match call {
                atm_herdr::testing::FakeHerdrCall::List { session } => session,
                _ => None,
            })
            .collect();
        assert_eq!(list_sessions.len(), 2);
        assert!(
            list_sessions
                .iter()
                .any(|session| session.as_str() == "aq27-session-a")
        );
        assert!(
            list_sessions
                .iter()
                .any(|session| session.as_str() == "aq27-session-b")
        );
    }

    #[tokio::test]
    async fn ac04_shutdown_send_stops_pump_before_drain_completes() {
        let (_root, runtime, fake, key) = cancel_inflight_prompt().await;
        let calls = fake.calls();
        assert_eq!(
            calls
                .iter()
                .filter(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::Prompt { .. }))
                .count(),
            1,
            "shutdown leaves no second prompt"
        );
        assert!(
            runtime
                .pending_nudge_store()
                .expect("pending store")
                .list_pending_members()
                .expect("pending members")
                .contains(&key)
        );
    }

    #[tokio::test]
    async fn ac05_fake_adapter_breaker_error_does_not_prompt() {
        let root = tempfile::tempdir().expect("temporary root");
        let assembly = open_isolated_sqlite_boundary(root.path()).expect("runtime");
        let team: TeamName = "aq27-team".parse().expect("team");
        assembly
            .service_runtime
            .shared_roster_store_arc()
            .save_roster(&RosterSnapshot {
                team_name: team.clone(),
                members: vec![herdr_member(&team, "aq27-agent")],
                refreshed_at: None,
            })
            .expect("roster");
        let fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
        fake.queue_list_result(Err(atm_herdr::HerdrError::ServerUnavailable {
            message: String::new(),
            retry_after: None,
            io_error_kind: None,
        }));
        let selector = Arc::new(FakeSelector {
            emitter: FakeEmitter {
                process: Arc::clone(&fake),
            },
        });
        let process: Arc<dyn HerdrProcessAdapter> = fake.clone();
        let runtime = assembly.service_runtime.clone();
        let pump = HerdrQueueWakePump::new(
            runtime.clone(),
            selector,
            super::RuntimeHealth::default(),
            process,
        );
        pump.tick_once().await;
        assert_eq!(pump.stats().prompted, 0);
        assert!(pump.stats().breaker_open > 0);
        let observation = runtime
            .roster_ephemeral_state(&team, &"aq27-agent".parse().expect("agent"))
            .expect("canonical roster member")
            .runtime;
        assert_eq!(observation.state, RuntimeMemberState::Unknown);
        assert_eq!(observation.revision.get(), 0);
        assert_eq!(
            observation.availability,
            RuntimeObservationAvailability::Unavailable
        );
    }

    #[tokio::test]
    async fn ac06_blocked_race_releases_pending_with_zero_injected_bytes() {
        let (_root, runtime, fake, pump, _health, key) = build_test_pump();
        fake.queue_prompt_result(Err(atm_herdr::HerdrError::AgentBlocked));

        pump.tick_once().await;

        assert_eq!(pump.stats().prompted, 0, "blocked prompt injected no bytes");
        assert_eq!(pump.stats().released, 1);
        assert_eq!(pump.release_streak_for(&key), 1);
        assert_eq!(
            fake.calls()
                .iter()
                .filter(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::Prompt { .. }))
                .count(),
            1,
            "the post-claim prompt was attempted exactly once"
        );

        let store = runtime.pending_nudge_store().expect("pending store");
        let claim = store
            .claim_next_pending(&key)
            .expect("claim released message")
            .expect("blocked claim remains pending");
        assert_eq!(claim.attempt, 0, "blocked input consumes no retry debt");
        store.release_pending(&key, &claim).expect("restore claim");
    }

    #[tokio::test]
    async fn ac06_not_found_family_releases_without_input() {
        for error in [
            atm_herdr::HerdrError::AgentNotFound,
            atm_herdr::HerdrError::AgentTargetAmbiguous,
            atm_herdr::HerdrError::AgentNotReady,
        ] {
            let (_root, runtime, fake, pump, _health, key) = build_test_pump();
            fake.queue_prompt_result(Err(error));

            pump.tick_once().await;

            assert_eq!(pump.stats().prompted, 0);
            assert_eq!(pump.stats().released, 1);
            assert_eq!(pump.release_streak_for(&key), 1);
            let prompt_calls = fake
                .calls()
                .iter()
                .filter(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::Prompt { .. }))
                .count();
            assert_eq!(
                prompt_calls, 1,
                "each lifecycle error reaches one prompt call"
            );

            let store = runtime.pending_nudge_store().expect("pending store");
            let claim = store
                .claim_next_pending(&key)
                .expect("claim released message")
                .expect("not-found-family claim remains pending");
            assert_eq!(claim.attempt, 0, "not-present input consumes no retry debt");
            store.release_pending(&key, &claim).expect("restore claim");
        }
    }

    #[tokio::test]
    async fn ac06_consecutive_release_bound_requeues_after_ten() {
        let (_root, runtime, fake, pump, _health, key) = build_test_pump();
        for _ in 0..HERDR_MAX_CONSECUTIVE_RELEASES {
            fake.queue_list_result(Ok(HerdrListOutcome {
                agents: vec![AgentSnapshot {
                    name: Some(key.agent().to_string()),
                    pane_id: None,
                    status: HerdrAgentStatus::Idle,
                    workspace_id: None,
                }],
            }));
        }
        for _ in 0..=HERDR_MAX_CONSECUTIVE_RELEASES {
            fake.queue_prompt_result(Err(atm_herdr::HerdrError::AgentBlocked));
        }

        let store = runtime.pending_nudge_store().expect("pending store");
        for release_number in 1..=HERDR_MAX_CONSECUTIVE_RELEASES + 1 {
            pump.tick_once().await;
            let claim = store
                .claim_next_pending(&key)
                .expect("claim resolved message")
                .expect("resolved claim remains pending");
            let expected_attempt = if release_number > HERDR_MAX_CONSECUTIVE_RELEASES {
                1
            } else {
                0
            };
            assert_eq!(claim.attempt, expected_attempt, "release {release_number}");
            assert_eq!(
                pump.release_streak_for(&key),
                if release_number > HERDR_MAX_CONSECUTIVE_RELEASES {
                    0
                } else {
                    release_number
                },
                "release counter at outcome {release_number}"
            );
            store.release_pending(&key, &claim).expect("restore claim");
        }
    }

    #[tokio::test]
    async fn ac07_absent_members_are_not_presented_as_idle() {
        let (_root, runtime, fake, pump, _health, key) = build_test_pump_with_agents(Vec::new());
        pump.tick_once().await;
        assert_eq!(pump.stats().not_present, 1);
        assert!(
            !fake
                .calls()
                .iter()
                .any(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::Prompt { .. }))
        );
        assert!(
            runtime
                .pending_nudge_store()
                .expect("pending store")
                .list_pending_members()
                .expect("pending members")
                .contains(&key)
        );
        let observation = runtime
            .roster_ephemeral_state(key.team(), key.agent())
            .expect("canonical roster member")
            .runtime;
        assert_eq!(observation.state, RuntimeMemberState::Unknown);
        assert_eq!(observation.revision.get(), 1);
        assert_eq!(
            observation.availability,
            RuntimeObservationAvailability::Fresh
        );
    }

    #[tokio::test]
    async fn ac08_dispatch_selector_is_used_by_tick_once() {
        let (_root, runtime, fake, key) = test_pump().await;
        let prompt_text = fake
            .calls()
            .into_iter()
            .find_map(|call| match call {
                atm_herdr::testing::FakeHerdrCall::Prompt { text, .. } => Some(text),
                _ => None,
            })
            .expect("queue tick prompt");
        let message_id = prompt_text
            .split("message-id=\"")
            .nth(1)
            .and_then(|value| value.split('\"').next())
            .expect("message id in rendered prompt");
        assert_eq!(
            prompt_text,
            format!(
                "<atm from=\"sender@aq27-team\" message-id=\"{message_id}\">\n  <action>atm read --message-id {message_id}</action>\n  <description>AQ2.7 test message</description>\n  <action>execute the assigned task</action>\n  <console announce=\"concise\" pause=\"false\"/>\n</atm>"
            )
        );
        assert!(
            fake.calls()
                .iter()
                .any(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::Prompt { .. }))
        );
        assert!(
            runtime
                .pending_nudge_store()
                .expect("pending store")
                .list_pending_members()
                .expect("pending members")
                .is_empty()
        );
        assert_eq!(key.agent().as_str(), "aq27-agent");
    }

    #[tokio::test]
    async fn ac09_fake_adapter_never_needs_wait_for_queue_wake() {
        let (_root, _runtime, fake, _key) = test_pump().await;
        assert!(
            !fake
                .calls()
                .iter()
                .any(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::Wait { .. }))
        );
    }

    #[tokio::test]
    async fn ac10_herdr_statuses_update_canonical_roster_state() {
        let agents = vec![AgentSnapshot {
            name: Some("aq27-agent".to_owned()),
            pane_id: None,
            status: HerdrAgentStatus::Working,
            workspace_id: None,
        }];
        let (_root, runtime, _fake, pump, health, key) = build_test_pump_with_agents(agents);
        pump.tick_once().await;
        let member = runtime
            .roster_ephemeral_state(key.team(), key.agent())
            .expect("Herdr member canonical observation")
            .runtime;
        assert_eq!(member.state, RuntimeMemberState::Active);
        assert_eq!(
            member.state_changed_by,
            Some(atm_core::protocol::RuntimeObservationSource::HerdrPoll)
        );
        assert!(health.snapshot().members.is_empty());
    }

    #[tokio::test]
    async fn ac11_claim_drop_guard_releases_marker_on_cancellation() {
        let (_root, runtime, _fake, key) = cancel_inflight_prompt().await;
        assert!(
            runtime
                .pending_nudge_store()
                .expect("pending store")
                .list_pending_members()
                .expect("pending members")
                .contains(&key)
        );
        assert_eq!(
            runtime
                .pending_nudge_store()
                .expect("pending store")
                .claim_next_pending(&key)
                .expect("claim after cancellation")
                .expect("released claim")
                .attempt,
            0
        );
    }

    #[tokio::test]
    async fn ac11_claim_drop_guard_release_is_joined_before_pump_shutdown() {
        let (_root, runtime, fake, pump, _health, key) = build_test_pump();
        let prompt_gate = fake.block_next_prompt();
        let prompt_started = pump.install_prompt_started_test_gate();
        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let task = pump.clone().start(shutdown_rx);
        tokio::time::timeout(Duration::from_secs(1), prompt_started.notified())
            .await
            .expect("the fake prompt is in flight before shutdown");

        shutdown_tx.send(()).expect("shutdown notification");
        task.await.expect("poll task joins after shutdown");

        assert!(
            pump.release_handles
                .lock()
                .expect("release handles lock")
                .is_empty(),
            "shutdown must drain and join every drop release handle"
        );
        let claim = runtime
            .pending_nudge_store()
            .expect("pending store")
            .claim_next_pending(&key)
            .expect("claim after shutdown")
            .expect("cancellation releases the claim before pump shutdown returns");
        assert_eq!(
            claim.attempt, 0,
            "cancellation release preserves retry state"
        );
        drop(prompt_gate);
    }

    #[test]
    fn release_pending_on_drop_without_runtime_releases_synchronously() {
        let (_root, runtime, _fake, _pump, _health, key) = build_test_pump();
        let store = runtime.pending_nudge_store().expect("pending store");
        let claim = store
            .claim_next_pending(&key)
            .expect("claim pending")
            .expect("queued message claim");
        let release_handles = Arc::new(Mutex::new(Vec::new()));
        let release = ReleasePendingOnDrop::new(
            Arc::clone(&store),
            key.clone(),
            claim,
            Arc::new(Mutex::new(HashMap::new())),
            runtime,
            release_handles,
        );

        drop(release);

        let released_claim = store
            .claim_next_pending(&key)
            .expect("claim after synchronous release")
            .expect("drop release makes the message claimable");
        assert_eq!(
            released_claim.attempt, 0,
            "inline fallback preserves retry state"
        );
    }

    #[tokio::test]
    async fn ac11_successful_prompt_cancellation_cannot_rerelease_claim() {
        let (_root, runtime, fake, pump, _health, key) = build_test_pump();
        let (clear_started, _allow_clear) = pump.install_handoff_cleanup_test_gate();
        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let task = pump.clone().start(shutdown_rx);
        tokio::time::timeout(Duration::from_secs(1), clear_started.notified())
            .await
            .expect("marker cleanup completes before cancellation");

        shutdown_tx.send(()).expect("shutdown notification");
        task.await.expect("poll task join");
        assert!(
            runtime
                .pending_nudge_store()
                .expect("pending store")
                .claim_next_pending(&key)
                .expect("claim while cleanup is gated")
                .is_none(),
            "a successful Herdr prompt must not be re-released while cleanup is pending"
        );

        assert!(
            runtime
                .pending_nudge_store()
                .expect("pending store")
                .list_pending_members()
                .expect("pending members")
                .is_empty(),
            "completed marker cleanup leaves no pending member"
        );

        fake.queue_list_result(Ok(HerdrListOutcome {
            agents: vec![AgentSnapshot {
                name: Some("aq27-agent".to_owned()),
                pane_id: None,
                status: HerdrAgentStatus::Idle,
                workspace_id: None,
            }],
        }));
        pump.tick_once().await;
        assert_eq!(
            fake.calls()
                .iter()
                .filter(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::Prompt { .. }))
                .count(),
            1,
            "the next tick must not prompt the already accepted message again"
        );
        pump.clear_handoff_cleanup_test_gate();
    }

    #[tokio::test]
    async fn ac12_cursor_contract_is_rotation_not_reordering() {
        let agents: Vec<AgentSnapshot> = (0..20)
            .map(|index| AgentSnapshot {
                name: Some(if index == 0 {
                    "aq27-agent".to_owned()
                } else {
                    format!("aq27-agent-{index:02}")
                }),
                pane_id: None,
                status: HerdrAgentStatus::Idle,
                workspace_id: None,
            })
            .collect();
        let (root, runtime, fake, pump, _health, _key) = build_test_pump_with_agents(agents);
        pump.tick_once().await;
        assert_eq!(pump.cursor_position(), HERDR_MAX_PROMPTS_PER_TICK);

        let changed_agents: Vec<AgentSnapshot> = (0..22)
            .map(|index| AgentSnapshot {
                name: Some(if index == 0 {
                    "aq27-agent".to_owned()
                } else {
                    format!("aq27-agent-{index:02}")
                }),
                pane_id: None,
                status: HerdrAgentStatus::Idle,
                workspace_id: None,
            })
            .collect();
        let team: TeamName = "aq27-team".parse().expect("team");
        let members = (0..22)
            .map(|index| {
                let agent = if index == 0 {
                    "aq27-agent".to_owned()
                } else {
                    format!("aq27-agent-{index:02}")
                };
                herdr_member(&team, &agent)
            })
            .collect();
        runtime
            .shared_roster_store_arc()
            .save_roster(&RosterSnapshot {
                team_name: team.clone(),
                members,
                refreshed_at: None,
            })
            .expect("changed roster");
        queue_message(root.path(), &runtime, &team, "aq27-agent");
        queue_message(root.path(), &runtime, &team, "aq27-agent-20");
        queue_message(root.path(), &runtime, &team, "aq27-agent-21");
        fake.queue_list_result(Ok(HerdrListOutcome {
            agents: changed_agents,
        }));
        pump.tick_once().await;
        let prompted: Vec<String> = fake
            .calls()
            .into_iter()
            .filter_map(|call| match call {
                atm_herdr::testing::FakeHerdrCall::Prompt { agent, .. } => Some(agent),
                _ => None,
            })
            .collect();
        assert_eq!(prompted.len(), 23);
        for index in 0..22 {
            let agent = if index == 0 {
                "aq27-agent".to_owned()
            } else {
                format!("aq27-agent-{index:02}")
            };
            let expected = usize::from(index == 0) + 1;
            assert_eq!(
                prompted
                    .iter()
                    .filter(|prompted| **prompted == agent)
                    .count(),
                expected,
                "prompt count for {agent}"
            );
        }
        assert_eq!(pump.stats().pending_members, 7);
        assert_eq!(pump.stats().prompted, 7);
        assert_eq!(pump.cursor_position(), 2);
    }

    #[test]
    fn herdr_statuses_project_to_runtime_states() {
        assert_eq!(
            runtime_state(Some(HerdrAgentStatus::Idle)),
            RuntimeMemberState::Idle
        );
        assert_eq!(
            runtime_state(Some(HerdrAgentStatus::Done)),
            RuntimeMemberState::Idle
        );
        assert_eq!(
            runtime_state(Some(HerdrAgentStatus::Working)),
            RuntimeMemberState::Active
        );
        assert_eq!(
            runtime_state(Some(HerdrAgentStatus::Unknown)),
            RuntimeMemberState::Unknown
        );
    }

    struct UnusedMailStore;
    impl atm_storage::contract::sealed::Sealed for UnusedMailStore {}
    impl atm_storage::MessageStore for UnusedMailStore {
        fn save_message(&self, _message: &atm_storage::Message) -> Result<(), AtmError> {
            unreachable!("herdr candidate test never touches the mail store boundary")
        }

        fn save_messages_atomically(
            &self,
            _messages: &[atm_storage::Message],
        ) -> Result<(), AtmError> {
            unreachable!("herdr candidate test never touches the mail store boundary")
        }

        fn load_message(
            &self,
            _key: &atm_storage::MessageKey,
        ) -> Result<Option<atm_storage::Message>, AtmError> {
            unreachable!("herdr candidate test never touches the mail store boundary")
        }

        fn list_messages(
            &self,
            _query: &atm_storage::MessageQuery,
        ) -> Result<Vec<atm_storage::Message>, AtmError> {
            unreachable!("herdr candidate test never touches the mail store boundary")
        }

        fn delete_message(&self, _key: &atm_storage::MessageKey) -> Result<(), AtmError> {
            unreachable!("herdr candidate test never touches the mail store boundary")
        }
    }

    struct NoopNudgeTemplateOverrideStore;
    impl atm_storage::contract::sealed::Sealed for NoopNudgeTemplateOverrideStore {}
    impl atm_core::boundary::NudgeTemplateOverrideStore for NoopNudgeTemplateOverrideStore {
        fn load_template_override(
            &self,
            _team: &TeamName,
            _kind: atm_core::boundary::BuiltInNudgeTemplateKind,
        ) -> Result<Option<atm_core::boundary::TeamNudgeTemplateOverrideRow>, AtmError> {
            Ok(None)
        }

        fn save_template_override(
            &self,
            _team: &TeamName,
            _kind: atm_core::boundary::BuiltInNudgeTemplateKind,
            _template_body: &str,
        ) -> Result<atm_core::boundary::TeamNudgeTemplateOverrideRow, AtmError> {
            unreachable!("herdr candidate test never touches the override-store boundary")
        }

        fn disable_template_override(
            &self,
            _team: &TeamName,
            _kind: atm_core::boundary::BuiltInNudgeTemplateKind,
        ) -> Result<atm_core::boundary::TeamNudgeTemplateOverrideRow, AtmError> {
            unreachable!("herdr candidate test never touches the override-store boundary")
        }

        fn clear_template_override(
            &self,
            _team: &TeamName,
            _kind: atm_core::boundary::BuiltInNudgeTemplateKind,
        ) -> Result<bool, AtmError> {
            unreachable!("herdr candidate test never touches the override-store boundary")
        }
    }

    struct UnusedNonClaudeOutbound;
    impl atm_core::boundary::sealed::Sealed for UnusedNonClaudeOutbound {}
    impl atm_core::boundary::NonClaudeOutbound for UnusedNonClaudeOutbound {
        fn deliver_payloads(
            &self,
            _request: atm_core::boundary::NonClaudeOutboundDeliveryRequest,
        ) -> Result<atm_core::boundary::NonClaudeOutboundDeliveryResponse, AtmError> {
            unreachable!("herdr candidate test never touches the non-Claude outbound boundary")
        }
    }

    /// Durable roster store that counts every call so the test can prove
    /// `herdr_candidates` reads exclusively from the RAM roster after the
    /// one hydration read performed at runtime construction.
    struct CountingRosterStore {
        roster: RosterSnapshot,
        load_roster_calls: std::sync::atomic::AtomicUsize,
        list_teams_calls: std::sync::atomic::AtomicUsize,
    }

    impl atm_storage::contract::sealed::Sealed for CountingRosterStore {}
    impl atm_storage::contract::RosterStore for CountingRosterStore {
        fn load_roster(&self, team: &TeamName) -> Result<RosterSnapshot, AtmError> {
            self.load_roster_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            assert_eq!(team, &self.roster.team_name, "unexpected team requested");
            Ok(self.roster.clone())
        }

        fn save_roster(&self, _roster: &RosterSnapshot) -> Result<(), AtmError> {
            unreachable!("herdr candidate test never mutates the durable roster")
        }

        fn list_teams(&self) -> Result<Vec<TeamName>, AtmError> {
            self.list_teams_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(vec![self.roster.team_name.clone()])
        }
    }

    #[test]
    fn herdr_candidates_never_reads_the_durable_roster_store_after_hydration() {
        let team: TeamName = "aq27-counting-team".parse().expect("team");
        let member = herdr_member(&team, "aq27-counting-agent");
        let durable = std::sync::Arc::new(CountingRosterStore {
            roster: RosterSnapshot {
                team_name: team.clone(),
                members: vec![member],
                refreshed_at: None,
            },
            load_roster_calls: std::sync::atomic::AtomicUsize::new(0),
            list_teams_calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let roster = atm_runtime_test_support::build_write_through_roster_for_test(durable.clone())
            .expect("write-through roster fixture hydrates from the counting fake");
        let runtime = LocalServiceRuntime::new_with_delivery_boundaries(
            std::sync::Arc::new(UnusedMailStore),
            roster,
            std::sync::Arc::new(NoopNudgeTemplateOverrideStore),
            std::sync::Arc::new(UnusedNonClaudeOutbound),
        );
        // Hydration (now performed by build_write_through_roster_for_test)
        // performs exactly one read of each kind.
        assert_eq!(
            durable
                .list_teams_calls
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        assert_eq!(
            durable
                .load_roster_calls
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );

        let pending = std::collections::HashSet::new();
        for _ in 0..5 {
            let roster_store = runtime.shared_roster_store_arc();
            let candidates =
                super::herdr_candidates(roster_store.as_ref(), &pending).expect("candidates");
            assert_eq!(candidates.len(), 1);
        }

        // Five additional herdr_candidates calls must not touch the durable
        // store again: RAM is the only read path once hydrated.
        assert_eq!(
            durable
                .list_teams_calls
                .load(std::sync::atomic::Ordering::SeqCst),
            1,
            "herdr_candidates must not call list_teams on the durable store"
        );
        assert_eq!(
            durable
                .load_roster_calls
                .load(std::sync::atomic::Ordering::SeqCst),
            1,
            "herdr_candidates must not call load_roster on the durable store"
        );
    }

    #[test]
    fn herdr_queue_wake_skips_a_nonconforming_canonical_name_without_panicking() {
        let team: TeamName = "aq27-invalid-herdr-name".parse().expect("team");
        let member = herdr_member(&team, "TeamLead");
        let durable = std::sync::Arc::new(CountingRosterStore {
            roster: RosterSnapshot {
                team_name: team,
                members: vec![member],
                refreshed_at: None,
            },
            load_roster_calls: std::sync::atomic::AtomicUsize::new(0),
            list_teams_calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let roster = atm_runtime_test_support::build_write_through_roster_for_test(durable)
            .expect("write-through roster fixture hydrates");
        let runtime = LocalServiceRuntime::new_with_delivery_boundaries(
            std::sync::Arc::new(UnusedMailStore),
            roster,
            std::sync::Arc::new(NoopNudgeTemplateOverrideStore),
            std::sync::Arc::new(UnusedNonClaudeOutbound),
        );

        let pending = std::collections::HashSet::new();
        let candidates =
            super::herdr_candidates(runtime.shared_roster_store_arc().as_ref(), &pending)
                .expect("invalid Herdr fallback is skipped, not surfaced as a failure");

        assert!(candidates.is_empty());
    }
}
