//! Bounded backend-owned reader lane for task-ledger projections.
//!
//! The task ledger shares the mailbox reader pool because both capabilities
//! are read-only projections over the same SQLite target. Task state changes
//! remain owned by the ordered writer transaction in `task_store`.

use atm_storage::{
    AgentName, AsyncTaskLedgerReader, AtmError, ReadDeadline, ReadLaneError, TaskEventRow, TaskId,
    TaskRow, TeamName,
};
use rusqlite::Connection;
use std::sync::Arc;

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
}

fn list_tasks(
    connection: &Connection,
    target: &SharedDbTarget,
    team: &TeamName,
    member: Option<&AgentName>,
) -> Result<Vec<TaskRow>, AtmError> {
    task_sql::select_tasks_for_team(connection, team, member)
        .map_err(|error| sqlite_error(target, "failed to list async tasks", error))
}

fn list_task_events(
    connection: &Connection,
    target: &SharedDbTarget,
    team: &TeamName,
    task_id: &TaskId,
    member: Option<&AgentName>,
) -> Result<Vec<TaskEventRow>, AtmError> {
    task_sql::select_task_events(connection, team, task_id, member)
        .map_err(|error| sqlite_error(target, "failed to list async task events", error))
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
