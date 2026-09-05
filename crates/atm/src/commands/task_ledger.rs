use anyhow::Result;
use atm_core::list::{ListOutcome, TaskLedgerQuery};
use atm_storage::TaskActor;
use atm_storage::contract::{TaskEventRow, TaskRow};
use chrono::SecondsFormat;

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
        TaskLedgerQuery::Tasks { .. } => Ok(render_tasks(&outcome.task_rows)),
        TaskLedgerQuery::Events { .. } => Ok(render_task_events(&outcome.task_event_rows)),
    }
}

fn render_tasks(rows: &[TaskRow]) -> String {
    let mut rendered = String::from(
        "TASK_ID     STATE     ASSIGNEE  ASSIGNER   ASSIGNED_AT               REMINDERS\n",
    );
    for row in rows {
        rendered.push_str(&format!(
            "{:<11} {:<9} {:<9} {:<10} {:<25} {}\n",
            row.task_id.as_str(),
            row.state.as_str(),
            row.assignee.as_str(),
            row.assigner.as_str(),
            row.assigned_at
                .into_inner()
                .to_rfc3339_opts(SecondsFormat::Secs, true),
            row.reminder_count,
        ));
    }
    rendered
}

fn render_task_events(rows: &[TaskEventRow]) -> String {
    let mut rendered = String::from(
        "SEQ  AT                        EVENT      FROM      TO        ACTOR    DETAIL\n",
    );
    for row in rows {
        rendered.push_str(&format!(
            "{:<4} {:<25} {:<10} {:<9} {:<9} {:<8} {}\n",
            row.seq,
            row.at
                .into_inner()
                .to_rfc3339_opts(SecondsFormat::Secs, true),
            row.event.as_str(),
            row.from_state.map(|state| state.as_str()).unwrap_or("-"),
            row.to_state.map(|state| state.as_str()).unwrap_or("-"),
            task_actor_name(&row.actor),
            task_event_detail(row),
        ));
    }
    rendered
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
            .map(|marker| marker.as_str())
            .or_else(|| row.outcome.map(|outcome| outcome.as_str()))
            .unwrap_or("-")
            .to_string()
    })
}
