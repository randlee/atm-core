//! Task-start mutations in the serial SQLite writer lane.

use super::task_ops::{
    TaskEventDraft, append_task_event, load_task_row, queue_order, renumber_queue,
};
use super::task_rejection::{
    task_already_active, task_already_closed, task_max_active_reached, task_not_counterparty,
    task_not_found,
};
use crate::shared_db::{SharedDbTarget, sqlite_error};
use atm_storage::contract::Message;
use atm_storage::error::AtmError;
use atm_storage::task_state::{
    DEFAULT_MAX_TASKS, MAX_TASKS_METADATA_KEY, TaskEvent, TaskEventKind, TaskRow, TaskState,
    Transition, parse_max_tasks, transition,
};
use atm_storage::types::{AgentName, TaskId};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, params};
use std::num::NonZeroU32;

pub(super) fn apply_task_start(
    record: &Message,
    task_id: &TaskId,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(AgentName, atm_storage::TaskEventRow), AtmError> {
    let row = load_startable_task(record, task_id, connection, target)?;
    if row.state == TaskState::Active {
        return Err(task_already_active(format!(
            "task {task_id} is already active"
        )));
    }
    let Transition(next_state) = transition(
        Some(row.state),
        TaskEvent::Started,
        task_id,
        &record.envelope.from,
        Some(&row.assignee),
        &row.assignee,
    )
    .map_err(|error| error.into_atm_error())?;
    let other_active = reject_over_max_tasks(record, task_id, &row, connection, target)?;

    let mut order = queue_order(connection, target, &record.team, &row.assignee)?;
    order.retain(|id| id != task_id);
    // The started group leads the queue in start order, so the new start goes
    // after the member's other active tasks.
    order.insert(other_active as usize, task_id.clone());
    connection
        .execute(
            "UPDATE tasks SET state=?3, lead_notified_count=0, updated_at=?4
             WHERE team=?1 AND task_id=?2",
            params![
                record.team.as_str(),
                task_id.as_str(),
                next_state.as_str(),
                record.envelope.timestamp.to_string()
            ],
        )
        .map_err(|error| sqlite_error(target, "failed to start task", error))?;
    renumber_queue(&record.team, &row.assignee, &order, connection, target)?;
    let event = append_task_event(
        connection,
        target,
        &TaskEventDraft {
            team: &record.team,
            task_id,
            assignee: &row.assignee,
            at: &record.envelope.timestamp,
            event: TaskEventKind::Started,
            from_state: Some(row.state.tag()),
            to_state: Some(next_state.tag()),
            close_outcome: None,
            actor: &record.envelope.from,
            message_id: record.envelope.message_id,
            outcome: None,
            marker: None,
            detail: None,
        },
    )?;
    Ok((row.assignee, event))
}

fn load_startable_task(
    record: &Message,
    task_id: &TaskId,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<TaskRow, AtmError> {
    let Some(row) = load_task_row(connection, target, &record.team, task_id)? else {
        return Err(task_not_found(format!(
            "no open task {task_id} for {}",
            record.envelope.from
        )));
    };
    if !row.state.is_open() {
        return Err(task_already_closed(format!(
            "no open task {task_id} for {}",
            record.envelope.from
        )));
    }
    if record.envelope.from != row.assignee {
        return Err(task_not_counterparty(format!(
            "task {task_id} is not assigned to {}",
            record.envelope.from
        )));
    }
    Ok(row)
}

/// Refuses the start when the member already holds `max_tasks` active tasks.
/// Returns how many other tasks the member holds active.
fn reject_over_max_tasks(
    record: &Message,
    task_id: &TaskId,
    row: &TaskRow,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<u32, AtmError> {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM tasks
             WHERE team=?1 AND assignee=?2 AND state='active' AND task_id<>?3",
            params![
                record.team.as_str(),
                row.assignee.as_str(),
                task_id.as_str()
            ],
            |raw| raw.get(0),
        )
        .map_err(|error| sqlite_error(target, "failed to count active tasks", error))?;
    let active = u32::try_from(count).unwrap_or(u32::MAX);
    let max = member_max_tasks(record, &row.assignee, connection, target)?;
    if active >= max.get() {
        return Err(task_max_active_reached(
            format!(
                "task {task_id}: {} already has {active} active task(s); max_tasks is {max}",
                row.assignee
            ),
            format!(
                "Complete or cancel an active task, or raise the limit with `atm teams update-member {} {} --max-tasks <N>`.",
                record.team, row.assignee
            ),
        ));
    }
    Ok(active)
}

/// The member's `max_tasks` from its roster `metadata_json`. A missing row or
/// key means the default; any value other than an integer of at least 1 is an
/// error, so a bad setting never silently widens or narrows the limit.
fn member_max_tasks(
    record: &Message,
    assignee: &AgentName,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<NonZeroU32, AtmError> {
    let stored: Option<Value> = connection
        .query_row(
            &format!(
                "SELECT json_extract(metadata_json, '$.{MAX_TASKS_METADATA_KEY}')
                 FROM team_roster WHERE team_name=?1 AND agent_name=?2"
            ),
            params![record.team.as_str(), assignee.as_str()],
            |raw| raw.get(0),
        )
        .optional()
        .map_err(|error| sqlite_error(target, "failed to read max_tasks", error))?;
    match stored {
        None | Some(Value::Null) => Ok(DEFAULT_MAX_TASKS),
        Some(Value::Integer(value)) => {
            parse_max_tasks(value).ok_or_else(|| invalid_max_tasks(record, assignee))
        }
        Some(_) => Err(invalid_max_tasks(record, assignee)),
    }
}

fn invalid_max_tasks(record: &Message, assignee: &AgentName) -> AtmError {
    AtmError::validation_with_recovery(
        format!(
            "{assignee} in team {} has an invalid {MAX_TASKS_METADATA_KEY}: it must be an integer of at least 1",
            record.team
        ),
        format!(
            "set it with `atm teams update-member {} {assignee} --max-tasks <N>`",
            record.team
        ),
    )
}
