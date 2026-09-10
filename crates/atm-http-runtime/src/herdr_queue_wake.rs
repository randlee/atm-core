//! Tokio-owned polling pump for deferred Herdr queue nudges.

#[path = "herdr_queue_wake_claim.rs"]
mod claim;
#[path = "herdr_queue_wake_reminders.rs"]
mod reminders;
#[path = "herdr_queue_wake_support.rs"]
mod support;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use atm_core::LocalServiceRuntime;
use atm_core::api::RequestDeadline;
use atm_core::boundary::{
    AssignmentAttempt, AttentionItem, AttentionReservation, AttentionReservationStatus,
    DurableRosterStore, LogicalTaskRow, MemberKey, MessageReceivedHookSelector, NudgeKind,
    PendingNudgeStore, ReadDeadline, TaskAssignmentAttempt,
};
use atm_core::delivery_channel::{
    DeliveryChannel, GraftLeaseState, HerdrAgentName, HerdrSession, classify_delivery_channel,
    local_message_received_backend,
};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::nudge_dispatch::{
    load_received_hook_dispatch_message, rebuild_received_hook_dispatch,
};
use atm_core::protocol::{
    RosterRuntimeObservationUpdate, RuntimeMemberState, RuntimeObservationSource,
};
use atm_core::types::{IsoTimestamp, TaskId};
use atm_herdr::{AgentSnapshot, HerdrProcessAdapter};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::herdr_breaker_escalation::HerdrBreakerEscalationGate;
use crate::herdr_escalation::EscalationState;
use crate::herdr_queue_wake_escalation::TaskReminderEmission;
use crate::runtime_health::RuntimeHealth;
use claim::ReleasePendingOnDrop;
use support::{log_herdr_list_failure, member_order, runtime_state};

/// Poll cadence required by AQ2.7.
pub const HERDR_POLL_INTERVAL_MS: u64 = 5_000;
/// Maximum number of prompts admitted by one poll tick.
pub const HERDR_MAX_PROMPTS_PER_TICK: usize = 16;
/// Consecutive no-input releases before one retry-budget attempt is spent.
pub const HERDR_MAX_CONSECUTIVE_RELEASES: u32 = 10;
/// Minimum spacing between task reminders for one Herdr assignee.
#[cfg(test)]
pub(crate) use super::herdr_attention_scheduler::TASK_REMINDER_INTERVAL_MS;
pub(crate) const HERDR_REQUEST_DEADLINE: Duration = Duration::from_secs(5);

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
    pub notifications_failed: usize,
    pub task_step_skipped: bool,
    pub last_tick_at: Option<IsoTimestamp>,
}

#[derive(Clone)]
pub struct HerdrQueueWakePump {
    pub(crate) service_runtime: LocalServiceRuntime,
    pub(crate) selector: Arc<dyn MessageReceivedHookSelector>,
    runtime_health: RuntimeHealth,
    pub(crate) herdr_process: Arc<dyn HerdrProcessAdapter>,
    release_streaks: Arc<Mutex<HashMap<MemberKey, u32>>>,
    clock: Arc<dyn Fn() -> IsoTimestamp + Send + Sync>,
    pub(crate) escalation_state: EscalationState,
    breaker_escalation_gates: Arc<Mutex<HashMap<Option<HerdrSession>, HerdrBreakerEscalationGate>>>,
    breaker_escalation_min_interval: Duration,
    breaker_cycle_opened_at: Arc<Mutex<HashMap<Option<HerdrSession>, IsoTimestamp>>>,
    breaker_failure_counts: Arc<Mutex<HashMap<Option<HerdrSession>, u32>>>,
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
            release_streaks: Arc::new(Mutex::new(HashMap::new())),
            clock: Arc::new(IsoTimestamp::now),
            escalation_state: EscalationState::default(),
            breaker_escalation_gates: Arc::new(Mutex::new(HashMap::new())),
            breaker_escalation_min_interval: Duration::from_secs(1_800),
            breaker_cycle_opened_at: Arc::new(Mutex::new(HashMap::new())),
            breaker_failure_counts: Arc::new(Mutex::new(HashMap::new())),
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

    /// Applies the bootstrap-validated breaker escalation cooldown.
    #[must_use]
    pub fn with_breaker_escalation_min_interval(mut self, min_interval: Duration) -> Self {
        self.breaker_escalation_gates = Arc::new(Mutex::new(HashMap::new()));
        self.breaker_escalation_min_interval = min_interval;
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
                match tokio::time::timeout(HERDR_REQUEST_DEADLINE, handle).await {
                    Ok(Ok(())) => {}
                    Ok(Err(source)) => {
                        let error = AtmError::new(
                            AtmErrorCode::InternalError,
                            "Herdr queue wake claim release task ended unexpectedly",
                        )
                        .with_cause(source);
                        tracing::warn!(
                            subsystem = "herdr_queue_wake",
                            action = "queue_claim_release",
                            outcome = "failed",
                            error = %error,
                            "failed to join Herdr queue claim release task during shutdown"
                        );
                    }
                    Err(_) => {
                        let error = AtmError::new(
                            AtmErrorCode::WaitTimeout,
                            "Herdr queue wake claim release exceeded its request deadline",
                        );
                        tracing::warn!(
                            subsystem = "herdr_queue_wake",
                            action = "queue_claim_release",
                            outcome = "timed_out",
                            error = %error,
                            "timed out joining Herdr queue claim release task during shutdown"
                        );
                    }
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
        let now = (self.clock)();
        let mut stats = HerdrQueueWakeStats {
            last_tick_at: Some(now),
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
        let opportunity_members = self.opportunity_members(&eligible, &task_candidates);
        self.run_opportunity_pass(&opportunity_members, now, &mut stats)
            .await;
        self.remind_open_tasks(task_candidates, list_complete, &mut stats)
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
            notifications_failed = stats.notifications_failed,
            task_step_skipped = stats.task_step_skipped,
            "Herdr queue wake poll tick"
        );
    }

    fn breaker_cycle_opened_at(
        &self,
        session: &Option<HerdrSession>,
        now: IsoTimestamp,
    ) -> IsoTimestamp {
        *self
            .breaker_cycle_opened_at
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(session.clone())
            .or_insert(now)
    }

    fn close_breaker_cycle(&self, session: &Option<HerdrSession>) {
        self.breaker_cycle_opened_at
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(session);
        self.breaker_failure_counts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(session);
    }

    fn record_breaker_failure(&self, session: &Option<HerdrSession>) -> u32 {
        let mut counts = self
            .breaker_failure_counts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let count = counts.entry(session.clone()).or_default();
        *count = count.saturating_add(1);
        *count
    }

    async fn maybe_escalate_breaker(
        &self,
        session: &Option<HerdrSession>,
        members: &[HerdrCandidate],
        now: IsoTimestamp,
        failure_count: u32,
        error: &atm_herdr::HerdrError,
    ) -> bool {
        if self.daemon_home.as_os_str().is_empty() {
            return false;
        }
        let Some(team) = members.first().map(|member| member.key.team().clone()) else {
            return false;
        };
        let opened_at = self.breaker_cycle_opened_at(session, now);
        let admitted = self
            .breaker_escalation_gates
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(session.clone())
            .or_insert_with(|| {
                HerdrBreakerEscalationGate::new(self.breaker_escalation_min_interval)
            })
            .claim(opened_at, now);
        if !admitted {
            return false;
        }
        let retry_after = self
            .herdr_process
            .breaker_retry_after()
            .or(match error {
                atm_herdr::HerdrError::Unavailable { retry_after } => Some(*retry_after),
                _ => None,
            })
            .unwrap_or(Duration::ZERO);
        let member_target = (members.len() == 1).then(|| members[0].herdr_agent.as_str());
        let task_store = self.service_runtime.task_store().ok();
        crate::herdr_breaker_escalation::escalate_breaker_cycle(
            &self.service_runtime,
            self.herdr_process.as_ref(),
            task_store.as_ref(),
            &self.daemon_home,
            &team,
            opened_at,
            failure_count,
            error,
            retry_after,
            member_target,
        )
        .await;
        true
    }

    async fn list_eligible(
        &self,
        candidates: Vec<HerdrCandidate>,
        stats: &mut HerdrQueueWakeStats,
    ) -> (Vec<HerdrCandidate>, Vec<TaskCandidate>, bool) {
        let mut by_session: HashMap<Option<HerdrSession>, Vec<HerdrCandidate>> = HashMap::new();
        for candidate in candidates {
            by_session
                .entry(candidate.session.clone())
                .or_default()
                .push(candidate);
        }
        let mut eligible = Vec::new();
        let mut task_candidates = Vec::new();
        let mut complete = true;
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
                    self.close_breaker_cycle(&session);
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
                        let now = (self.clock)();
                        let failure_count = self.record_breaker_failure(&session);
                        let _ = self
                            .maybe_escalate_breaker(&session, &members, now, failure_count, &error)
                            .await;
                    }
                    log_herdr_list_failure(&session, &error);
                }
            }
        }
        eligible.sort_by(|left, right| member_order(&left.key, &right.key));
        task_candidates.sort_by(|left, right| member_order(&left.member.key, &right.member.key));
        (eligible, task_candidates, complete)
    }

    fn collect_idle_members(
        &self,
        agents: Vec<AgentSnapshot>,
        members: Vec<HerdrCandidate>,
        observed_at: IsoTimestamp,
        stats: &mut HerdrQueueWakeStats,
        eligible: &mut Vec<HerdrCandidate>,
        task_candidates: &mut Vec<TaskCandidate>,
    ) {
        let snapshots: HashMap<&str, &AgentSnapshot> = agents
            .iter()
            .filter_map(|snapshot| snapshot.name.as_deref().map(|name| (name, snapshot)))
            .collect();
        let mut updates_by_team = HashMap::new();
        for member in &members {
            let state = snapshots
                .get(member.herdr_agent.as_str())
                .map_or(RuntimeMemberState::Unknown, |snapshot| {
                    runtime_state(snapshot.status)
                });
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
            if !snapshots.contains_key(member.herdr_agent.as_str()) {
                if member.pending {
                    stats.not_present += 1;
                    tracing::info!(
                        event = "herdr_queue_poll_outcome",
                        member = %member.key,
                        herdr_agent = %member.herdr_agent,
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
            if matches!(
                observation.state,
                RuntimeMemberState::Idle | RuntimeMemberState::Blocked
            ) {
                task_candidates.push(TaskCandidate {
                    member: member.clone(),
                    blocked: observation.state == RuntimeMemberState::Blocked,
                });
            }
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

    fn build_idle_opportunities(
        &self,
        eligible: &[HerdrCandidate],
    ) -> Vec<atm_core::boundary::IdleOpportunity> {
        eligible
            .iter()
            .filter_map(|candidate| {
                let state = self
                    .service_runtime
                    .roster_ephemeral_state(candidate.key.team(), candidate.key.agent())?;
                Some(atm_core::boundary::IdleOpportunity {
                    id: Default::default(),
                    member: candidate.key.clone(),
                    roster_state_revision: state.runtime.revision,
                })
            })
            .collect()
    }

    /// Combines the former queue and task lanes into one chance to reserve
    /// attention for each currently idle member. Blocked task candidates stay
    /// out of the dispatch pass but continue to their escalation-only pass.
    fn opportunity_members(
        &self,
        eligible: &[HerdrCandidate],
        task_candidates: &[TaskCandidate],
    ) -> Vec<HerdrCandidate> {
        let mut members = HashMap::new();
        for candidate in eligible {
            members.insert(candidate.key.clone(), candidate.clone());
        }
        for candidate in task_candidates {
            if !candidate.blocked {
                members
                    .entry(candidate.member.key.clone())
                    .or_insert_with(|| candidate.member.clone());
            }
        }
        let mut members: Vec<_> = members.into_values().collect();
        members.sort_by(|left, right| member_order(&left.key, &right.key));
        members
    }

    async fn run_opportunity_pass(
        &self,
        eligible: &[HerdrCandidate],
        now: IsoTimestamp,
        stats: &mut HerdrQueueWakeStats,
    ) {
        for opportunity in self.build_idle_opportunities(eligible) {
            if stats.prompted >= HERDR_MAX_PROMPTS_PER_TICK {
                break;
            }
            let member = opportunity.member.clone();
            match super::herdr_attention_scheduler::reserve_next_attention(
                &self.service_runtime,
                opportunity,
                now,
            )
            .await
            {
                Ok(Some(reservation)) => {
                    if reservation.status
                        != atm_core::boundary::AttentionReservationStatus::Reserved
                    {
                        continue;
                    }
                    match self
                        .dispatch_reserved_attention(&reservation, now, stats)
                        .await
                    {
                        Ok(status) => {
                            if let Err(error) =
                                super::herdr_attention_scheduler::finalize_attention(
                                    &self.service_runtime,
                                    &reservation,
                                    status,
                                )
                                .await
                            {
                                tracing::warn!(
                                    subsystem = "herdr_queue_wake",
                                    action = "finalize_attention",
                                    outcome = "failed",
                                    member = %member,
                                    error = %error,
                                    "attention reservation finalization failed"
                                );
                            }
                        }
                        Err(error) => {
                            tracing::warn!(
                                subsystem = "herdr_queue_wake",
                                action = "dispatch_reserved_attention",
                                outcome = "failed",
                                member = %member,
                                error = %error,
                                "attention reservation dispatch failed"
                            );
                        }
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(
                        subsystem = "herdr_queue_wake",
                        action = "reserve_next_attention",
                        outcome = "failed",
                        member = %member,
                        error = %error,
                        "attention reservation failed"
                    );
                }
            }
        }
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

    async fn dispatch_reserved_attention(
        &self,
        reservation: &AttentionReservation,
        now: IsoTimestamp,
        stats: &mut HerdrQueueWakeStats,
    ) -> Result<AttentionReservationStatus, AtmError> {
        let member = match &reservation.item {
            AttentionItem::EphemeralMessage { member, .. }
            | AttentionItem::PersistentTaskReminder { member, .. } => member,
        };
        let roster_is_current = self
            .service_runtime
            .roster_ephemeral_state(member.team(), member.agent())
            .is_some_and(|state| {
                state.runtime.state == RuntimeMemberState::Idle
                    && state.runtime.revision == reservation.opportunity.roster_state_revision
            });
        if !roster_is_current {
            return Ok(AttentionReservationStatus::Stale);
        }
        match &reservation.item {
            AttentionItem::EphemeralMessage { member, message_id } => {
                self.dispatch_ephemeral_attention(member.clone(), *message_id, stats)
                    .await
            }
            AttentionItem::PersistentTaskReminder {
                member,
                task_id,
                attempt,
                assignment_message_id,
            } => {
                self.dispatch_task_reminder_attention(
                    member,
                    task_id,
                    *attempt,
                    *assignment_message_id,
                    now,
                    stats,
                )
                .await
            }
        }
    }

    async fn dispatch_ephemeral_attention(
        &self,
        member: MemberKey,
        message_id: atm_core::schema::AtmMessageId,
        stats: &mut HerdrQueueWakeStats,
    ) -> Result<AttentionReservationStatus, AtmError> {
        let pending_store = self.service_runtime.pending_nudge_store()?;
        let claim_member = member.clone();
        let claim_store = Arc::clone(&pending_store);
        let Some(claim) =
            run_blocking(move || claim_store.claim_pending(&claim_member, &message_id)).await?
        else {
            return Ok(AttentionReservationStatus::Stale);
        };
        let mut guard =
            self.queue_claim_guard(Arc::clone(&pending_store), member.clone(), claim.clone());
        let dispatch = match self.rebuild_dispatch(&member, claim.msg).await {
            Ok(Some(dispatch)) => dispatch,
            Ok(None) | Err(_) => {
                guard.release_without_input().await;
                stats.released += 1;
                return Ok(AttentionReservationStatus::Stale);
            }
        };
        let Some(emitter) = self.selector.select_emitter(&dispatch) else {
            guard.release_without_input().await;
            stats.released += 1;
            return Ok(AttentionReservationStatus::Stale);
        };
        #[cfg(test)]
        self.notify_prompt_started_test_gate();
        match emitter
            .emit_received_message(dispatch, RequestDeadline::after(HERDR_REQUEST_DEADLINE))
            .await
        {
            Ok(_) => {
                self.complete_successful_claim(&member, &claim, &mut guard, stats)
                    .await;
                Ok(AttentionReservationStatus::Delivered)
            }
            Err(error) => {
                if error.code() == AtmErrorCode::HerdrUnavailable {
                    stats.breaker_open += 1;
                }
                if error.code() == AtmErrorCode::HerdrPromptFailed {
                    guard.requeue().await;
                } else {
                    guard.release_without_input().await;
                }
                stats.released += 1;
                Ok(AttentionReservationStatus::Stale)
            }
        }
    }

    async fn dispatch_task_reminder_attention(
        &self,
        member: &MemberKey,
        task_id: &TaskId,
        attempt: AssignmentAttempt,
        assignment_message_id: atm_core::schema::AtmMessageId,
        now: IsoTimestamp,
        stats: &mut HerdrQueueWakeStats,
    ) -> Result<AttentionReservationStatus, AtmError> {
        let Some((row, assignment)) = self
            .load_current_task_reminder(member, task_id, attempt, assignment_message_id, now)
            .await?
        else {
            return Ok(AttentionReservationStatus::Stale);
        };
        let runtime = self.service_runtime.clone();
        let dispatch_member = member.clone();
        let dispatch_task_id = task_id.clone();
        let dispatch = run_blocking(move || {
            atm_core::nudge_dispatch::build_logical_task_reminder_dispatch(
                &runtime,
                &dispatch_member,
                &dispatch_task_id,
                &assignment,
            )
        })
        .await?;
        let Some(dispatch) = dispatch else {
            return Ok(AttentionReservationStatus::PermanentlyFailed);
        };
        let Some(emitter) = self.selector.select_emitter(&dispatch) else {
            return Ok(AttentionReservationStatus::Stale);
        };
        crate::herdr_queue_wake_escalation::emit_task_reminder(
            self,
            TaskReminderEmission {
                member: member.clone(),
                task_id: task_id.clone(),
                attempt,
                row,
                dispatch,
            },
            emitter,
            now,
            stats,
        )
        .await
    }

    async fn load_current_task_reminder(
        &self,
        member: &MemberKey,
        task_id: &TaskId,
        attempt: AssignmentAttempt,
        assignment_message_id: atm_core::schema::AtmMessageId,
        now: IsoTimestamp,
    ) -> Result<Option<(LogicalTaskRow, TaskAssignmentAttempt)>, AtmError> {
        let reader = self.service_runtime.async_task_ledger_reader()?;
        let deadline = ReadDeadline::new(HERDR_REQUEST_DEADLINE)?;
        let Some(row) = reader
            .top_runnable_task(member.team().clone(), member.agent().clone(), deadline)
            .await
            .map_err(|error| AtmError::daemon_unavailable(error.to_string()))?
            .filter(|row| {
                row.task_id == *task_id
                    && row.current_attempt == attempt
                    && row.assignment_message_id == assignment_message_id
                    && super::herdr_attention_scheduler::task_reminder_due(row, now)
            })
        else {
            return Ok(None);
        };
        let deadline = ReadDeadline::new(HERDR_REQUEST_DEADLINE)?;
        let assignment = reader
            .list_task_assignment_attempts(member.team().clone(), task_id.clone(), deadline)
            .await
            .map_err(|error| AtmError::daemon_unavailable(error.to_string()))?
            .into_iter()
            .find(|assignment| {
                assignment.attempt == attempt
                    && assignment.assignee == *member.agent()
                    && assignment.assignment_message_id == assignment_message_id
            });
        Ok(assignment.map(|assignment| (row, assignment)))
    }

    pub(crate) async fn rebuild_dispatch(
        &self,
        member: &MemberKey,
        message_id: atm_core::schema::AtmMessageId,
    ) -> Result<Option<atm_core::boundary::BuiltInPostSendDispatch>, AtmError> {
        let runtime = self.service_runtime.clone();
        let member_key = member.clone();
        let message = run_blocking(move || {
            load_received_hook_dispatch_message(&runtime, &member_key, message_id)
        })
        .await?;
        let Some(message) = message else {
            return Ok(None);
        };
        rebuild_received_hook_dispatch(
            &self.service_runtime,
            member,
            message_id,
            NudgeKind::Queue,
            &message,
        )
    }

    pub(crate) async fn complete_successful_claim(
        &self,
        member: &MemberKey,
        claim: &atm_core::boundary::NudgeClaim,
        release: &mut ReleasePendingOnDrop,
        stats: &mut HerdrQueueWakeStats,
    ) {
        // Herdr has accepted the prompt. Disarm before any cleanup await so
        // cancellation cannot re-release an already delivered claim.
        release.disarm();
        let runtime = self.service_runtime.clone();
        let member_key = member.clone();
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
        self.reset_release_streak(member);
        stats.prompted += 1;
        tracing::info!(
            event = "herdr_queue_poll_outcome",
            member = %member,
            msg_id = %claim.msg,
            queue_kind = NudgeKind::Queue.as_str(),
            outcome = "prompted",
            "Herdr queue prompt accepted"
        );
    }

    pub(crate) fn queue_claim_guard(
        &self,
        pending_store: Arc<dyn PendingNudgeStore + Send + Sync>,
        member: MemberKey,
        claim: atm_core::boundary::NudgeClaim,
    ) -> ReleasePendingOnDrop {
        self.service_runtime
            .set_roster_herdr_wake_pending(member.team(), member.agent(), true);
        ReleasePendingOnDrop::new(
            pending_store,
            member,
            claim,
            Arc::clone(&self.release_streaks),
            self.service_runtime.clone(),
            Arc::clone(&self.release_handles),
        )
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

#[derive(Clone)]
struct HerdrCandidate {
    key: MemberKey,
    herdr_agent: HerdrAgentName,
    session: Option<HerdrSession>,
    pending: bool,
}

#[derive(Clone)]
struct TaskCandidate {
    member: HerdrCandidate,
    blocked: bool,
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
            let Some(backend) = local_message_received_backend(&member) else {
                continue;
            };
            if classify_delivery_channel(Some(&backend), GraftLeaseState::Absent)
                != DeliveryChannel::HerdrSteer
            {
                continue;
            }
            let (configured_agent, session) = match backend {
                atm_core::delivery_channel::LocalMessageReceivedBackend::Herdr {
                    session,
                    agent,
                } => (agent, session),
                atm_core::delivery_channel::LocalMessageReceivedBackend::Tmux { .. } => continue,
            };
            let Some(herdr_agent) = atm_core::delivery_channel::resolve_herdr_agent_target(
                key.agent(),
                configured_agent,
                "herdr_queue_wake",
            ) else {
                continue;
            };
            candidates.push(HerdrCandidate {
                pending: pending.contains(&key),
                key,
                herdr_agent,
                session,
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
    match tokio::time::timeout(HERDR_REQUEST_DEADLINE, tokio::task::spawn_blocking(job)).await {
        Ok(joined) => joined.map_err(|source| {
            AtmError::new(
                AtmErrorCode::InternalError,
                "Herdr queue wake blocking operation ended unexpectedly",
            )
            .with_cause(source)
        })?,
        Err(_) => Err(AtmError::new(
            AtmErrorCode::WaitTimeout,
            "Herdr queue wake blocking operation exceeded its request deadline",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::claim::ReleasePendingOnDrop;
    use super::support::{log_herdr_list_failure, runtime_state};
    use super::{
        HERDR_MAX_CONSECUTIVE_RELEASES, HERDR_MAX_PROMPTS_PER_TICK, HERDR_POLL_INTERVAL_MS,
        HERDR_REQUEST_DEADLINE, HerdrQueueWakePump, TASK_REMINDER_INTERVAL_MS,
    };
    use atm_core::LocalServiceRuntime;
    use atm_core::ack::{AckRequest, ack_mail_with_runtime};
    use atm_core::api::RequestDeadline;
    use atm_core::boundary::{
        AsyncMessageReceivedHookEmitter, BuiltInPostSendDispatch, MemberKey,
        MessageReceivedHookSelector, PostSendEmissionPath, ReadDeadline, RosterEntry,
        RosterHarness, RosterMemberKind,
    };
    use atm_core::error::{AtmError, AtmErrorCode};
    use atm_core::observability::NullObservability;
    use atm_core::protocol::{RuntimeMemberState, RuntimeObservationAvailability};
    use atm_core::schema::AtmMessageId;
    use atm_core::send::{NudgeMode, SendMessageSource, WriteRequest, write_mail_with_runtime};
    use atm_core::task_command::{
        AssignmentInput, ComposedMessageInput, CoreTaskCommandService, NonEmptyText, TaskAction,
        TaskCommandRequest, TaskCommandService, TaskMutationCommand, TaskOperationId,
    };
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
    use atm_storage::{RosterSnapshot, TaskRow, TaskState, TaskStore};
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

    async fn queue_task_message(
        _root: &std::path::Path,
        runtime: &LocalServiceRuntime,
        team: &TeamName,
        agent: &str,
        task_id: TaskId,
    ) -> AtmMessageId {
        let assignee = MemberKey::new(team.clone(), agent.parse().expect("agent"));
        let actor = MemberKey::new(
            team.clone(),
            "scheduler-assigner".parse().expect("assigner"),
        );
        let mut roster = runtime
            .shared_roster_store_arc()
            .load_roster(team)
            .expect("load roster");
        if !roster
            .members
            .iter()
            .any(|member| member.agent_name == *actor.agent())
        {
            roster
                .members
                .push(herdr_member(team, "scheduler-assigner"));
            runtime
                .shared_roster_store_arc()
                .save_roster(&roster)
                .expect("save sender roster member");
        }
        let response = CoreTaskCommandService::new(runtime.clone())
            .execute(
                TaskCommandRequest::Mutate(Box::new(TaskMutationCommand {
                    operation_id: TaskOperationId::new(),
                    actor,
                    task_id,
                    expected_revision: None,
                    action: TaskAction::LegacyAssign(AssignmentInput {
                        assignee,
                        priority: atm_core::boundary::TaskPriority::Normal,
                        message: ComposedMessageInput {
                            body: NonEmptyText::new("AZ.4 reminder task").expect("task body"),
                            template_sha: None,
                        },
                    }),
                })),
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("logical task assignment");
        match response {
            atm_core::task_command::TaskCommandResponse::Mutation(response) => {
                response.message_id.expect("assignment message id")
            }
            _ => unreachable!("task mutation returns a mutation response"),
        }
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

    async fn start_task(
        runtime: &LocalServiceRuntime,
        key: &atm_core::boundary::MemberKey,
        task_id: TaskId,
    ) {
        CoreTaskCommandService::new(runtime.clone())
            .execute(
                TaskCommandRequest::Mutate(Box::new(TaskMutationCommand {
                    operation_id: TaskOperationId::new(),
                    actor: key.clone(),
                    task_id,
                    expected_revision: None,
                    action: TaskAction::Start,
                })),
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("explicit task start");
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
            "scheduler-assigner".parse().expect("assigner"),
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

    type V2TaskOnlyPumpFixture = (
        tempfile::TempDir,
        LocalServiceRuntime,
        Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
        HerdrQueueWakePump,
        Vec<atm_storage::MemberKey>,
        Arc<Mutex<IsoTimestamp>>,
    );

    /// Builds the production v2 task path.  The legacy `TaskOnlyPumpFixture`
    /// remains below solely for the pre-existing escalation tests; scheduler
    /// tests must use this fixture so they exercise the logical ledger and
    /// async mutation store actually used by the pump.
    async fn build_v2_task_only_pump(statuses: Vec<HerdrAgentStatus>) -> V2TaskOnlyPumpFixture {
        let root = tempfile::tempdir().expect("temporary root");
        let assembly = open_isolated_sqlite_boundary(root.path()).expect("runtime");
        let team: TeamName = "ax5-task-only".parse().expect("team");
        let agents: Vec<String> = (0..statuses.len())
            .map(|index| format!("ax5-agent-{index:02}"))
            .collect();
        assembly
            .service_runtime
            .shared_roster_store_arc()
            .save_roster(&RosterSnapshot {
                team_name: team.clone(),
                members: agents
                    .iter()
                    .map(|agent| herdr_member(&team, agent))
                    .collect(),
                refreshed_at: None,
            })
            .expect("roster");
        let runtime = assembly.service_runtime.clone();
        for (index, agent) in agents.iter().enumerate() {
            queue_task_message(
                root.path(),
                &runtime,
                &team,
                agent,
                format!("AX5-TASK-{index:02}").parse().expect("task id"),
            )
            .await;
        }
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
        let now = Arc::new(Mutex::new(
            IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("timestamp"),
        ));
        let pump = pump_with_clock(
            runtime.clone(),
            fake.clone(),
            super::RuntimeHealth::default(),
            Arc::clone(&now),
        );
        let keys = agents
            .into_iter()
            .map(|agent| atm_storage::MemberKey::new(team.clone(), agent.parse().expect("agent")))
            .collect();
        (root, runtime, fake, pump, keys, now)
    }

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
        let reader = Arc::new(
            atm_runtime_test_support::InMemoryTaskLedgerReader::with_rows(rows, Vec::new()),
        );
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
        assert_eq!(TASK_REMINDER_INTERVAL_MS, 60_000);
    }

    #[tokio::test]
    async fn az4_task_reminder_records_v2_lead_audit_at_every_tenth_delivery() {
        let (root, runtime, fake, pump, keys, now) =
            build_v2_task_only_pump(vec![HerdrAgentStatus::Idle]).await;
        let pump = pump.with_daemon_home(root.path().join("home"));
        let team = keys[0].team().clone();
        let mut roster = runtime
            .shared_roster_store_arc()
            .load_roster(&team)
            .expect("roster");
        let mut lead = herdr_member(&team, "az4-lead");
        lead.agent_type = atm_storage::AgentType::Lead;
        roster.members.push(lead);
        runtime
            .shared_roster_store_arc()
            .save_roster(&roster)
            .expect("roster");

        pump.tick_once().await;
        for minute in 1..10 {
            *now.lock().expect("clock") =
                IsoTimestamp::from_str(&format!("2030-01-01T00:{minute:02}:00Z"))
                    .expect("timestamp");
            queue_status_result(&fake, &keys, HerdrAgentStatus::Idle);
            pump.tick_once().await;
        }

        let task_id: TaskId = "AX5-TASK-00".parse().expect("task id");
        let reader = runtime
            .async_task_ledger_reader()
            .expect("logical task reader");
        let row = reader
            .load_logical_task(
                team.clone(),
                task_id.clone(),
                ReadDeadline::new(HERDR_REQUEST_DEADLINE).expect("deadline"),
            )
            .await
            .expect("logical task")
            .expect("assigned task");
        assert_eq!(row.reminder_ordinal, 10);
        let events = reader
            .list_task_lifecycle_events(
                team,
                task_id,
                None,
                ReadDeadline::new(HERDR_REQUEST_DEADLINE).expect("deadline"),
            )
            .await
            .expect("task events");
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event.as_str() == "lead_notified")
                .count(),
            1
        );
        assert_eq!(pump.stats().lead_notifications, 1);
        assert_eq!(notifications(&fake), 1);
        assert_eq!(
            runtime
                .task_store()
                .expect("compatibility task store")
                .load_task(&keys[0], &"AX5-TASK-00".parse().expect("task id"))
                .expect("compatibility task")
                .expect("compatibility row")
                .lead_notified_count,
            1
        );
    }

    #[tokio::test]
    async fn ax6_02_blocked_escalation_obeys_episode_and_renotify_cadence() {
        let (root, runtime, fake, pump, _task_store, keys, now) =
            build_task_only_pump(vec![HerdrAgentStatus::Blocked], false);
        let team = keys[0].team().clone();
        let mut roster = runtime
            .shared_roster_store_arc()
            .load_roster(&team)
            .expect("roster");
        let mut lead = herdr_member(&team, "ax6-lead");
        lead.agent_type = atm_storage::AgentType::Lead;
        roster.members.push(lead);
        runtime
            .shared_roster_store_arc()
            .save_roster(&roster)
            .expect("roster");
        let pump = pump.with_daemon_home(root.path().join("home"));
        pump.tick_once().await;
        assert_eq!(pump.stats().blocked_escalations, 0);

        *now.lock().expect("clock") =
            IsoTimestamp::from_str("2030-01-01T00:01:00Z").expect("timestamp");
        queue_status_result(&fake, &keys, HerdrAgentStatus::Blocked);
        pump.tick_once().await;
        assert_eq!(pump.stats().blocked_escalations, 1);
        assert_eq!(notifications(&fake), 1);

        *now.lock().expect("clock") =
            IsoTimestamp::from_str("2030-01-01T00:05:00Z").expect("timestamp");
        queue_status_result(&fake, &keys, HerdrAgentStatus::Blocked);
        pump.tick_once().await;
        assert_eq!(notifications(&fake), 1);

        *now.lock().expect("clock") =
            IsoTimestamp::from_str("2030-01-01T00:11:00Z").expect("timestamp");
        queue_status_result(&fake, &keys, HerdrAgentStatus::Blocked);
        pump.tick_once().await;
        assert_eq!(pump.stats().blocked_escalations, 1);
        assert_eq!(notifications(&fake), 2);
    }

    #[tokio::test]
    async fn ay4_breaker_escalation_is_once_per_cycle_and_interval_bounded() {
        let (root, runtime, fake, pump, _task_store, keys, now) =
            build_task_only_pump(vec![HerdrAgentStatus::Idle], false);
        let team = keys[0].team().clone();
        let mut roster = runtime
            .shared_roster_store_arc()
            .load_roster(&team)
            .expect("roster");
        let mut lead = herdr_member(&team, "ay4-lead");
        lead.agent_type = atm_storage::AgentType::Lead;
        roster.members.push(lead);
        runtime
            .shared_roster_store_arc()
            .save_roster(&roster)
            .expect("roster");
        let pump = pump
            .with_daemon_home(root.path().join("home"))
            .with_breaker_escalation_min_interval(Duration::from_secs(30));

        pump.tick_once().await;
        fake.queue_list_result(Err(atm_herdr::HerdrError::ServerUnavailable {
            message: String::new(),
            retry_after: None,
            io_error_kind: None,
        }));
        pump.tick_once().await;
        assert_eq!(notifications(&fake), 1, "first open cycle escalates once");
        let notification = fake
            .calls()
            .into_iter()
            .find_map(|call| match call {
                atm_herdr::testing::FakeHerdrCall::Notify { body, .. } => Some(body),
                _ => None,
            })
            .expect("breaker notification");
        assert!(notification.contains("state=breaker_open"));
        assert!(!notification.contains("Herdr breaker opened"));

        fake.queue_list_result(Err(atm_herdr::HerdrError::ServerUnavailable {
            message: String::new(),
            retry_after: None,
            io_error_kind: None,
        }));
        pump.tick_once().await;
        assert_eq!(notifications(&fake), 1, "same cycle is deduplicated");

        queue_status_result(&fake, &keys, HerdrAgentStatus::Idle);
        pump.tick_once().await;
        *now.lock().expect("clock") =
            IsoTimestamp::from_str("2030-01-01T00:00:10Z").expect("timestamp");
        fake.queue_list_result(Err(atm_herdr::HerdrError::ServerUnavailable {
            message: String::new(),
            retry_after: None,
            io_error_kind: None,
        }));
        pump.tick_once().await;
        assert_eq!(
            notifications(&fake),
            1,
            "interval suppresses a flapping endpoint"
        );

        queue_status_result(&fake, &keys, HerdrAgentStatus::Idle);
        pump.tick_once().await;
        *now.lock().expect("clock") =
            IsoTimestamp::from_str("2030-01-01T00:00:30Z").expect("timestamp");
        fake.queue_list_result(Err(atm_herdr::HerdrError::ServerUnavailable {
            message: String::new(),
            retry_after: None,
            io_error_kind: None,
        }));
        pump.tick_once().await;
        assert_eq!(
            notifications(&fake),
            2,
            "later cycle is eligible after interval"
        );
    }

    #[tokio::test]
    async fn ax6_02_blocked_poll_failure_preserves_episode_for_retry() {
        let (_root, _runtime, fake, pump, _task_store, keys, now) =
            build_task_only_pump(vec![HerdrAgentStatus::Blocked], false);
        pump.tick_once().await;
        assert_eq!(notifications(&fake), 0);

        *now.lock().expect("clock") =
            IsoTimestamp::from_str("2030-01-01T00:01:00Z").expect("timestamp");
        fake.queue_list_result(Err(atm_herdr::HerdrError::ServerUnavailable {
            message: String::new(),
            retry_after: None,
            io_error_kind: None,
        }));
        pump.tick_once().await;
        assert_eq!(notifications(&fake), 0);

        *now.lock().expect("clock") =
            IsoTimestamp::from_str("2030-01-01T00:02:00Z").expect("timestamp");
        queue_status_result(&fake, &keys, HerdrAgentStatus::Blocked);
        pump.tick_once().await;
        assert_eq!(notifications(&fake), 1);
    }

    #[tokio::test]
    async fn ax6_03_recipient_override_fans_out_only_to_the_team_scope() {
        let (root, runtime, fake, _pump, task_store, keys, now) =
            build_task_only_pump(vec![HerdrAgentStatus::Idle], false);
        let team = keys[0].team().clone();
        let mut roster = runtime
            .shared_roster_store_arc()
            .load_roster(&team)
            .expect("roster");
        let mut lead = herdr_member(&team, "ax6-lead");
        lead.agent_type = atm_storage::AgentType::Lead;
        roster.members.push(lead);
        runtime
            .shared_roster_store_arc()
            .save_roster(&roster)
            .expect("roster");
        let timestamp = *now.lock().expect("clock");
        task_store
            .add_escalation_recipient(
                &atm_storage::EscalationScope::Daemon,
                "daemon-ops@ax5-task-only",
                timestamp,
            )
            .expect("daemon recipient");
        task_store
            .add_escalation_recipient(
                &atm_storage::EscalationScope::Team(team.clone()),
                "ax5-agent-00@ax5-task-only",
                timestamp,
            )
            .expect("team recipient");
        let task_store: Arc<dyn atm_core::boundary::TaskStore + Send + Sync> = task_store;
        let notification = crate::herdr_escalation::EscalationNotification {
            title: "AX6 test escalation".to_owned(),
            body: "reason=test member=ax5-agent-00 task_id=AX6-RECIPIENT remediation=doctor"
                .to_owned(),
        };
        let outcome = crate::herdr_escalation::escalate(
            &runtime,
            fake.as_ref(),
            Some(&task_store),
            &root.path().join("home"),
            &team,
            "mail body is separate",
            &notification,
            crate::herdr_escalation::EscalationKind::LeadNotified,
        )
        .await;
        assert_eq!(outcome.recipients_written, 1);
        assert!(outcome.lead_write.is_some());
        assert_eq!(notifications(&fake), 1);
    }

    fn notifications(fake: &atm_herdr::testing::FakeHerdrProcessAdapter) -> usize {
        fake.calls()
            .into_iter()
            .filter(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::Notify { .. }))
            .count()
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
        )
        .await;
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
                .load_task(&key, &task_id)
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
    async fn ac01_explicit_start_and_completion_advance_to_the_next_task_reminder() {
        let (root, runtime, fake, _old_pump, health, key) = build_test_pump();
        let first: TaskId = "AX5-AC1-FIRST".parse().expect("task id");
        let second: TaskId = "AX5-AC1-SECOND".parse().expect("task id");
        let first_message = queue_task_message(
            root.path(),
            &runtime,
            key.team(),
            key.agent().as_str(),
            first.clone(),
        )
        .await;
        queue_task_message(
            root.path(),
            &runtime,
            key.team(),
            key.agent().as_str(),
            second.clone(),
        )
        .await;
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
                .load_task(&key, &first)
                .expect("load first task")
                .expect("first task")
                .state,
            TaskState::Assigned,
            "acknowledgement settles mail without changing task lifecycle"
        );
        start_task(&runtime, &key, first.clone()).await;
        assert_eq!(
            runtime
                .task_store()
                .expect("task store")
                .load_task(&key, &first)
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
                .load_task(&key, &"AX5-AC1-FIRST".parse().expect("task id"))
                .expect("load completed task")
                .expect("completed task")
                .state,
            TaskState::Complete
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
                .load_task(&key, &second)
                .expect("load second task")
                .expect("second task")
                .state,
            TaskState::Assigned
        );
    }

    #[tokio::test]
    async fn ax5_02_ephemeral_selection_defers_the_task_lane() {
        let (root, runtime, fake, _old_pump, health, key) = build_test_pump();
        let task_id: TaskId = "AX5-BUDGET".parse().expect("task id");
        queue_task_message(
            root.path(),
            &runtime,
            key.team(),
            key.agent().as_str(),
            task_id,
        )
        .await;
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
            "each idle opportunity selects only one attention lane"
        );
    }

    #[tokio::test]
    async fn ac02_seventeen_due_reminders_split_across_ticks_at_sixteen() {
        let statuses = vec![HerdrAgentStatus::Idle; 17];
        let (_root, runtime, fake, pump, keys, _now) =
            build_v2_task_only_pump(statuses.clone()).await;
        pump.tick_once().await;
        assert_eq!(pump.stats().prompted, 16);
        assert_eq!(pump.stats().task_reminders, 16);
        assert_eq!(prompt_texts(&fake).len(), 16);

        queue_status_result(&fake, &keys, HerdrAgentStatus::Idle);
        pump.tick_once().await;
        assert_eq!(pump.stats().prompted, 1);
        assert_eq!(pump.stats().task_reminders, 1);
        assert_eq!(prompt_texts(&fake).len(), 17);
        let events = runtime
            .async_task_ledger_reader()
            .expect("logical task reader")
            .list_task_lifecycle_events(
                keys[16].team().clone(),
                "AX5-TASK-16".parse().expect("task id"),
                None,
                ReadDeadline::new(HERDR_REQUEST_DEADLINE).expect("deadline"),
            )
            .await
            .expect("task events");
        assert!(
            events
                .iter()
                .any(|event| event.event.as_str() == "reminded")
        );
    }

    #[tokio::test]
    async fn ax5_07_blocked_candidate_after_budget_is_not_prompted() {
        let mut statuses = vec![HerdrAgentStatus::Idle; 17];
        statuses.push(HerdrAgentStatus::Blocked);
        let (_root, _runtime, fake, pump, _keys, _now) = build_v2_task_only_pump(statuses).await;
        pump.tick_once().await;

        assert_eq!(pump.stats().prompted, 16);
        assert_eq!(prompt_texts(&fake).len(), 16);
        assert_eq!(pump.stats().task_reminders, 16);
    }

    #[tokio::test]
    async fn ax5_05_emitted_prompts_consume_budget_and_append_v2_audits() {
        let statuses = vec![HerdrAgentStatus::Idle; 17];
        let (_root, runtime, fake, pump, keys, _now) = build_v2_task_only_pump(statuses).await;
        pump.tick_once().await;

        assert_eq!(pump.stats().prompted, 16);
        assert_eq!(pump.stats().task_reminders, 16);
        assert_eq!(prompt_texts(&fake).len(), 16);
        let row = runtime
            .async_task_ledger_reader()
            .expect("logical task reader")
            .load_logical_task(
                keys[0].team().clone(),
                "AX5-TASK-00".parse().expect("task id"),
                ReadDeadline::new(HERDR_REQUEST_DEADLINE).expect("deadline"),
            )
            .await
            .expect("logical task")
            .expect("assigned task");
        assert_eq!(row.reminder_ordinal, 1);
    }

    #[tokio::test]
    async fn ax5_09_retryable_task_failure_retries_on_the_next_idle_opportunity() {
        let (_root, runtime, fake, pump, keys, now) =
            build_v2_task_only_pump(vec![HerdrAgentStatus::Idle]).await;
        fake.queue_prompt_result(Err(atm_herdr::HerdrError::AgentPromptStalled));
        pump.tick_once().await;

        let task_id: TaskId = "AX5-TASK-00".parse().expect("task id");
        assert_eq!(pump.stats().task_reminders_failed, 1);
        assert_eq!(pump.stats().task_reminders, 0);
        let row = runtime
            .async_task_ledger_reader()
            .expect("logical task reader")
            .load_logical_task(
                keys[0].team().clone(),
                task_id.clone(),
                ReadDeadline::new(HERDR_REQUEST_DEADLINE).expect("deadline"),
            )
            .await
            .expect("logical task")
            .expect("assigned task");
        assert_eq!(row.reminder_ordinal, 0);
        assert_eq!(prompt_texts(&fake).len(), 1);

        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str("2030-01-01T00:00:05Z").expect("test timestamp");
        queue_status_result(&fake, &keys, HerdrAgentStatus::Idle);
        pump.tick_once().await;
        assert_eq!(
            prompt_texts(&fake).len(),
            2,
            "next idle opportunity retries once"
        );
        assert_eq!(pump.stats().task_reminders, 1);
        let row = runtime
            .async_task_ledger_reader()
            .expect("logical task reader")
            .load_logical_task(
                keys[0].team().clone(),
                task_id,
                ReadDeadline::new(HERDR_REQUEST_DEADLINE).expect("deadline"),
            )
            .await
            .expect("logical task")
            .expect("assigned task");
        assert_eq!(row.reminder_ordinal, 1);
    }

    #[tokio::test]
    async fn ax5_10_failed_blocked_and_unrenderable_audits_count_and_cool_down() {
        let (_root, _runtime, fake, pump, store, keys, now) =
            build_task_only_pump(vec![HerdrAgentStatus::Blocked], true);
        pump.tick_once().await;
        assert_eq!(pump.stats().task_reminders_blocked, 0);
        assert_eq!(
            store
                .row(&keys[0], &"AX5-TASK-00".parse().expect("task id"))
                .reminder_count,
            0
        );
        assert!(prompt_texts(&fake).is_empty());

        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str("2030-01-01T00:00:05Z").expect("test timestamp");
        queue_status_result(&fake, &keys, HerdrAgentStatus::Blocked);
        pump.tick_once().await;
        assert_eq!(pump.stats().task_reminders_blocked, 0);

        let (_root, _runtime, fake, pump, store, keys, _now) = build_task_only_pump_with_template(
            vec![HerdrAgentStatus::Idle],
            true,
            Some("{{missing}}"),
        );
        pump.tick_once().await;
        assert_eq!(pump.stats().task_reminders_unrenderable, 0);
        assert_eq!(
            store
                .row(&keys[0], &"AX5-TASK-00".parse().expect("task id"))
                .reminder_count,
            0
        );
        assert!(prompt_texts(&fake).is_empty());
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
        )
        .await;
        queue_task_message(
            root.path(),
            &runtime,
            key.team(),
            key.agent().as_str(),
            second.clone(),
        )
        .await;
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
                .load_task(&key, &second)
                .expect("load task")
                .expect("second task")
                .state,
            atm_storage::TaskState::Assigned
        );
    }

    #[tokio::test]
    async fn ax5_04_emit_failure_records_no_reminder_and_retries() {
        let (root, runtime, fake, _old_pump, health, key) = build_test_pump();
        let task_id: TaskId = "AX5-RETRY".parse().expect("task id");
        queue_task_message(
            root.path(),
            &runtime,
            key.team(),
            key.agent().as_str(),
            task_id.clone(),
        )
        .await;
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
        let reminders_before_failure = pump.stats().task_reminders;
        fake.queue_prompt_result(Err(atm_herdr::HerdrError::AgentNotReady));
        queue_idle_result(&fake, &key);
        pump.tick_once().await;
        assert_eq!(
            runtime
                .async_task_ledger_reader()
                .expect("reader")
                .load_logical_task(
                    key.team().clone(),
                    task_id.clone(),
                    ReadDeadline::new(HERDR_REQUEST_DEADLINE).expect("deadline"),
                )
                .await
                .expect("task")
                .expect("task row")
                .reminder_ordinal,
            u64::try_from(reminders_before_failure).expect("reminder count")
        );
        assert_eq!(pump.stats().task_reminders_failed, 1);

        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str("2030-01-01T00:02:05Z").expect("test timestamp");
        queue_idle_result(&fake, &key);
        pump.tick_once().await;
        assert_eq!(
            pump.stats().task_reminders,
            1,
            "the reserved task retries once"
        );

        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str("2030-01-01T00:03:00Z").expect("test timestamp");
        queue_idle_result(&fake, &key);
        pump.tick_once().await;
        assert_eq!(
            pump.stats().task_reminders,
            0,
            "the successful retry resets cadence"
        );
    }

    #[tokio::test]
    async fn ac04_breaker_and_absent_emitter_leave_no_v2_reminder_audit() {
        let (_root, runtime, fake, pump, keys, _now) =
            build_v2_task_only_pump(vec![HerdrAgentStatus::Idle]).await;
        fake.queue_prompt_result(Err(atm_herdr::HerdrError::ServerUnavailable {
            message: String::new(),
            retry_after: None,
            io_error_kind: None,
        }));
        pump.tick_once().await;
        let task_id: TaskId = "AX5-TASK-00".parse().expect("task id");
        assert_eq!(pump.stats().prompted, 0);
        assert_eq!(pump.stats().breaker_open, 1);
        assert_eq!(
            runtime
                .async_task_ledger_reader()
                .expect("reader")
                .load_logical_task(
                    keys[0].team().clone(),
                    task_id.clone(),
                    ReadDeadline::new(HERDR_REQUEST_DEADLINE).expect("deadline")
                )
                .await
                .expect("task")
                .expect("row")
                .reminder_ordinal,
            0
        );

        queue_status_result(&fake, &keys, HerdrAgentStatus::Idle);
        pump.tick_once().await;
        assert_eq!(
            pump.stats().task_reminders,
            1,
            "closed breaker resumes next tick"
        );
        assert_eq!(
            runtime
                .async_task_ledger_reader()
                .expect("reader")
                .load_logical_task(
                    keys[0].team().clone(),
                    task_id.clone(),
                    ReadDeadline::new(HERDR_REQUEST_DEADLINE).expect("deadline")
                )
                .await
                .expect("task")
                .expect("row")
                .reminder_ordinal,
            1
        );

        let (_root, runtime, fake, _pump, keys, now) =
            build_v2_task_only_pump(vec![HerdrAgentStatus::Idle]).await;
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
        assert_eq!(
            runtime
                .async_task_ledger_reader()
                .expect("reader")
                .load_logical_task(
                    keys[0].team().clone(),
                    task_id,
                    ReadDeadline::new(HERDR_REQUEST_DEADLINE).expect("deadline")
                )
                .await
                .expect("task")
                .expect("row")
                .reminder_ordinal,
            0
        );
    }

    #[tokio::test]
    async fn ax5_06_task_reminder_only_appends_v2_audit_bookkeeping() {
        let (root, runtime, fake, _old_pump, health, key) = build_test_pump();
        let task_id: TaskId = "AX5-AUDIT-ONLY".parse().expect("task id");
        queue_task_message(
            root.path(),
            &runtime,
            key.team(),
            key.agent().as_str(),
            task_id.clone(),
        )
        .await;
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

        let reader = runtime.async_task_ledger_reader().expect("reader");
        let row = reader
            .load_logical_task(
                key.team().clone(),
                task_id.clone(),
                ReadDeadline::new(HERDR_REQUEST_DEADLINE).expect("deadline"),
            )
            .await
            .expect("logical task")
            .expect("task row");
        let events = reader
            .list_task_lifecycle_events(
                key.team().clone(),
                task_id,
                None,
                ReadDeadline::new(HERDR_REQUEST_DEADLINE).expect("deadline"),
            )
            .await
            .expect("task events");
        assert!(matches!(
            row.state,
            atm_storage::TaskLifecycleState::Assigned
        ));
        assert_eq!(row.reminder_ordinal, 2);
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event.as_str() == "assigned")
                .count(),
            1
        );
        assert!(events.iter().all(|event| event.event.as_str() != "acked"));
    }

    #[tokio::test]
    async fn ax5_05_attention_selection_and_clock_control_task_cadence() {
        let (root, runtime, fake, _old_pump, health, key) = build_test_pump();
        let task_id: TaskId = "AX5-REMINDER".parse().expect("task id");
        queue_task_message(
            root.path(),
            &runtime,
            key.team(),
            key.agent().as_str(),
            task_id.clone(),
        )
        .await;
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

        // The existing ephemeral item and task assignment each receive one
        // idle opportunity. Neither may produce a second prompt in one pass.
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
        assert_eq!(
            pump.stats().task_reminders,
            1,
            "the unified scheduler may select the task lane"
        );

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
            .load_task(&key, &task_id)
            .expect("load task")
            .expect("task row");
        assert_eq!(row.reminder_count, 2);
        assert_eq!(pump.stats().task_reminders, 1);
        assert_eq!(
            fake.calls()
                .iter()
                .filter(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::Prompt { .. }))
                .count(),
            3,
            "each idle observation can service one attention item"
        );
    }

    #[tokio::test]
    async fn ax5_07_blocked_tasks_are_audited_without_a_prompt() {
        let (root, runtime, fake, _old_pump, health, key) =
            build_test_pump_with_agents(vec![AgentSnapshot {
                name: Some("aq27-agent".to_owned()),
                pane_id: None,
                status: HerdrAgentStatus::Blocked,
                workspace_id: None,
            }]);
        let task_id: TaskId = "AX5-BLOCKED".parse().expect("task id");
        queue_task_message(
            root.path(),
            &runtime,
            key.team(),
            key.agent().as_str(),
            task_id.clone(),
        )
        .await;
        let now = Arc::new(Mutex::new(
            IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("test timestamp"),
        ));
        let pump = pump_with_clock(
            runtime.clone(),
            fake.clone(),
            health.clone(),
            Arc::clone(&now),
        );

        pump.tick_once().await;

        let row = runtime
            .task_store()
            .expect("task store")
            .load_task(&key, &task_id)
            .expect("load task")
            .expect("task row");
        assert_eq!(row.reminder_count, 0);
        assert_eq!(pump.stats().task_reminders_blocked, 0);
        assert!(
            fake.calls()
                .iter()
                .all(|call| !matches!(call, atm_herdr::testing::FakeHerdrCall::Prompt { .. })),
            "blocked task reminders never prompt Herdr"
        );
        assert_eq!(
            runtime
                .roster_ephemeral_state(key.team(), key.agent())
                .expect("canonical member state")
                .runtime
                .state,
            RuntimeMemberState::Blocked,
        );

        clear_pending_markers(&runtime, &key);

        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str("2030-01-01T00:01:00Z").expect("test timestamp");
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
            .load_task(&key, &task_id)
            .expect("load task")
            .expect("task row");
        assert_eq!(
            row.reminder_count, 1,
            "the compatibility projection follows the v2 reminder audit"
        );
        assert_eq!(pump.stats().task_reminders, 1);
        assert_eq!(
            runtime
                .roster_ephemeral_state(key.team(), key.agent())
                .expect("canonical member state")
                .runtime
                .state,
            RuntimeMemberState::Idle
        );
        assert_eq!(
            prompt_texts(&fake)
                .iter()
                .filter(|text| text.contains("AX5-BLOCKED"))
                .count(),
            1,
            "the blocked task is emitted after returning to idle"
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
        let attention_schedule = assembly
            .service_runtime
            .attention_schedule_store()
            .expect("attention schedule store");
        let async_attention_schedule = assembly
            .service_runtime
            .async_attention_schedule_store()
            .expect("async attention schedule store");
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
        .with_attention_schedule_store(attention_schedule)
        .with_async_attention_schedule_store(async_attention_schedule)
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
        assert_eq!(
            pump.stats().prompted,
            1,
            "ephemeral selection remains active"
        );
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

    #[derive(Clone, Default)]
    struct WarningOutcomeRecorder(Arc<Mutex<Vec<String>>>);

    impl<S> tracing_subscriber::Layer<S> for WarningOutcomeRecorder
    where
        S: tracing::Subscriber,
    {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _context: tracing_subscriber::layer::Context<'_, S>,
        ) {
            if *event.metadata().level() != tracing::Level::WARN {
                return;
            }
            let mut visitor = WarningOutcomeVisitor::default();
            event.record(&mut visitor);
            if let Some(outcome) = visitor.outcome {
                self.0.lock().expect("warning outcomes").push(outcome);
            }
        }
    }

    #[derive(Default)]
    struct WarningOutcomeVisitor {
        outcome: Option<String>,
    }

    impl tracing::field::Visit for WarningOutcomeVisitor {
        fn record_debug(&mut self, field: &tracing::field::Field, _value: &dyn std::fmt::Debug) {
            if field.name() == "outcome" {
                self.outcome = Some("timed_out".to_owned());
            }
        }
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn ac11_claim_drop_guard_release_timeout_does_not_block_pump_shutdown() {
        let (_root, _runtime, _fake, pump, _health, _key) = build_test_pump();
        pump.release_handles
            .lock()
            .expect("release handles lock")
            .push(tokio::spawn(std::future::pending::<()>()));

        let outcomes = Arc::new(Mutex::new(Vec::new()));
        let subscriber =
            tracing_subscriber::registry().with(WarningOutcomeRecorder(Arc::clone(&outcomes)));
        let _default = tracing::subscriber::set_default(subscriber);
        tokio::time::timeout(Duration::from_secs(10), pump.await_release_handles())
            .await
            .expect("timed-out release join must return");
        assert_eq!(
            outcomes.lock().expect("warning outcomes").as_slice(),
            ["timed_out"],
            "the bounded join must emit its WaitTimeout warning"
        );
        assert!(
            pump.release_handles
                .lock()
                .expect("release handles lock")
                .is_empty(),
            "timed-out release handle must be removed after the WaitTimeout path"
        );
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
    async fn ac12_prompt_cap_retains_unclaimed_messages_for_the_next_tick() {
        let agents: Vec<AgentSnapshot> = (0..22)
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
        let team: TeamName = "aq27-team".parse().expect("team");
        for index in 0..22 {
            let agent = if index == 0 {
                "aq27-agent".to_owned()
            } else {
                format!("aq27-agent-{index:02}")
            };
            queue_message(root.path(), &runtime, &team, &agent);
        }
        pump.tick_once().await;
        assert_eq!(pump.stats().prompted, HERDR_MAX_PROMPTS_PER_TICK);
        assert_eq!(prompt_texts(&fake).len(), HERDR_MAX_PROMPTS_PER_TICK);

        let pending = runtime
            .pending_nudge_store()
            .expect("pending store")
            .list_pending_members()
            .expect("pending members");
        assert_eq!(
            pending.len(),
            22,
            "the durable queue keeps one member entry per recipient"
        );
    }

    #[test]
    fn herdr_statuses_project_to_runtime_states() {
        assert_eq!(
            runtime_state(HerdrAgentStatus::Idle),
            RuntimeMemberState::Idle
        );
        assert_eq!(
            runtime_state(HerdrAgentStatus::Done),
            RuntimeMemberState::Idle
        );
        assert_eq!(
            runtime_state(HerdrAgentStatus::Working),
            RuntimeMemberState::Active
        );
        assert_eq!(
            runtime_state(HerdrAgentStatus::Unknown),
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
