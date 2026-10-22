//! Task-reminder and escalation pass for the Herdr queue wake pump.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use atm_core::boundary::{
    AsyncTaskLedgerReader, MemberKey, ReadDeadline, ReminderOutcome,
    TASK_CONSECUTIVE_REFUSAL_THRESHOLD, TASK_REMINDER_INTERVAL_MS, TaskRow,
};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::nudge_dispatch::build_task_reminder_dispatch;
use atm_core::protocol::RuntimeMemberState;
use atm_core::types::{IsoTimestamp, TaskId};
use atm_herdr::{AgentSnapshot, HerdrAgentStatus};

use crate::herdr_task_disposition::{TaskDisposition, dispose, escalation_due};

use super::{
    CandidateTarget, HERDR_MAX_PROMPTS_PER_TICK, HERDR_REQUEST_BUDGET, HerdrCandidate,
    HerdrQueueWakePump, HerdrQueueWakeStats, MemberObservation, herdr_request_deadline,
};

pub(super) fn queue_drain_eligible(observation: &MemberObservation) -> bool {
    observation.state == RuntimeMemberState::Idle
}

pub(super) fn runtime_state(status: Option<HerdrAgentStatus>) -> RuntimeMemberState {
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

pub(super) fn runtime_state_with_absence(
    absences: &mut HashMap<MemberKey, IsoTimestamp>,
    member: &MemberKey,
    status: Option<HerdrAgentStatus>,
    observed_at: IsoTimestamp,
) -> RuntimeMemberState {
    const OFFLINE_ABSENCE_INTERVALS: i64 = 2;

    let Some(status) = status else {
        let started_at = absences.entry(member.clone()).or_insert(observed_at);
        let absent_for = (observed_at.into_inner() - started_at.into_inner()).num_milliseconds();
        return if absent_for >= TASK_REMINDER_INTERVAL_MS * OFFLINE_ABSENCE_INTERVALS {
            RuntimeMemberState::Offline
        } else {
            RuntimeMemberState::Unknown
        };
    };
    absences.remove(member);
    runtime_state(Some(status))
}

pub(super) fn observe_escalation_episode_if_eligible(
    escalation_state: &crate::herdr_escalation::EscalationState,
    member: &atm_core::boundary::MemberKey,
    state: RuntimeMemberState,
    has_open_task: bool,
) -> bool {
    let is_episode_state = matches!(
        state,
        RuntimeMemberState::Blocked | RuntimeMemberState::Offline
    );
    if has_open_task && is_episode_state || !is_episode_state {
        escalation_state.observe(member, state)
    } else {
        false
    }
}

pub(super) struct PreparedTaskPass {
    reader: Arc<dyn AsyncTaskLedgerReader + Send + Sync>,
    task_store: Arc<dyn atm_core::boundary::TaskStore + Send + Sync>,
    /// Every open task per member, in queue order. The first row is the
    /// member's bookkeeping head; the whole list is named in one nudge.
    open: HashMap<MemberKey, Vec<TaskRow>>,
}

impl HerdrQueueWakePump {
    pub(super) async fn prepare_task_pass(
        &self,
        stats: &mut HerdrQueueWakeStats,
        candidates: &[MemberObservation],
    ) -> Option<PreparedTaskPass> {
        let (reader, task_store) = self.task_capabilities(stats)?;
        self.note_task_step_availability(true, None);
        let open = self.open_tasks_by_member(reader.as_ref(), candidates).await;
        Some(PreparedTaskPass {
            reader,
            task_store,
            open,
        })
    }

    pub(super) async fn remind_open_tasks(
        &self,
        prepared: PreparedTaskPass,
        candidates: Vec<MemberObservation>,
        open_mail: &HashSet<MemberKey>,
        stats: &mut HerdrQueueWakeStats,
    ) {
        let now = (self.clock)();
        for candidate in candidates {
            self.process_task_candidate(
                prepared.reader.as_ref(),
                &prepared.task_store,
                &prepared.open,
                candidate,
                open_mail,
                now,
                stats,
            )
            .await;
        }
    }

    pub(super) fn collect_idle_members(
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
        let accepted = self.apply_herdr_observations(&snapshots, &members, observed_at);
        for member in members {
            let CandidateTarget::Herdr(target) = &member.target else {
                continue;
            };
            if !snapshots.contains_key(target.agent.as_str()) && member.pending {
                stats.not_present += 1;
                tracing::info!(
                    event = "herdr_queue_poll_outcome",
                    member = %member.key,
                    herdr_agent = %target.agent,
                    queue_kind = atm_core::boundary::NudgeKind::Queue.as_str(),
                    outcome = "held_target_not_present",
                    "Herdr queue target was absent from the poll result"
                );
            }
            let Some(observation) = accepted.get(&member.key) else {
                continue;
            };
            task_candidates.push(MemberObservation {
                member: member.key.clone(),
                state: observation.state,
                state_changed_at: observation.state_changed_at,
            });
            if member.pending
                && queue_drain_eligible(&MemberObservation {
                    member: member.key.clone(),
                    state: observation.state,
                    state_changed_at: observation.state_changed_at,
                })
            {
                stats.idle_members += 1;
                eligible.push(member);
            }
        }
    }

    fn task_capabilities(
        &self,
        stats: &mut HerdrQueueWakeStats,
    ) -> Option<(
        Arc<dyn AsyncTaskLedgerReader + Send + Sync>,
        Arc<dyn atm_core::boundary::TaskStore + Send + Sync>,
    )> {
        let reader = match self.service_runtime.async_task_ledger_reader() {
            Ok(reader) => reader,
            Err(error) => {
                stats.task_step_skipped = true;
                self.note_task_step_availability(false, Some(&error));
                return None;
            }
        };
        let task_store = match self.service_runtime.task_store() {
            Ok(store) => store,
            Err(error) => {
                stats.task_step_skipped = true;
                self.note_task_step_availability(false, Some(&error));
                return None;
            }
        };
        Some((reader, task_store))
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the disposition boundary keeps each durable input explicit"
    )]
    async fn process_task_candidate(
        &self,
        reader: &(dyn AsyncTaskLedgerReader + Send + Sync),
        task_store: &Arc<dyn atm_core::boundary::TaskStore + Send + Sync>,
        open: &HashMap<MemberKey, Vec<TaskRow>>,
        candidate: MemberObservation,
        open_mail: &HashSet<MemberKey>,
        now: IsoTimestamp,
        stats: &mut HerdrQueueWakeStats,
    ) {
        let member_open = open.get(&candidate.member).map_or(&[][..], Vec::as_slice);
        let head = member_open.first();
        let mail_pending = open_mail.contains(&candidate.member);
        let active_since = candidate.state_changed_at;
        // A dormant Blocked/Offline member is not an escalation episode. In
        // particular, do not consume its episode marker: a task assigned
        // while that same outage persists must still produce one escalation.
        // Non-episode states continue through `observe` so recovery clears a
        // previously reported episode.
        let new_episode = observe_escalation_episode_if_eligible(
            &self.escalation_state,
            &candidate.member,
            candidate.state,
            head.is_some(),
        );
        let provisional = dispose(
            mail_pending,
            candidate.state,
            head,
            now,
            active_since,
            new_episode,
            0,
        );
        if candidate.state != RuntimeMemberState::Idle || head.is_none() {
            self.apply_task_disposition(
                task_store,
                reader,
                member_open,
                candidate,
                now,
                stats,
                provisional,
            )
            .await;
            return;
        }
        let Some(refusal_count) = self
            .refusal_check(reader, task_store, &candidate.member, stats)
            .await
        else {
            return;
        };
        let disposition = dispose(
            mail_pending,
            candidate.state,
            head,
            now,
            active_since,
            new_episode,
            refusal_count,
        );
        self.apply_task_disposition(
            task_store,
            reader,
            member_open,
            candidate,
            now,
            stats,
            disposition,
        )
        .await;
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the disposition boundary keeps each durable input explicit"
    )]
    async fn apply_task_disposition(
        &self,
        task_store: &Arc<dyn atm_core::boundary::TaskStore + Send + Sync>,
        reader: &(dyn AsyncTaskLedgerReader + Send + Sync),
        open: &[TaskRow],
        candidate: MemberObservation,
        now: IsoTimestamp,
        stats: &mut HerdrQueueWakeStats,
        disposition: TaskDisposition,
    ) {
        let head = open.first();
        match disposition {
            TaskDisposition::EscalateEpisode(kind) => {
                crate::herdr_queue_wake_escalation::escalate_episode(
                    self,
                    task_store,
                    &candidate.member,
                    kind,
                    candidate.state_changed_at.unwrap_or(now),
                    stats,
                )
                .await;
            }
            TaskDisposition::EscalateStalled => {
                if let Some(row) = head {
                    crate::herdr_queue_wake_escalation::escalate_stalled_task(
                        self, reader, task_store, row, now, stats,
                    )
                    .await;
                }
            }
            TaskDisposition::Nudge => {
                if stats.prompted < HERDR_MAX_PROMPTS_PER_TICK && head.is_some() {
                    self.emit_task_reminder(task_store, candidate, open, now, stats)
                        .await;
                }
            }
            TaskDisposition::ResetReminders => {
                if let Some(row) = head {
                    self.reset_task_reminders(task_store, &candidate.member, row, now)
                        .await;
                }
            }
            TaskDisposition::Hold(_) => {}
        }
    }

    async fn refusal_run(
        &self,
        reader: &(dyn AsyncTaskLedgerReader + Send + Sync),
        member: &MemberKey,
    ) -> Result<(u32, Option<IsoTimestamp>), AtmError> {
        let deadline = ReadDeadline::new(HERDR_REQUEST_BUDGET)?;
        reader
            .refusal_run(member.team().clone(), member.agent().clone(), deadline)
            .await
            .map(|run| (run.count, run.started_at))
            .map_err(Into::into)
    }

    /// Reads the member's refusal run and escalates it once it reaches the
    /// threshold. `None` holds the disposition because the history is unreadable.
    async fn refusal_check(
        &self,
        reader: &(dyn AsyncTaskLedgerReader + Send + Sync),
        task_store: &Arc<dyn atm_core::boundary::TaskStore + Send + Sync>,
        member: &MemberKey,
        stats: &mut HerdrQueueWakeStats,
    ) -> Option<u32> {
        let (refusal_count, refusal_started_at) = match self.refusal_run(reader, member).await {
            Ok(run) => run,
            Err(error) => {
                tracing::warn!(
                    subsystem = "herdr_queue_wake",
                    action = "refusal_history_read",
                    outcome = "failed",
                    member = %member,
                    error = %error,
                    "Task disposition held because refusal history is unavailable"
                );
                return None;
            }
        };
        if refusal_count >= TASK_CONSECUTIVE_REFUSAL_THRESHOLD
            && let Some(since) = refusal_started_at
        {
            crate::herdr_queue_wake_escalation::escalate_refusals(
                self, task_store, member, since, stats,
            )
            .await;
        }
        Some(refusal_count)
    }

    async fn open_tasks_by_member(
        &self,
        reader: &dyn AsyncTaskLedgerReader,
        candidates: &[MemberObservation],
    ) -> HashMap<MemberKey, Vec<TaskRow>> {
        let teams: HashSet<_> = candidates
            .iter()
            .map(|candidate| candidate.member.team().clone())
            .collect();
        let mut open: HashMap<MemberKey, Vec<TaskRow>> = HashMap::new();
        let deadline = match ReadDeadline::new(HERDR_REQUEST_BUDGET) {
            Ok(deadline) => deadline,
            Err(error) => {
                tracing::warn!(subsystem = "herdr_queue_wake", action = "task_reminder_read", outcome = "deadline_invalid", error = %error, "Herdr task reminder read skipped");
                return open;
            }
        };
        for team in teams {
            match reader.open_tasks_for_team(team.clone(), deadline).await {
                Ok(rows) => {
                    for row in rows {
                        open.entry(MemberKey::new(team.clone(), row.assignee.clone()))
                            .or_default()
                            .push(row);
                    }
                }
                Err(error) => {
                    tracing::warn!(subsystem = "herdr_queue_wake", action = "task_reminder_read", outcome = "failed", error = %error, team = %team, "Herdr open-task read failed");
                }
            }
        }
        open
    }

    async fn emit_task_reminder(
        &self,
        task_store: &Arc<dyn atm_core::boundary::TaskStore + Send + Sync>,
        candidate: MemberObservation,
        open: &[TaskRow],
        now: IsoTimestamp,
        stats: &mut HerdrQueueWakeStats,
    ) {
        let Some(row) = open.first().cloned() else {
            return;
        };
        let context = crate::herdr_queue_wake_escalation::TaskReminderContext {
            task_store,
            member: &candidate.member,
        };
        if stats.prompted >= HERDR_MAX_PROMPTS_PER_TICK {
            return;
        }
        let runtime = self.service_runtime.clone();
        let member = candidate.member.clone();
        let row_for_dispatch = row.clone();
        let open_tasks: Vec<TaskId> = open.iter().map(|task| task.task_id.clone()).collect();
        let dispatch = self
            .blocking_bridge
            .run(herdr_request_deadline(), move || {
                build_task_reminder_dispatch(&runtime, &member, &row_for_dispatch, &open_tasks)
            })
            .await;
        let dispatch = match dispatch {
            Ok(Some(dispatch)) => dispatch,
            Ok(None) => {
                tracing::warn!(subsystem = "herdr_queue_wake", action = "task_reminder_dispatch", outcome = "no_delivery_channel", member = %candidate.member, "Task reminder held because the member has no delivery channel");
                return;
            }
            Err(error) => {
                tracing::warn!(subsystem = "herdr_queue_wake", action = "task_reminder_render", outcome = "unrenderable", error = %error, member = %candidate.member, "Herdr task reminder could not render");
                self.record_task_outcome(&context, &row, now, ReminderOutcome::Unrenderable, stats)
                    .await;
                return;
            }
        };
        if !super::still_idle(&self.service_runtime, &candidate.member) {
            tracing::info!(
                event = "herdr_queue_poll_outcome",
                member = %candidate.member,
                outcome = "reminder_held_not_idle",
                "Herdr task reminder skipped after the live idle recheck"
            );
            return;
        }
        let Some(emitter) = self.selector.select_emitter(&dispatch) else {
            tracing::info!(event = "herdr_queue_poll_outcome", member = %candidate.member, outcome = "reminder_target_not_present", "Herdr task reminder selector returned no emitter");
            return;
        };
        match emitter
            .emit_received_message(dispatch.clone(), herdr_request_deadline())
            .await
        {
            Ok(_) => {
                self.record_task_outcome(&context, &row, now, ReminderOutcome::Emitted, stats)
                    .await;
                crate::prompt_handoff_record::record_prompt_handoff(
                    &self.blocking_bridge,
                    herdr_request_deadline(),
                    Ok(Arc::clone(task_store)),
                    &dispatch,
                    atm_core::boundary::PromptTrigger::TaskPass,
                    now,
                    &self.task_telemetry,
                )
                .await;
            }
            Err(error) if error.code() == AtmErrorCode::HerdrUnavailable => stats.breaker_open += 1,
            Err(error) => {
                stats.task_reminders_failed += 1;
                tracing::warn!(subsystem = "herdr_queue_wake", action = "task_reminder_emit", outcome = "failed", error = %error, error_code = ?error.code(), member = %candidate.member, "Herdr task reminder emission failed")
            }
        }
    }

    async fn record_task_outcome(
        &self,
        context: &crate::herdr_queue_wake_escalation::TaskReminderContext<'_>,
        row: &TaskRow,
        now: IsoTimestamp,
        outcome: ReminderOutcome,
        stats: &mut HerdrQueueWakeStats,
    ) {
        let recorded_row = self
            .record_task_reminder(context.task_store, context.member, row, now, outcome)
            .await;
        match outcome {
            ReminderOutcome::Emitted => {
                stats.prompted += 1;
                stats.task_reminders += 1;
            }
            ReminderOutcome::Unrenderable => stats.task_reminders_unrenderable += 1,
            // Retained for decoding durable pre-BA.3 reminder rows; this
            // runtime no longer produces a blocked reminder outcome.
            ReminderOutcome::Blocked => {}
        }
        self.warn_failed_reminder_record(context, row, recorded_row);
    }

    fn warn_failed_reminder_record(
        &self,
        context: &crate::herdr_queue_wake_escalation::TaskReminderContext<'_>,
        row: &TaskRow,
        recorded_row: Result<TaskRow, AtmError>,
    ) {
        if let Err(error) = recorded_row {
            tracing::warn!(
                subsystem = "herdr_queue_wake",
                action = "task_reminder_handoff",
                outcome = "failed",
                member = %context.member,
                task_id = %row.task_id,
                error = %error,
                "Task reminder was emitted but its durable handoff failed"
            );
        }
    }

    async fn record_task_reminder(
        &self,
        store: &Arc<dyn atm_core::boundary::TaskStore + Send + Sync>,
        member: &MemberKey,
        row: &TaskRow,
        now: IsoTimestamp,
        outcome: ReminderOutcome,
    ) -> Result<TaskRow, AtmError> {
        let store = Arc::clone(store);
        let member = member.clone();
        let task_id = row.task_id.clone();
        let write_member = member.clone();
        let write_task_id = task_id.clone();
        match self
            .blocking_bridge
            .run(herdr_request_deadline(), move || {
                store.record_reminder(&write_member, &write_task_id, now, outcome)
            })
            .await
        {
            Ok(record) => {
                crate::task_telemetry::project_task_events(
                    &self.task_telemetry,
                    std::slice::from_ref(&record.event),
                );
                Ok(record.row)
            }
            Err(error) => {
                tracing::warn!(subsystem = "herdr_queue_wake", action = "task_reminder_record", outcome = "failed", error = %error, member = %member, task_id = %task_id, "Herdr task reminder bookkeeping failed");
                Err(error)
            }
        }
    }

    /// Resets an open task's reminder budget after the assignee has been
    /// observed continuously active for at least one reminder interval. The
    /// budget counts from the assignee's last observed activity, not from
    /// task start.
    async fn reset_task_reminders(
        &self,
        task_store: &Arc<dyn atm_core::boundary::TaskStore + Send + Sync>,
        member: &MemberKey,
        row: &TaskRow,
        now: IsoTimestamp,
    ) {
        let store = Arc::clone(task_store);
        let member = member.clone();
        let task_id = row.task_id.clone();
        let write_member = member.clone();
        let write_task_id = task_id.clone();
        match self
            .blocking_bridge
            .run(herdr_request_deadline(), move || {
                store.reset_reminders(&write_member, &write_task_id, now)
            })
            .await
        {
            Ok(record) => {
                crate::task_telemetry::project_task_events(
                    &self.task_telemetry,
                    std::slice::from_ref(&record.event),
                );
                tracing::info!(
                    subsystem = "herdr_queue_wake",
                    action = "task_reminder_reset",
                    outcome = "reset",
                    member = %member,
                    task_id = %task_id,
                    "Herdr task reminder budget reset after sustained assignee activity"
                );
            }
            Err(error) => {
                tracing::warn!(
                    subsystem = "herdr_queue_wake",
                    action = "task_reminder_reset",
                    outcome = "failed",
                    error = %error,
                    member = %member,
                    task_id = %task_id,
                    "Herdr task reminder reset bookkeeping failed"
                );
            }
        }
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
}

impl PreparedTaskPass {
    /// Whether queue draining may proceed this tick for `member`'s pending
    /// mail. This mirrors [`dispose`]'s `EscalateStalled` trigger: draining is
    /// held only in the tick where a stall escalation is about to fire, not
    /// for the whole time a task remains open.
    pub(super) fn queue_drain_allowed(&self, member: &MemberKey) -> bool {
        self.open
            .get(member)
            .and_then(|rows| rows.first())
            .is_none_or(|head| !escalation_due(head))
    }
}
