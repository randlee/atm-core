use crate::shared_db::{SharedDbTarget, sqlite_error};
use atm_storage::contract::Message;
use atm_storage::error::AtmError;
use atm_storage::schema::AtmMessageId;
use atm_storage::types::{AgentName, IsoTimestamp};
use rusqlite::{Connection, OptionalExtension, params};

const TASK_RECOVERY: &str = "Run: atm list --task-events <task_id> --member <assignee>";

fn task_rejected(detail: impl std::fmt::Display) -> AtmError {
    AtmError::validation(format!("{detail}; {TASK_RECOVERY}"))
}

/// Applies the only local message-insert task transitions. This function runs
/// on the writer's transaction connection, so message, task row, and audit
/// event either commit together or roll back together.
pub(super) fn apply_task_message(
    record: &Message,
    connection: &Connection,
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
        return apply_task_completion(record, task_id.as_str(), connection, target);
    }
    Ok(())
}

fn apply_task_assignment(
    record: &Message,
    task_id: &str,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    let existing: Option<String> = connection
        .query_row(
            "SELECT state FROM tasks WHERE team = ?1 AND task_id = ?2 AND assignee = ?3",
            params![record.team.as_str(), task_id, record.agent.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| sqlite_error(target, "failed to resolve task assignment", error))?;
    let at = record.envelope.timestamp.to_string();
    let message_id = record
        .envelope
        .message_id
        .ok_or_else(|| task_rejected("task assignment is missing message id"))?;
    match existing {
        Some(state) => {
            refresh_task_assignment(record, task_id, connection, target, &state, &at, message_id)
        }
        None => insert_task_assignment(record, task_id, connection, target, &at, message_id),
    }
}

fn refresh_task_assignment(
    record: &Message,
    task_id: &str,
    connection: &Connection,
    target: &SharedDbTarget,
    state: &str,
    at: &str,
    message_id: AtmMessageId,
) -> Result<(), AtmError> {
    if state == "complete" {
        return Err(task_rejected(format!(
            "task {task_id} already complete; use a new id"
        )));
    }
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

fn apply_task_completion(
    record: &Message,
    task_id: &str,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    let actor = record.envelope.from.as_str();
    let found: Option<(String, String, String)> = connection.query_row(
        "SELECT assignee, assigner, state FROM tasks WHERE team = ?1 AND task_id = ?2 AND assignee = ?3",
        params![record.team.as_str(), task_id, actor], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).optional().map_err(|error| sqlite_error(target, "failed to resolve assignee task completion", error))?;
    let found = match found {
        Some(found) => Some(found),
        None => connection.query_row(
        "SELECT assignee, assigner, state FROM tasks WHERE team = ?1 AND task_id = ?2 AND assignee = ?3 AND assigner = ?4",
        params![record.team.as_str(), task_id, record.agent.as_str(), actor], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).optional().map_err(|error| sqlite_error(target, "failed to resolve assigner task completion", error))?,
    };
    let Some((assignee, _assigner, state)) = found else {
        return Err(task_rejected(format!("no open task {task_id} for {actor}")));
    };
    if state == "complete" {
        return Err(task_rejected(format!("task {task_id} already complete")));
    }
    let at = record.envelope.timestamp.to_string();
    connection.execute(
        "UPDATE tasks SET state = 'complete', updated_at = ?4 WHERE team = ?1 AND task_id = ?2 AND assignee = ?3",
        params![record.team.as_str(), task_id, assignee, at],
    ).map_err(|error| sqlite_error(target, "failed to complete task", error))?;
    let marker = if state == "assigned" {
        let changed = connection.execute(
            "UPDATE mail_message_states SET read = 1, pending_ack_at = NULL, acknowledged_at = ?4, updated_at = ?4
              WHERE team = ?1 AND agent = ?2 AND message_key = (
                  SELECT message_key FROM mail_messages WHERE team = ?1 AND agent = ?2
                    AND message_id = (SELECT assignment_message_id FROM tasks WHERE team = ?1 AND task_id = ?3 AND assignee = ?2)
              )",
            params![record.team.as_str(), assignee, task_id, at],
        ).map_err(|error| sqlite_error(target, "failed to acknowledge completed task assignment", error))?;
        (changed == 0).then_some("assignment_missing")
    } else {
        None
    };
    append_task_event(
        connection,
        target,
        record.team.as_str(),
        task_id,
        &assignee,
        &at,
        "completed",
        Some(&state),
        Some("complete"),
        actor,
        record.envelope.message_id,
        None,
        marker,
        None,
    )
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
    let task_id = task_id.as_str();
    let state: Option<String> = connection
        .query_row(
            "SELECT state FROM tasks WHERE team = ?1 AND task_id = ?2 AND assignee = ?3",
            params![source.team.as_str(), task_id, source.agent.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| sqlite_error(target, "failed to resolve acknowledged task", error))?;
    let Some(state) = state else {
        return Ok(());
    };
    if state == "complete" {
        return Err(task_rejected(format!("task {task_id} already complete")));
    }
    if state == "assigned" {
        let other: Option<String> = connection.query_row(
            "SELECT task_id FROM tasks WHERE team = ?1 AND assignee = ?2 AND state = 'active' AND task_id <> ?3 LIMIT 1",
            params![source.team.as_str(), source.agent.as_str(), task_id], |row| row.get(0),
        ).optional().map_err(|error| sqlite_error(target, "failed to check active task guard", error))?;
        if let Some(other) = other {
            return Err(task_rejected(format!(
                "task {other} is active; complete it first"
            )));
        }
    }
    let at = IsoTimestamp::now().to_string();
    connection.execute(
        "UPDATE tasks SET state = 'active', updated_at = ?4 WHERE team = ?1 AND task_id = ?2 AND assignee = ?3",
        params![source.team.as_str(), task_id, source.agent.as_str(), at],
    ).map_err(|error| sqlite_error(target, "failed to activate acknowledged task", error))?;
    append_task_event(
        connection,
        target,
        source.team.as_str(),
        task_id,
        source.agent.as_str(),
        &at,
        "acked",
        Some(&state),
        Some("active"),
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
