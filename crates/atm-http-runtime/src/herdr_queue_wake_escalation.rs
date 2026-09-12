//! Task and blocked-runtime escalation for the Herdr queue wake pump.

use std::sync::Arc;
use std::time::Duration;

use atm_core::boundary::{
    AsyncTaskLedgerReader, MemberKey, ReadDeadline, ReminderOutcome, TaskEventKind, TaskEventRow,
    TaskRow,
};
use atm_core::types::IsoTimestamp;

use crate::herdr_escalation::{EscalationKind, escalate_mail, escalation_summary};
use crate::herdr_queue_wake::{HerdrQueueWakePump, HerdrQueueWakeStats, herdr_request_deadline};
use crate::herdr_task_disposition::EpisodeKind;

const TASK_READ_DEADLINE: Duration = Duration::from_secs(5);
const MAX_BLOCKED_MAIL_BODY_BYTES: usize = 4_096;

pub(crate) struct TaskReminderContext<'a> {
    pub(crate) task_store: &'a Arc<dyn atm_core::boundary::TaskStore + Send + Sync>,
    pub(crate) member: &'a MemberKey,
}

pub(crate) async fn escalate_stalled_task(
    pump: &HerdrQueueWakePump,
    reader: &(dyn AsyncTaskLedgerReader + Send + Sync),
    task_store: &Arc<dyn atm_core::boundary::TaskStore + Send + Sync>,
    row: &TaskRow,
    now: IsoTimestamp,
    stats: &mut HerdrQueueWakeStats,
) {
    let events = match reminder_events(reader, row).await {
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
    let outcome = escalate_mail(
        &pump.blocking_bridge,
        &pump.service_runtime,
        Some(task_store),
        &pump.daemon_home,
        &row.team,
        &escalation_summary(
            EscalationKind::TaskStalled,
            &MemberKey::new(row.team.clone(), row.assignee.clone()),
            Some(&row.task_id),
        ),
        &body,
        EscalationKind::TaskStalled,
        None,
    )
    .await;
    record_escalation_stats(stats, &outcome);
    let (audit_actor, message_id) = outcome.audit_delivery.unwrap_or_else(|| {
        (
            atm_core::types::AgentName::from_validated(atm_core::boundary::DAEMON_ACTOR_NAME),
            atm_core::schema::AtmMessageId::new(),
        )
    });
    record_stall_audit(pump, task_store, row, now, audit_actor, message_id, stats).await;
}

async fn reminder_events(
    reader: &(dyn AsyncTaskLedgerReader + Send + Sync),
    row: &TaskRow,
) -> Result<Vec<TaskEventRow>, atm_core::error::AtmError> {
    let deadline = ReadDeadline::new(TASK_READ_DEADLINE)
        .map_err(|error| atm_core::error::AtmError::daemon_unavailable(error.to_string()))?;
    reader
        .list_task_events(
            row.team.clone(),
            row.task_id.clone(),
            Some(row.assignee.clone()),
            deadline,
        )
        .await
        .map_err(|error| atm_core::error::AtmError::daemon_unavailable(error.to_string()))
}

fn task_escalation_body(row: &TaskRow, now: IsoTimestamp, events: &[TaskEventRow]) -> String {
    let first = events
        .iter()
        .find(|event| event.event == TaskEventKind::Reminded)
        .map(|event| event.at)
        .unwrap_or(row.assigned_at);
    let outcome = events
        .iter()
        .rev()
        .find(|event| event.event == TaskEventKind::Reminded)
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

async fn record_stall_audit(
    pump: &HerdrQueueWakePump,
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
    if let Err(error) = pump
        .blocking_bridge
        .run(herdr_request_deadline(), move || {
            store.record_lead_notified(&member, &task_id, now, &lead, &message_id)
        })
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
}

pub(crate) async fn escalate_episode(
    pump: &HerdrQueueWakePump,
    task_store: &Arc<dyn atm_core::boundary::TaskStore + Send + Sync>,
    member: &MemberKey,
    kind: EpisodeKind,
    since: IsoTimestamp,
    stats: &mut HerdrQueueWakeStats,
) {
    let body = episode_body(member, kind, since);
    let outcome = escalate_mail(
        &pump.blocking_bridge,
        &pump.service_runtime,
        Some(task_store),
        &pump.daemon_home,
        member.team(),
        &escalation_summary(kind.into(), member, None),
        &body,
        kind.into(),
        Some(since),
    )
    .await;
    record_escalation_stats(stats, &outcome);
    stats.blocked_escalations += usize::from(outcome.reached_anyone());
}

pub(crate) async fn escalate_refusals(
    pump: &HerdrQueueWakePump,
    task_store: &Arc<dyn atm_core::boundary::TaskStore + Send + Sync>,
    member: &MemberKey,
    since: IsoTimestamp,
    stats: &mut HerdrQueueWakeStats,
) {
    let body = format!(
        "member {} has refused three consecutive task handoffs since {}\nRun: atm list --task-events --member {}",
        member.agent(),
        since,
        member.agent(),
    );
    let outcome = escalate_mail(
        &pump.blocking_bridge,
        &pump.service_runtime,
        Some(task_store),
        &pump.daemon_home,
        member.team(),
        &escalation_summary(EscalationKind::RefusalsEscalated, member, None),
        &body,
        EscalationKind::RefusalsEscalated,
        Some(since),
    )
    .await;
    record_escalation_stats(stats, &outcome);
    stats.blocked_escalations += usize::from(outcome.reached_anyone());
}

fn episode_body(member: &MemberKey, kind: EpisodeKind, since: IsoTimestamp) -> String {
    truncate_body(format!(
        "{} member {} has been in this state since {}\nRun: atm members --team {}",
        kind.as_str(),
        member.agent(),
        since,
        member.team(),
    ))
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
