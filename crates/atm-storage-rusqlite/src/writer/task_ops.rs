//! The rusqlite writer's sole task-ledger application site (AX.3, C6).
//!
//! `tasks.state` and `task_events` are mutated only here, inside the writer's
//! transaction connection, so a message insert or acknowledgement, its task
//! row, and its audit event either commit together or roll back together.
//! `atm_storage::task_state` defines the pure, backend-neutral transition
//! table this module's SQL mirrors; nothing here changes that table's rules.

use super::ops::{execute_upsert_message, load_existing_message, mark_source_acknowledged};
use super::stmt_cache::WriterStatementCache;
use crate::shared_db::{SharedDbTarget, sqlite_error};
use atm_storage::MessageWriteOrigin;
use atm_storage::contract::Message;
use atm_storage::error::AtmError;
use atm_storage::schema::AtmMessageId;
use atm_storage::task_state::{TaskEvent, TaskRow, TaskState, Transition, admit, transition};
use atm_storage::types::{AgentName, TaskId, TeamName};
use rusqlite::{Connection, OptionalExtension, params};

const TASK_RECOVERY: &str = "Run: atm list --task-events <task_id> --member <assignee>";

fn task_rejected(detail: impl std::fmt::Display) -> AtmError {
    AtmError::validation(format!("{detail}; {TASK_RECOVERY}"))
}

fn load_task_row(
    connection: &Connection,
    target: &SharedDbTarget,
    team: &TeamName,
    task_id: &TaskId,
    assignee: &AgentName,
) -> Result<Option<TaskRow>, AtmError> {
    connection
        .query_row(
            "SELECT team, task_id, assignee, assigner, state, assignment_message_id,
                    description, assigned_at, updated_at, last_reminded_at, reminder_count,
                    lead_notified_count
               FROM tasks WHERE team = ?1 AND task_id = ?2 AND assignee = ?3",
            params![team.as_str(), task_id.as_str(), assignee.as_str()],
            crate::SqliteTaskStore::decode_row,
        )
        .optional()
        .map_err(|error| sqlite_error(target, "failed to load task row", error))
}

fn load_open_task_rows(
    connection: &Connection,
    target: &SharedDbTarget,
    team: &TeamName,
    assignee: &AgentName,
) -> Result<Vec<TaskRow>, AtmError> {
    let mut statement = connection
        .prepare(
            "SELECT team, task_id, assignee, assigner, state, assignment_message_id,
                    description, assigned_at, updated_at, last_reminded_at, reminder_count,
                    lead_notified_count
               FROM tasks WHERE team = ?1 AND assignee = ?2 AND state <> 'complete'",
        )
        .map_err(|error| sqlite_error(target, "failed to prepare open task lookup", error))?;
    statement
        .query_map(
            params![team.as_str(), assignee.as_str()],
            crate::SqliteTaskStore::decode_row,
        )
        .map_err(|error| sqlite_error(target, "failed to load open tasks", error))?
        .map(|row| row.map_err(|error| sqlite_error(target, "failed to decode open task", error)))
        .collect()
}

fn transition_for(
    row: Option<&TaskRow>,
    open: &[TaskRow],
    event: TaskEvent,
    actor: &AgentName,
) -> Result<Transition, AtmError> {
    admit(row, open, event, actor).map_err(|error| error.into_atm_error())?;
    transition(row.map(|task| task.state), event).map_err(|error| error.into_atm_error())
}

/// Applies the only local message-insert task transitions. This function runs
/// on the writer's transaction connection, so message, task row, and audit
/// event either commit together or roll back together.
pub(super) fn apply_task_message(
    record: &Message,
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    if record.envelope.task_id.is_some() && record.envelope.task_complete.is_some() {
        return Err(task_rejected(
            "a message cannot assign and complete a task at the same time",
        ));
    }
    if let Some(task_id) = record.envelope.task_id.as_ref() {
        return apply_task_assignment(record, task_id.as_str(), connection, target);
    }
    if let Some(task_id) = record.envelope.task_complete.as_ref() {
        return apply_task_completion(record, task_id.as_str(), connection, cache, target);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn refresh_task_assignment(
    record: &Message,
    task_id: &str,
    connection: &Connection,
    target: &SharedDbTarget,
    state: &str,
    at: &str,
    message_id: AtmMessageId,
) -> Result<(), AtmError> {
    connection
        .execute(
            "UPDATE tasks SET assignment_message_id = ?4, description = ?5, updated_at = ?6
          WHERE team = ?1 AND task_id = ?2 AND assignee = ?3",
            params![
                record.team.as_str(),
                task_id,
                record.agent.as_str(),
                message_id.to_string(),
                record.envelope.text,
                at
            ],
        )
        .map_err(|error| sqlite_error(target, "failed to refresh task assignment", error))?;
    append_task_event(
        connection,
        target,
        record.team.as_str(),
        task_id,
        record.agent.as_str(),
        at,
        "assigned",
        Some(state),
        Some(state),
        record.envelope.from.as_str(),
        Some(message_id),
        None,
        Some("resend"),
        None,
    )
}

fn insert_task_assignment(
    record: &Message,
    task_id: &str,
    connection: &Connection,
    target: &SharedDbTarget,
    at: &str,
    message_id: AtmMessageId,
) -> Result<(), AtmError> {
    connection
        .execute(
            "INSERT INTO tasks(team, task_id, assignee, assigner, state, assignment_message_id,
                           description, assigned_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, 'assigned', ?5, ?6, ?7, ?7)",
            params![
                record.team.as_str(),
                task_id,
                record.agent.as_str(),
                record.envelope.from.as_str(),
                message_id.to_string(),
                record.envelope.text,
                at
            ],
        )
        .map_err(|error| sqlite_error(target, "failed to insert task assignment", error))?;
    append_task_event(
        connection,
        target,
        record.team.as_str(),
        task_id,
        record.agent.as_str(),
        at,
        "assigned",
        None,
        Some("assigned"),
        record.envelope.from.as_str(),
        Some(message_id),
        None,
        None,
        None,
    )
}

fn apply_task_assignment(
    record: &Message,
    task_id: &str,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    let typed_task_id = task_id
        .parse::<TaskId>()
        .map_err(|error| task_rejected(format!("invalid task id: {error}")))?;
    let row = load_task_row(
        connection,
        target,
        &record.team,
        &typed_task_id,
        &record.agent,
    )?;
    let open = load_open_task_rows(connection, target, &record.team, &record.agent)?;
    let next = transition_for(
        row.as_ref(),
        &open,
        TaskEvent::Assigned,
        &record.envelope.from,
    )?;
    let at = record.envelope.timestamp.to_string();
    let message_id = record
        .envelope
        .message_id
        .ok_or_else(|| task_rejected("task assignment is missing message id"))?;
    match (row, next) {
        (Some(row), Transition::To(_)) => refresh_task_assignment(
            record,
            task_id,
            connection,
            target,
            state_name(row.state),
            &at,
            message_id,
        ),
        (None, Transition::To(TaskState::Assigned)) => {
            insert_task_assignment(record, task_id, connection, target, &at, message_id)
        }
        (_, Transition::NoOp) | (None, Transition::To(_)) => Err(task_rejected(
            "task assignment did not produce an assigned state",
        )),
    }
}

const fn state_name(state: TaskState) -> &'static str {
    match state {
        TaskState::Assigned => "assigned",
        TaskState::Active => "active",
        TaskState::Complete => "complete",
    }
}

fn apply_task_completion(
    record: &Message,
    task_id: &str,
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    let typed_task_id = task_id
        .parse::<TaskId>()
        .map_err(|error| task_rejected(format!("invalid task id: {error}")))?;
    let direct = load_task_row(
        connection,
        target,
        &record.team,
        &typed_task_id,
        &record.envelope.from,
    )?;
    let row = match direct {
        Some(row) => Some(row),
        None => load_task_row(
            connection,
            target,
            &record.team,
            &typed_task_id,
            &record.agent,
        )?,
    };
    let Some(row) = row else {
        return Err(task_rejected(format!(
            "no open task {task_id} for {}",
            record.envelope.from
        )));
    };
    let next = transition_for(Some(&row), &[], TaskEvent::Completed, &record.envelope.from)?;
    let Transition::To(next_state) = next else {
        return Err(task_rejected("task completion did not produce a state"));
    };
    let at = record.envelope.timestamp.to_string();
    connection.execute(
        "UPDATE tasks SET state = ?4, updated_at = ?5 WHERE team = ?1 AND task_id = ?2 AND assignee = ?3",
        params![record.team.as_str(), task_id, row.assignee.as_str(), state_name(next_state), at],
    ).map_err(|error| sqlite_error(target, "failed to complete task", error))?;
    let marker = acknowledge_completed_assignment(
        connection,
        cache,
        target,
        record,
        task_id,
        row.assignee.as_str(),
        row.state,
    )?;
    append_task_event(
        connection,
        target,
        record.team.as_str(),
        task_id,
        row.assignee.as_str(),
        &at,
        "completed",
        Some(state_name(row.state)),
        Some(state_name(next_state)),
        record.envelope.from.as_str(),
        record.envelope.message_id,
        None,
        marker,
        None,
    )
}

/// Completing a task skipped the ack (code contract C7): mark the
/// assignment message acknowledged in the same transaction so it does not
/// remain pending-ack forever. Returns the `assignment_missing` audit marker
/// when the assignment message row could not be found.
#[allow(clippy::too_many_arguments)]
fn acknowledge_completed_assignment(
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
    record: &Message,
    task_id: &str,
    assignee: &str,
    state: TaskState,
) -> Result<Option<&'static str>, AtmError> {
    if state != TaskState::Assigned {
        return Ok(None);
    }
    let message_key: Option<String> = connection
        .query_row(
            "SELECT mail_messages.message_key FROM mail_messages
              WHERE team = ?1 AND agent = ?2
                AND message_id = (SELECT assignment_message_id FROM tasks WHERE team = ?1 AND task_id = ?3 AND assignee = ?2)",
            params![record.team.as_str(), assignee, task_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| sqlite_error(target, "failed to load completed task assignment", error))?;
    let Some(message_key) = message_key else {
        return Ok(Some("assignment_missing"));
    };
    let requested = Message {
        team: record.team.clone(),
        agent: assignee
            .parse()
            .map_err(|error| task_rejected(format!("invalid task assignee: {error}")))?,
        message_key: message_key
            .parse()
            .map_err(|error| task_rejected(format!("invalid assignment message key: {error}")))?,
        envelope: record.envelope.clone(),
    };
    let mut assignment = load_existing_message(&requested, connection, target)?;
    mark_source_acknowledged(&mut assignment, record.envelope.timestamp);
    let _ = execute_upsert_message(
        &assignment,
        MessageWriteOrigin::Local,
        connection,
        cache,
        target,
    )?;
    Ok(None)
}

pub(super) fn apply_task_acknowledgement(
    source: &Message,
    actor: &AgentName,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    let Some(task_id) = source.envelope.task_id.as_ref() else {
        return Ok(());
    };
    let row = load_task_row(connection, target, &source.team, task_id, &source.agent)?;
    let Some(row) = row else {
        return Ok(());
    };
    let open = load_open_task_rows(connection, target, &source.team, &source.agent)?;
    let next = transition_for(Some(&row), &open, TaskEvent::Acked, actor)?;
    let Transition::To(next_state) = next else {
        return Ok(());
    };
    let at = atm_storage::types::IsoTimestamp::now().to_string();
    connection.execute(
        "UPDATE tasks SET state = ?4, updated_at = ?5 WHERE team = ?1 AND task_id = ?2 AND assignee = ?3",
        params![source.team.as_str(), task_id.as_str(), source.agent.as_str(), state_name(next_state), at],
    ).map_err(|error| sqlite_error(target, "failed to activate acknowledged task", error))?;
    append_task_event(
        connection,
        target,
        source.team.as_str(),
        task_id.as_str(),
        source.agent.as_str(),
        &at,
        "acked",
        Some(state_name(row.state)),
        Some(state_name(next_state)),
        actor.as_str(),
        source.envelope.message_id,
        None,
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn append_task_event(
    connection: &Connection,
    target: &SharedDbTarget,
    team: &str,
    task_id: &str,
    assignee: &str,
    at: &str,
    event: &str,
    from_state: Option<&str>,
    to_state: Option<&str>,
    actor: &str,
    message_id: Option<AtmMessageId>,
    outcome: Option<&str>,
    marker: Option<&str>,
    detail: Option<&str>,
) -> Result<(), AtmError> {
    let seq: u64 = connection.query_row(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM task_events WHERE team = ?1 AND task_id = ?2 AND assignee = ?3",
        params![team, task_id, assignee], |row| row.get(0),
    ).map_err(|error| sqlite_error(target, "failed to allocate task event sequence", error))?;
    connection.execute(
        "INSERT INTO task_events(team, task_id, assignee, seq, at, event, from_state, to_state, actor, message_id, outcome, marker, detail)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![team, task_id, assignee, seq, at, event, from_state, to_state, actor,
            message_id.map(|value| value.to_string()), outcome, marker, detail],
    ).map_err(|error| sqlite_error(target, "failed to append task event", error))?;
    Ok(())
}
