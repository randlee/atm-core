//! The rusqlite writer's sole task-ledger application site (AX.3, C6).
//!
//! `tasks.state` and `task_events` are mutated only here, inside the writer's
//! transaction connection, so a message insert or acknowledgement, its task
//! row, and its audit event either commit together or roll back together.
//! `atm_storage::task_state` defines the pure, backend-neutral transition
//! table this module's SQL mirrors; nothing here changes that table's rules.

use super::ops::{
    WriteOp, execute_task_mutation_message_upsert, execute_upsert_message, load_existing_message,
    load_pending_ack_source, mark_source_acknowledged,
};
use super::stmt_cache::WriterStatementCache;
use crate::shared_db::{SharedDbTarget, sqlite_error};
use atm_storage::contract::Message;
use atm_storage::error::AtmError;
use atm_storage::schema::AtmMessageId;
use atm_storage::task_state::{TaskEvent, TaskRow, TaskState, Transition, admit, transition};
use atm_storage::types::{AgentName, TaskId, TeamName};
use atm_storage::{AtmErrorCode, MessageWriteOrigin};
use atm_storage::{
    TaskLifecycleAction, TaskLifecycleState, TaskLifecycleTransition, TaskMutationOutcome,
    TaskMutationRequest, TaskOperation, TaskOutcome, lifecycle_transition,
};
use rusqlite::{Connection, OptionalExtension, params};

use crate::task_sql;

const TASK_RECOVERY: &str = "Run: atm list --task-events <task_id> --member <assignee>";

/// Applies a canonical v2 mutation inside the existing writer transaction.
///
/// The full operation table is deliberately centralized here so the retained
/// v1 message adapter cannot grow an independent transition policy.
pub(super) fn execute_task_mutation(
    request: &TaskMutationRequest,
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
) -> Result<TaskMutationOutcome, AtmError> {
    let fingerprint = serde_json::to_string(request).map_err(|error| {
        AtmError::new(
            AtmErrorCode::SerializationFailed,
            "failed to fingerprint task mutation",
        )
        .with_cause(error)
    })?;
    let replay: Option<(String, String)> = connection
        .query_row(
            "SELECT request_fingerprint, result_json FROM task_operations WHERE team = ?1 AND operation_id = ?2",
            params![request.actor.team().as_str(), request.operation_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| sqlite_error(target, "failed to load task operation", error))?;
    if let Some((previous_fingerprint, result)) = replay {
        if previous_fingerprint != fingerprint {
            return Err(task_error(
                AtmErrorCode::TaskOperationConflict,
                "task operation id was reused with different input",
            ));
        }
        let mut outcome: TaskMutationOutcome = serde_json::from_str(&result).map_err(|error| {
            AtmError::new(
                AtmErrorCode::SerializationFailed,
                "stored task operation result is invalid",
            )
            .with_cause(error)
        })?;
        outcome.replayed = true;
        return Ok(outcome);
    }

    let current: Option<(String, Option<String>, u64)> = connection
        .query_row(
            "SELECT state, outcome, revision FROM tasks_v2 WHERE team = ?1 AND task_id = ?2",
            params![request.actor.team().as_str(), request.task_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|error| sqlite_error(target, "failed to load v2 task", error))?;
    if let Some(expected) = request.expected_revision
        && current.as_ref().map(|row| row.2) != Some(expected)
    {
        return Err(task_error(
            AtmErrorCode::TaskRevisionStale,
            "task revision is stale",
        ));
    }

    let now = atm_storage::IsoTimestamp::now().to_string();
    let (state, revision, event, detail, related_task_id) = match &request.operation {
        TaskOperation::Assign(assignment) => {
            if current.is_some() {
                return Err(task_rejected(
                    "logical task already exists; use reassign or reopen",
                ));
            }
            if assignment.assignee.team() != request.actor.team()
                || assignment.assigner.team() != request.actor.team()
            {
                return Err(task_rejected("cross-team task assignment is unsupported"));
            }
            let message_id = assignment_message_id(request, assignment)?;
            validate_assignment(request, assignment)?;
            persist_prepared_message(&assignment.message, connection, cache, target)?;
            connection.execute(
                "INSERT INTO tasks_v2(team, task_id, current_assignee, state, outcome, abort_reason, superseded_by, priority, original_assigned_at, current_attempt, reminder_ordinal, revision, updated_at)
                 VALUES (?1, ?2, ?3, 'assigned', NULL, NULL, NULL, ?4, ?5, 1, 0, 1, ?5)",
                params![request.actor.team().as_str(), request.task_id.as_str(), assignment.assignee.agent().as_str(), priority_name(assignment.priority), now],
            ).map_err(|error| sqlite_error(target, "failed to insert v2 task", error))?;
            connection.execute(
                "INSERT INTO task_assignment_attempts(team, task_id, attempt, assignee, assigner, assignment_message_id, template_sha, assigned_at)
                 VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6, ?7)",
                params![request.actor.team().as_str(), request.task_id.as_str(), assignment.assignee.agent().as_str(), assignment.assigner.agent().as_str(), message_id, assignment.template_sha.as_ref().map(ToString::to_string), now],
            ).map_err(|error| sqlite_error(target, "failed to insert task assignment attempt", error))?;
            (TaskLifecycleState::Assigned, 1, "assigned", None, None)
        }
        TaskOperation::Start => transition_v2(
            connection, target, request, "active", None, &now, "started", None, None,
        )?,
        TaskOperation::Block { .. } => {
            let result = transition_v2(
                connection, target, request, "blocked", None, &now, "blocked", None, None,
            )?;
            clear_task_nudge_markers(connection, target, request)?;
            result
        }
        TaskOperation::Unblock { .. } => transition_v2(
            connection,
            target,
            request,
            "assigned",
            None,
            &now,
            "unblocked",
            None,
            None,
        )?,
        TaskOperation::Close { outcome, .. } => {
            let TaskOperation::Close { handoff, .. } = &request.operation else {
                unreachable!("matched close operation")
            };
            persist_prepared_message(handoff, connection, cache, target)?;
            let result = transition_v2(
                connection,
                target,
                request,
                "closed",
                Some(outcome),
                &now,
                "closed",
                None,
                None,
            )?;
            clear_task_nudge_markers(connection, target, request)?;
            result
        }
        TaskOperation::LegacyCloseSucceeded { completion_notice } => {
            persist_prepared_message(completion_notice, connection, cache, target)?;
            let result = transition_v2(
                connection,
                target,
                request,
                "closed",
                Some(&TaskOutcome::Succeeded),
                &now,
                "legacy_close_succeeded",
                None,
                None,
            )?;
            clear_task_nudge_markers(connection, target, request)?;
            result
        }
        TaskOperation::Reassign(assignment) => {
            let result = reassign_v2(connection, cache, target, request, assignment, &now, false)?;
            clear_task_nudge_markers(connection, target, request)?;
            result
        }
        TaskOperation::Reopen(assignment) => {
            let result = reassign_v2(connection, cache, target, request, assignment, &now, true)?;
            clear_task_nudge_markers(connection, target, request)?;
            result
        }
        TaskOperation::Supersede {
            handoff,
            successor_task_id,
            successor,
        } => supersede_v2(
            connection,
            cache,
            target,
            request,
            handoff,
            successor_task_id,
            successor,
            &now,
        )?,
    };
    let result = TaskMutationOutcome {
        task_id: request.task_id.clone(),
        state,
        revision,
        replayed: false,
    };
    let result_json = serde_json::to_string(&result).map_err(|error| {
        AtmError::new(
            AtmErrorCode::SerializationFailed,
            "failed to encode task mutation result",
        )
        .with_cause(error)
    })?;
    connection.execute(
        "INSERT INTO task_operations(team, operation_id, request_fingerprint, result_json) VALUES (?1, ?2, ?3, ?4)",
        params![request.actor.team().as_str(), request.operation_id.to_string(), fingerprint, result_json],
    ).map_err(|error| sqlite_error(target, "failed to persist task operation", error))?;
    append_v2_event(
        connection,
        target,
        request,
        event,
        detail,
        related_task_id,
        &now,
    )?;
    Ok(result)
}

fn transition_v2(
    connection: &Connection,
    target: &SharedDbTarget,
    request: &TaskMutationRequest,
    next_state: &str,
    terminal: Option<&TaskOutcome>,
    at: &str,
    event: &'static str,
    detail: Option<&str>,
    related_task_id: Option<&TaskId>,
) -> Result<
    (
        TaskLifecycleState,
        u64,
        &'static str,
        Option<String>,
        Option<TaskId>,
    ),
    AtmError,
> {
    let (current_state, revision, assignee): (String, u64, String) = connection
        .query_row(
            "SELECT state, revision, current_assignee FROM tasks_v2 WHERE team = ?1 AND task_id = ?2",
            params![request.actor.team().as_str(), request.task_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|error| sqlite_error(target, "task mutation requires an existing task", error))?;
    let current = match current_state.as_str() {
        "assigned" => TaskLifecycleState::Assigned,
        "active" => TaskLifecycleState::Active,
        "blocked" => TaskLifecycleState::Blocked,
        _ => return Err(task_rejected("task transition requires an open task")),
    };
    let action = match (next_state, terminal, event) {
        ("active", None, "started") => TaskLifecycleAction::Start,
        ("blocked", None, "blocked") => TaskLifecycleAction::Block,
        ("assigned", None, "unblocked") => TaskLifecycleAction::Unblock,
        ("closed", Some(outcome), "legacy_close_succeeded") => {
            if *outcome != TaskOutcome::Succeeded {
                return Err(task_error(
                    AtmErrorCode::TaskTerminalMetadataInvalid,
                    "legacy completion must succeed",
                ));
            }
            TaskLifecycleAction::LegacyCloseSucceeded
        }
        ("closed", Some(outcome), "closed") => TaskLifecycleAction::Close(outcome.clone()),
        _ => {
            return Err(task_error(
                AtmErrorCode::TaskTerminalMetadataInvalid,
                "task transition does not carry valid terminal metadata",
            ));
        }
    };
    let TaskLifecycleTransition::To(expected) = lifecycle_transition(Some(&current), &action)
        .map_err(|error| task_error(AtmErrorCode::TaskTransitionInvalid, error.detail))?;
    if next_state == "active" {
        let active_exists: bool = connection
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM tasks_v2
                    WHERE team = ?1 AND current_assignee = ?2 AND state = 'active' AND task_id <> ?3
                )",
                params![
                    request.actor.team().as_str(),
                    assignee,
                    request.task_id.as_str()
                ],
                |row| row.get(0),
            )
            .map_err(|error| {
                sqlite_error(target, "failed to check active task invariant", error)
            })?;
        if active_exists {
            return Err(task_error(
                AtmErrorCode::TaskActiveConflict,
                "member already has an active task",
            ));
        }
    }
    let next_revision = revision.saturating_add(1);
    let outcome_name = terminal.map(outcome_name);
    connection
        .execute(
            "UPDATE tasks_v2 SET state = ?3, outcome = ?4, revision = ?5, updated_at = ?6
         WHERE team = ?1 AND task_id = ?2",
            params![
                request.actor.team().as_str(),
                request.task_id.as_str(),
                next_state,
                outcome_name,
                next_revision,
                at
            ],
        )
        .map_err(|error| sqlite_error(target, "failed to transition v2 task", error))?;
    let state = expected;
    Ok((
        state,
        next_revision,
        event,
        detail.map(str::to_owned),
        related_task_id.cloned(),
    ))
}

fn assignment_message_id(
    request: &TaskMutationRequest,
    assignment: &atm_storage::PreparedAssignment,
) -> Result<String, AtmError> {
    validate_assignment(request, assignment)?;
    assignment
        .message
        .message
        .envelope
        .message_id
        .map(|id| id.to_string())
        .ok_or_else(|| task_rejected("task assignment message requires a message id"))
}

fn validate_assignment(
    request: &TaskMutationRequest,
    assignment: &atm_storage::PreparedAssignment,
) -> Result<(), AtmError> {
    if assignment.assignee.team() != request.actor.team()
        || assignment.assigner.team() != request.actor.team()
        || assignment.message.message.team != *request.actor.team()
        || assignment.message.message.agent != *assignment.assignee.agent()
        || assignment.message.message.envelope.task_id.as_ref() != Some(&request.task_id)
    {
        return Err(task_rejected(
            "assignment message and members must belong to the task's team and task id",
        ));
    }
    Ok(())
}

fn persist_prepared_message(
    prepared: &atm_storage::PreparedMessage,
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    match execute_task_mutation_message_upsert(&prepared.message, connection, cache, target)? {
        super::ops::WriteOpResult::UpsertMessage { .. } => Ok(()),
        other => Err(AtmError::daemon_unavailable(format!(
            "sqlite writer returned the wrong result for task mutation message: {other:?}"
        ))),
    }
}

fn reassign_v2(
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
    request: &TaskMutationRequest,
    assignment: &atm_storage::PreparedAssignment,
    at: &str,
    reopen: bool,
) -> Result<
    (
        TaskLifecycleState,
        u64,
        &'static str,
        Option<String>,
        Option<TaskId>,
    ),
    AtmError,
> {
    let message_id = assignment_message_id(request, assignment)?;
    let (current_state, revision, attempt): (String, u64, u32) = connection
        .query_row(
            "SELECT state, revision, current_attempt FROM tasks_v2 WHERE team = ?1 AND task_id = ?2",
            params![request.actor.team().as_str(), request.task_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|error| sqlite_error(target, "task reassignment requires an existing task", error))?;
    let current = match current_state.as_str() {
        "assigned" => TaskLifecycleState::Assigned,
        "active" => TaskLifecycleState::Active,
        "blocked" => TaskLifecycleState::Blocked,
        "closed" => TaskLifecycleState::Closed(TaskOutcome::Failed),
        _ => {
            return Err(task_rejected(
                "task reassignment has an invalid current state",
            ));
        }
    };
    let action = if reopen {
        TaskLifecycleAction::Reopen
    } else {
        TaskLifecycleAction::Reassign
    };
    let TaskLifecycleTransition::To(TaskLifecycleState::Assigned) =
        lifecycle_transition(Some(&current), &action)
            .map_err(|error| task_error(AtmErrorCode::TaskTransitionInvalid, error.detail))?
    else {
        return Err(task_rejected(
            "task reassignment must return task to assigned",
        ));
    };
    let next_attempt = attempt
        .checked_add(1)
        .ok_or_else(|| task_rejected("task assignment attempt overflow"))?;
    persist_prepared_message(&assignment.message, connection, cache, target)?;
    let next_revision = revision.saturating_add(1);
    connection
        .execute(
            "UPDATE tasks_v2
             SET current_assignee = ?3, state = 'assigned', outcome = NULL,
                 abort_reason = NULL, superseded_by = NULL, priority = ?4,
                 current_attempt = ?5, revision = ?6, updated_at = ?7
             WHERE team = ?1 AND task_id = ?2",
            params![
                request.actor.team().as_str(),
                request.task_id.as_str(),
                assignment.assignee.agent().as_str(),
                priority_name(assignment.priority),
                next_attempt,
                next_revision,
                at
            ],
        )
        .map_err(|error| sqlite_error(target, "failed to reassign v2 task", error))?;
    connection
        .execute(
            "INSERT INTO task_assignment_attempts(team, task_id, attempt, assignee, assigner, assignment_message_id, template_sha, assigned_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                request.actor.team().as_str(),
                request.task_id.as_str(),
                next_attempt,
                assignment.assignee.agent().as_str(),
                assignment.assigner.agent().as_str(),
                message_id,
                assignment.template_sha.as_ref().map(ToString::to_string),
                at,
            ],
        )
        .map_err(|error| sqlite_error(target, "failed to append task assignment attempt", error))?;
    Ok((
        TaskLifecycleState::Assigned,
        next_revision,
        if reopen { "reopened" } else { "reassigned" },
        None,
        None,
    ))
}

#[allow(clippy::too_many_arguments)]
fn supersede_v2(
    connection: &Connection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
    request: &TaskMutationRequest,
    handoff: &atm_storage::PreparedMessage,
    successor_task_id: &TaskId,
    successor: &atm_storage::PreparedAssignment,
    at: &str,
) -> Result<
    (
        TaskLifecycleState,
        u64,
        &'static str,
        Option<String>,
        Option<TaskId>,
    ),
    AtmError,
> {
    if successor_task_id == &request.task_id {
        return Err(task_rejected("a successor task must use a new task id"));
    }
    let message_id = assignment_message_id_for(successor_task_id, request.actor.team(), successor)?;
    let (current_state, revision): (String, u64) = connection
        .query_row(
            "SELECT state, revision FROM tasks_v2 WHERE team = ?1 AND task_id = ?2",
            params![request.actor.team().as_str(), request.task_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| {
            sqlite_error(target, "task supersession requires an existing task", error)
        })?;
    let current = match current_state.as_str() {
        "assigned" => TaskLifecycleState::Assigned,
        "active" => TaskLifecycleState::Active,
        "blocked" => TaskLifecycleState::Blocked,
        "closed" => TaskLifecycleState::Closed(TaskOutcome::Failed),
        _ => {
            return Err(task_rejected(
                "task supersession has an invalid current state",
            ));
        }
    };
    let TaskLifecycleTransition::To(TaskLifecycleState::Closed(_)) = lifecycle_transition(
        Some(&current),
        &TaskLifecycleAction::Supersede {
            successor_task_id: successor_task_id.clone(),
        },
    )
    .map_err(|error| task_error(AtmErrorCode::TaskTransitionInvalid, error.detail))?
    else {
        return Err(task_rejected("task supersession must close the old task"));
    };
    let successor_exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks_v2 WHERE team = ?1 AND task_id = ?2)",
            params![request.actor.team().as_str(), successor_task_id.as_str()],
            |row| row.get(0),
        )
        .map_err(|error| sqlite_error(target, "failed to check successor task", error))?;
    if successor_exists {
        return Err(task_rejected("successor task id already exists"));
    }
    persist_prepared_message(handoff, connection, cache, target)?;
    persist_prepared_message(&successor.message, connection, cache, target)?;
    let next_revision = revision.saturating_add(1);
    connection
        .execute(
            "UPDATE tasks_v2
             SET state = 'closed', outcome = 'aborted', abort_reason = 'superseded',
                 superseded_by = ?3, revision = ?4, updated_at = ?5
             WHERE team = ?1 AND task_id = ?2",
            params![
                request.actor.team().as_str(),
                request.task_id.as_str(),
                successor_task_id.as_str(),
                next_revision,
                at
            ],
        )
        .map_err(|error| sqlite_error(target, "failed to close superseded task", error))?;
    connection
        .execute(
            "INSERT INTO tasks_v2(team, task_id, current_assignee, state, outcome, abort_reason, superseded_by, priority, original_assigned_at, current_attempt, reminder_ordinal, revision, updated_at)
             VALUES (?1, ?2, ?3, 'assigned', NULL, NULL, NULL, ?4, ?5, 1, 0, 1, ?5)",
            params![request.actor.team().as_str(), successor_task_id.as_str(), successor.assignee.agent().as_str(), priority_name(successor.priority), at],
        )
        .map_err(|error| sqlite_error(target, "failed to create successor task", error))?;
    connection
        .execute(
            "INSERT INTO task_assignment_attempts(team, task_id, attempt, assignee, assigner, assignment_message_id, template_sha, assigned_at)
             VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6, ?7)",
            params![request.actor.team().as_str(), successor_task_id.as_str(), successor.assignee.agent().as_str(), successor.assigner.agent().as_str(), message_id, successor.template_sha.as_ref().map(ToString::to_string), at],
        )
        .map_err(|error| sqlite_error(target, "failed to create successor assignment attempt", error))?;
    append_v2_event_for(
        connection,
        target,
        request.actor.team(),
        successor_task_id,
        &request.operation_id,
        successor.assignee.agent(),
        "assigned",
        None,
        None,
        at,
    )?;
    clear_task_nudge_markers(connection, target, request)?;
    Ok((
        TaskLifecycleState::Closed(TaskOutcome::Aborted(
            atm_storage::TaskAbortReason::Superseded {
                successor_task_id: successor_task_id.clone(),
            },
        )),
        next_revision,
        "superseded",
        None,
        Some(successor_task_id.clone()),
    ))
}

fn assignment_message_id_for(
    task_id: &TaskId,
    team: &TeamName,
    assignment: &atm_storage::PreparedAssignment,
) -> Result<String, AtmError> {
    if assignment.assignee.team() != team
        || assignment.assigner.team() != team
        || assignment.message.message.team != *team
        || assignment.message.message.agent != *assignment.assignee.agent()
        || assignment.message.message.envelope.task_id.as_ref() != Some(task_id)
    {
        return Err(task_rejected(
            "successor assignment must target the successor task in the same team",
        ));
    }
    assignment
        .message
        .message
        .envelope
        .message_id
        .map(|id| id.to_string())
        .ok_or_else(|| task_rejected("successor assignment message requires a message id"))
}

fn clear_task_nudge_markers(
    connection: &Connection,
    target: &SharedDbTarget,
    request: &TaskMutationRequest,
) -> Result<(), AtmError> {
    connection
        .execute(
            "UPDATE mail_message_states SET nudge_pending_at = NULL, updated_at = ?3
             WHERE team = ?1
               AND message_key IN (
                   SELECT message_key FROM mail_messages
                   WHERE team = ?1 AND json_extract(envelope_json, '$.taskId') = ?2
               )",
            params![
                request.actor.team().as_str(),
                request.task_id.as_str(),
                atm_storage::IsoTimestamp::now().to_string()
            ],
        )
        .map_err(|error| {
            sqlite_error(target, "failed to clear task-linked nudge markers", error)
        })?;
    Ok(())
}

fn append_v2_event(
    connection: &Connection,
    target: &SharedDbTarget,
    request: &TaskMutationRequest,
    event: &str,
    detail: Option<String>,
    related_task_id: Option<TaskId>,
    at: &str,
) -> Result<(), AtmError> {
    append_v2_event_for(
        connection,
        target,
        request.actor.team(),
        &request.task_id,
        &request.operation_id,
        request.actor.agent(),
        event,
        detail.as_deref(),
        related_task_id.as_ref(),
        at,
    )
}

#[allow(clippy::too_many_arguments)]
fn append_v2_event_for(
    connection: &Connection,
    target: &SharedDbTarget,
    team: &TeamName,
    task_id: &TaskId,
    operation_id: &atm_storage::TaskOperationId,
    actor: &AgentName,
    event: &str,
    detail: Option<&str>,
    related_task_id: Option<&TaskId>,
    at: &str,
) -> Result<(), AtmError> {
    let seq: u64 = connection
        .query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM task_events_v2 WHERE team = ?1 AND task_id = ?2",
            params![team.as_str(), task_id.as_str()],
            |row| row.get(0),
        )
        .map_err(|error| {
            sqlite_error(target, "failed to allocate v2 task event sequence", error)
        })?;
    connection
        .execute(
            "INSERT INTO task_events_v2(team, task_id, seq, operation_id, attempt, at, actor, event, outcome, related_task_id, detail)
             SELECT ?1, ?2, ?3, ?4, current_attempt, ?5, ?6, ?7, outcome, ?8, ?9
             FROM tasks_v2 WHERE team = ?1 AND task_id = ?2",
            params![team.as_str(), task_id.as_str(), seq, operation_id.to_string(), at, actor.as_str(), event, related_task_id.map(TaskId::as_str), detail],
        )
        .map_err(|error| sqlite_error(target, "failed to append v2 task event", error))?;
    Ok(())
}

const fn priority_name(value: atm_storage::TaskPriority) -> &'static str {
    match value {
        atm_storage::TaskPriority::High => "high",
        atm_storage::TaskPriority::Normal => "normal",
        atm_storage::TaskPriority::Low => "low",
    }
}

const fn outcome_name(value: &TaskOutcome) -> &'static str {
    match value {
        TaskOutcome::Succeeded => "succeeded",
        TaskOutcome::Failed => "failed",
        TaskOutcome::Aborted(_) => "aborted",
    }
}

fn task_rejected(detail: impl std::fmt::Display) -> AtmError {
    task_error(AtmErrorCode::TaskTransitionInvalid, detail)
}

fn task_error(code: AtmErrorCode, detail: impl std::fmt::Display) -> AtmError {
    AtmError::new(code, format!("{detail}; {TASK_RECOVERY}"))
}

fn load_task_row(
    connection: &Connection,
    target: &SharedDbTarget,
    team: &TeamName,
    task_id: &TaskId,
    assignee: &AgentName,
) -> Result<Option<TaskRow>, AtmError> {
    task_sql::select_task_row(connection, team, task_id, assignee)
        .map_err(|error| sqlite_error(target, "failed to load task row", error))
}

fn load_open_task_rows(
    connection: &Connection,
    target: &SharedDbTarget,
    team: &TeamName,
    assignee: &AgentName,
) -> Result<Vec<TaskRow>, AtmError> {
    task_sql::select_open_tasks_for_member(connection, team, assignee)
        .map_err(|error| sqlite_error(target, "failed to load open tasks", error))
}

fn transition_for(
    row: Option<&TaskRow>,
    open: &[TaskRow],
    event: TaskEvent,
    task_id: &TaskId,
    actor: &AgentName,
) -> Result<Transition, AtmError> {
    admit(row, open, event, task_id, actor).map_err(|error| error.into_atm_error())?;
    transition(row.map(|task| task.state), event, task_id, actor)
        .map_err(|error| error.into_atm_error())
}

/// Writes the durable rejection audit after the failed operation's savepoint
/// has rolled back its tentative message/reply mutations. The outer writer
/// transaction remains open, so the audit row commits atomically with no
/// rejected message becoming visible to callers.
pub(super) fn append_rejected_task_event(
    op: &WriteOp,
    connection: &Connection,
    target: &SharedDbTarget,
    error: &AtmError,
) -> Result<(), AtmError> {
    if error.code() != AtmErrorCode::MessageValidationFailed {
        return Ok(());
    }
    let (team, task_id, assignee, actor, message_id) = match op {
        WriteOp::UpsertMessage { record, provenance }
            if *provenance == MessageWriteOrigin::Local =>
        {
            let Some(task_id) = record
                .envelope
                .task_id
                .as_ref()
                .or(record.envelope.task_complete.as_ref())
            else {
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
        _ => return Ok(()),
    };
    let row = load_task_row(connection, target, &team, &task_id, &assignee)?;
    let at = atm_storage::types::IsoTimestamp::now().to_string();
    append_task_event(
        connection,
        target,
        team.as_str(),
        task_id.as_str(),
        assignee.as_str(),
        &at,
        "rejected",
        row.as_ref().map(|row| state_name(row.state)),
        row.as_ref().map(|row| state_name(row.state)),
        actor.as_str(),
        message_id,
        None,
        None,
        Some(error.message()),
    )
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
        &typed_task_id,
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
    let next = transition_for(
        row.as_ref(),
        &[],
        TaskEvent::Completed,
        &typed_task_id,
        &record.envelope.from,
    )?;
    let row =
        row.ok_or_else(|| task_rejected("task completion admission found no existing task row"))?;
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
    let next = transition_for(Some(&row), &open, TaskEvent::Acked, task_id, actor)?;
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
