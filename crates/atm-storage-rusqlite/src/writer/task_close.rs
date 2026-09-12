//! Task-close mutations in the serial SQLite writer lane.

use super::stmt_cache::WriterStatementCache;
use super::task_ops::{
    TaskMessageResult, acknowledge_assignment, append_task_event, load_task_row, queue_order,
    renumber_queue,
};
use super::task_rejection::{task_not_found, task_stale_counterparty};
use super::task_report::drop_task_link_from_mail;
use crate::shared_db::{SharedDbTarget, sqlite_error};
use atm_storage::contract::Message;
use atm_storage::error::AtmError;
use atm_storage::task_state::{
    TaskCloseOutcome, TaskEvent, TaskEventKind, TaskRow, TaskState, Transition, admit, transition,
};
use atm_storage::types::{IsoTimestamp, TaskId};
use rusqlite::{Connection, params};

#[allow(clippy::too_many_arguments)]
pub(super) fn apply_task_close(
    record: &Message,
    task_id: &TaskId,
    outcome: TaskCloseOutcome,
    reason: Option<&str>,
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
) -> Result<TaskMessageResult, AtmError> {
    let Some(row) = load_task_row(connection, target, &record.team, task_id)? else {
        return Err(task_not_found(format!(
            "no open task {task_id} for {}",
            record.envelope.from
        )));
    };
    if let TaskState::Complete(already) = row.state {
        drop_task_link_from_mail(record, connection, target)?;
        return Ok(TaskMessageResult::Applied {
            already_closed: Some(already),
            task_assignee: None,
            queued_position: None,
            reassign_notice: None,
        });
    }
    if let Err(error) = admit(
        Some(&row),
        TaskEvent::Completed(outcome),
        task_id,
        &record.envelope.from,
    )
    .map_err(|error| error.into_atm_error())
    {
        return deliver_rejected_close_report(record, task_id, &row, error, connection, target);
    }
    let Transition(next_state) = transition(
        Some(row.state),
        TaskEvent::Completed(outcome),
        task_id,
        &record.envelope.from,
        Some(&row.assignee),
        &row.assignee,
    )
    .map_err(|error| error.into_atm_error())?;
    let expected = if record.envelope.from == row.assignee {
        &row.assigner
    } else {
        &row.assignee
    };
    if &record.agent != expected {
        let error = task_stale_counterparty(format!(
            "task {task_id}: {} is no longer the counterparty — re-run the command",
            record.agent
        ));
        return deliver_rejected_close_report(record, task_id, &row, error, connection, target);
    }
    persist_task_close(
        record, task_id, outcome, reason, &row, next_state, connection, cache, target,
    )?;
    Ok(TaskMessageResult::Applied {
        already_closed: None,
        task_assignee: Some(row.assignee),
        queued_position: None,
        reassign_notice: None,
    })
}

fn deliver_rejected_close_report(
    record: &Message,
    task_id: &TaskId,
    row: &TaskRow,
    error: AtmError,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<TaskMessageResult, AtmError> {
    drop_task_link_from_mail(record, connection, target)?;
    append_task_event(
        connection,
        target,
        &record.team,
        task_id,
        &row.assignee,
        &IsoTimestamp::now(),
        TaskEventKind::Rejected,
        Some(row.state.tag()),
        Some(row.state.tag()),
        row.state.close_outcome(),
        &record.envelope.from,
        record.envelope.message_id,
        None,
        None,
        Some(error.message()),
    )?;
    Ok(TaskMessageResult::RejectedReportDelivered(AtmError::new(
        error.code(),
        format!("{}; report delivered", error.detail()),
    )))
}

#[allow(clippy::too_many_arguments)]
fn persist_task_close(
    record: &Message,
    task_id: &TaskId,
    outcome: TaskCloseOutcome,
    reason: Option<&str>,
    row: &TaskRow,
    next_state: TaskState,
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    acknowledge_assignment(connection, cache, target, record, row)?;
    connection
        .execute(
            "UPDATE tasks SET state=?3, close_outcome=?4, position=NULL, updated_at=?5
         WHERE team=?1 AND task_id=?2",
            params![
                record.team.as_str(),
                task_id.as_str(),
                next_state.as_str(),
                outcome.as_str(),
                record.envelope.timestamp.to_string()
            ],
        )
        .map_err(|error| sqlite_error(target, "failed to close task", error))?;
    let order = queue_order(connection, target, &record.team, &row.assignee)?;
    renumber_queue(&record.team, &row.assignee, &order, connection, target)?;
    append_task_event(
        connection,
        target,
        &record.team,
        task_id,
        &row.assignee,
        &record.envelope.timestamp,
        close_event_kind(outcome),
        Some(row.state.tag()),
        Some(next_state.tag()),
        Some(outcome),
        &record.envelope.from,
        record.envelope.message_id,
        None,
        None,
        reason,
    )
}

const fn close_event_kind(outcome: TaskCloseOutcome) -> TaskEventKind {
    match outcome {
        TaskCloseOutcome::Completed => TaskEventKind::Completed,
        TaskCloseOutcome::Refused => TaskEventKind::Refused,
        TaskCloseOutcome::Cancelled => TaskEventKind::Cancelled,
    }
}
