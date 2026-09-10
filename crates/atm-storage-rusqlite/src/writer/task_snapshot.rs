//! Current-projection snapshots persisted with idempotent task operations.

use atm_storage::{AgentName, AssignmentAttempt, AtmError, AtmErrorCode, TaskMutationRequest};
use rusqlite::{Connection, params};

use crate::shared_db::{SharedDbTarget, sqlite_error};

pub(super) fn current_projection_snapshot(
    request: &TaskMutationRequest,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(AgentName, AssignmentAttempt), AtmError> {
    let (assignee, attempt): (String, u32) = connection
        .query_row(
            "SELECT current_assignee, current_attempt FROM tasks_v2 WHERE team = ?1 AND task_id = ?2",
            params![request.actor.team().as_str(), request.task_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| sqlite_error(target, "failed to load committed task projection", error))?;
    let assignee = assignee.parse().map_err(|error| {
        AtmError::new(
            AtmErrorCode::SerializationFailed,
            "committed task projection has an invalid assignee",
        )
        .with_cause(error)
    })?;
    let attempt = AssignmentAttempt::new(attempt).map_err(|error| {
        AtmError::new(
            AtmErrorCode::SerializationFailed,
            "committed task projection has an invalid assignment attempt",
        )
        .with_cause(error.into_atm_error())
    })?;
    Ok((assignee, attempt))
}
