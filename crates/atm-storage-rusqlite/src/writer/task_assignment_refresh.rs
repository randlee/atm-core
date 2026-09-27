//! Same-assignee assignment refresh: an update, not a new assignment.
//!
//! Rand's ruling (2026-09-27, #1619): re-assigning an open task to the agent
//! that already holds it follows the normal assignment path. It appends one
//! `updated` task event carrying the same actor and message fields the
//! `assigned`/`reassigned` events carry, and it resets the reminder budget
//! exactly as `assign`/`reassign`/`reopen` do, so a stalled task is eligible
//! for reminders again. State and queue position are left unchanged: this is
//! not a reassignment, so the queue is not renumbered and `assigned_at` is
//! not reset.

use atm_storage::{
    AtmError, AtmMessageId, IsoTimestamp, Message, QueuePosition, TaskEventKind, TaskId, TaskRow,
};

use super::task_ops::{append_task_event, params};
use crate::shared_db::{SharedDbTarget, SqliteConnection, sqlite_error};

#[allow(clippy::too_many_arguments)]
pub(super) fn refresh_same_assignment(
    record: &Message,
    task_id: &TaskId,
    row: Option<&TaskRow>,
    message_id: AtmMessageId,
    at: IsoTimestamp,
    connection: &SqliteConnection,
    target: &SharedDbTarget,
) -> Result<Option<u32>, AtmError> {
    let Some(row) = row.filter(|row| row.state.is_open() && row.assignee == record.agent) else {
        return Ok(None);
    };
    connection
        .execute(
            "UPDATE tasks SET assignment_message_id=?3, description=?4, updated_at=?5,
             last_reminded_at=NULL, reminder_count=0, lead_notified_count=0
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
    append_task_event(
        connection,
        target,
        &record.team,
        task_id,
        &record.agent,
        &at,
        TaskEventKind::Updated,
        Some(row.state.tag()),
        Some(row.state.tag()),
        None,
        &record.envelope.from,
        Some(message_id),
        None,
        None,
        None,
    )?;
    Ok(Some(row.position.map_or(1, QueuePosition::get)))
}
