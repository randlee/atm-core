//! Tokio-owned polling pump for deferred Herdr queue nudges.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use atm_core::LocalServiceRuntime;
use atm_core::api::RequestDeadline;
use atm_core::boundary::{
    AsyncTaskLedgerReader, DurableRosterStore, MemberKey, MessageReceivedHookSelector, NudgeKind,
    PendingNudgeStore, ReadDeadline, ReminderOutcome, TaskRow, TaskState,
};
use atm_core::delivery_channel::{
    DeliveryChannel, GraftLeaseState, HerdrSession, classify_delivery_channel,
    local_message_received_backend,
};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::nudge_dispatch::{build_task_reminder_dispatch, rebuild_received_hook_dispatch};
use atm_core::protocol::RuntimeMemberState;
use atm_core::types::IsoTimestamp;
use atm_herdr::{AgentSnapshot, HerdrAgentStatus, HerdrProcessAdapter};
use tokio::sync::watch;
use tokio::task::JoinHandle;

#[cfg(test)]
use tokio::sync::Notify;

use crate::runtime_health::RuntimeHealth;

/// Poll cadence required by AQ2.7.
pub const HERDR_POLL_INTERVAL_MS: u64 = 5_000;
/// Maximum number of prompts admitted by one poll tick.
pub const HERDR_MAX_PROMPTS_PER_TICK: usize = 16;
/// Consecutive no-input releases before one retry-budget attempt is spent.
pub const HERDR_MAX_CONSECUTIVE_RELEASES: u32 = 10;
/// Minimum spacing between task reminders for one Herdr assignee.
pub const TASK_REMINDER_INTERVAL_MS: u64 = 60_000;
const HERDR_REQUEST_DEADLINE: Duration = Duration::from_secs(5);

#[cfg(test)]
type HandoffCleanupTestGate = (Arc<Notify>, Arc<Notify>);

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
    pub task_step_skipped: bool,
    pub last_tick_at: Option<IsoTimestamp>,
}

#[derive(Clone)]
pub struct HerdrQueueWakePump {
    service_runtime: LocalServiceRuntime,
    selector: Arc<dyn MessageReceivedHookSelector>,
    runtime_health: RuntimeHealth,
    herdr_process: Arc<dyn HerdrProcessAdapter>,
    cursor: Arc<Mutex<usize>>,
    release_streaks: Arc<Mutex<HashMap<MemberKey, u32>>>,
    clock: Arc<dyn Fn() -> IsoTimestamp + Send + Sync>,
    last_task_attempt: Arc<Mutex<HashMap<MemberKey, IsoTimestamp>>>,
    task_step_available: Arc<Mutex<Option<bool>>>,
    last_stats: Arc<Mutex<HerdrQueueWakeStats>>,
    #[cfg(test)]
    handoff_cleanup_test_gate: Arc<Mutex<Option<HandoffCleanupTestGate>>>,
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
            last_task_attempt: Arc::new(Mutex::new(HashMap::new())),
            task_step_available: Arc::new(Mutex::new(None)),
            last_stats: Arc::new(Mutex::new(HerdrQueueWakeStats::default())),
            #[cfg(test)]
            handoff_cleanup_test_gate: Arc::new(Mutex::new(None)),
        }
    }

    #[cfg(test)]
    #[must_use]
    fn with_clock(mut self, clock: Arc<dyn Fn() -> IsoTimestamp + Send + Sync>) -> Self {
        self.clock = clock;
        self
    }

    #[cfg(test)]
    fn install_handoff_cleanup_test_gate(&self) -> (Arc<Notify>, Arc<Notify>) {
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        self.handoff_cleanup_test_gate
            .lock()
            .expect("handoff cleanup gate lock")
            .replace((Arc::clone(&entered), Arc::clone(&release)));
        (entered, release)
    }

    #[cfg(test)]
    fn clear_handoff_cleanup_test_gate(&self) {
        self.handoff_cleanup_test_gate
            .lock()
            .expect("handoff cleanup gate lock")
            .take();
    }

    #[cfg(test)]
    async fn await_handoff_cleanup_test_gate(&self) {
        let Some((entered, release)) = self
            .handoff_cleanup_test_gate
            .lock()
            .expect("handoff cleanup gate lock")
            .as_ref()
            .cloned()
        else {
            return;
        };
        entered.notify_one();
        release.notified().await;
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
        })
    }

    /// Runs one complete roster/list/claim/dispatch pass.
    pub async fn tick_once(&self) {
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

        let (eligible, task_candidates) = self.list_eligible(candidates, &mut stats).await;
        let prompted_by_drain = self
            .drain_eligible(pending_store, eligible, &mut stats)
            .await;
        self.remind_open_tasks(task_candidates, &prompted_by_drain, &mut stats)
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
            task_step_skipped = stats.task_step_skipped,
            "Herdr queue wake poll tick"
        );
    }

    async fn list_eligible(
        &self,
        candidates: Vec<HerdrCandidate>,
        stats: &mut HerdrQueueWakeStats,
    ) -> (Vec<HerdrCandidate>, Vec<TaskCandidate>) {
        let mut by_session: HashMap<Option<HerdrSession>, Vec<HerdrCandidate>> = HashMap::new();
        for candidate in candidates {
            by_session
                .entry(candidate.session.clone())
                .or_default()
                .push(candidate);
        }
        let mut eligible = Vec::new();
        let mut task_candidates = Vec::new();
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
                Ok(outcome) => self.collect_idle_members(
                    outcome.agents,
                    members,
                    stats,
                    &mut eligible,
                    &mut task_candidates,
                ),
                Err(error) => {
                    if error.is_infrastructure() {
                        stats.breaker_open += 1;
                    }
                    tracing::warn!(
                        subsystem = "herdr_queue_wake",
                        action = "herdr_list",
                        outcome = "failed",
                        session = ?session,
                        error = ?error,
                        "Herdr queue wake list failed"
                    );
                }
            }
        }
        eligible.sort_by(|left, right| member_order(&left.key, &right.key));
        task_candidates.sort_by(|left, right| member_order(&left.member.key, &right.member.key));
        (eligible, task_candidates)
    }

    fn collect_idle_members(
        &self,
        agents: Vec<AgentSnapshot>,
        members: Vec<HerdrCandidate>,
        stats: &mut HerdrQueueWakeStats,
        eligible: &mut Vec<HerdrCandidate>,
        task_candidates: &mut Vec<TaskCandidate>,
    ) {
        let snapshots: HashMap<&str, &AgentSnapshot> = agents
            .iter()
            .filter_map(|snapshot| snapshot.name.as_deref().map(|name| (name, snapshot)))
            .collect();
        for member in members {
            let Some(snapshot) = snapshots.get(member.key.agent().as_str()) else {
                if member.pending {
                    stats.not_present += 1;
                    tracing::info!(
                        event = "herdr_queue_poll_outcome",
                        member = %member.key,
                        queue_kind = NudgeKind::Queue.as_str(),
                        outcome = "held_target_not_present",
                        "Herdr queue target was absent from the poll result"
                    );
                }
                continue;
            };
            if snapshot.status == HerdrAgentStatus::Unknown {
                continue;
            }
            self.runtime_health
                .record_herdr_poll_state(&member.key, runtime_state(snapshot.status));
            if matches!(
                snapshot.status,
                HerdrAgentStatus::Idle | HerdrAgentStatus::Done | HerdrAgentStatus::Blocked
            ) {
                task_candidates.push(TaskCandidate {
                    member: member.clone(),
                    blocked: snapshot.status == HerdrAgentStatus::Blocked,
                });
            }
            if member.pending
                && matches!(
                    snapshot.status,
                    HerdrAgentStatus::Idle | HerdrAgentStatus::Done
                )
            {
                stats.idle_members += 1;
                eligible.push(member);
            }
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

    async fn remind_open_tasks(
        &self,
        candidates: Vec<TaskCandidate>,
        prompted_by_drain: &HashSet<MemberKey>,
        stats: &mut HerdrQueueWakeStats,
    ) {
        let reader = match self.service_runtime.async_task_ledger_reader() {
            Ok(reader) => reader,
            Err(error) => {
                stats.task_step_skipped = true;
                self.note_task_step_availability(false, Some(&error));
                return;
            }
        };
        let task_store = match self.service_runtime.task_store() {
            Ok(store) => store,
            Err(error) => {
                stats.task_step_skipped = true;
                self.note_task_step_availability(false, Some(&error));
                return;
            }
        };
        self.note_task_step_availability(true, None);
        let now = (self.clock)();
        for candidate in candidates {
            if !candidate.blocked && stats.prompted >= HERDR_MAX_PROMPTS_PER_TICK {
                continue;
            }
            if prompted_by_drain.contains(&candidate.member.key) {
                self.stamp_task_attempt(&candidate.member.key, now);
                continue;
            }
            let Some(row) = self.read_due_task(reader.as_ref(), &candidate, now).await else {
                continue;
            };
            self.emit_task_reminder(&task_store, candidate, row, now, stats)
                .await;
        }
    }

    async fn read_due_task(
        &self,
        reader: &dyn AsyncTaskLedgerReader,
        candidate: &TaskCandidate,
        now: IsoTimestamp,
    ) -> Option<TaskRow> {
        let deadline = match ReadDeadline::new(HERDR_REQUEST_DEADLINE) {
            Ok(deadline) => deadline,
            Err(error) => {
                tracing::warn!(subsystem = "herdr_queue_wake", action = "task_reminder_read", outcome = "deadline_invalid", error = %error, member = %candidate.member.key, "Herdr task reminder read skipped");
                return None;
            }
        };
        let rows = match reader
            .list_tasks(
                candidate.member.key.team().clone(),
                Some(candidate.member.key.agent().clone()),
                deadline,
            )
            .await
        {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(subsystem = "herdr_queue_wake", action = "task_reminder_read", outcome = "failed", error = %error, member = %candidate.member.key, "Herdr task reminder read failed");
                return None;
            }
        };
        let row = select_open_task(rows)?;
        self.reminder_due(&candidate.member.key, &row, now)
            .then_some(row)
    }

    async fn emit_task_reminder(
        &self,
        task_store: &Arc<dyn atm_core::boundary::TaskStore + Send + Sync>,
        candidate: TaskCandidate,
        row: TaskRow,
        now: IsoTimestamp,
        stats: &mut HerdrQueueWakeStats,
    ) {
        if candidate.blocked {
            self.record_task_outcome(
                task_store,
                &candidate.member.key,
                &row,
                now,
                ReminderOutcome::Blocked,
                stats,
            )
            .await;
            return;
        }
        if stats.prompted >= HERDR_MAX_PROMPTS_PER_TICK {
            return;
        }
        let runtime = self.service_runtime.clone();
        let member = candidate.member.key.clone();
        let row_for_dispatch = row.clone();
        let dispatch = run_blocking(move || {
            build_task_reminder_dispatch(&runtime, &member, &row_for_dispatch)
        })
        .await;
        let dispatch = match dispatch {
            Ok(Some(dispatch)) => dispatch,
            Ok(None) => return,
            Err(error) => {
                tracing::warn!(subsystem = "herdr_queue_wake", action = "task_reminder_render", outcome = "unrenderable", error = %error, member = %candidate.member.key, "Herdr task reminder could not render");
                self.record_task_outcome(
                    task_store,
                    &candidate.member.key,
                    &row,
                    now,
                    ReminderOutcome::Unrenderable,
                    stats,
                )
                .await;
                return;
            }
        };
        let Some(emitter) = self.selector.select_emitter(&dispatch) else {
            tracing::info!(event = "herdr_queue_poll_outcome", member = %candidate.member.key, outcome = "reminder_target_not_present", "Herdr task reminder selector returned no emitter");
            return;
        };
        match emitter
            .emit_received_message(dispatch, RequestDeadline::after(HERDR_REQUEST_DEADLINE))
            .await
        {
            Ok(_) => {
                self.record_task_outcome(
                    task_store,
                    &candidate.member.key,
                    &row,
                    now,
                    ReminderOutcome::Emitted,
                    stats,
                )
                .await
            }
            Err(error) if error.code() == AtmErrorCode::HerdrUnavailable => stats.breaker_open += 1,
            Err(error) => {
                stats.task_reminders_failed += 1;
                self.stamp_task_attempt(&candidate.member.key, now);
                tracing::warn!(subsystem = "herdr_queue_wake", action = "task_reminder_emit", outcome = "failed", error = %error, error_code = ?error.code(), member = %candidate.member.key, "Herdr task reminder emission failed")
            }
        }
    }

    async fn record_task_outcome(
        &self,
        task_store: &Arc<dyn atm_core::boundary::TaskStore + Send + Sync>,
        member: &MemberKey,
        row: &TaskRow,
        now: IsoTimestamp,
        outcome: ReminderOutcome,
        stats: &mut HerdrQueueWakeStats,
    ) {
        if !self
            .record_task_reminder(task_store, member, row, now, outcome)
            .await
        {
            match outcome {
                ReminderOutcome::Emitted => {
                    stats.prompted += 1;
                    stats.task_reminders += 1;
                }
                ReminderOutcome::Unrenderable => stats.task_reminders_unrenderable += 1,
                ReminderOutcome::Blocked => stats.task_reminders_blocked += 1,
            }
            self.stamp_task_attempt(member, now);
            return;
        }
        match outcome {
            ReminderOutcome::Emitted => {
                stats.prompted += 1;
                stats.task_reminders += 1;
            }
            ReminderOutcome::Unrenderable => stats.task_reminders_unrenderable += 1,
            ReminderOutcome::Blocked => stats.task_reminders_blocked += 1,
        }
        self.stamp_task_attempt(member, now);
    }

    async fn record_task_reminder(
        &self,
        store: &Arc<dyn atm_core::boundary::TaskStore + Send + Sync>,
        member: &MemberKey,
        row: &TaskRow,
        now: IsoTimestamp,
        outcome: ReminderOutcome,
    ) -> bool {
        let store = Arc::clone(store);
        let member = member.clone();
        let task_id = row.task_id.clone();
        let write_member = member.clone();
        let write_task_id = task_id.clone();
        match run_blocking(move || {
            store.record_reminder(&write_member, &write_task_id, now, outcome)
        })
        .await
        {
            Ok(_) => true,
            Err(error) => {
                tracing::warn!(subsystem = "herdr_queue_wake", action = "task_reminder_record", outcome = "failed", error = %error, member = %member, task_id = %task_id, "Herdr task reminder bookkeeping failed");
                false
            }
        }
    }

    fn reminder_due(&self, member: &MemberKey, row: &TaskRow, now: IsoTimestamp) -> bool {
        let due = |then: IsoTimestamp| {
            now.into_inner()
                .signed_duration_since(then.into_inner())
                .num_milliseconds()
                >= i64::try_from(TASK_REMINDER_INTERVAL_MS).unwrap_or(i64::MAX)
        };
        row.last_reminded_at.is_none_or(due)
            && self
                .last_task_attempt
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(member)
                .copied()
                .is_none_or(due)
    }

    fn stamp_task_attempt(&self, member: &MemberKey, now: IsoTimestamp) {
        self.last_task_attempt
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(member.clone(), now);
    }

    fn prune_member_state(&self, candidates: &[HerdrCandidate]) {
        let members: HashSet<_> = candidates
            .iter()
            .map(|candidate| candidate.key.clone())
            .collect();
        self.last_task_attempt
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .retain(|member, _| members.contains(member));
        self.release_streaks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .retain(|member, _| members.contains(member));
    }

    fn note_task_step_availability(&self, available: bool, error: Option<&AtmError>) {
        let mut previous = self
            .task_step_available
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if *previous == Some(available) {
            return;
        }
        *previous = Some(available);
        if available {
            tracing::info!(
                subsystem = "herdr_queue_wake",
                action = "task_store_resolve",
                outcome = "available",
                "Herdr task reminder step available"
            );
        } else {
            tracing::warn!(
                subsystem = "herdr_queue_wake",
                action = "task_store_resolve",
                outcome = "unavailable",
                error = ?error,
                "Herdr task reminder step skipped: task store unavailable"
            );
        }
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
        let mut release = ReleasePendingOnDrop::new(
            Arc::clone(pending_store),
            member.key.clone(),
            claim.clone(),
            Arc::clone(&self.release_streaks),
        );
        let dispatch = match self.rebuild_dispatch(member, claim.msg).await {
            Ok(Some(dispatch)) => dispatch,
            Ok(None) | Err(_) => {
                release.release_without_input();
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
        let Some(emitter) = self.selector.select_emitter(&dispatch) else {
            release.release_without_input();
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
        run_blocking(move || {
            rebuild_received_hook_dispatch(&runtime, &member_key, message_id, NudgeKind::Queue)
        })
        .await
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
                        release.requeue();
                        "dispatch_failed_requeued"
                    }
                    AtmErrorCode::HerdrAgentNotVisible => {
                        release.release_without_input();
                        "held_target_not_present"
                    }
                    AtmErrorCode::PostSendHerdrPromptFailed => {
                        release.release_without_input();
                        "blocked_before_input_released"
                    }
                    _ => {
                        release.release_without_input();
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
        #[cfg(test)]
        self.await_handoff_cleanup_test_gate().await;
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

impl crate::RuntimeMaintenance for HerdrQueueWakePump {
    fn start(&self, shutdown: watch::Receiver<()>) -> JoinHandle<()> {
        Arc::new(self.clone()).start(shutdown)
    }
}

#[derive(Clone)]
struct HerdrCandidate {
    key: MemberKey,
    session: Option<HerdrSession>,
    pending: bool,
}

#[derive(Clone)]
struct TaskCandidate {
    member: HerdrCandidate,
    blocked: bool,
}

fn select_open_task(mut rows: Vec<TaskRow>) -> Option<TaskRow> {
    rows.retain(|row| row.state != TaskState::Complete);
    rows.sort_by(|left, right| {
        left.assigned_at
            .cmp(&right.assigned_at)
            .then_with(|| left.task_id.as_str().cmp(right.task_id.as_str()))
    });
    rows.iter()
        .find(|row| row.state == TaskState::Active)
        .or_else(|| rows.iter().find(|row| row.state == TaskState::Assigned))
        .cloned()
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
            let session = match backend {
                atm_core::delivery_channel::LocalMessageReceivedBackend::Herdr { session } => {
                    session
                }
                atm_core::delivery_channel::LocalMessageReceivedBackend::Tmux { .. } => continue,
            };
            candidates.push(HerdrCandidate {
                pending: pending.contains(&key),
                key,
                session,
            });
        }
    }
    candidates.sort_by(|left, right| member_order(&left.key, &right.key));
    Ok(candidates)
}

async fn run_blocking<T, F>(job: F) -> Result<T, AtmError>
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

fn runtime_state(status: HerdrAgentStatus) -> RuntimeMemberState {
    match status {
        HerdrAgentStatus::Idle | HerdrAgentStatus::Done => RuntimeMemberState::Idle,
        HerdrAgentStatus::Working => RuntimeMemberState::Active,
        HerdrAgentStatus::Blocked => RuntimeMemberState::Blocked,
        HerdrAgentStatus::Unknown => RuntimeMemberState::Unknown,
    }
}

struct ReleasePendingOnDrop {
    store: Arc<dyn PendingNudgeStore + Send + Sync>,
    member: MemberKey,
    claim: atm_core::boundary::NudgeClaim,
    release_streaks: Arc<Mutex<HashMap<MemberKey, u32>>>,
    armed: bool,
}

impl ReleasePendingOnDrop {
    fn new(
        store: Arc<dyn PendingNudgeStore + Send + Sync>,
        member: MemberKey,
        claim: atm_core::boundary::NudgeClaim,
        release_streaks: Arc<Mutex<HashMap<MemberKey, u32>>>,
    ) -> Self {
        Self {
            store,
            member,
            claim,
            release_streaks,
            armed: true,
        }
    }

    fn release_without_input(&mut self) {
        if self.armed {
            let should_requeue = {
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
            };
            let result = if should_requeue {
                self.store.requeue_pending(&self.member, &self.claim)
            } else {
                self.store.release_pending(&self.member, &self.claim)
            };
            if let Err(error) = result {
                tracing::warn!(
                    subsystem = "herdr_queue_wake",
                    action = "queue_claim_release",
                    outcome = "failed",
                    error = %error,
                    member = %self.member,
                    "failed to resolve Herdr queue claim"
                );
            }
            self.armed = false;
        }
    }

    fn requeue(&mut self) {
        if self.armed {
            if let Err(error) = self.store.requeue_pending(&self.member, &self.claim) {
                tracing::warn!(
                    subsystem = "herdr_queue_wake",
                    action = "queue_claim_requeue",
                    outcome = "failed",
                    error = %error,
                    member = %self.member,
                    "failed to requeue Herdr queue claim"
                );
            }
            self.release_streaks
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(&self.member);
            self.armed = false;
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ReleasePendingOnDrop {
    fn drop(&mut self) {
        self.release_without_input();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        HERDR_MAX_CONSECUTIVE_RELEASES, HERDR_MAX_PROMPTS_PER_TICK, HERDR_POLL_INTERVAL_MS,
        HerdrQueueWakePump, TASK_REMINDER_INTERVAL_MS, runtime_state,
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
    use atm_core::protocol::RuntimeMemberState;
    use atm_core::schema::AtmMessageId;
    use atm_core::send::{NudgeMode, SendMessageSource, WriteRequest, write_mail_with_runtime};
    use atm_core::test_support as atm_storage;
    use atm_core::types::{IsoTimestamp, ModelName, TaskId, TeamName};
    use atm_herdr::{
        AgentSnapshot, HerdrAgentStatus, HerdrListOutcome, HerdrProcessAdapter, HerdrPromptOutcome,
    };
    use atm_runtime_test_support::open_isolated_sqlite_boundary;
    use atm_storage::{RosterSnapshot, TaskRow, TaskState};
    use serde_json::json;
    use std::future::Future;
    use std::pin::Pin;
    use std::str::FromStr;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::sync::watch;

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
                        &dispatch.event.recipient,
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
            status: HerdrAgentStatus::Idle,
            workspace_id: None,
        }])
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
        let rows: Vec<TaskRow> = agents
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
            .collect();
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
                status: HerdrAgentStatus::Idle,
                workspace_id: None,
            },
            AgentSnapshot {
                name: Some("aq27-agent-b".to_owned()),
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
        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let sender_clone = shutdown_tx.clone();
        let task = pump.clone().start(shutdown_rx);
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if fake
                    .calls()
                    .iter()
                    .any(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::Prompt { .. }))
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
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
        runtime
            .shared_roster_store_arc()
            .save_roster(&roster)
            .expect("add task sender to roster");
        runtime.clear_roster_cache();
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
    async fn ax5_07_blocked_candidate_after_budget_is_still_audited() {
        let mut statuses = vec![HerdrAgentStatus::Idle; 17];
        statuses.push(HerdrAgentStatus::Blocked);
        let (_root, _runtime, fake, pump, store, keys, _now) =
            build_task_only_pump(statuses, false);
        pump.tick_once().await;

        assert_eq!(pump.stats().prompted, 16);
        assert_eq!(pump.stats().task_reminders_blocked, 1);
        assert_eq!(prompt_texts(&fake).len(), 16);
        assert_eq!(
            store
                .row(&keys[17], &"AX5-TASK-17".parse().expect("task id"))
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
        queue_status_result(&fake, &keys, HerdrAgentStatus::Idle);
        pump.tick_once().await;
        assert_eq!(prompt_texts(&fake).len(), 1, "cooldown suppresses a retry");

        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str("2030-01-01T00:01:00Z").expect("test timestamp");
        queue_status_result(&fake, &keys, HerdrAgentStatus::Idle);
        pump.tick_once().await;
        assert_eq!(pump.stats().task_reminders, 1);
        assert_eq!(
            prompt_texts(&fake).len(),
            2,
            "cooldown expires after one minute"
        );
        assert_eq!(store.row(&keys[0], &task_id).reminder_count, 1);
    }

    #[tokio::test]
    async fn ax5_10_failed_blocked_and_unrenderable_audits_count_and_cool_down() {
        let (_root, _runtime, fake, pump, store, keys, now) =
            build_task_only_pump(vec![HerdrAgentStatus::Blocked], true);
        pump.tick_once().await;
        assert_eq!(pump.stats().task_reminders_blocked, 1);
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
        assert_eq!(
            pump.stats().task_reminders_blocked,
            0,
            "blocked cooldown holds"
        );

        let (_root, _runtime, fake, pump, store, keys, _now) = build_task_only_pump_with_template(
            vec![HerdrAgentStatus::Idle],
            true,
            Some("{{missing}}"),
        );
        pump.tick_once().await;
        assert_eq!(pump.stats().task_reminders_unrenderable, 1);
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
        runtime
            .shared_roster_store_arc()
            .save_roster(&roster)
            .expect("add task sender to roster");
        runtime.clear_roster_cache();
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
                .load_task(&key, &task_id)
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
            0,
            "cooldown suppresses a retry"
        );

        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str("2030-01-01T00:03:00Z").expect("test timestamp");
        queue_idle_result(&fake, &key);
        pump.tick_once().await;
        assert_eq!(
            pump.stats().task_reminders,
            1,
            "the cooldown eventually expires"
        );
    }

    #[tokio::test]
    async fn ac04_breaker_and_absent_emitter_leave_no_reminder_audit() {
        let (_root, _runtime, fake, pump, store, keys, _now) =
            build_task_only_pump(vec![HerdrAgentStatus::Idle], false);
        fake.queue_prompt_result(Err(atm_herdr::HerdrError::ServerUnavailable));
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
            .load_task(&key, &task_id)
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
    async fn ax5_07_blocked_tasks_are_audited_without_a_prompt() {
        let (root, runtime, fake, _old_pump, health, key) =
            build_test_pump_with_agents(vec![AgentSnapshot {
                name: Some("aq27-agent".to_owned()),
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
        );
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
        assert_eq!(row.reminder_count, 1);
        assert_eq!(pump.stats().task_reminders_blocked, 1);
        assert!(
            fake.calls()
                .iter()
                .all(|call| !matches!(call, atm_herdr::testing::FakeHerdrCall::Prompt { .. })),
            "blocked task reminders never prompt Herdr"
        );
        assert_eq!(
            health.snapshot().members[0].state,
            RuntimeMemberState::Blocked,
        );

        clear_pending_markers(&runtime, &key);

        *now.lock().expect("test clock lock") =
            IsoTimestamp::from_str("2030-01-01T00:01:00Z").expect("test timestamp");
        fake.queue_list_result(Ok(HerdrListOutcome {
            agents: vec![AgentSnapshot {
                name: Some(key.agent().to_string()),
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
            row.reminder_count, 2,
            "blocked and idle outcomes are audited"
        );
        assert_eq!(pump.stats().task_reminders, 1);
        assert_eq!(health.snapshot().members[0].state, RuntimeMemberState::Idle);
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
        let async_reader = assembly
            .service_runtime
            .async_task_ledger_reader()
            .expect("task reader");
        let runtime = LocalServiceRuntime::new_with_delivery_boundaries(
            assembly.message_store_arc(),
            assembly.shared_roster_store_arc(),
            assembly.nudge_template_override_store.clone(),
            Arc::new(atm_core::LocalFileNonClaudeOutbound::new()),
        )
        .with_pending_nudge_store(pending)
        .with_async_task_ledger_reader(async_reader);
        let fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
        fake.queue_list_result(Ok(HerdrListOutcome {
            agents: vec![AgentSnapshot {
                name: Some("aq27-agent".to_owned()),
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

    #[tokio::test]
    async fn ac02_burst_cap_is_sixteen_successful_prompts() {
        let agents: Vec<AgentSnapshot> = (0..17)
            .map(|index| AgentSnapshot {
                name: Some(if index == 0 {
                    "aq27-agent".to_owned()
                } else {
                    format!("aq27-agent-{index:02}")
                }),
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
        fake.queue_list_result(Err(atm_herdr::HerdrError::ServerUnavailable));
        let selector = Arc::new(FakeSelector {
            emitter: FakeEmitter {
                process: Arc::clone(&fake),
            },
        });
        let process: Arc<dyn HerdrProcessAdapter> = fake.clone();
        let pump = HerdrQueueWakePump::new(
            assembly.service_runtime,
            selector,
            super::RuntimeHealth::default(),
            process,
        );
        pump.tick_once().await;
        assert_eq!(pump.stats().prompted, 0);
        assert!(pump.stats().breaker_open > 0);
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
    async fn ac10_herdr_statuses_update_runtime_health_states() {
        let agents = vec![AgentSnapshot {
            name: Some("aq27-agent".to_owned()),
            status: HerdrAgentStatus::Working,
            workspace_id: None,
        }];
        let (_root, _runtime, _fake, pump, health, key) = build_test_pump_with_agents(agents);
        pump.tick_once().await;
        let member = health
            .snapshot()
            .members
            .into_iter()
            .find(|member| member.member.as_str() == key.agent().as_str())
            .expect("Herdr member health observation");
        assert_eq!(member.state, RuntimeMemberState::Active);
        assert_eq!(
            member.state_changed_by,
            Some(atm_core::protocol::RuntimeObservationSource::HerdrPoll)
        );
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
    async fn ac11_successful_prompt_cancellation_cannot_rerelease_claim() {
        let (_root, runtime, fake, pump, _health, key) = build_test_pump();
        let (clear_started, allow_clear) = pump.install_handoff_cleanup_test_gate();
        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let task = pump.clone().start(shutdown_rx);
        clear_started.notified().await;

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

        allow_clear.notify_one();
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if runtime
                    .pending_nudge_store()
                    .expect("pending store")
                    .list_pending_members()
                    .expect("pending members")
                    .is_empty()
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("marker cleanup completes");

        fake.queue_list_result(Ok(HerdrListOutcome {
            agents: vec![AgentSnapshot {
                name: Some("aq27-agent".to_owned()),
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
        runtime.clear_roster_cache();
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
}
