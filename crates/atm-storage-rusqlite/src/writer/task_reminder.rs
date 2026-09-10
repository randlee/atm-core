//! Attempt-aware reminder persistence for the canonical task ledger.

use atm_storage::{
    AssignmentAttempt, AtmError, IsoTimestamp, TaskLifecycleState, TaskMutationRequest,
};
use rusqlite::{Connection, params};

use super::task_ops::{TransitionResult, task_rejected};
use crate::shared_db::{SharedDbTarget, sqlite_error};

pub(super) fn record_reminder(
    connection: &Connection,
    target: &SharedDbTarget,
    request: &TaskMutationRequest,
    attempt: AssignmentAttempt,
    at: &IsoTimestamp,
) -> Result<TransitionResult, AtmError> {
    let (state, revision, current_attempt): (String, u64, u32) = connection
        .query_row(
            "SELECT state, revision, current_attempt FROM tasks_v2 WHERE team = ?1 AND task_id = ?2",
            params![request.actor.team().as_str(), request.task_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|error| sqlite_error(target, "task reminder requires an existing task", error))?;
    if !matches!(state.as_str(), "assigned" | "active") || current_attempt != attempt.get() {
        return Err(task_rejected("task reminder attempt is no longer runnable"));
    }
    let next_revision = revision.saturating_add(1);
    connection
        .execute(
            "UPDATE tasks_v2 SET reminder_ordinal = reminder_ordinal + 1, revision = ?3, updated_at = ?4
             WHERE team = ?1 AND task_id = ?2 AND state IN ('assigned', 'active') AND current_attempt = ?5",
            params![
                request.actor.team().as_str(),
                request.task_id.as_str(),
                next_revision,
                at.to_string(),
                attempt.get(),
            ],
        )
        .map_err(|error| sqlite_error(target, "failed to record v2 task reminder", error))?;
    let state = match state.as_str() {
        "assigned" => TaskLifecycleState::Assigned,
        "active" => TaskLifecycleState::Active,
        _ => unreachable!("checked runnable task state"),
    };
    Ok(TransitionResult {
        state,
        revision: next_revision,
        event: "reminded",
        detail: None,
        related_task_id: None,
    })
}

pub(super) fn record_lead_notified(
    connection: &Connection,
    target: &SharedDbTarget,
    request: &TaskMutationRequest,
    attempt: AssignmentAttempt,
    at: &IsoTimestamp,
    lead: &atm_storage::AgentName,
    message_id: &atm_storage::AtmMessageId,
) -> Result<TransitionResult, AtmError> {
    let (state, revision, current_attempt): (String, u64, u32) = connection
        .query_row(
            "SELECT state, revision, current_attempt FROM tasks_v2 WHERE team = ?1 AND task_id = ?2",
            params![request.actor.team().as_str(), request.task_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|error| sqlite_error(target, "task lead audit requires an existing task", error))?;
    if !matches!(state.as_str(), "assigned" | "active") || current_attempt != attempt.get() {
        return Err(task_rejected(
            "task lead audit attempt is no longer runnable",
        ));
    }
    let next_revision = revision.saturating_add(1);
    connection
        .execute(
            "UPDATE tasks_v2 SET revision = ?3, updated_at = ?4
             WHERE team = ?1 AND task_id = ?2 AND state IN ('assigned', 'active') AND current_attempt = ?5",
            params![
                request.actor.team().as_str(),
                request.task_id.as_str(),
                next_revision,
                at.to_string(),
                attempt.get(),
            ],
        )
        .map_err(|error| sqlite_error(target, "failed to record v2 task lead audit", error))?;
    let state = match state.as_str() {
        "assigned" => TaskLifecycleState::Assigned,
        "active" => TaskLifecycleState::Active,
        _ => unreachable!("checked runnable task state"),
    };
    Ok(TransitionResult {
        state,
        revision: next_revision,
        event: "lead_notified",
        detail: Some(format!("lead={lead} message_id={message_id}")),
        related_task_id: None,
    })
}
