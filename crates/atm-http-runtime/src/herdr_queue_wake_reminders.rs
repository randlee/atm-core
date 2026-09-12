//! Task-reminder and escalation pass for the Herdr queue wake pump.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use atm_core::api::RequestDeadline;
use atm_core::boundary::{
    AsyncTaskLedgerReader, MemberKey, ReadDeadline, ReminderOutcome, TaskRow,
};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::nudge_dispatch::build_task_reminder_dispatch;
use atm_core::types::IsoTimestamp;

use crate::herdr_task_disposition::{EpisodeKind, TaskDisposition, dispose};

use super::{
    HERDR_MAX_PROMPTS_PER_TICK, HERDR_REQUEST_DEADLINE, HerdrQueueWakePump, HerdrQueueWakeStats,
    MemberObservation, run_blocking,
};

impl HerdrQueueWakePump {
    pub(super) async fn remind_open_tasks(
        &self,
        candidates: Vec<MemberObservation>,
        open_mail: &HashSet<MemberKey>,
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
            .filter(|candidate| candidate.state == atm_core::protocol::RuntimeMemberState::Blocked)
            .map(|candidate| candidate.member.clone())
            .collect();
        if list_complete {
            self.escalation_state.prune_blocked(&blocked_members);
        }

        if let (Some(reader), Some(task_store)) = (reader.as_ref(), task_store.as_ref()) {
            let heads = self.open_task_heads(reader.as_ref(), &candidates).await;
            for candidate in candidates {
                let head = heads.get(&candidate.member);
                let disposition = dispose(
                    open_mail.contains(&candidate.member),
                    candidate.state,
                    head,
                    now,
                    self.escalation_state
                        .observe(&candidate.member, candidate.state),
                    0,
                );
                if matches!(
                    disposition,
                    TaskDisposition::EscalateEpisode(EpisodeKind::Blocked)
                ) {
                    if let Some(row) = head {
                        self.emit_task_reminder(
                            reader.as_ref(),
                            task_store,
                            candidate,
                            row.clone(),
                            now,
                            stats,
                        )
                        .await;
                    }
                    continue;
                }
                let TaskDisposition::Nudge = disposition else {
                    continue;
                };
                if stats.prompted >= HERDR_MAX_PROMPTS_PER_TICK {
                    continue;
                }
                let Some(row) = head.cloned() else {
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

    async fn open_task_heads(
        &self,
        reader: &dyn AsyncTaskLedgerReader,
        candidates: &[MemberObservation],
    ) -> HashMap<MemberKey, TaskRow> {
        let teams: HashSet<_> = candidates
            .iter()
            .map(|candidate| candidate.member.team().clone())
            .collect();
        let mut heads = HashMap::new();
        let deadline = match ReadDeadline::new(HERDR_REQUEST_DEADLINE) {
            Ok(deadline) => deadline,
            Err(error) => {
                tracing::warn!(subsystem = "herdr_queue_wake", action = "task_reminder_read", outcome = "deadline_invalid", error = %error, "Herdr task reminder read skipped");
                return heads;
            }
        };
        for team in teams {
            match reader.open_tasks_for_team(team.clone(), deadline).await {
                Ok(rows) => {
                    for row in rows {
                        heads
                            .entry(MemberKey::new(team.clone(), row.assignee.clone()))
                            .or_insert(row);
                    }
                }
                Err(error) => {
                    tracing::warn!(subsystem = "herdr_queue_wake", action = "task_reminder_read", outcome = "failed", error = %error, team = %team, "Herdr open-task read failed");
                }
            }
        }
        heads
    }

    async fn emit_task_reminder(
        &self,
        reader: &(dyn AsyncTaskLedgerReader + Send + Sync),
        task_store: &Arc<dyn atm_core::boundary::TaskStore + Send + Sync>,
        candidate: MemberObservation,
        row: TaskRow,
        now: IsoTimestamp,
        stats: &mut HerdrQueueWakeStats,
    ) {
        let context = crate::herdr_queue_wake_escalation::TaskReminderContext {
            reader,
            task_store,
            member: &candidate.member,
        };
        if candidate.state == atm_core::protocol::RuntimeMemberState::Blocked {
            self.record_task_outcome(&context, &row, now, ReminderOutcome::Blocked, stats)
                .await;
            return;
        }
        if stats.prompted >= HERDR_MAX_PROMPTS_PER_TICK {
            return;
        }
        let runtime = self.service_runtime.clone();
        let member = candidate.member.clone();
        let row_for_dispatch = row.clone();
        let dispatch = run_blocking(move || {
            build_task_reminder_dispatch(&runtime, &member, &row_for_dispatch)
        })
        .await;
        let dispatch = match dispatch {
            Ok(Some(dispatch)) => dispatch,
            Ok(None) => return,
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
                self.stamp_task_attempt(&candidate.member, now);
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
