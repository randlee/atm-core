//! Same-assignee assignment refresh without a task-state event.

use atm_storage::{AtmError, AtmMessageId, Message, QueuePosition, TaskId, TaskRow};

use super::task_ops::params;
use crate::shared_db::{SharedDbTarget, SqliteConnection, sqlite_error};

#[allow(clippy::too_many_arguments)]
pub(super) fn refresh_same_assignment(
    record: &Message,
    task_id: &TaskId,
    row: Option<&TaskRow>,
    message_id: AtmMessageId,
    connection: &SqliteConnection,
    target: &SharedDbTarget,
) -> Result<Option<u32>, AtmError> {
    let Some(row) = row.filter(|row| row.state.is_open() && row.assignee == record.agent) else {
        return Ok(None);
    };
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
    Ok(Some(row.position.map_or(1, QueuePosition::get)))
}
