//! Bounded backend-owned reader lane for task-ledger projections.
//!
//! The task ledger shares the mailbox reader pool because both capabilities
//! are read-only projections over the same SQLite target. Task state changes
//! remain owned by the ordered writer transaction in `task_store`.

use atm_storage::{
    AgentName, AssignmentAttempt, AsyncTaskLedgerReader, AtmError, LogicalTaskRow, ReadDeadline,
    ReadLaneError, TaskAbortReason, TaskAssignmentAttempt, TaskEventRow, TaskId,
    TaskLifecycleEventKind, TaskLifecycleEventRow, TaskLifecycleState, TaskOperationId,
    TaskOutcome, TaskPriority, TaskRow, TeamName,
};
use rusqlite::{Connection, OptionalExtension, Row, params};
use std::sync::Arc;

use crate::SqliteTaskStore;
use crate::reader_pool::ReaderPool;
use crate::shared_db::{SharedDbTarget, sqlite_error};
use crate::task_sql;

struct TaskLedgerReader {
    pool: ReaderPool,
}

impl TaskLedgerReader {
    fn from_pool(pool: ReaderPool) -> Self {
        Self { pool }
    }
}

pub(crate) fn start_task_ledger_reader_from_pool(
    pool: ReaderPool,
) -> Arc<dyn AsyncTaskLedgerReader + Send + Sync> {
    Arc::new(TaskLedgerReader::from_pool(pool))
}

impl atm_storage::contract::sealed::Sealed for TaskLedgerReader {}

impl std::fmt::Debug for TaskLedgerReader {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TaskLedgerReader")
            .finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl AsyncTaskLedgerReader for TaskLedgerReader {
    async fn list_tasks(
        &self,
        team: TeamName,
        member: Option<AgentName>,
        deadline: ReadDeadline,
    ) -> Result<Vec<TaskRow>, ReadLaneError> {
        self.pool
            .submit(deadline.remaining(), move |connection, target| {
                list_tasks(connection, target, &team, member.as_ref()).map_err(read_lane_error)
            })
            .await
    }

    async fn list_task_events(
        &self,
        team: TeamName,
        task_id: TaskId,
        member: Option<AgentName>,
        deadline: ReadDeadline,
    ) -> Result<Vec<TaskEventRow>, ReadLaneError> {
        self.pool
            .submit(deadline.remaining(), move |connection, target| {
                list_task_events(connection, target, &team, &task_id, member.as_ref())
                    .map_err(read_lane_error)
            })
            .await
    }

    async fn list_logical_tasks(
        &self,
        team: TeamName,
        member: Option<AgentName>,
        limit: Option<usize>,
        deadline: ReadDeadline,
    ) -> Result<Vec<LogicalTaskRow>, ReadLaneError> {
        self.pool
            .submit(deadline.remaining(), move |connection, target| {
                list_logical_tasks(connection, target, &team, member.as_ref(), limit)
                    .map_err(read_lane_error)
            })
            .await
    }

    async fn top_runnable_task(
        &self,
        team: TeamName,
        member: AgentName,
        deadline: ReadDeadline,
    ) -> Result<Option<LogicalTaskRow>, ReadLaneError> {
        self.pool
            .submit(deadline.remaining(), move |connection, target| {
                top_runnable_task(connection, target, &team, &member).map_err(read_lane_error)
            })
            .await
    }

    async fn list_task_assignment_attempts(
        &self,
        team: TeamName,
        task_id: TaskId,
        deadline: ReadDeadline,
    ) -> Result<Vec<TaskAssignmentAttempt>, ReadLaneError> {
        self.pool
            .submit(deadline.remaining(), move |connection, target| {
                list_assignment_attempts(connection, target, &team, &task_id)
                    .map_err(read_lane_error)
            })
            .await
    }

    async fn list_task_lifecycle_events(
        &self,
        team: TeamName,
        task_id: TaskId,
        limit: Option<usize>,
        deadline: ReadDeadline,
    ) -> Result<Vec<TaskLifecycleEventRow>, ReadLaneError> {
        self.pool
            .submit(deadline.remaining(), move |connection, target| {
                list_lifecycle_events(connection, target, &team, &task_id, limit)
                    .map_err(read_lane_error)
            })
            .await
    }
}

fn list_tasks(
    connection: &Connection,
    target: &SharedDbTarget,
    team: &TeamName,
    member: Option<&AgentName>,
) -> Result<Vec<TaskRow>, AtmError> {
    let mut statement = connection
        .prepare(&task_sql::select_tasks_for_team_sql())
        .map_err(|error| sqlite_error(target, "failed to prepare async task list", error))?;
    statement
        .query_map(
            params![team.as_str(), member.map(AgentName::as_str)],
            SqliteTaskStore::decode_row,
        )
        .map_err(|error| sqlite_error(target, "failed to list async tasks", error))?
        .map(|row| {
            row.map_err(|error| sqlite_error(target, "failed to decode async task row", error))
        })
        .collect()
}

fn list_task_events(
    connection: &Connection,
    target: &SharedDbTarget,
    team: &TeamName,
    task_id: &TaskId,
    member: Option<&AgentName>,
) -> Result<Vec<TaskEventRow>, AtmError> {
    let mut statement = connection
        .prepare(&task_sql::select_task_events_sql())
        .map_err(|error| sqlite_error(target, "failed to prepare async task event list", error))?;
    statement
        .query_map(
            params![
                team.as_str(),
                task_id.as_str(),
                member.map(AgentName::as_str)
            ],
            SqliteTaskStore::decode_event_row,
        )
        .map_err(|error| sqlite_error(target, "failed to list async task events", error))?
        .map(|row| {
            row.map_err(|error| sqlite_error(target, "failed to decode async task event", error))
        })
        .collect()
}

fn list_logical_tasks(
    connection: &Connection,
    target: &SharedDbTarget,
    team: &TeamName,
    member: Option<&AgentName>,
    limit: Option<usize>,
) -> Result<Vec<LogicalTaskRow>, AtmError> {
    let mut statement = connection
        .prepare(
            "SELECT team, task_id, current_assignee, state, outcome, abort_reason, superseded_by,
                    priority, original_assigned_at, current_attempt, reminder_ordinal, revision, updated_at
             FROM tasks_v2
             WHERE team = ?1 AND (?2 IS NULL OR current_assignee = ?2)
             ORDER BY CASE state WHEN 'active' THEN 0 WHEN 'assigned' THEN 1 WHEN 'blocked' THEN 2 ELSE 3 END,
                      CASE WHEN state = 'assigned' THEN CASE priority WHEN 'high' THEN 0 WHEN 'normal' THEN 1 ELSE 2 END ELSE 0 END,
                      CASE WHEN state = 'closed' THEN updated_at END DESC,
                      original_assigned_at ASC, task_id ASC
             LIMIT ?3",
        )
        .map_err(|error| sqlite_error(target, "failed to prepare logical task list", error))?;
    statement
        .query_map(
            params![
                team.as_str(),
                member.map(AgentName::as_str),
                limit.map_or(i64::MAX, |value| value as i64),
            ],
            decode_logical_task_row,
        )
        .map_err(|error| sqlite_error(target, "failed to list logical tasks", error))?
        .map(|row| {
            row.map_err(|error| sqlite_error(target, "failed to decode logical task", error))
        })
        .collect()
}

fn top_runnable_task(
    connection: &Connection,
    target: &SharedDbTarget,
    team: &TeamName,
    member: &AgentName,
) -> Result<Option<LogicalTaskRow>, AtmError> {
    connection
        .query_row(
            "SELECT team, task_id, current_assignee, state, outcome, abort_reason, superseded_by,
                    priority, original_assigned_at, current_attempt, reminder_ordinal, revision, updated_at
             FROM tasks_v2
             WHERE team = ?1 AND current_assignee = ?2 AND state IN ('active', 'assigned')
             ORDER BY CASE state WHEN 'active' THEN 0 ELSE 1 END,
                      CASE priority WHEN 'high' THEN 0 WHEN 'normal' THEN 1 ELSE 2 END,
                      original_assigned_at ASC, task_id ASC
             LIMIT 1",
            params![team.as_str(), member.as_str()],
            decode_logical_task_row,
        )
        .optional()
        .map_err(|error| sqlite_error(target, "failed to select top runnable task", error))
}

fn list_assignment_attempts(
    connection: &Connection,
    target: &SharedDbTarget,
    team: &TeamName,
    task_id: &TaskId,
) -> Result<Vec<TaskAssignmentAttempt>, AtmError> {
    let mut statement = connection
        .prepare(
            "SELECT team, task_id, attempt, assignee, assigner, assignment_message_id, template_sha, assigned_at
             FROM task_assignment_attempts WHERE team = ?1 AND task_id = ?2 ORDER BY attempt ASC",
        )
        .map_err(|error| sqlite_error(target, "failed to prepare assignment-attempt list", error))?;
    statement
        .query_map(
            params![team.as_str(), task_id.as_str()],
            decode_assignment_attempt,
        )
        .map_err(|error| sqlite_error(target, "failed to list assignment attempts", error))?
        .map(|row| {
            row.map_err(|error| sqlite_error(target, "failed to decode assignment attempt", error))
        })
        .collect()
}

fn list_lifecycle_events(
    connection: &Connection,
    target: &SharedDbTarget,
    team: &TeamName,
    task_id: &TaskId,
    limit: Option<usize>,
) -> Result<Vec<TaskLifecycleEventRow>, AtmError> {
    let mut statement = connection
        .prepare(
            "SELECT team, task_id, seq, operation_id, attempt, at, actor, event, outcome, related_task_id, detail
             FROM task_events_v2 WHERE team = ?1 AND task_id = ?2 ORDER BY seq ASC LIMIT ?3",
        )
        .map_err(|error| sqlite_error(target, "failed to prepare lifecycle event list", error))?;
    statement
        .query_map(
            params![
                team.as_str(),
                task_id.as_str(),
                limit.map_or(i64::MAX, |value| value as i64),
            ],
            decode_lifecycle_event,
        )
        .map_err(|error| sqlite_error(target, "failed to list lifecycle events", error))?
        .map(|row| {
            row.map_err(|error| sqlite_error(target, "failed to decode lifecycle event", error))
        })
        .collect()
}

fn decode_logical_task_row(row: &Row<'_>) -> rusqlite::Result<LogicalTaskRow> {
    let state: String = row.get(3)?;
    let outcome: Option<String> = row.get(4)?;
    let abort_reason: Option<String> = row.get(5)?;
    let superseded_by: Option<String> = row.get(6)?;
    Ok(LogicalTaskRow {
        team: parse_value(&row.get::<_, String>(0)?, "logical task team")?,
        task_id: parse_value(&row.get::<_, String>(1)?, "logical task id")?,
        current_assignee: parse_value(&row.get::<_, String>(2)?, "logical task assignee")?,
        state: parse_lifecycle_state(
            &state,
            outcome.as_deref(),
            abort_reason.as_deref(),
            superseded_by.as_deref(),
        )?,
        priority: parse_priority(&row.get::<_, String>(7)?)?,
        original_assigned_at: parse_value(
            &row.get::<_, String>(8)?,
            "logical task original assignment time",
        )?,
        current_attempt: AssignmentAttempt::new(row.get(9)?).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                9,
                rusqlite::types::Type::Integer,
                Box::new(error.into_atm_error()),
            )
        })?,
        reminder_ordinal: row.get(10)?,
        revision: row.get(11)?,
        updated_at: parse_value(&row.get::<_, String>(12)?, "logical task update time")?,
    })
}

fn decode_assignment_attempt(row: &Row<'_>) -> rusqlite::Result<TaskAssignmentAttempt> {
    Ok(TaskAssignmentAttempt {
        team: parse_value(&row.get::<_, String>(0)?, "assignment attempt team")?,
        task_id: parse_value(&row.get::<_, String>(1)?, "assignment attempt task id")?,
        attempt: AssignmentAttempt::new(row.get(2)?).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                2,
                rusqlite::types::Type::Integer,
                Box::new(error.into_atm_error()),
            )
        })?,
        assignee: parse_value(&row.get::<_, String>(3)?, "assignment attempt assignee")?,
        assigner: parse_value(&row.get::<_, String>(4)?, "assignment attempt assigner")?,
        assignment_message_id: parse_value(&row.get::<_, String>(5)?, "assignment message id")?,
        template_sha: row
            .get::<_, Option<String>>(6)?
            .map(|raw| parse_value(&raw, "assignment template sha"))
            .transpose()?,
        assigned_at: parse_value(&row.get::<_, String>(7)?, "assignment attempt timestamp")?,
    })
}

fn decode_lifecycle_event(row: &Row<'_>) -> rusqlite::Result<TaskLifecycleEventRow> {
    let related_task_id = row.get::<_, Option<String>>(9)?;
    let outcome = row.get::<_, Option<String>>(8)?;
    Ok(TaskLifecycleEventRow {
        team: parse_value(&row.get::<_, String>(0)?, "lifecycle event team")?,
        task_id: parse_value(&row.get::<_, String>(1)?, "lifecycle event task id")?,
        seq: row.get(2)?,
        operation_id: row
            .get::<_, Option<String>>(3)?
            .map(|raw| parse_task_operation_id(&raw))
            .transpose()?,
        attempt: row
            .get::<_, Option<u32>>(4)?
            .map(AssignmentAttempt::new)
            .transpose()
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    4,
                    rusqlite::types::Type::Integer,
                    Box::new(error.into_atm_error()),
                )
            })?,
        at: parse_value(&row.get::<_, String>(5)?, "lifecycle event timestamp")?,
        actor: parse_value(&row.get::<_, String>(6)?, "lifecycle event actor")?,
        event: parse_event_kind(&row.get::<_, String>(7)?)?,
        outcome: outcome
            .map(|raw| parse_event_outcome(&raw, related_task_id.as_deref()))
            .transpose()?,
        related_task_id: related_task_id
            .map(|raw| parse_value(&raw, "related task id"))
            .transpose()?,
        detail: row.get(10)?,
    })
}

fn parse_lifecycle_state(
    state: &str,
    outcome: Option<&str>,
    abort_reason: Option<&str>,
    superseded_by: Option<&str>,
) -> rusqlite::Result<TaskLifecycleState> {
    match state {
        "assigned" => Ok(TaskLifecycleState::Assigned),
        "active" => Ok(TaskLifecycleState::Active),
        "blocked" => Ok(TaskLifecycleState::Blocked),
        "closed" => Ok(TaskLifecycleState::Closed(parse_outcome(
            outcome,
            abort_reason,
            superseded_by,
        )?)),
        _ => invalid_value("logical task state"),
    }
}

fn parse_outcome(
    outcome: Option<&str>,
    abort_reason: Option<&str>,
    superseded_by: Option<&str>,
) -> rusqlite::Result<TaskOutcome> {
    match outcome {
        Some("succeeded") => Ok(TaskOutcome::Succeeded),
        Some("failed") => Ok(TaskOutcome::Failed),
        Some("aborted") => match abort_reason {
            Some("superseded") => Ok(TaskOutcome::Aborted(TaskAbortReason::Superseded {
                successor_task_id: parse_value(
                    superseded_by.ok_or_else(|| rusqlite::Error::InvalidQuery)?,
                    "successor task id",
                )?,
            })),
            Some("cancelled") | None => Ok(TaskOutcome::Aborted(TaskAbortReason::Cancelled)),
            Some(_) => invalid_value("task abort reason"),
        },
        _ => invalid_value("task terminal outcome"),
    }
}

fn parse_priority(value: &str) -> rusqlite::Result<TaskPriority> {
    match value {
        "high" => Ok(TaskPriority::High),
        "normal" => Ok(TaskPriority::Normal),
        "low" => Ok(TaskPriority::Low),
        _ => invalid_value("task priority"),
    }
}

fn parse_event_kind(value: &str) -> rusqlite::Result<TaskLifecycleEventKind> {
    match value {
        "assigned" => Ok(TaskLifecycleEventKind::Assigned),
        "started" => Ok(TaskLifecycleEventKind::Started),
        "blocked" => Ok(TaskLifecycleEventKind::Blocked),
        "unblocked" => Ok(TaskLifecycleEventKind::Unblocked),
        "reassigned" => Ok(TaskLifecycleEventKind::Reassigned),
        "reopened" => Ok(TaskLifecycleEventKind::Reopened),
        "closed" => Ok(TaskLifecycleEventKind::Closed),
        "superseded" => Ok(TaskLifecycleEventKind::Superseded),
        "rejected" => Ok(TaskLifecycleEventKind::Rejected),
        "reminded" => Ok(TaskLifecycleEventKind::Reminded),
        "lead_notified" => Ok(TaskLifecycleEventKind::LeadNotified),
        "migrated" => Ok(TaskLifecycleEventKind::Migrated),
        "migrated_active_conflict_demotion" => {
            Ok(TaskLifecycleEventKind::MigratedActiveConflictDemotion)
        }
        "legacy_close_succeeded" => Ok(TaskLifecycleEventKind::LegacyCloseSucceeded),
        "legacy_v1_updated" => Ok(TaskLifecycleEventKind::LegacyV1Updated),
        _ => invalid_value("lifecycle event kind"),
    }
}

fn parse_event_outcome(
    value: &str,
    related_task_id: Option<&str>,
) -> rusqlite::Result<TaskOutcome> {
    parse_outcome(
        Some(value),
        if value == "aborted" && related_task_id.is_some() {
            Some("superseded")
        } else if value == "aborted" {
            Some("cancelled")
        } else {
            None
        },
        related_task_id,
    )
}

fn parse_task_operation_id(value: &str) -> rusqlite::Result<TaskOperationId> {
    TaskOperationId::parse(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            3,
            rusqlite::types::Type::Text,
            Box::new(error.into_atm_error()),
        )
    })
}

fn parse_value<T>(value: &str, _label: &str) -> rusqlite::Result<T>
where
    T: std::str::FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    value.parse().map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
    })
}

fn invalid_value<T>(label: &str) -> rusqlite::Result<T> {
    Err(rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        format!("invalid {label}").into(),
    ))
}

fn read_lane_error(error: AtmError) -> ReadLaneError {
    ReadLaneError::Unavailable {
        message: error.message().to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use crate::SqliteStorageBackend;
    use atm_storage::{ReadDeadline, TaskId, TeamName};
    use std::time::Duration;

    #[tokio::test]
    async fn sqlite_task_ledger_reader_uses_the_bounded_reader_pool() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let reader = backend.async_task_ledger_reader();
        let team: TeamName = "reader-test-team".parse().expect("team");
        let deadline = || ReadDeadline::new(Duration::from_secs(1)).expect("deadline");

        assert!(
            reader
                .list_tasks(team.clone(), None, deadline())
                .await
                .expect("task list")
                .is_empty()
        );
        assert!(
            reader
                .list_task_events(
                    team,
                    "reader-test-task".parse::<TaskId>().expect("task id"),
                    None,
                    deadline(),
                )
                .await
                .expect("task event list")
                .is_empty()
        );
    }
}
