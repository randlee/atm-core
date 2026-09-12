//! Task-reminder and escalation pass for the Herdr queue wake pump.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use atm_core::api::RequestDeadline;
use atm_core::boundary::{
    AsyncTaskLedgerReader, MemberKey, ReadDeadline, ReminderOutcome,
    TASK_CONSECUTIVE_REFUSAL_THRESHOLD, TaskRow,
};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::nudge_dispatch::build_task_reminder_dispatch;
use atm_core::types::IsoTimestamp;

use crate::herdr_task_disposition::{TaskDisposition, dispose};

use super::{
    HERDR_MAX_PROMPTS_PER_TICK, HERDR_REQUEST_DEADLINE, HerdrQueueWakePump, HerdrQueueWakeStats,
    MemberObservation, run_blocking,
};

pub(super) struct PreparedTaskPass {
    reader: Arc<dyn AsyncTaskLedgerReader + Send + Sync>,
    task_store: Arc<dyn atm_core::boundary::TaskStore + Send + Sync>,
    heads: HashMap<MemberKey, TaskRow>,
}

impl HerdrQueueWakePump {
    pub(super) async fn prepare_task_pass(
        &self,
        stats: &mut HerdrQueueWakeStats,
        candidates: &[MemberObservation],
    ) -> Option<PreparedTaskPass> {
        let (reader, task_store) = self.task_capabilities(stats)?;
        self.note_task_step_availability(true, None);
        let heads = self.open_task_heads(reader.as_ref(), candidates).await;
        self.start_owed_tasks(&heads).await;
        Some(PreparedTaskPass {
            reader,
            task_store,
            heads,
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
                &prepared.heads,
                candidate,
                open_mail,
                now,
                stats,
            )
            .await;
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

    async fn start_owed_tasks(&self, heads: &HashMap<MemberKey, TaskRow>) {
        for head in heads.values().filter(|head| {
            head.state == atm_core::boundary::TaskState::Assigned && head.last_reminded_at.is_some()
        }) {
            if let Err(error) = crate::herdr_task_start::start_assigned_task(
                &self.service_runtime,
                &self.daemon_home,
                head,
            )
            .await
            {
                tracing::warn!(
                    subsystem = "herdr_queue_wake",
                    action = "task_start_owed",
                    outcome = "failed",
                    member = %head.assignee,
                    task_id = %head.task_id,
                    error = %error,
                    "Owed task start could not be completed"
                );
            }
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the disposition boundary keeps each durable input explicit"
    )]
    async fn process_task_candidate(
        &self,
        reader: &(dyn AsyncTaskLedgerReader + Send + Sync),
        task_store: &Arc<dyn atm_core::boundary::TaskStore + Send + Sync>,
        heads: &HashMap<MemberKey, TaskRow>,
        candidate: MemberObservation,
        open_mail: &HashSet<MemberKey>,
        now: IsoTimestamp,
        stats: &mut HerdrQueueWakeStats,
    ) {
        let head = heads.get(&candidate.member);
        let (refusal_count, refusal_started_at) = self.refusal_run(reader, &candidate.member).await;
        if refusal_count >= TASK_CONSECUTIVE_REFUSAL_THRESHOLD
            && let Some(since) = refusal_started_at
        {
            crate::herdr_queue_wake_escalation::escalate_refusals(
                self,
                task_store,
                &candidate.member,
                since,
                stats,
            )
            .await;
        }
        let disposition = dispose(
            open_mail.contains(&candidate.member),
            candidate.state,
            head,
            now,
            self.escalation_state
                .observe(&candidate.member, candidate.state),
            refusal_count,
        );
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
                if stats.prompted < HERDR_MAX_PROMPTS_PER_TICK
                    && let Some(row) = head.cloned()
                {
                    self.emit_task_reminder(task_store, candidate, row, now, stats)
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
    ) -> (u32, Option<IsoTimestamp>) {
        let Ok(deadline) = ReadDeadline::new(HERDR_REQUEST_DEADLINE) else {
            return (0, None);
        };
        let Ok(run) = reader
            .refusal_run(member.team().clone(), member.agent().clone(), deadline)
            .await
        else {
            return (0, None);
        };
        (run.count, run.started_at)
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
        task_store: &Arc<dyn atm_core::boundary::TaskStore + Send + Sync>,
        candidate: MemberObservation,
        row: TaskRow,
        now: IsoTimestamp,
        stats: &mut HerdrQueueWakeStats,
    ) {
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
        let dispatch = run_blocking(move || {
            build_task_reminder_dispatch(&runtime, &member, &row_for_dispatch)
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
        let recorded_row = if outcome == ReminderOutcome::Emitted {
            crate::herdr_task_start::complete_task_handoff(
                &self.service_runtime,
                context.task_store,
                &self.daemon_home,
                context.member,
                row,
                now,
            )
            .await
        } else {
            self.record_task_reminder(context.task_store, context.member, row, now, outcome)
                .await
        };
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
