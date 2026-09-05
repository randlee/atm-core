//! Task and blocked-runtime escalation for the Herdr queue wake pump.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use atm_core::boundary::{
    AsyncTaskLedgerReader, MemberKey, ReadDeadline, ReminderOutcome, TaskEventRow, TaskRow,
    TaskState,
};
use atm_core::types::IsoTimestamp;

use crate::herdr_escalation::{BLOCKED_NOTIFY_MS, BLOCKED_RENOTIFY_MS, escalate};
use crate::herdr_queue_wake::{HerdrQueueWakePump, HerdrQueueWakeStats, run_blocking};

const TASK_READ_DEADLINE: Duration = Duration::from_secs(5);

pub(crate) async fn maybe_escalate_task(
    pump: &HerdrQueueWakePump,
    task_store: &Arc<dyn atm_core::boundary::TaskStore + Send + Sync>,
    row: &TaskRow,
    now: IsoTimestamp,
    stats: &mut HerdrQueueWakeStats,
) {
    let threshold = row.lead_notified_count.saturating_add(1).saturating_mul(10);
    if row.reminder_count < threshold {
        return;
    }
    let events = match reminder_events(task_store, row).await {
        Ok(events) => events,
        Err(error) => {
            tracing::warn!(
                subsystem = "herdr_queue_wake",
                action = "task_escalation_events",
                outcome = "failed",
                task_id = %row.task_id,
                error = %error,
                "Task escalation skipped because reminder events could not be read"
            );
            return;
        }
    };
    let body = task_escalation_body(row, now, &events);
    let outcome = escalate(
        &pump.service_runtime,
        pump.herdr_process.as_ref(),
        Some(task_store),
        &pump.daemon_home,
        &row.team,
        &body,
        "lead_notified",
    )
    .await;
    record_escalation_stats(stats, &outcome);
    if let (Some(lead), Some(message_id)) = (outcome.lead, outcome.lead_write) {
        record_lead_audit(task_store, row, now, lead, message_id, stats).await;
    }
}

async fn reminder_events(
    task_store: &Arc<dyn atm_core::boundary::TaskStore + Send + Sync>,
    row: &TaskRow,
) -> Result<Vec<TaskEventRow>, atm_core::error::AtmError> {
    let store = Arc::clone(task_store);
    let team = row.team.clone();
    let task_id = row.task_id.clone();
    let assignee = row.assignee.clone();
    run_blocking(move || store.list_task_events(&team, &task_id, Some(&assignee))).await
}

fn task_escalation_body(row: &TaskRow, now: IsoTimestamp, events: &[TaskEventRow]) -> String {
    let first = events
        .iter()
        .find(|event| event.event.as_str() == "reminded")
        .map(|event| event.at)
        .unwrap_or(row.assigned_at);
    let outcome = events
        .iter()
        .rev()
        .find(|event| event.event.as_str() == "reminded")
        .and_then(|event| event.outcome)
        .map(ReminderOutcome::as_str)
        .unwrap_or("unknown");
    format!(
        "task {} assigned to {} by {} has been reminded {} times\n(first {}, last {}, last outcome {}).\nRun: atm list --task-events {} --member {}",
        row.task_id,
        row.assignee,
        row.assigner,
        row.reminder_count,
        first,
        row.last_reminded_at.unwrap_or(now),
        outcome,
        row.task_id,
        row.assignee,
    )
}

async fn record_lead_audit(
    task_store: &Arc<dyn atm_core::boundary::TaskStore + Send + Sync>,
    row: &TaskRow,
    now: IsoTimestamp,
    lead: atm_core::types::AgentName,
    message_id: atm_core::schema::AtmMessageId,
    stats: &mut HerdrQueueWakeStats,
) {
    let store = Arc::clone(task_store);
    let member = MemberKey::new(row.team.clone(), row.assignee.clone());
    let task_id = row.task_id.clone();
    if let Err(error) =
        run_blocking(move || store.record_lead_notified(&member, &task_id, now, &lead, &message_id))
            .await
    {
        tracing::warn!(
            subsystem = "herdr_queue_wake",
            action = "task_lead_notification_record",
            outcome = "failed",
            task_id = %row.task_id,
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
    for member in members {
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
    let since = blocked_start(pump, member, now);
    if elapsed_millis(now, since) < BLOCKED_NOTIFY_MS || blocked_cooldown(pump, member, now) {
        return;
    }
    let open_tasks = blocked_tasks(reader, member).await;
    let body = blocked_body(member, since, now, &open_tasks);
    let outcome = escalate(
        &pump.service_runtime,
        pump.herdr_process.as_ref(),
        task_store,
        &pump.daemon_home,
        member.team(),
        &body,
        "blocked_escalated",
    )
    .await;
    record_escalation_stats(stats, &outcome);
    if outcome.reached_anyone() {
        pump.last_blocked_notice
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(member.clone(), now);
        stats.blocked_escalations += 1;
    }
}

fn blocked_start(pump: &HerdrQueueWakePump, member: &MemberKey, now: IsoTimestamp) -> IsoTimestamp {
    let mut blocked_since = pump
        .blocked_since
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *blocked_since.entry(member.clone()).or_insert(now)
}

fn blocked_cooldown(pump: &HerdrQueueWakePump, member: &MemberKey, now: IsoTimestamp) -> bool {
    pump.last_blocked_notice
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(member)
        .is_some_and(|last| elapsed_millis(now, *last) < BLOCKED_RENOTIFY_MS)
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
            .map(|row| {
                format!(
                    "{} (assigned by {}, {} reminders)",
                    row.task_id, row.assigner, row.reminder_count
                )
            })
            .collect::<Vec<_>>()
            .join(" | ")
    };
    format!(
        "{} has been waiting for interactive input since {} ({})\nopen tasks: {}\nAttach to its Herdr agent and answer the prompt. Run: atm members --team {}",
        member.agent(),
        since,
        format_age(elapsed_millis(now, since)),
        tasks,
        member.team(),
    )
}

fn format_age(milliseconds: u64) -> String {
    format!("{}s", milliseconds / 1_000)
}
