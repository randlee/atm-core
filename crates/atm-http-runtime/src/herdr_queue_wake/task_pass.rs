//! Task-reminder and escalation pass for the Herdr queue wake pump.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use atm_core::boundary::{
    AsyncTaskLedgerReader, MemberKey, ReadDeadline, ReminderOutcome,
    TASK_CONSECUTIVE_REFUSAL_THRESHOLD, TaskRow,
};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::nudge_dispatch::build_task_reminder_dispatch;
use atm_core::protocol::RuntimeMemberState;
use atm_core::types::IsoTimestamp;
use atm_herdr::{AgentSnapshot, HerdrAgentStatus};

use crate::herdr_task_disposition::{TaskDisposition, dispose};

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

    pub(super) async fn record_queue_prompt_reminders(
        &self,
        prepared: &PreparedTaskPass,
        prompted: &HashMap<MemberKey, atm_core::schema::AtmMessageId>,
        stats: &mut HerdrQueueWakeStats,
    ) {
        let now = (self.clock)();
        for (member, message_id) in prompted {
            let Some(row) = prepared.queue_reminder_head(member) else {
                continue;
            };
            let context = crate::herdr_queue_wake_escalation::TaskReminderContext {
                task_store: &prepared.task_store,
                member,
            };
            let recorded_row = if self
                .queue_prompt_is_head_assignment(member, *message_id, row)
                .await
            {
                crate::herdr_task_start::complete_task_handoff(
                    self,
                    context.task_store,
                    &self.daemon_home,
                    context.member,
                    row,
                    now,
                )
                .await
            } else {
                self.record_task_reminder(
                    context.task_store,
                    context.member,
                    row,
                    now,
                    ReminderOutcome::Emitted,
                )
                .await
            };
            stats.task_reminders += 1;
            self.warn_failed_reminder_record(&context, row, recorded_row);
        }
    }

    async fn queue_prompt_is_head_assignment(
        &self,
        member: &MemberKey,
        message_id: atm_core::schema::AtmMessageId,
        row: &TaskRow,
    ) -> bool {
        if row.state != atm_core::boundary::TaskState::Assigned
            || row.position.is_none_or(|position| position.get() != 1)
        {
            return false;
        }
        let runtime = self.service_runtime.clone();
        let member = member.clone();
        matches!(
            self.blocking_bridge
                .run(super::herdr_request_deadline(), move || {
                    super::load_received_hook_dispatch_message(&runtime, &member, message_id)
                })
                .await,
            Ok(Some(message)) if message.envelope.task_id.as_ref() == Some(&row.task_id)
        )
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
            if !snapshots.contains_key(target.agent.as_str()) {
                if member.pending {
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

    async fn start_owed_tasks(&self, heads: &HashMap<MemberKey, TaskRow>) {
        for head in heads.values().filter(|head| {
            head.state == atm_core::boundary::TaskState::Assigned && head.last_reminded_at.is_some()
        }) {
            if let Err(error) =
                crate::herdr_task_start::start_assigned_task(self, &self.daemon_home, head).await
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
        let (refusal_count, refusal_started_at) =
            match self.refusal_run(reader, &candidate.member).await {
                Ok(run) => run,
                Err(error) => {
                    tracing::warn!(
                        subsystem = "herdr_queue_wake",
                        action = "refusal_history_read",
                        outcome = "failed",
                        member = %candidate.member,
                        error = %error,
                        "Task disposition held because refusal history is unavailable"
                    );
                    return;
                }
            };
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
    ) -> Result<(u32, Option<IsoTimestamp>), AtmError> {
        let deadline = ReadDeadline::new(HERDR_REQUEST_BUDGET)?;
        reader
            .refusal_run(member.team().clone(), member.agent().clone(), deadline)
            .await
            .map(|run| (run.count, run.started_at))
            .map_err(Into::into)
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
        let deadline = match ReadDeadline::new(HERDR_REQUEST_BUDGET) {
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
        let dispatch = self
            .blocking_bridge
            .run(herdr_request_deadline(), move || {
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
                self.record_task_outcome(
                    &context,
                    &row,
                    now,
                    ReminderOutcome::Unrenderable,
                    false,
                    stats,
                )
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
            .emit_received_message(dispatch, herdr_request_deadline())
            .await
        {
            Ok(_) => {
                self.record_task_outcome(
                    &context,
                    &row,
                    now,
                    ReminderOutcome::Emitted,
                    false,
                    stats,
                )
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
        prompt_already_counted: bool,
        stats: &mut HerdrQueueWakeStats,
    ) {
        let recorded_row = if outcome == ReminderOutcome::Emitted {
            crate::herdr_task_start::complete_task_handoff(
                self,
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
                if !prompt_already_counted {
                    stats.prompted += 1;
                }
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

impl PreparedTaskPass {
    pub(super) fn queue_drain_allowed(&self, member: &MemberKey) -> bool {
        self.heads.get(member).is_none_or(|head| {
            head.lead_notified_count > 0
                || head.reminder_count < atm_core::boundary::TASK_STALLED_REMINDER_THRESHOLD
        })
    }

    fn queue_reminder_head(&self, member: &MemberKey) -> Option<&TaskRow> {
        self.heads.get(member).filter(|head| {
            head.lead_notified_count == 0
                && head.reminder_count < atm_core::boundary::TASK_STALLED_REMINDER_THRESHOLD
        })
    }
}
