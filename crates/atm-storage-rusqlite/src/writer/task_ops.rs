//! The rusqlite writer's sole task-ledger application site (AX.3, C6).
//!
//! `tasks.state` and `task_events` are mutated only here, inside the writer's
//! transaction connection, so a message insert or acknowledgement, its task
//! row, and its audit event either commit together or roll back together.
//! `atm_storage::task_state` defines the pure, backend-neutral transition
//! table this module's SQL mirrors; nothing here changes that table's rules.

use super::ops::execute_task_mutation_message_upsert;
use super::stmt_cache::WriterStatementCache;
use crate::shared_db::{SharedDbTarget, sqlite_error};
use atm_storage::error::AtmError;
use atm_storage::types::{AgentName, TaskId, TeamName};
use atm_storage::{AtmErrorCode, MessageWriteOrigin};
use atm_storage::{
    TaskLifecycleAction, TaskLifecycleState, TaskLifecycleTransition, TaskMutationOutcome,
    TaskMutationRequest, TaskOperation, TaskOutcome, lifecycle_transition,
};
use rusqlite::{Connection, OptionalExtension, params};

const TASK_RECOVERY: &str = "Run: atm list --task-events <task_id> --member <assignee>";

struct TransitionSpec {
    next_state: &'static str,
    terminal: Option<TaskOutcome>,
    event: &'static str,
    detail: Option<String>,
    related_task_id: Option<TaskId>,
}

impl TransitionSpec {
    fn open(next_state: &'static str, event: &'static str) -> Self {
        Self {
            next_state,
            terminal: None,
            event,
            detail: None,
            related_task_id: None,
        }
    }

    fn closed(terminal: TaskOutcome, event: &'static str) -> Self {
        Self {
            next_state: "closed",
            terminal: Some(terminal),
            event,
            detail: None,
            related_task_id: None,
        }
    }

    fn with_detail(mut self, detail: String) -> Self {
        self.detail = Some(detail);
        self
    }
}

struct TransitionResult {
    state: TaskLifecycleState,
    revision: u64,
    event: &'static str,
    detail: Option<String>,
    related_task_id: Option<TaskId>,
}

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
    let transition = match &request.operation {
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
            TransitionResult {
                state: TaskLifecycleState::Assigned,
                revision: 1,
                event: "assigned",
                detail: None,
                related_task_id: None,
            }
        }
        TaskOperation::Start => transition_v2(
            connection,
            target,
            request,
            &TransitionSpec::open("active", "started"),
            &now,
        )?,
        TaskOperation::Block { reason } => {
            let result = transition_v2(
                connection,
                target,
                request,
                &TransitionSpec::open("blocked", "blocked").with_detail(reason.clone()),
                &now,
            )?;
            clear_task_nudge_markers(connection, target, request)?;
            result
        }
        TaskOperation::Unblock { resolution } => transition_v2(
            connection,
            target,
            request,
            &TransitionSpec::open("assigned", "unblocked").with_detail(resolution.clone()),
            &now,
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
                &TransitionSpec::closed(outcome.clone(), "closed"),
                &now,
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
                &TransitionSpec::closed(TaskOutcome::Succeeded, "legacy_close_succeeded"),
                &now,
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
        state: transition.state.clone(),
        revision: transition.revision,
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
        transition.event,
        transition.detail.clone(),
        transition.related_task_id.clone(),
        &now,
    )?;
    sync_v1_compat_projection(connection, target, request.actor.team(), &request.task_id)?;
    if let Some(successor_task_id) = transition.related_task_id.as_ref() {
        sync_v1_compat_projection(connection, target, request.actor.team(), successor_task_id)?;
    }
    Ok(result)
}

/// Refreshes the retained 1.6.x row shape from a canonical v2 task after a
/// committed lifecycle decision. The context marker suppresses the reciprocal
/// v1 bridge triggers for just these projection writes; all statements run in
/// the caller's writer transaction and therefore cannot leave the two
/// projections partially reconciled.
fn sync_v1_compat_projection(
    connection: &Connection,
    target: &SharedDbTarget,
    team: &TeamName,
    task_id: &TaskId,
) -> Result<(), AtmError> {
    connection
        .execute(
            "INSERT INTO task_v2_projection_context(singleton) VALUES (1)",
            [],
        )
        .map_err(|error| sqlite_error(target, "failed to begin v1 task projection", error))?;

    let result = (|| {
        connection
            .execute(
                "UPDATE tasks
                 SET state = 'complete', updated_at = (
                         SELECT updated_at FROM tasks_v2 WHERE team = ?1 AND task_id = ?2
                     )
                 WHERE team = ?1 AND task_id = ?2",
                params![team.as_str(), task_id.as_str()],
            )
            .map_err(|error| sqlite_error(target, "failed to retire stale v1 task rows", error))?;
        connection
            .execute(
                "INSERT INTO tasks(
                     team, task_id, assignee, assigner, state, assignment_message_id,
                     description, assigned_at, updated_at, reminder_count, lead_notified_count
                 )
                 SELECT task.team, task.task_id, task.current_assignee, attempt.assigner,
                        CASE task.state
                            WHEN 'active' THEN 'active'
                            WHEN 'closed' THEN 'complete'
                            ELSE 'assigned'
                        END,
                        attempt.assignment_message_id,
                        COALESCE((
                            SELECT message_text FROM mail_messages
                             WHERE team = task.team
                               AND agent = task.current_assignee
                               AND message_id = attempt.assignment_message_id
                        ), '[canonical task assignment; see assignment message]'),
                        task.original_assigned_at, task.updated_at, task.reminder_ordinal, 0
                   FROM tasks_v2 AS task
                   JOIN task_assignment_attempts AS attempt
                     ON attempt.team = task.team
                    AND attempt.task_id = task.task_id
                    AND attempt.attempt = task.current_attempt
                  WHERE task.team = ?1 AND task.task_id = ?2
                 ON CONFLICT(team, task_id, assignee) DO UPDATE SET
                     assigner = excluded.assigner,
                     state = excluded.state,
                     assignment_message_id = excluded.assignment_message_id,
                     description = excluded.description,
                     updated_at = excluded.updated_at,
                     reminder_count = excluded.reminder_count",
                params![team.as_str(), task_id.as_str()],
            )
            .map_err(|error| sqlite_error(target, "failed to refresh v1 task projection", error))?;
        Ok(())
    })();
    let cleanup = connection
        .execute(
            "DELETE FROM task_v2_projection_context WHERE singleton = 1",
            [],
        )
        .map(|_| ())
        .map_err(|error| sqlite_error(target, "failed to end v1 task projection", error));
    result.and(cleanup)
}

fn transition_v2(
    connection: &Connection,
    target: &SharedDbTarget,
    request: &TaskMutationRequest,
    spec: &TransitionSpec,
    at: &str,
) -> Result<TransitionResult, AtmError> {
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
    let action = match (spec.next_state, spec.terminal.as_ref(), spec.event) {
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
    if spec.next_state == "active" {
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
    let outcome_name = spec.terminal.as_ref().map(outcome_name);
    let abort_reason = spec.terminal.as_ref().map(abort_reason_name).transpose()?;
    connection
        .execute(
            "UPDATE tasks_v2 SET state = ?3, outcome = ?4, abort_reason = ?5,
             superseded_by = NULL, revision = ?6, updated_at = ?7
         WHERE team = ?1 AND task_id = ?2",
            params![
                request.actor.team().as_str(),
                request.task_id.as_str(),
                spec.next_state,
                outcome_name,
                abort_reason,
                next_revision,
                at
            ],
        )
        .map_err(|error| sqlite_error(target, "failed to transition v2 task", error))?;
    let state = expected;
    Ok(TransitionResult {
        state,
        revision: next_revision,
        event: spec.event,
        detail: spec.detail.clone(),
        related_task_id: spec.related_task_id.clone(),
    })
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
    if assignment.delivery_origin != MessageWriteOrigin::Local {
        return Err(task_error(
            AtmErrorCode::TaskHandoffCrossHostUnsupported,
            "peer-origin task assignment cannot join the local task transaction",
        ));
    }
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
) -> Result<TransitionResult, AtmError> {
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
    Ok(TransitionResult {
        state: TaskLifecycleState::Assigned,
        revision: next_revision,
        event: if reopen { "reopened" } else { "reassigned" },
        detail: None,
        related_task_id: None,
    })
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
) -> Result<TransitionResult, AtmError> {
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
    Ok(TransitionResult {
        state: TaskLifecycleState::Closed(TaskOutcome::Aborted(
            atm_storage::TaskAbortReason::Superseded {
                successor_task_id: successor_task_id.clone(),
            },
        )),
        revision: next_revision,
        event: "superseded",
        detail: None,
        related_task_id: Some(successor_task_id.clone()),
    })
}

fn assignment_message_id_for(
    task_id: &TaskId,
    team: &TeamName,
    assignment: &atm_storage::PreparedAssignment,
) -> Result<String, AtmError> {
    if assignment.delivery_origin != MessageWriteOrigin::Local {
        return Err(task_error(
            AtmErrorCode::TaskHandoffCrossHostUnsupported,
            "peer-origin successor assignment cannot join the local task transaction",
        ));
    }
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

fn abort_reason_name(value: &TaskOutcome) -> Result<Option<&'static str>, AtmError> {
    match value {
        TaskOutcome::Succeeded | TaskOutcome::Failed => Ok(None),
        TaskOutcome::Aborted(atm_storage::TaskAbortReason::Cancelled) => Ok(Some("cancelled")),
        TaskOutcome::Aborted(atm_storage::TaskAbortReason::Superseded { .. }) => Err(task_error(
            AtmErrorCode::TaskTerminalMetadataInvalid,
            "superseded outcomes require the atomic supersede operation",
        )),
    }
}

fn task_rejected(detail: impl std::fmt::Display) -> AtmError {
    task_error(AtmErrorCode::TaskTransitionInvalid, detail)
}

fn task_error(code: AtmErrorCode, detail: impl std::fmt::Display) -> AtmError {
    AtmError::new(code, format!("{detail}; {TASK_RECOVERY}"))
}
