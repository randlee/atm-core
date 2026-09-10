use crate::shared_db::{SharedDbTarget, sqlite_error};
use atm_storage::error::AtmError;
use atm_storage::types::{TaskId, TeamName};
use rusqlite::{Connection, params};

pub(super) fn sync_v1_compat_projection(
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
                "UPDATE tasks SET state = 'complete', updated_at = (
                     SELECT updated_at FROM tasks_v2 WHERE team = ?1 AND task_id = ?2
                 ) WHERE team = ?1 AND task_id = ?2",
                params![team.as_str(), task_id.as_str()],
            )
            .map_err(|error| sqlite_error(target, "failed to retire stale v1 task rows", error))?;
        connection
            .execute(
                "INSERT INTO tasks(team, task_id, assignee, assigner, state, assignment_message_id,
                     description, assigned_at, updated_at, reminder_count, lead_notified_count)
                 SELECT task.team, task.task_id, task.current_assignee, attempt.assigner,
                        CASE task.state WHEN 'active' THEN 'active' WHEN 'closed' THEN 'complete' ELSE 'assigned' END,
                        attempt.assignment_message_id,
                        COALESCE((SELECT message_text FROM mail_messages
                                  WHERE team = task.team AND agent = task.current_assignee
                                    AND message_id = attempt.assignment_message_id),
                                 '[canonical task assignment; see assignment message]'),
                        task.original_assigned_at, task.updated_at, task.reminder_ordinal, 0
                   FROM tasks_v2 AS task JOIN task_assignment_attempts AS attempt
                     ON attempt.team = task.team AND attempt.task_id = task.task_id
                    AND attempt.attempt = task.current_attempt
                  WHERE task.team = ?1 AND task.task_id = ?2
                 ON CONFLICT(team, task_id, assignee) DO UPDATE SET
                     assigner = excluded.assigner, state = excluded.state,
                     assignment_message_id = excluded.assignment_message_id,
                     description = excluded.description, updated_at = excluded.updated_at,
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
