//! Bounded backend-owned reader lane for task-ledger projections.
//!
//! The task ledger shares the mailbox reader pool because both capabilities
//! are read-only projections over the same SQLite target. Task state changes
//! remain owned by the ordered writer transaction in `task_store`.

use atm_storage::{
    AgentName, AsyncTaskLedgerReader, AtmError, ReadDeadline, ReadLaneError, TaskEventRow, TaskId,
    TaskRow, TeamName,
};
use rusqlite::{Connection, params};

use crate::SqliteTaskStore;
use crate::reader_pool::ReaderPool;
use crate::shared_db::{SharedDbTarget, sqlite_error};

pub(crate) struct TaskLedgerReader {
    pool: ReaderPool,
}

impl TaskLedgerReader {
    pub(crate) fn from_pool(pool: ReaderPool) -> Self {
        Self { pool }
    }
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
}

fn list_tasks(
    connection: &Connection,
    target: &SharedDbTarget,
    team: &TeamName,
    member: Option<&AgentName>,
) -> Result<Vec<TaskRow>, AtmError> {
    let mut statement = connection
        .prepare(
            "SELECT team, task_id, assignee, assigner, state, assignment_message_id,
                    description, assigned_at, updated_at, last_reminded_at, reminder_count,
                    lead_notified_count
               FROM tasks WHERE team = ?1 AND (?2 IS NULL OR assignee = ?2)
               ORDER BY assigned_at DESC, task_id DESC",
        )
        .map_err(|error| sqlite_error(target, "failed to prepare async task list", error))?;
    let rows = statement
        .query_map(
            params![team.as_str(), member.map(AgentName::as_str)],
            SqliteTaskStore::decode_row,
        )
        .map_err(|error| sqlite_error(target, "failed to execute async task list", error))?;
    rows.map(|row| {
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
        .prepare(
            "SELECT team, task_id, assignee, seq, at, event, from_state, to_state, actor,
                    message_id, outcome, marker, detail
               FROM task_events
              WHERE team = ?1 AND task_id = ?2 AND (?3 IS NULL OR assignee = ?3)
              ORDER BY seq ASC",
        )
        .map_err(|error| sqlite_error(target, "failed to prepare async task event list", error))?;
    let rows = statement
        .query_map(
            params![
                team.as_str(),
                task_id.as_str(),
                member.map(AgentName::as_str)
            ],
            SqliteTaskStore::decode_event_row,
        )
        .map_err(|error| sqlite_error(target, "failed to execute async task event list", error))?;
    rows.map(|row| {
        row.map_err(|error| sqlite_error(target, "failed to decode async task event", error))
    })
    .collect()
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
