//! The rusqlite writer's sole task-ledger mutation site.

use super::ops::{
    WriteOp, execute_upsert_message, load_existing_message, load_pending_ack_source,
    mark_source_acknowledged,
};
use super::ops_envelope::StorageEnvelope;
use super::stmt_cache::WriterStatementCache;
use crate::shared_db::{SharedDbTarget, sqlite_error};
use atm_storage::contract::Message;
use atm_storage::error::AtmError;
use atm_storage::schema::AtmMessageId;
use atm_storage::task_state::{
    DAEMON_ACTOR_NAME, QueuePosition, TaskCloseOutcome, TaskEvent, TaskRow, TaskState, admit,
};
use atm_storage::types::{AgentName, IsoTimestamp, TaskId, TeamName};
use atm_storage::{AtmErrorCode, MessageWriteOrigin, MoveTarget, TaskOp};
use rusqlite::{Connection, OptionalExtension, params};

use crate::task_sql;

fn task_rejected(detail: impl std::fmt::Display) -> AtmError {
    AtmError::validation(detail.to_string())
}

fn load_task_row(
    connection: &Connection,
    target: &SharedDbTarget,
    team: &TeamName,
    task_id: &TaskId,
) -> Result<Option<TaskRow>, AtmError> {
    task_sql::select_task_row(connection, team, task_id)
        .map_err(|error| sqlite_error(target, "failed to load task row", error))
}

pub(super) fn append_rejected_task_event(
    op: &WriteOp,
    connection: &Connection,
    target: &SharedDbTarget,
    error: &AtmError,
) -> Result<(), AtmError> {
    if error.code() != AtmErrorCode::MessageValidationFailed {
        return Ok(());
    }
    let (team, task_id, requested, actor, message_id) = match op {
        WriteOp::UpsertMessage { record, provenance }
            if *provenance == MessageWriteOrigin::Local =>
        {
            let Some(task_id) = record.envelope.task_id.as_ref() else {
                return Ok(());
            };
            (
                record.team.clone(),
                task_id.clone(),
                record.agent.clone(),
                record.envelope.from.clone(),
                record.envelope.message_id,
            )
        }
        WriteOp::Acknowledge { source, .. } => {
            let source = load_pending_ack_source(source, connection, target)?;
            let Some(task_id) = source.envelope.task_id.clone() else {
                return Ok(());
            };
            (
                source.team,
                task_id,
                source.agent.clone(),
                source.agent,
                source.envelope.message_id,
            )
        }
        WriteOp::TaskMove {
            team,
            task_id,
            actor,
            ..
        } => (
            team.clone(),
            task_id.clone(),
            actor.clone(),
            actor.clone(),
            None,
        ),
        _ => return Ok(()),
    };
    let row = load_task_row(connection, target, &team, &task_id)?;
    let assignee = row.as_ref().map_or(&requested, |row| &row.assignee);
    let state = row.as_ref().map(|row| row.state.as_str());
    append_task_event(
        connection,
        target,
        &team,
        &task_id,
        assignee,
        &IsoTimestamp::now(),
        "rejected",
        state,
        state,
        None,
        &actor,
        message_id,
        None,
        None,
        Some(error.message()),
    )
}

pub(super) fn apply_task_message(
    record: &Message,
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    let Some(task_id) = record.envelope.task_id.as_ref() else {
        return Ok(());
    };
    match record.envelope.task_op.as_ref() {
        None => apply_task_assignment(
            record,
            task_id,
            record.envelope.placement.as_ref(),
            connection,
            cache,
            target,
        ),
        Some(TaskOp::Start) => apply_task_start(record, task_id, connection, target),
        Some(TaskOp::Close { outcome, reason }) => apply_task_close(
            record,
            task_id,
            *outcome,
            reason.as_deref(),
            connection,
            cache,
            target,
        )
        .map(|_| ()),
    }
}

fn apply_task_assignment(
    record: &Message,
    task_id: &TaskId,
    placement: Option<&MoveTarget>,
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    let row = load_task_row(connection, target, &record.team, task_id)?;
    let at = record.envelope.timestamp;
    let message_id = record
        .envelope
        .message_id
        .ok_or_else(|| task_rejected("task assignment is missing message id"))?;

    if refresh_same_assignment(
        record,
        task_id,
        row.as_ref(),
        message_id,
        connection,
        cache,
        target,
    )? {
        return Ok(());
    }

    let was_closed = row
        .as_ref()
        .is_some_and(|row| matches!(row.state, TaskState::Complete(_)));
    release_previous_assignment(record, task_id, row.as_ref(), connection, cache, target)?;

    let mut order = queue_order(connection, target, &record.team, &record.agent)?;
    order.retain(|id| id != task_id);
    insert_at_placement(
        &mut order,
        task_id,
        placement.unwrap_or(&MoveTarget::End),
        connection,
        target,
        &record.team,
        &record.agent,
    )?;
    let temporary =
        u32::try_from(order.len()).map_err(|_| task_rejected("task queue too large"))?;
    apply_task_assignment_row(
        record,
        task_id,
        row.as_ref(),
        temporary,
        message_id,
        at,
        connection,
        target,
    )?;
    renumber_previous_assignment(record, row.as_ref(), connection, target)?;
    renumber_queue(&record.team, &record.agent, &order, connection, target)?;
    append_assignment_event(
        record,
        task_id,
        row.as_ref(),
        was_closed,
        message_id,
        at,
        connection,
        target,
    )
}

#[allow(clippy::too_many_arguments)]
fn refresh_same_assignment(
    record: &Message,
    task_id: &TaskId,
    row: Option<&TaskRow>,
    message_id: AtmMessageId,
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
) -> Result<bool, AtmError> {
    let Some(row) = row.filter(|row| row.state.is_open() && row.assignee == record.agent) else {
        return Ok(false);
    };
    acknowledge_assignment(connection, cache, target, record, row)?;
    connection
        .execute(
            "UPDATE tasks SET assignment_message_id=?3, description=?4, updated_at=?5
             WHERE team=?1 AND task_id=?2",
            params![
                record.team.as_str(),
                task_id.as_str(),
                message_id.to_string(),
                record.envelope.text,
                record.envelope.timestamp.to_string()
            ],
        )
        .map_err(|error| sqlite_error(target, "failed to refresh task assignment", error))?;
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
fn release_previous_assignment(
    record: &Message,
    task_id: &TaskId,
    row: Option<&TaskRow>,
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    let Some(row) = row.filter(|row| row.state.is_open()) else {
        return Ok(());
    };
    acknowledge_assignment(connection, cache, target, record, row)?;
    let old_order = queue_order(connection, target, &record.team, &row.assignee)?
        .into_iter()
        .filter(|id| id != task_id)
        .collect::<Vec<_>>();
    connection
        .execute(
            "UPDATE tasks SET position=position+?3
             WHERE team=?1 AND assignee=?2 AND state<>'complete'",
            params![
                record.team.as_str(),
                row.assignee.as_str(),
                old_order.len() + 1
            ],
        )
        .map_err(|error| sqlite_error(target, "failed to release old task position", error))?;
    write_queue_positions(&record.team, &old_order, connection, target)
}

#[allow(clippy::too_many_arguments)]
fn apply_task_assignment_row(
    record: &Message,
    task_id: &TaskId,
    row: Option<&TaskRow>,
    temporary: u32,
    message_id: AtmMessageId,
    at: IsoTimestamp,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    if row.is_some() {
        connection
            .execute(
                "UPDATE tasks SET assignee=?3, assigner=?4, state='assigned', close_outcome=NULL,
                 position=?5, assignment_message_id=?6, description=?7, assigned_at=?8,
                 updated_at=?8, last_reminded_at=NULL, reminder_count=0, lead_notified_count=0
                 WHERE team=?1 AND task_id=?2",
                params![
                    record.team.as_str(),
                    task_id.as_str(),
                    record.agent.as_str(),
                    record.envelope.from.as_str(),
                    temporary,
                    message_id.to_string(),
                    record.envelope.text,
                    at.to_string()
                ],
            )
            .map_err(|error| sqlite_error(target, "failed to reassign task", error))?;
    } else {
        connection
            .execute(
                "INSERT INTO tasks(team, task_id, assignee, assigner, state, close_outcome, position,
                 assignment_message_id, description, assigned_at, updated_at,
                 last_reminded_at, reminder_count, lead_notified_count)
                 VALUES (?1,?2,?3,?4,'assigned',NULL,?5,?6,?7,?8,?8,NULL,0,0)",
                params![
                    record.team.as_str(),
                    task_id.as_str(),
                    record.agent.as_str(),
                    record.envelope.from.as_str(),
                    temporary,
                    message_id.to_string(),
                    record.envelope.text,
                    at.to_string()
                ],
            )
            .map_err(|error| sqlite_error(target, "failed to insert task assignment", error))?;
    }
    Ok(())
}

fn renumber_previous_assignment(
    record: &Message,
    row: Option<&TaskRow>,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    let Some(old) = row.filter(|old| old.state.is_open()) else {
        return Ok(());
    };
    let old_order = queue_order(connection, target, &record.team, &old.assignee)?;
    renumber_queue(&record.team, &old.assignee, &old_order, connection, target)
}

#[allow(clippy::too_many_arguments)]
fn append_assignment_event(
    record: &Message,
    task_id: &TaskId,
    row: Option<&TaskRow>,
    was_closed: bool,
    message_id: AtmMessageId,
    at: IsoTimestamp,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    let event = if was_closed {
        "reopened"
    } else if row.is_some() {
        "reassigned"
    } else {
        "assigned"
    };
    append_task_event(
        connection,
        target,
        &record.team,
        task_id,
        &record.agent,
        &at,
        event,
        row.map(|row| row.state.as_str()),
        Some("assigned"),
        row.and_then(|row| row.state.close_outcome()),
        &record.envelope.from,
        Some(message_id),
        None,
        None,
        None,
    )
}

fn apply_task_start(
    record: &Message,
    task_id: &TaskId,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    let row = load_startable_task(record, task_id, connection, target)?;
    if !start_reminder_was_emitted(record, task_id, connection, target)?
        || row.state == TaskState::Active
    {
        return Ok(());
    }
    reject_concurrent_active_task(record, task_id, &row, connection, target)?;

    let mut order = queue_order(connection, target, &record.team, &row.assignee)?;
    order.retain(|id| id != task_id);
    order.insert(0, task_id.clone());
    connection
        .execute(
            "UPDATE tasks SET state='active', reminder_count=0, lead_notified_count=0, updated_at=?3
             WHERE team=?1 AND task_id=?2",
            params![
                record.team.as_str(),
                task_id.as_str(),
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
        "started",
        Some(row.state.as_str()),
        Some("active"),
        None,
        &record.envelope.from,
        record.envelope.message_id,
        None,
        None,
        None,
    )
}

fn load_startable_task(
    record: &Message,
    task_id: &TaskId,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<TaskRow, AtmError> {
    let Some(row) = load_task_row(connection, target, &record.team, task_id)? else {
        return Err(task_rejected(format!(
            "no open task {task_id} for {}",
            record.envelope.from
        )));
    };
    if !row.state.is_open() {
        return Err(task_rejected(format!(
            "no open task {task_id} for {}",
            record.envelope.from
        )));
    }
    if record.envelope.from.as_str() != DAEMON_ACTOR_NAME {
        return Err(task_rejected(format!(
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
        return Err(task_rejected(format!(
            "task {task_id}: {} already has an active task",
            row.assignee
        )));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_task_close(
    record: &Message,
    task_id: &TaskId,
    outcome: TaskCloseOutcome,
    reason: Option<&str>,
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
) -> Result<Option<TaskCloseOutcome>, AtmError> {
    let Some(row) = load_task_row(connection, target, &record.team, task_id)? else {
        return Err(task_rejected(format!(
            "no open task {task_id} for {}",
            record.envelope.from
        )));
    };
    if let TaskState::Complete(already) = row.state {
        drop_task_link_from_mail(record, connection, target)?;
        return Ok(Some(already));
    }
    admit(
        Some(&row),
        TaskEvent::Completed(outcome),
        task_id,
        &record.envelope.from,
    )
    .map_err(|error| error.into_atm_error())?;
    let expected = if record.envelope.from == row.assignee {
        &row.assigner
    } else {
        &row.assignee
    };
    if &record.agent != expected {
        return Err(task_rejected(format!(
            "task {task_id}: {} is no longer the counterparty — re-run the command",
            record.agent
        )));
    }
    acknowledge_assignment(connection, cache, target, record, &row)?;
    connection
        .execute(
            "UPDATE tasks SET state='complete', close_outcome=?3, position=NULL, updated_at=?4
         WHERE team=?1 AND task_id=?2",
            params![
                record.team.as_str(),
                task_id.as_str(),
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
        outcome.as_str(),
        Some(row.state.as_str()),
        Some("complete"),
        Some(outcome),
        &record.envelope.from,
        record.envelope.message_id,
        None,
        None,
        reason,
    )?;
    Ok(None)
}

fn drop_task_link_from_mail(
    record: &Message,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    let mut envelope = record.envelope.clone();
    envelope.task_id = None;
    envelope.task_op = None;
    envelope.task_complete = None;
    envelope.placement = None;
    let json = serde_json::to_string(&StorageEnvelope::new(&envelope))
        .map_err(|error| AtmError::mailbox_write(error.to_string()))?;
    connection
        .execute(
            "UPDATE mail_messages SET envelope_json=?4 WHERE team=?1 AND agent=?2 AND message_key=?3",
            params![record.team.as_str(), record.agent.as_str(), record.message_key.as_str(), json],
        )
        .map_err(|error| sqlite_error(target, "failed to detach already-closed task report", error))?;
    Ok(())
}

pub(super) fn apply_task_move(
    team: &TeamName,
    task_id: &TaskId,
    actor: &AgentName,
    target_pos: &MoveTarget,
    at: IsoTimestamp,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<QueuePosition, AtmError> {
    let Some(row) = load_task_row(connection, target, team, task_id)? else {
        return Err(task_rejected(format!("no open task {task_id} for {actor}")));
    };
    if !row.state.is_open() {
        return Err(task_rejected(format!("no open task {task_id} for {actor}")));
    }
    if row.state == TaskState::Active {
        append_task_event(
            connection,
            target,
            team,
            task_id,
            &row.assignee,
            &at,
            "moved",
            Some("active"),
            Some("active"),
            None,
            actor,
            None,
            None,
            None,
            Some("1→1"),
        )?;
        return Ok(QueuePosition::HEAD);
    }
    let from = row
        .position
        .ok_or_else(|| task_rejected("open task has no queue position"))?;
    let mut order = queue_order(connection, target, team, &row.assignee)?;
    order.retain(|id| id != task_id);
    insert_at_placement(
        &mut order,
        task_id,
        target_pos,
        connection,
        target,
        team,
        &row.assignee,
    )?;
    renumber_queue(team, &row.assignee, &order, connection, target)?;
    let to = order
        .iter()
        .position(|id| id == task_id)
        .map(|index| index + 1)
        .and_then(|value| u32::try_from(value).ok())
        .and_then(QueuePosition::new)
        .ok_or_else(|| task_rejected("task move produced no queue position"))?;
    let detail = format!("{}→{}", from.get(), to.get());
    append_task_event(
        connection,
        target,
        team,
        task_id,
        &row.assignee,
        &at,
        "moved",
        Some("assigned"),
        Some("assigned"),
        None,
        actor,
        None,
        None,
        None,
        Some(&detail),
    )?;
    Ok(to)
}

fn queue_order(
    connection: &Connection,
    target: &SharedDbTarget,
    team: &TeamName,
    assignee: &AgentName,
) -> Result<Vec<TaskId>, AtmError> {
    let mut statement = connection
        .prepare(
            "SELECT task_id FROM tasks WHERE team=?1 AND assignee=?2 AND state<>'complete'
             ORDER BY position, task_id",
        )
        .map_err(|error| sqlite_error(target, "failed to prepare task queue", error))?;
    statement
        .query_map(params![team.as_str(), assignee.as_str()], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|error| sqlite_error(target, "failed to query task queue", error))?
        .map(|value| {
            value
                .map_err(|error| sqlite_error(target, "failed to read task queue", error))?
                .parse()
                .map_err(|error| task_rejected(format!("invalid queued task id: {error}")))
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn insert_at_placement(
    order: &mut Vec<TaskId>,
    task_id: &TaskId,
    placement: &MoveTarget,
    connection: &Connection,
    target: &SharedDbTarget,
    team: &TeamName,
    assignee: &AgentName,
) -> Result<(), AtmError> {
    let index = match placement {
        MoveTarget::End => order.len(),
        MoveTarget::Head => {
            let active = order
                .first()
                .map(|id| load_task_row(connection, target, team, id))
                .transpose()?
                .flatten();
            active
                .filter(|row| row.state == TaskState::Active)
                .map_or(0, |_| 1)
        }
        MoveTarget::Before { task_id: other } => {
            let valid = load_task_row(connection, target, team, other)?
                .is_some_and(|row| row.assignee == *assignee && row.state == TaskState::Assigned);
            if !valid {
                return Err(task_rejected(format!(
                    "task {task_id}: placement target {other} is not an open queued task of {assignee}"
                )));
            }
            order.iter().position(|id| id == other).ok_or_else(|| {
                task_rejected(format!(
                    "task {task_id}: placement target {other} is not an open queued task of {assignee}"
                ))
            })?
        }
    };
    order.insert(index, task_id.clone());
    Ok(())
}

/// Renumbers one member's open queue without transient unique-index overlap.
/// Phase one adds `MAX(current position) + order.len()` so every old value is
/// above the occupied range; phase two writes the exact `1..=n` order.
fn renumber_queue(
    team: &TeamName,
    assignee: &AgentName,
    order: &[TaskId],
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    let maximum: i64 = connection
        .query_row(
            "SELECT COALESCE(MAX(position),0) FROM tasks WHERE team=?1 AND assignee=?2 AND state<>'complete'",
            params![team.as_str(), assignee.as_str()],
            |row| row.get(0),
        )
        .map_err(|error| sqlite_error(target, "failed to size task queue offset", error))?;
    let offset = maximum
        .checked_add(i64::try_from(order.len()).map_err(|_| task_rejected("task queue too large"))?)
        .ok_or_else(|| task_rejected("task queue too large"))?;
    connection
        .execute(
            "UPDATE tasks SET position=position+?3 WHERE team=?1 AND assignee=?2 AND state<>'complete'",
            params![team.as_str(), assignee.as_str(), offset],
        )
        .map_err(|error| sqlite_error(target, "failed to offset task queue", error))?;
    write_queue_positions(team, order, connection, target)?;
    let gap: Option<(i64, i64)> = connection
        .query_row(
            "SELECT COUNT(*), COALESCE(MAX(position),0) FROM tasks
             WHERE team=?1 AND assignee=?2 AND state<>'complete'
             HAVING COUNT(*)<>COALESCE(MAX(position),0)",
            params![team.as_str(), assignee.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| sqlite_error(target, "failed to verify task queue", error))?;
    if gap.is_some() {
        return Err(task_rejected(format!(
            "task queue for {assignee} is not contiguous"
        )));
    }
    Ok(())
}

fn write_queue_positions(
    team: &TeamName,
    order: &[TaskId],
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    for (index, task_id) in order.iter().enumerate() {
        let position =
            i64::try_from(index + 1).map_err(|_| task_rejected("task queue too large"))?;
        connection
            .execute(
                "UPDATE tasks SET position=?3 WHERE team=?1 AND task_id=?2",
                params![team.as_str(), task_id.as_str(), position],
            )
            .map_err(|error| sqlite_error(target, "failed to renumber task queue", error))?;
    }
    Ok(())
}

fn acknowledge_assignment(
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
    record: &Message,
    row: &TaskRow,
) -> Result<(), AtmError> {
    let message_key: Option<String> = connection
        .query_row(
            "SELECT message_key FROM mail_messages WHERE team=?1 AND agent=?2 AND message_id=?3",
            params![
                record.team.as_str(),
                row.assignee.as_str(),
                row.assignment_message_id.to_string()
            ],
            |raw| raw.get(0),
        )
        .optional()
        .map_err(|error| sqlite_error(target, "failed to load task assignment", error))?;
    let Some(message_key) = message_key else {
        return Ok(());
    };
    let requested = Message {
        team: record.team.clone(),
        agent: row.assignee.clone(),
        message_key: message_key
            .parse()
            .map_err(|error| task_rejected(format!("invalid assignment message key: {error}")))?,
        envelope: record.envelope.clone(),
    };
    let mut assignment = load_existing_message(&requested, connection, target)?;
    if assignment.envelope.acknowledged_at.is_none() {
        mark_source_acknowledged(&mut assignment, record.envelope.timestamp);
        let _ = execute_upsert_message(
            &assignment,
            MessageWriteOrigin::Local,
            connection,
            cache,
            target,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn append_task_event(
    connection: &Connection,
    target: &SharedDbTarget,
    team: &TeamName,
    task_id: &TaskId,
    assignee: &AgentName,
    at: &IsoTimestamp,
    event: &str,
    from_state: Option<&str>,
    to_state: Option<&str>,
    close_outcome: Option<TaskCloseOutcome>,
    actor: &AgentName,
    message_id: Option<AtmMessageId>,
    outcome: Option<&str>,
    marker: Option<&str>,
    detail: Option<&str>,
) -> Result<(), AtmError> {
    let seq: u64 = connection
        .query_row(
            "SELECT COALESCE(MAX(seq),0)+1 FROM task_events WHERE team=?1 AND task_id=?2",
            params![team.as_str(), task_id.as_str()],
            |row| row.get(0),
        )
        .map_err(|error| sqlite_error(target, "failed to allocate task event sequence", error))?;
    connection
        .execute(
            "INSERT INTO task_events(team,task_id,assignee,seq,at,event,from_state,to_state,
             close_outcome,actor,message_id,outcome,marker,detail)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            params![
                team.as_str(),
                task_id.as_str(),
                assignee.as_str(),
                seq,
                at.to_string(),
                event,
                from_state,
                to_state,
                close_outcome.map(TaskCloseOutcome::as_str),
                actor.as_str(),
                message_id.map(|id| id.to_string()),
                outcome,
                marker,
                detail
            ],
        )
        .map_err(|error| sqlite_error(target, "failed to append task event", error))?;
    Ok(())
}
