//! Bounded backend-owned reader lane for task-ledger projections.
//!
//! The task ledger shares the mailbox reader pool because both capabilities
//! are read-only projections over the same SQLite target. Task state changes
//! remain owned by the ordered writer transaction in `task_store`.

use atm_storage::{
    AgentName, AsyncTaskLedgerReader, AtmError, ReadDeadline, ReadLaneError, RefusalRun,
    TaskEventRow, TaskId, TaskRow, TeamName,
};
use rusqlite::{Connection, params};
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
    async fn load_task(
        &self,
        team: TeamName,
        task_id: TaskId,
        deadline: ReadDeadline,
    ) -> Result<Option<TaskRow>, ReadLaneError> {
        self.pool
            .submit(deadline.remaining(), move |connection, target| {
                task_sql::select_task_row(connection, &team, &task_id)
                    .map_err(|error| sqlite_error(target, "failed to load task row", error))
                    .map_err(read_lane_error)
            })
            .await
    }

    async fn open_tasks_for_team(
        &self,
        team: TeamName,
        deadline: ReadDeadline,
    ) -> Result<Vec<TaskRow>, ReadLaneError> {
        self.pool
            .submit(deadline.remaining(), move |connection, target| {
                task_sql::select_open_tasks_for_team(connection, &team)
                    .map_err(|error| sqlite_error(target, "failed to list open team tasks", error))
                    .map_err(read_lane_error)
            })
            .await
    }

    async fn refusal_run(
        &self,
        team: TeamName,
        assignee: AgentName,
        deadline: ReadDeadline,
    ) -> Result<RefusalRun, ReadLaneError> {
        self.pool
            .submit(deadline.remaining(), move |connection, target| {
                task_sql::trailing_refusal_run(connection, &team, &assignee)
                    .map_err(|error| sqlite_error(target, "failed to read refusal run", error))
                    .map_err(read_lane_error)
            })
            .await
    }

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

fn read_lane_error(error: AtmError) -> ReadLaneError {
    ReadLaneError::Unavailable {
        message: error.message().to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use crate::SqliteStorageBackend;
    use atm_storage::{
        AgentName, AtmMessageId, IsoTimestamp, Message, MessageEnvelope, MessageKey, ReadDeadline,
        TaskId, TeamName,
    };
    use serde_json::Map;
    use std::time::Duration;

    #[tokio::test]
    async fn sqlite_task_ledger_reader_uses_the_bounded_reader_pool() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let reader = backend.async_task_ledger_reader();
        let team: TeamName = "reader-test-team".parse().expect("team");
        let task_id: TaskId = "reader-test-task".parse().expect("task id");
        let deadline = || ReadDeadline::new(Duration::from_secs(1)).expect("deadline");

        let message_id = AtmMessageId::new();
        backend
            .message_store()
            .save_message(&Message {
                team: team.clone(),
                agent: "assignee".parse::<AgentName>().expect("agent"),
                message_key: MessageKey::new(format!("atm:{message_id}")).expect("message key"),
                envelope: MessageEnvelope {
                    from: "assigner".parse().expect("assigner"),
                    source_chat_id: None,
                    text: "assignment".to_owned(),
                    timestamp: IsoTimestamp::now(),
                    read: false,
                    source_team: Some(team.clone()),
                    destination_chat_id: None,
                    summary: None,
                    message_id: Some(message_id),
                    requires_ack: true,
                    pending_ack_at: Some(IsoTimestamp::now()),
                    acknowledged_at: None,
                    acknowledges_message_id: None,
                    parent_message_id: None,
                    thread_mode: None,
                    expires_at: None,
                    task_id: Some(task_id.clone()),
                    placement: None,
                    task_op: None,
                    task_complete: None,
                    extra: Map::new(),
                },
            })
            .expect("seed task");

        assert_eq!(
            reader
                .list_tasks(team.clone(), None, deadline())
                .await
                .expect("task list")
                .len(),
            1
        );
        let loaded = reader
            .load_task(team.clone(), task_id.clone(), deadline())
            .await
            .expect("task load")
            .expect("seeded task");
        assert_eq!(loaded.task_id, task_id.clone());
        assert!(
            reader
                .load_task("other-team".parse().expect("team"), task_id, deadline())
                .await
                .expect("other-team task load")
                .is_none()
        );
        assert_eq!(
            reader
                .list_task_events(
                    team,
                    "reader-test-task".parse::<TaskId>().expect("task id"),
                    None,
                    deadline(),
                )
                .await
                .expect("task event list")
                .len(),
            1
        );
    }
}
