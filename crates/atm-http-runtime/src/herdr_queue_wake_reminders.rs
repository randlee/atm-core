//! Task-reminder and escalation pass for the Herdr queue wake pump.

use std::collections::HashSet;
use std::sync::Arc;

use atm_core::api::RequestDeadline;
use atm_core::boundary::{
    AsyncTaskLedgerReader, MemberKey, ReadDeadline, ReminderOutcome, TaskRow,
};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::nudge_dispatch::build_task_reminder_dispatch;
use atm_core::types::IsoTimestamp;

use super::{
    HERDR_MAX_PROMPTS_PER_TICK, HERDR_REQUEST_DEADLINE, HerdrQueueWakePump, HerdrQueueWakeStats,
    TASK_REMINDER_INTERVAL_MS, TaskCandidate, run_blocking, select_open_task,
};

impl HerdrQueueWakePump {
    pub(super) async fn remind_open_tasks(
        &self,
        candidates: Vec<TaskCandidate>,
        prompted_by_drain: &HashSet<MemberKey>,
        list_complete: bool,
        stats: &mut HerdrQueueWakeStats,
    ) {
        let reader = match self.service_runtime.async_task_ledger_reader() {
            Ok(reader) => Some(reader),
            Err(error) => {
                stats.task_step_skipped = true;
                self.note_task_step_availability(false, Some(&error));
                None
            }
        };
        let task_store = match self.service_runtime.task_store() {
            Ok(store) => Some(store),
            Err(error) => {
                stats.task_step_skipped = true;
                self.note_task_step_availability(false, Some(&error));
                None
            }
        };
        if reader.is_some() && task_store.is_some() {
            self.note_task_step_availability(true, None);
        }
        let now = (self.clock)();
        let blocked_members: HashSet<_> = candidates
            .iter()
            .filter(|candidate| candidate.blocked)
            .map(|candidate| candidate.member.key.clone())
            .collect();
        if list_complete {
            self.escalation_state.prune_blocked(&blocked_members);
        }

        if let (Some(reader), Some(task_store)) = (reader.as_ref(), task_store.as_ref()) {
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
                self.emit_task_reminder(reader.as_ref(), task_store, candidate, row, now, stats)
                    .await;
            }
        }
        self.escalate_blocked(
            &blocked_members,
            reader.as_ref().map(Arc::as_ref),
            task_store.as_ref(),
            now,
            stats,
        )
        .await;
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
        reader: &(dyn AsyncTaskLedgerReader + Send + Sync),
        task_store: &Arc<dyn atm_core::boundary::TaskStore + Send + Sync>,
        candidate: TaskCandidate,
        row: TaskRow,
        now: IsoTimestamp,
        stats: &mut HerdrQueueWakeStats,
    ) {
        let context = crate::herdr_queue_wake_escalation::TaskReminderContext {
            reader,
            task_store,
            member: &candidate.member.key,
        };
        if candidate.blocked {
            self.record_task_outcome(&context, &row, now, ReminderOutcome::Blocked, stats)
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
                self.record_task_outcome(&context, &row, now, ReminderOutcome::Unrenderable, stats)
                    .await;
                return;
            }
        };
        if !super::still_idle(&self.service_runtime, &candidate.member.key) {
            tracing::info!(
                event = "herdr_queue_poll_outcome",
                member = %candidate.member.key,
                outcome = "reminder_held_not_idle",
                "Herdr task reminder skipped after the live idle recheck"
            );
            return;
        }
        let Some(emitter) = self.selector.select_emitter(&dispatch) else {
            tracing::info!(event = "herdr_queue_poll_outcome", member = %candidate.member.key, outcome = "reminder_target_not_present", "Herdr task reminder selector returned no emitter");
            return;
        };
        match emitter
            .emit_received_message(dispatch, RequestDeadline::after(HERDR_REQUEST_DEADLINE))
            .await
        {
            Ok(_) => {
                self.record_task_outcome(&context, &row, now, ReminderOutcome::Emitted, stats)
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
            ReminderOutcome::Blocked => stats.task_reminders_blocked += 1,
        }
        self.stamp_task_attempt(context.member, now);
        if let Ok(recorded_row) = recorded_row {
            crate::herdr_queue_wake_escalation::maybe_escalate_task(
                self,
                context.reader,
                context.task_store,
                &recorded_row,
                now,
                stats,
            )
            .await;
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
        match run_blocking(move || {
            store.record_reminder(&write_member, &write_task_id, now, outcome)
        })
        .await
        {
            Ok(row) => Ok(row),
            Err(error) => {
                tracing::warn!(subsystem = "herdr_queue_wake", action = "task_reminder_record", outcome = "failed", error = %error, member = %member, task_id = %task_id, "Herdr task reminder bookkeeping failed");
                Err(error)
            }
        }
    }

    async fn escalate_blocked(
        &self,
        blocked_members: &HashSet<MemberKey>,
        reader: Option<&(dyn AsyncTaskLedgerReader + Send + Sync)>,
        task_store: Option<&Arc<dyn atm_core::boundary::TaskStore + Send + Sync>>,
        now: IsoTimestamp,
        stats: &mut HerdrQueueWakeStats,
    ) {
        crate::herdr_queue_wake_escalation::escalate_blocked(
            self,
            blocked_members,
            reader,
            task_store,
            now,
            stats,
        )
        .await;
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
