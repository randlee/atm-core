//! The rusqlite writer's sole task-ledger mutation site.

use super::message_admission::execute_upsert_message;
use super::ops::{
    WriteOp, load_existing_message, load_pending_ack_source, mark_source_acknowledged,
};
use super::stmt_cache::WriterStatementCache;
use super::task_close::apply_task_close;
use super::task_rejection::{
    is_task_rejection, task_already_closed, task_move_invalid, task_not_found,
};
use super::task_start::apply_task_start;
use crate::shared_db::{SharedDbTarget, sqlite_error};
use atm_storage::contract::Message;
use atm_storage::error::AtmError;
use atm_storage::schema::AtmMessageId;
use atm_storage::task_state::{
    QueuePosition, TaskCloseOutcome, TaskEvent, TaskEventKind, TaskRow, TaskState, TaskStateTag,
    Transition, transition,
};
use atm_storage::types::{AgentName, IsoTimestamp, TaskId, TeamName};
use atm_storage::{MessageWriteOrigin, MoveTarget, TaskOp};
pub(super) use rusqlite::params;
use rusqlite::{Connection, OptionalExtension};

use crate::task_sql::select_task_row;

pub(super) enum TaskMessageResult {
    Applied {
        already_closed: Option<TaskCloseOutcome>,
        task_assignee: Option<AgentName>,
        queued_position: Option<u32>,
        reassign_notice: Option<Box<Message>>,
    },
    RejectedReportDelivered(AtmError),
}

impl TaskMessageResult {
    pub(super) fn into_admission_parts(self) -> TaskAdmissionParts {
        match self {
            Self::Applied {
                already_closed,
                task_assignee,
                queued_position,
                reassign_notice,
            } => (
                already_closed,
                task_assignee,
                queued_position,
                reassign_notice,
                None,
            ),
            Self::RejectedReportDelivered(error) => (None, None, None, None, Some(error)),
        }
    }
}

type TaskAdmissionParts = (
    Option<TaskCloseOutcome>,
    Option<AgentName>,
    Option<u32>,
    Option<Box<Message>>,
    Option<AtmError>,
);

pub(super) fn load_task_row(
    connection: &Connection,
    target: &SharedDbTarget,
    team: &TeamName,
    task_id: &TaskId,
) -> Result<Option<TaskRow>, AtmError> {
    select_task_row(connection, team, task_id)
        .map_err(|error| sqlite_error(target, "failed to load task row", error))
}

pub(super) fn append_rejected_task_event(
    op: &WriteOp,
    connection: &Connection,
    target: &SharedDbTarget,
    error: &AtmError,
) -> Result<(), AtmError> {
    if !is_task_rejection(error.code()) {
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
    let state = row.as_ref().map(|row| row.state.tag());
    append_task_event(
        connection,
        target,
        &team,
        &task_id,
        assignee,
        &IsoTimestamp::now(),
        TaskEventKind::Rejected,
        state,
        state,
        row.as_ref().and_then(|row| row.state.close_outcome()),
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
) -> Result<TaskMessageResult, AtmError> {
    let Some(task_id) = record.envelope.task_id.as_ref() else {
        return Ok(TaskMessageResult::Applied {
            already_closed: None,
            task_assignee: None,
            queued_position: None,
            reassign_notice: None,
        });
    };
    match record.envelope.task_op.as_ref() {
        None if is_non_assigner_report(record, task_id, connection, target)? => {
            Ok(TaskMessageResult::Applied {
                already_closed: None,
                task_assignee: None,
                queued_position: None,
                reassign_notice: None,
            })
        }
        None => apply_task_assignment(
            record,
            task_id,
            record.envelope.placement.as_ref(),
            connection,
            cache,
            target,
        )
        .map(|applied| TaskMessageResult::Applied {
            already_closed: None,
            task_assignee: None,
            queued_position: Some(applied.queued_position),
            reassign_notice: applied.reassign_notice.map(Box::new),
        }),
        Some(TaskOp::Start) => {
            apply_task_start(record, task_id, connection, target).map(|task_assignee| {
                TaskMessageResult::Applied {
                    already_closed: None,
                    task_assignee: Some(task_assignee),
                    queued_position: None,
                    reassign_notice: None,
                }
            })
        }
        Some(TaskOp::Close { outcome, reason }) => apply_task_close(
            record,
            task_id,
            *outcome,
            reason.as_deref(),
            connection,
            cache,
            target,
        ),
    }
}

fn is_non_assigner_report(
    record: &Message,
    task_id: &TaskId,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<bool, AtmError> {
    Ok(load_task_row(connection, target, &record.team, task_id)?
        .is_some_and(|row| record.envelope.from != row.assigner))
}

struct TaskAssignmentApplied {
    queued_position: u32,
    reassign_notice: Option<Message>,
}

fn apply_task_assignment(
    record: &Message,
    task_id: &TaskId,
    placement: Option<&MoveTarget>,
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
) -> Result<TaskAssignmentApplied, AtmError> {
    let row = load_task_row(connection, target, &record.team, task_id)?;
    let Transition(next_state) = transition(
        row.as_ref().map(|row| row.state),
        TaskEvent::Assigned,
        task_id,
        &record.envelope.from,
        row.as_ref().map(|row| &row.assignee),
        &record.agent,
    )
    .map_err(|error| error.into_atm_error())?;
    let at = record.envelope.timestamp;
    let message_id = record
        .envelope
        .message_id
        .ok_or_else(|| task_move_invalid("task assignment is missing message id"))?;

    if let Some(queued_position) = super::task_assignment_refresh::refresh_same_assignment(
        record,
        task_id,
        row.as_ref(),
        message_id,
        connection,
        target,
    )? {
        return Ok(TaskAssignmentApplied {
            queued_position,
            reassign_notice: None,
        });
    }

    let previous_assignee = row
        .as_ref()
        .filter(|row| row.state.is_open() && row.assignee != record.agent)
        .map(|row| row.assignee.clone());

    let order = persist_task_assignment(
        record,
        task_id,
        placement,
        row.as_ref(),
        message_id,
        at,
        next_state,
        connection,
        target,
    )?;
    let queued_position = order
        .iter()
        .position(|id| id == task_id)
        .and_then(|index| u32::try_from(index + 1).ok())
        .ok_or_else(|| task_move_invalid("assigned task is absent from its normalized queue"))?;
    let reassign_notice = previous_assignee
        .map(|old_assignee| {
            super::task_reassign_notice::insert_reassign_notice(
                record,
                task_id,
                old_assignee,
                connection,
                cache,
                target,
            )
        })
        .transpose()?;
    Ok(TaskAssignmentApplied {
        queued_position,
        reassign_notice,
    })
}

#[allow(clippy::too_many_arguments)]
fn persist_task_assignment(
    record: &Message,
    task_id: &TaskId,
    placement: Option<&MoveTarget>,
    row: Option<&TaskRow>,
    message_id: AtmMessageId,
    at: IsoTimestamp,
    next_state: TaskState,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<Vec<TaskId>, AtmError> {
    let was_closed = row.is_some_and(|row| matches!(row.state, TaskState::Complete(_)));
    release_previous_assignment(record, task_id, row, connection, target)?;
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
        u32::try_from(order.len()).map_err(|_| task_move_invalid("task queue too large"))?;
    apply_task_assignment_row(
        record, task_id, row, temporary, message_id, at, next_state, connection, target,
    )?;
    renumber_previous_assignment(record, row, connection, target)?;
    renumber_queue(&record.team, &record.agent, &order, connection, target)?;
    append_assignment_event(
        record, task_id, row, was_closed, message_id, at, next_state, connection, target,
    )?;
    Ok(order)
}

#[allow(clippy::too_many_arguments)]
fn release_previous_assignment(
    record: &Message,
    task_id: &TaskId,
    row: Option<&TaskRow>,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    let Some(row) = row.filter(|row| row.state.is_open()) else {
        return Ok(());
    };
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
    next_state: TaskState,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    if row.is_some() {
        connection
            .execute(
                "UPDATE tasks SET assignee=?3, assigner=?4, state=?5, close_outcome=NULL,
                 position=?6, assignment_message_id=?7, description=?8, assigned_at=?9,
                 updated_at=?9, last_reminded_at=NULL, reminder_count=0, lead_notified_count=0
                 WHERE team=?1 AND task_id=?2",
                params![
                    record.team.as_str(),
                    task_id.as_str(),
                    record.agent.as_str(),
                    record.envelope.from.as_str(),
                    next_state.as_str(),
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
                 VALUES (?1,?2,?3,?4,?5,NULL,?6,?7,?8,?9,?9,NULL,0,0)",
                params![
                    record.team.as_str(),
                    task_id.as_str(),
                    record.agent.as_str(),
                    record.envelope.from.as_str(),
                    next_state.as_str(),
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
    next_state: TaskState,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    let event = if was_closed {
        TaskEventKind::Reopened
    } else if row.is_some() {
        TaskEventKind::Reassigned
    } else {
        TaskEventKind::Assigned
    };
    append_task_event(
        connection,
        target,
        &record.team,
        task_id,
        &record.agent,
        &at,
        event,
        row.map(|row| row.state.tag()),
        Some(next_state.tag()),
        row.and_then(|row| row.state.close_outcome()),
        &record.envelope.from,
        Some(message_id),
        None,
        None,
        None,
    )
}

pub(super) fn apply_task_move(
    team: &TeamName,
    task_id: &TaskId,
    actor: &AgentName,
    target_pos: &MoveTarget,
    at: IsoTimestamp,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(AgentName, QueuePosition, QueuePosition), AtmError> {
    let Some(row) = load_task_row(connection, target, team, task_id)? else {
        return Err(task_not_found(format!(
            "no open task {task_id} for {actor}"
        )));
    };
    if !row.state.is_open() {
        return Err(task_already_closed(format!(
            "no open task {task_id} for {actor}"
        )));
    }
    let Transition(next_state) = transition(
        Some(row.state),
        TaskEvent::Assigned,
        task_id,
        actor,
        Some(&row.assignee),
        &row.assignee,
    )
    .map_err(|error| error.into_atm_error())?;
    if row.state == TaskState::Active {
        append_active_task_move(
            team, task_id, actor, &at, &row, next_state, connection, target,
        )?;
        return Ok((row.assignee, QueuePosition::HEAD, QueuePosition::HEAD));
    }
    apply_queued_task_move(
        team, task_id, actor, target_pos, &at, &row, next_state, connection, target,
    )
}

#[allow(clippy::too_many_arguments)]
fn apply_queued_task_move(
    team: &TeamName,
    task_id: &TaskId,
    actor: &AgentName,
    target_pos: &MoveTarget,
    at: &IsoTimestamp,
    row: &TaskRow,
    next_state: TaskState,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(AgentName, QueuePosition, QueuePosition), AtmError> {
    let from = row
        .position
        .ok_or_else(|| task_move_invalid("open task has no queue position"))?;
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
        .ok_or_else(|| task_move_invalid("task move produced no queue position"))?;
    let detail = format!("{}→{}", from.get(), to.get());
    append_task_event(
        connection,
        target,
        team,
        task_id,
        &row.assignee,
        at,
        TaskEventKind::Moved,
        Some(row.state.tag()),
        Some(next_state.tag()),
        None,
        actor,
        None,
        None,
        None,
        Some(&detail),
    )?;
    Ok((row.assignee.clone(), from, to))
}

#[allow(clippy::too_many_arguments)]
fn append_active_task_move(
    team: &TeamName,
    task_id: &TaskId,
    actor: &AgentName,
    at: &IsoTimestamp,
    row: &TaskRow,
    next_state: TaskState,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    append_task_event(
        connection,
        target,
        team,
        task_id,
        &row.assignee,
        at,
        TaskEventKind::Moved,
        Some(row.state.tag()),
        Some(next_state.tag()),
        None,
        actor,
        None,
        None,
        None,
        Some("1→1"),
    )
}

pub(super) fn queue_order(
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
                .map_err(|error| task_move_invalid(format!("invalid queued task id: {error}")))
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
                return Err(task_move_invalid(format!(
                    "task {task_id}: placement target {other} is not an open queued task of {assignee}"
                )));
            }
            order.iter().position(|id| id == other).ok_or_else(|| {
                task_move_invalid(format!(
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
pub(super) fn renumber_queue(
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
        .checked_add(
            i64::try_from(order.len()).map_err(|_| task_move_invalid("task queue too large"))?,
        )
        .ok_or_else(|| task_move_invalid("task queue too large"))?;
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
        return Err(task_move_invalid(format!(
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
            i64::try_from(index + 1).map_err(|_| task_move_invalid("task queue too large"))?;
        connection
            .execute(
                "UPDATE tasks SET position=?3 WHERE team=?1 AND task_id=?2",
                params![team.as_str(), task_id.as_str(), position],
            )
            .map_err(|error| sqlite_error(target, "failed to renumber task queue", error))?;
    }
    Ok(())
}

pub(super) fn acknowledge_assignment(
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
        message_key: message_key.parse().map_err(|error| {
            task_move_invalid(format!("invalid assignment message key: {error}"))
        })?,
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
pub(super) fn append_task_event(
    connection: &Connection,
    target: &SharedDbTarget,
    team: &TeamName,
    task_id: &TaskId,
    assignee: &AgentName,
    at: &IsoTimestamp,
    event: TaskEventKind,
    from_state: Option<TaskStateTag>,
    to_state: Option<TaskStateTag>,
    close_outcome: Option<TaskCloseOutcome>,
    actor: &AgentName,
    message_id: Option<AtmMessageId>,
    outcome: Option<&str>,
    marker: Option<&str>,
    detail: Option<&str>,
) -> Result<(), AtmError> {
    let from_state = from_state.map(task_state_tag_name);
    let to_state = to_state.map(task_state_tag_name);
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
                event.as_str(),
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

const fn task_state_tag_name(state: TaskStateTag) -> &'static str {
    match state {
        TaskStateTag::Assigned => "assigned",
        TaskStateTag::Active => "active",
        TaskStateTag::Complete => "complete",
    }
}
