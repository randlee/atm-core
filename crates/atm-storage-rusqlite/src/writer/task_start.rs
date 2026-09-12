//! Task-start mutations in the serial SQLite writer lane.

use super::task_ops::{append_task_event, load_task_row, queue_order, renumber_queue};
use super::task_rejection::{
    task_already_closed, task_move_invalid, task_not_counterparty, task_not_found,
};
use crate::shared_db::{SharedDbTarget, sqlite_error};
use atm_storage::contract::Message;
use atm_storage::error::AtmError;
use atm_storage::task_state::{
    DAEMON_ACTOR_NAME, TaskEvent, TaskEventKind, TaskRow, TaskState, Transition, transition,
};
use atm_storage::types::{AgentName, TaskId};
use rusqlite::{Connection, OptionalExtension, params};

pub(super) fn apply_task_start(
    record: &Message,
    task_id: &TaskId,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<AgentName, AtmError> {
    let row = load_startable_task(record, task_id, connection, target)?;
    let Transition(next_state) = transition(
        Some(row.state),
        TaskEvent::Started,
        task_id,
        &record.envelope.from,
        Some(&row.assignee),
        &row.assignee,
    )
    .map_err(|error| error.into_atm_error())?;
    if !start_reminder_was_emitted(record, task_id, connection, target)?
        || row.state == TaskState::Active
    {
        return Ok(row.assignee);
    }
    reject_concurrent_active_task(record, task_id, &row, connection, target)?;

    let mut order = queue_order(connection, target, &record.team, &row.assignee)?;
    order.retain(|id| id != task_id);
    order.insert(0, task_id.clone());
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
    append_task_event(
        connection,
        target,
        &record.team,
        task_id,
        &row.assignee,
        &record.envelope.timestamp,
        TaskEventKind::Started,
        Some(row.state.tag()),
        Some(next_state.tag()),
        None,
        &record.envelope.from,
        record.envelope.message_id,
        None,
        None,
        None,
    )?;
    Ok(row.assignee)
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
    if record.envelope.from.as_str() != DAEMON_ACTOR_NAME {
        return Err(task_not_counterparty(format!(
            "task {task_id} start requires {DAEMON_ACTOR_NAME}"
        )));
    }
    Ok(row)
}

fn start_reminder_was_emitted(
    record: &Message,
    task_id: &TaskId,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<bool, AtmError> {
    let reminder: Option<String> = connection
        .query_row(
            "SELECT outcome FROM task_events WHERE team=?1 AND task_id=?2 AND event='reminded'
             AND rowid > (SELECT COALESCE(MAX(rowid),0) FROM task_events WHERE team=?1 AND task_id=?2
               AND event IN ('assigned','reassigned','reopened'))
             ORDER BY rowid DESC LIMIT 1",
            params![record.team.as_str(), task_id.as_str()],
            |raw| raw.get(0),
        )
        .optional()
        .map_err(|error| sqlite_error(target, "failed to load task reminder gate", error))?;
    Ok(reminder.as_deref() == Some("emitted"))
}

fn reject_concurrent_active_task(
    record: &Message,
    task_id: &TaskId,
    row: &TaskRow,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    let active: Option<String> = connection
        .query_row(
            "SELECT task_id FROM tasks
             WHERE team=?1 AND assignee=?2 AND state='active' AND task_id<>?3",
            params![
                record.team.as_str(),
                row.assignee.as_str(),
                task_id.as_str()
            ],
            |raw| raw.get(0),
        )
        .optional()
        .map_err(|error| sqlite_error(target, "failed to check active task", error))?;
    if active.is_some() {
        return Err(task_move_invalid(format!(
            "task {task_id}: {} already has an active task",
            row.assignee
        )));
    }
    Ok(())
}
