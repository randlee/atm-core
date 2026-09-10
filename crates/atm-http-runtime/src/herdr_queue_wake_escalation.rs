//! Task and blocked-runtime escalation for the Herdr queue wake pump.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use atm_core::boundary::{
    AsyncTaskLedgerReader, LogicalTaskRow, MemberKey, ReadDeadline, TaskMutationRequest,
    TaskOperation, TaskOperationId, TaskRow, TaskState,
};
use atm_core::types::IsoTimestamp;

use crate::herdr_escalation::{
    BLOCKED_NOTIFY_MS, EscalationKind, EscalationNotification, MAX_BLOCKED_ESCALATIONS_PER_TICK,
    escalate,
};
use crate::herdr_queue_wake::{HerdrQueueWakePump, HerdrQueueWakeStats};

const TASK_READ_DEADLINE: Duration = Duration::from_secs(5);
const MAX_BLOCKED_TASKS_IN_BODY: usize = 8;
const MAX_BLOCKED_MAIL_BODY_BYTES: usize = 4_096;

struct LeadAudit<'a> {
    member: &'a MemberKey,
    row: &'a LogicalTaskRow,
    reminder_revision: u64,
    at: IsoTimestamp,
    lead: atm_core::types::AgentName,
    message_id: atm_core::schema::AtmMessageId,
}

pub(crate) async fn maybe_escalate_task(
    pump: &HerdrQueueWakePump,
    member: &MemberKey,
    row: &LogicalTaskRow,
    reminder_revision: u64,
    now: IsoTimestamp,
    stats: &mut HerdrQueueWakeStats,
) {
    if !row.reminder_ordinal.is_multiple_of(u64::from(
        atm_core::boundary::TASK_STALLED_REMINDER_THRESHOLD,
    )) {
        return;
    }
    let task_store = match pump.service_runtime.task_store() {
        Ok(store) => store,
        Err(error) => {
            tracing::warn!(
                subsystem = "herdr_queue_wake",
                action = "task_lead_escalation_store",
                outcome = "failed",
                task_id = %row.task_id,
                error = %error,
                "Task escalation skipped because the task store is unavailable"
            );
            return;
        }
    };
    let body = task_escalation_body(member, row);
    let notification = task_escalation_notification(member, row);
    let outcome = escalate(
        &pump.service_runtime,
        pump.herdr_process.as_ref(),
        Some(&task_store),
        &pump.daemon_home,
        &row.team,
        &body,
        &notification,
        EscalationKind::LeadNotified,
    )
    .await;
    record_escalation_stats(stats, &outcome);
    if let (Some(lead), Some(message_id)) = (outcome.lead, outcome.lead_write) {
        record_lead_audit(
            pump,
            LeadAudit {
                member,
                row,
                reminder_revision,
                at: now,
                lead,
                message_id,
            },
            stats,
        )
        .await;
    }
}

fn task_escalation_body(member: &MemberKey, row: &LogicalTaskRow) -> String {
    format!(
        "task {} assigned to {} has reached reminder ordinal {}.\nRun: atm task list {}",
        row.task_id, row.current_assignee, row.reminder_ordinal, member,
    )
}

fn task_escalation_notification(
    member: &MemberKey,
    row: &LogicalTaskRow,
) -> EscalationNotification {
    EscalationNotification {
        title: "ATM task escalation".to_owned(),
        body: format!(
            "reason=lead_notified task_id={} member={} reminder_ordinal={} remediation=atm task list {}",
            row.task_id, member, row.reminder_ordinal, member,
        ),
    }
}

async fn record_lead_audit(
    pump: &HerdrQueueWakePump,
    audit: LeadAudit<'_>,
    stats: &mut HerdrQueueWakeStats,
) {
    let mutation_store = match pump.service_runtime.async_task_mutation_store() {
        Ok(store) => store,
        Err(error) => {
            tracing::warn!(
                subsystem = "herdr_queue_wake",
                action = "task_lead_notification_record",
                outcome = "failed",
                task_id = %audit.row.task_id,
                error = %error,
                "Task lead notification audit store is unavailable"
            );
            return;
        }
    };
    let daemon_actor =
        atm_core::types::AgentName::from_validated(atm_core::boundary::DAEMON_ACTOR_NAME);
    if let Err(error) = mutation_store
        .apply(TaskMutationRequest {
            operation_id: TaskOperationId::new(),
            actor: MemberKey::new(audit.member.team().clone(), daemon_actor),
            task_id: audit.row.task_id.clone(),
            expected_revision: Some(audit.reminder_revision),
            operation: TaskOperation::RecordLeadNotified {
                attempt: audit.row.current_attempt,
                at: audit.at,
                lead: audit.lead,
                message_id: audit.message_id,
            },
        })
        .await
    {
        tracing::warn!(
            subsystem = "herdr_queue_wake",
            action = "task_lead_notification_record",
            outcome = "failed",
            task_id = %audit.row.task_id,
            error = %error,
            "Task lead notification audit write failed"
        );
    } else {
        stats.lead_notifications += 1;
    }
}

fn record_escalation_stats(
    stats: &mut HerdrQueueWakeStats,
    outcome: &crate::herdr_escalation::EscalationOutcome,
) {
    stats.escalation_writes_failed = stats
        .escalation_writes_failed
        .saturating_add(outcome.recipients_failed as usize);
    stats.notifications_failed = stats
        .notifications_failed
        .saturating_add(usize::from(!outcome.notify_ok));
}

pub(crate) async fn escalate_blocked(
    pump: &HerdrQueueWakePump,
    blocked_members: &HashSet<MemberKey>,
    reader: Option<&(dyn AsyncTaskLedgerReader + Send + Sync)>,
    task_store: Option<&Arc<dyn atm_core::boundary::TaskStore + Send + Sync>>,
    now: IsoTimestamp,
    stats: &mut HerdrQueueWakeStats,
) {
    let mut members: Vec<_> = blocked_members.iter().cloned().collect();
    members.sort_by(|left, right| {
        left.team()
            .as_str()
            .cmp(right.team().as_str())
            .then_with(|| left.agent().as_str().cmp(right.agent().as_str()))
    });
    for member in pump
        .escalation_state
        .next_blocked_batch(&members)
        .into_iter()
        .take(MAX_BLOCKED_ESCALATIONS_PER_TICK)
    {
        escalate_one_blocked(pump, &member, reader, task_store, now, stats).await;
    }
}

async fn escalate_one_blocked(
    pump: &HerdrQueueWakePump,
    member: &MemberKey,
    reader: Option<&(dyn AsyncTaskLedgerReader + Send + Sync)>,
    task_store: Option<&Arc<dyn atm_core::boundary::TaskStore + Send + Sync>>,
    now: IsoTimestamp,
    stats: &mut HerdrQueueWakeStats,
) {
    let since = pump.escalation_state.blocked_start(member, now);
    if elapsed_millis(now, since) < BLOCKED_NOTIFY_MS
        || pump.escalation_state.blocked_cooldown(member, now)
    {
        return;
    }
    let open_tasks = blocked_tasks(reader, member).await;
    let body = blocked_body(member, since, now, &open_tasks);
    let notification = blocked_notification(member, since, now, &open_tasks);
    let outcome = escalate(
        &pump.service_runtime,
        pump.herdr_process.as_ref(),
        task_store,
        &pump.daemon_home,
        member.team(),
        &body,
        &notification,
        EscalationKind::BlockedEscalated,
    )
    .await;
    record_escalation_stats(stats, &outcome);
    if outcome.reached_anyone() {
        pump.escalation_state.stamp_blocked_notice(member, now);
        stats.blocked_escalations += 1;
    }
}

async fn blocked_tasks(
    reader: Option<&(dyn AsyncTaskLedgerReader + Send + Sync)>,
    member: &MemberKey,
) -> Vec<TaskRow> {
    let Some(reader) = reader else {
        return Vec::new();
    };
    let Ok(deadline) = ReadDeadline::new(TASK_READ_DEADLINE) else {
        return Vec::new();
    };
    match reader
        .list_tasks(
            member.team().clone(),
            Some(member.agent().clone()),
            deadline,
        )
        .await
    {
        Ok(rows) => rows
            .into_iter()
            .filter(|row| row.state != TaskState::Complete)
            .collect(),
        Err(error) => {
            tracing::warn!(
                subsystem = "herdr_queue_wake",
                action = "blocked_escalation_tasks",
                outcome = "failed",
                member = %member,
                error = %error,
                "Blocked escalation task read failed"
            );
            Vec::new()
        }
    }
}

fn elapsed_millis(now: IsoTimestamp, since: IsoTimestamp) -> u64 {
    now.into_inner()
        .signed_duration_since(since.into_inner())
        .num_milliseconds()
        .max(0) as u64
}

fn blocked_body(
    member: &MemberKey,
    since: IsoTimestamp,
    now: IsoTimestamp,
    open_tasks: &[TaskRow],
) -> String {
    let tasks = if open_tasks.is_empty() {
        "none".to_owned()
    } else {
        open_tasks
            .iter()
            .take(MAX_BLOCKED_TASKS_IN_BODY)
            .map(|row| {
                format!(
                    "{} (assigned by {}, {} reminders)",
                    row.task_id, row.assigner, row.reminder_count
                )
            })
            .collect::<Vec<_>>()
            .join(" | ")
    };
    truncate_body(format!(
        "{} has been waiting for interactive input since {} ({})\nopen tasks: {}\nAttach to its Herdr agent and answer the prompt. Run: atm members --team {}",
        member.agent(),
        since,
        format_age(elapsed_millis(now, since)),
        tasks,
        member.team(),
    ))
}

fn blocked_notification(
    member: &MemberKey,
    since: IsoTimestamp,
    now: IsoTimestamp,
    open_tasks: &[TaskRow],
) -> EscalationNotification {
    let task_ids = open_tasks
        .iter()
        .take(MAX_BLOCKED_TASKS_IN_BODY)
        .map(|row| row.task_id.to_string())
        .collect::<Vec<_>>()
        .join(",");
    EscalationNotification {
        title: "ATM blocked member escalation".to_owned(),
        body: format!(
            "reason=blocked member={} age={} since={} task_ids={} remediation=atm members --team {}",
            member.agent(),
            format_age(elapsed_millis(now, since)),
            since,
            if task_ids.is_empty() {
                "none"
            } else {
                &task_ids
            },
            member.team(),
        ),
    }
}

fn truncate_body(mut body: String) -> String {
    if body.len() > MAX_BLOCKED_MAIL_BODY_BYTES {
        let mut end = MAX_BLOCKED_MAIL_BODY_BYTES;
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        body.truncate(end);
        body.push('…');
    }
    body
}

fn format_age(milliseconds: u64) -> String {
    format!("{}s", milliseconds / 1_000)
}
