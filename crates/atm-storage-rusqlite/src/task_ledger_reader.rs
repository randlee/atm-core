//! Bounded backend-owned reader lane for task-ledger projections.
//!
//! The task ledger shares the mailbox reader pool because both capabilities
//! are read-only projections over the same SQLite target. Task state changes
//! remain owned by the ordered writer transaction in `task_store`.

use atm_storage::{
    AgentName, AsyncTaskLedgerReader, AtmError, PromptHandoff, ReadDeadline, ReadLaneError,
    RefusalRun, TaskEventRow, TaskId, TaskRow, TeamName,
};
use rusqlite::{Connection, params, params_from_iter, types::Value};
use std::sync::Arc;

use crate::SqliteTaskStore;
use crate::mailbox_reader::read_lane_storage_error;
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
                    .map_err(read_lane_storage_error)
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
                    .map_err(read_lane_storage_error)
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
                    .map_err(read_lane_storage_error)
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
                list_tasks(connection, target, &team, member.as_ref())
                    .map_err(read_lane_storage_error)
            })
            .await
    }

    async fn list_task_history(
        &self,
        team: TeamName,
        member: Option<AgentName>,
        limit: usize,
        deadline: ReadDeadline,
    ) -> Result<Vec<TaskRow>, ReadLaneError> {
        self.pool
            .submit(deadline.remaining(), move |connection, target| {
                list_task_history(connection, target, &team, member.as_ref(), limit)
                    .map_err(read_lane_storage_error)
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
                    .map_err(read_lane_storage_error)
            })
            .await
    }

    async fn list_task_events_for_tasks(
        &self,
        team: TeamName,
        task_ids: Vec<TaskId>,
        deadline: ReadDeadline,
    ) -> Result<Vec<TaskEventRow>, ReadLaneError> {
        if task_ids.is_empty() {
            return Ok(Vec::new());
        }
        self.pool
            .submit(deadline.remaining(), move |connection, target| {
                list_task_events_for_tasks(connection, target, &team, &task_ids)
                    .map_err(read_lane_storage_error)
            })
            .await
    }

    async fn list_prompt_handoffs(
        &self,
        team: TeamName,
        task_id: TaskId,
        deadline: ReadDeadline,
    ) -> Result<Vec<PromptHandoff>, ReadLaneError> {
        self.pool
            .submit(deadline.remaining(), move |connection, target| {
                list_prompt_handoffs(connection, target, &team, &task_id)
                    .map_err(read_lane_storage_error)
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

fn list_task_history(
    connection: &Connection,
    target: &SharedDbTarget,
    team: &TeamName,
    member: Option<&AgentName>,
    limit: usize,
) -> Result<Vec<TaskRow>, AtmError> {
    let mut statement = connection
        .prepare(&task_sql::select_task_history_sql())
        .map_err(|error| sqlite_error(target, "failed to prepare async task history", error))?;
    let limit = task_sql::clamp_limit_to_i64(limit);
    statement
        .query_map(
            params![team.as_str(), member.map(AgentName::as_str), limit],
            SqliteTaskStore::decode_row,
        )
        .map_err(|error| sqlite_error(target, "failed to list async task history", error))?
        .map(|row| {
            row.map_err(|error| {
                sqlite_error(target, "failed to decode async task history row", error)
            })
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

/// Every task-event ledger row for `task_ids`, in one query, rather than one
/// query per task id. Backs `atm task history`'s ledger-derived
/// started/closed/outcome columns.
fn list_task_events_for_tasks(
    connection: &Connection,
    target: &SharedDbTarget,
    team: &TeamName,
    task_ids: &[TaskId],
) -> Result<Vec<TaskEventRow>, AtmError> {
    let sql = task_sql::select_task_events_for_tasks_sql(task_ids.len());
    let mut statement = connection.prepare(&sql).map_err(|error| {
        sqlite_error(target, "failed to prepare batched task event list", error)
    })?;
    let mut bindings = Vec::with_capacity(task_ids.len() + 1);
    bindings.push(Value::Text(team.as_str().to_owned()));
    bindings.extend(
        task_ids
            .iter()
            .map(|task_id| Value::Text(task_id.as_str().to_owned())),
    );
    statement
        .query_map(
            params_from_iter(bindings),
            SqliteTaskStore::decode_event_row,
        )
        .map_err(|error| sqlite_error(target, "failed to list batched task events", error))?
        .map(|row| {
            row.map_err(|error| sqlite_error(target, "failed to decode batched task event", error))
        })
        .collect()
}

fn list_prompt_handoffs(
    connection: &Connection,
    target: &SharedDbTarget,
    team: &TeamName,
    task_id: &TaskId,
) -> Result<Vec<PromptHandoff>, AtmError> {
    let mut statement = connection
        .prepare(&format!(
            "SELECT {} FROM prompt_handoffs WHERE team = ?1 AND task_id = ?2
             ORDER BY at ASC, rowid ASC",
            task_sql::PROMPT_HANDOFF_COLUMNS
        ))
        .map_err(|error| {
            sqlite_error(target, "failed to prepare async prompt handoff list", error)
        })?;
    statement
        .query_map(
            params![team.as_str(), task_id.as_str()],
            SqliteTaskStore::decode_prompt_handoff,
        )
        .map_err(|error| sqlite_error(target, "failed to list async prompt handoffs", error))?
        .map(|row| {
            row.map_err(|error| sqlite_error(target, "failed to decode prompt handoff", error))
        })
        .collect()
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

    /// No-Claim: proves the batched read returns every seeded task's events
    /// in one call; it does not prove the caller only issues one such call
    /// per `atm task history` invocation (that is
    /// `history_round_trip_count_does_not_scale_with_row_count` in
    /// `crates/atm/tests/task_ledger_cli.rs`).
    #[tokio::test]
    async fn sqlite_task_ledger_reader_batches_events_for_multiple_task_ids() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let reader = backend.async_task_ledger_reader();
        let team: TeamName = "batch-events-team".parse().expect("team");
        let deadline = || ReadDeadline::new(Duration::from_secs(1)).expect("deadline");

        let mut task_ids = Vec::new();
        for label in ["batch-task-a", "batch-task-b"] {
            let task_id: TaskId = label.parse().expect("task id");
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
            task_ids.push(task_id);
        }

        let batched = reader
            .list_task_events_for_tasks(team.clone(), task_ids.clone(), deadline())
            .await
            .expect("batched task events");
        assert_eq!(batched.len(), 2, "one assigned event per seeded task");
        assert!(
            task_ids
                .iter()
                .all(|task_id| batched.iter().any(|event| &event.task_id == task_id)),
            "every seeded task id is represented"
        );

        let empty = reader
            .list_task_events_for_tasks(team, Vec::new(), deadline())
            .await
            .expect("empty task-id set short-circuits without a query");
        assert!(empty.is_empty());
    }
}
