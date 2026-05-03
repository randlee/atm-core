use atm_core::store::{MessageKey, StoreError};
use atm_core::task_store::{TaskRecord, TaskStatus, TaskStore};
use atm_core::types::{IsoTimestamp, TaskId};
use rusqlite::OptionalExtension;

use crate::{
    RusqliteStore, classify_store_error, invalid_store_data, parse_optional, parse_required,
};

#[derive(Debug)]
struct RawTaskRow {
    task_id: String,
    message_key: String,
    status: String,
    created_at: String,
    acknowledged_at: Option<String>,
    metadata_json: Option<String>,
}

impl TaskStore for RusqliteStore {
    fn upsert_task(&self, task: &TaskRecord) -> Result<TaskRecord, StoreError> {
        let connection = self.lock_connection()?;
        upsert_task_row(&connection, task)
            .map_err(|error| classify_store_error(error, "failed to upsert task row"))?;
        Ok(task.clone())
    }

    fn load_task(&self, task_id: &TaskId) -> Result<Option<TaskRecord>, StoreError> {
        let connection = self.lock_connection()?;
        let raw = connection
            .query_row(
                "SELECT task_id, message_key, status, created_at, acknowledged_at, metadata_json FROM tasks WHERE task_id = ?1",
                [task_id.as_str()],
                |row| {
                    Ok(RawTaskRow {
                        task_id: row.get(0)?,
                        message_key: row.get(1)?,
                        status: row.get(2)?,
                        created_at: row.get(3)?,
                        acknowledged_at: row.get(4)?,
                        metadata_json: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(|error| classify_store_error(error, "failed to load task row"))?;
        raw.map(convert_task_row).transpose()
    }

    fn load_tasks_for_message(
        &self,
        message_key: &MessageKey,
    ) -> Result<Vec<TaskRecord>, StoreError> {
        let connection = self.lock_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT task_id, message_key, status, created_at, acknowledged_at, metadata_json FROM tasks WHERE message_key = ?1 ORDER BY task_id",
            )
            .map_err(|error| classify_store_error(error, "failed to prepare task-by-message query"))?;
        let rows = statement
            .query_map([message_key.as_str()], |row| {
                Ok(RawTaskRow {
                    task_id: row.get(0)?,
                    message_key: row.get(1)?,
                    status: row.get(2)?,
                    created_at: row.get(3)?,
                    acknowledged_at: row.get(4)?,
                    metadata_json: row.get(5)?,
                })
            })
            .map_err(|error| classify_store_error(error, "failed to query task-by-message rows"))?;

        let mut tasks = Vec::new();
        for row in rows {
            let raw =
                row.map_err(|error| classify_store_error(error, "failed to read task row"))?;
            tasks.push(convert_task_row(raw)?);
        }
        Ok(tasks)
    }

    fn acknowledge_task(
        &self,
        task_id: &TaskId,
        acknowledged_at: IsoTimestamp,
    ) -> Result<Option<TaskRecord>, StoreError> {
        self.with_transaction(|transaction| {
            transaction
                .execute(
                    "UPDATE tasks SET status = ?1, acknowledged_at = ?2 WHERE task_id = ?3",
                    (
                        TaskStatus::Acknowledged.as_str(),
                        acknowledged_at.to_string(),
                        task_id.as_str(),
                    ),
                )
                .map_err(|error| classify_store_error(error, "failed to acknowledge task"))?;
            let raw = transaction
                .query_row(
                    "SELECT task_id, message_key, status, created_at, acknowledged_at, metadata_json FROM tasks WHERE task_id = ?1",
                    [task_id.as_str()],
                    |row| {
                        Ok(RawTaskRow {
                            task_id: row.get(0)?,
                            message_key: row.get(1)?,
                            status: row.get(2)?,
                            created_at: row.get(3)?,
                            acknowledged_at: row.get(4)?,
                            metadata_json: row.get(5)?,
                        })
                    },
                )
                .optional()
                .map_err(|error| {
                    classify_store_error(error, "failed to reload acknowledged task")
                })?;
            raw.map(convert_task_row).transpose()
        })
    }
}

pub(crate) fn upsert_task_row(
    connection: &rusqlite::Connection,
    task: &TaskRecord,
) -> rusqlite::Result<()> {
    connection.execute(
        r#"
        INSERT INTO tasks (
            task_id,
            message_key,
            status,
            created_at,
            acknowledged_at,
            metadata_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ON CONFLICT(task_id) DO UPDATE SET
            message_key = excluded.message_key,
            status = excluded.status,
            created_at = excluded.created_at,
            acknowledged_at = excluded.acknowledged_at,
            metadata_json = excluded.metadata_json
        "#,
        (
            task.task_id.as_str(),
            task.message_key.as_str(),
            task.status.as_str(),
            task.created_at.to_string(),
            task.acknowledged_at.as_ref().map(ToString::to_string),
            task.metadata_json.as_deref(),
        ),
    )?;
    Ok(())
}

fn convert_task_row(raw: RawTaskRow) -> Result<TaskRecord, StoreError> {
    Ok(TaskRecord {
        task_id: parse_required(raw.task_id, "task_id")?,
        message_key: parse_required(raw.message_key, "message_key")?,
        status: parse_task_status(raw.status)?,
        created_at: parse_required(raw.created_at, "created_at")?,
        acknowledged_at: parse_optional(raw.acknowledged_at, "acknowledged_at")?,
        metadata_json: raw.metadata_json,
    })
}

fn parse_task_status(value: String) -> Result<TaskStatus, StoreError> {
    match value.as_str() {
        "pending_ack" => Ok(TaskStatus::PendingAck),
        "acknowledged" => Ok(TaskStatus::Acknowledged),
        _ => Err(invalid_store_data("task_status", "unsupported task status")),
    }
}
