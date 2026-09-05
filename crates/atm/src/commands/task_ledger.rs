use anyhow::Result;
use atm_core::list::{ListOutcome, TaskLedgerQuery};
use atm_storage::contract::{TaskEventRow, TaskRow};
use atm_storage::{ReminderOutcome, TaskActor, TaskEventKind, TaskEventMarker, TaskState};

pub(super) fn print_task_ledger(
    outcome: &ListOutcome,
    task_ledger: &TaskLedgerQuery,
    json: bool,
) -> Result<()> {
    print!("{}", render_task_ledger(outcome, task_ledger, json)?);
    Ok(())
}

pub(super) fn render_task_ledger(
    outcome: &ListOutcome,
    task_ledger: &TaskLedgerQuery,
    json: bool,
) -> Result<String> {
    if json {
        return match task_ledger {
            TaskLedgerQuery::Tasks { .. } => Ok(format!(
                "{}\n",
                serde_json::to_string_pretty(&outcome.task_rows)?
            )),
            TaskLedgerQuery::Events { .. } => Ok(format!(
                "{}\n",
                serde_json::to_string_pretty(&outcome.task_event_rows)?
            )),
        };
    }
    match task_ledger {
        TaskLedgerQuery::Tasks { .. } => render_tasks(&outcome.task_rows),
        TaskLedgerQuery::Events { .. } => render_task_events(&outcome.task_event_rows),
    }
}

fn render_tasks(rows: &[TaskRow]) -> Result<String> {
    let mut rendered = String::from(
        "TASK_ID     STATE     ASSIGNEE  ASSIGNER   ASSIGNED_AT               REMINDERS\n",
    );
    for row in rows {
        rendered.push_str(&format!(
            "{:<11} {:<9} {:<9} {:<10} {:<25} {}\n",
            row.task_id.as_str(),
            task_state_name(row.state),
            row.assignee.as_str(),
            row.assigner.as_str(),
            row.assigned_at.into_inner().to_rfc3339(),
            row.reminder_count,
        ));
    }
    Ok(rendered)
}

fn render_task_events(rows: &[TaskEventRow]) -> Result<String> {
    let mut rendered = String::from(
        "SEQ  AT                        EVENT      FROM      TO        ACTOR       DETAIL\n",
    );
    for row in rows {
        rendered.push_str(&format!(
            "{:<4} {:<25} {:<10} {:<9} {:<9} {:<11} {}\n",
            row.seq,
            row.at.into_inner().to_rfc3339(),
            task_event_name(row.event),
            row.from_state.map(task_state_name).unwrap_or("-"),
            row.to_state.map(task_state_name).unwrap_or("-"),
            task_actor_name(&row.actor),
            task_event_detail(row),
        ));
    }
    Ok(rendered)
}

const fn task_state_name(state: TaskState) -> &'static str {
    match state {
        TaskState::Assigned => "assigned",
        TaskState::Active => "active",
        TaskState::Complete => "complete",
    }
}

const fn task_event_name(event: TaskEventKind) -> &'static str {
    match event {
        TaskEventKind::Assigned => "assigned",
        TaskEventKind::Acked => "acked",
        TaskEventKind::Completed => "completed",
        TaskEventKind::Rejected => "rejected",
        TaskEventKind::Reminded => "reminded",
        TaskEventKind::LeadNotified => "lead_notified",
    }
}

fn task_actor_name(actor: &TaskActor) -> &str {
    match actor {
        TaskActor::Member(member) => member.as_str(),
        TaskActor::Daemon => "atm-daemon",
    }
}

fn task_event_detail(row: &TaskEventRow) -> String {
    row.detail.clone().unwrap_or_else(|| {
        row.marker
            .map(task_event_marker_name)
            .or_else(|| row.outcome.map(reminder_outcome_name))
            .unwrap_or("-")
            .to_string()
    })
}

const fn task_event_marker_name(marker: TaskEventMarker) -> &'static str {
    match marker {
        TaskEventMarker::Resend => "resend",
        TaskEventMarker::AssignmentMissing => "assignment_missing",
    }
}

const fn reminder_outcome_name(outcome: ReminderOutcome) -> &'static str {
    match outcome {
        ReminderOutcome::Emitted => "emitted",
        ReminderOutcome::Unrenderable => "unrenderable",
        ReminderOutcome::Blocked => "blocked",
    }
}
