#![forbid(unsafe_code)]
#![allow(dead_code)]

//! SQLite-backed adapter implementations for the Phase R store boundaries.

mod roster_store;

use atm_core::boundary;
use atm_core::error::AtmError;
use atm_core::types::{IsoTimestamp, TeamName};
use rusqlite::{Connection, Error as RusqliteError, OptionalExtension, params};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const DB_MIGRATIONS: &str = r#"
PRAGMA journal_mode=WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS mail_messages (
    team TEXT NOT NULL,
    agent TEXT NOT NULL,
    message_key TEXT NOT NULL,
    envelope_json TEXT NOT NULL,
    imported_from TEXT,
    recorded_at TEXT,
    PRIMARY KEY (team, agent, message_key)
);

CREATE TABLE IF NOT EXISTS mail_visibility_states (
    team TEXT NOT NULL,
    agent TEXT NOT NULL,
    message_key TEXT NOT NULL,
    state_json TEXT NOT NULL,
    PRIMARY KEY (team, agent, message_key)
);

CREATE TABLE IF NOT EXISTS mail_ingest_replay_states (
    team TEXT NOT NULL,
    agent TEXT NOT NULL,
    source TEXT NOT NULL,
    state_json TEXT NOT NULL,
    PRIMARY KEY (team, agent, source)
);

CREATE TABLE IF NOT EXISTS task_records (
    team TEXT NOT NULL,
    task_id TEXT NOT NULL,
    record_json TEXT NOT NULL,
    PRIMARY KEY (team, task_id)
);

CREATE TABLE IF NOT EXISTS task_ack_transitions (
    team TEXT NOT NULL,
    task_id TEXT NOT NULL,
    transition_index INTEGER NOT NULL,
    transition_json TEXT NOT NULL,
    PRIMARY KEY (team, task_id, transition_index)
);

CREATE TABLE IF NOT EXISTS rosters (
    team TEXT PRIMARY KEY,
    roster_json TEXT NOT NULL,
    source TEXT,
    updated_at TEXT NOT NULL
);
"#;

#[derive(Debug, Clone)]
struct SharedDb {
    path: Arc<PathBuf>,
}

impl SharedDb {
    fn open(path: impl AsRef<Path>) -> Result<Self, AtmError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                AtmError::validation(format!(
                    "failed to create sqlite parent directory {}: {error}",
                    parent.display()
                ))
                .with_recovery(
                    "Check the sqlite database directory permissions or choose a different Phase R state path.",
                )
                .with_source(error)
            })?;
        }

        let db = Self {
            path: Arc::new(path),
        };
        db.with_connection(|connection| {
            connection
                .execute_batch(DB_MIGRATIONS)
                .map_err(|error| sqlite_error("failed to initialize sqlite schema", error))
        })?;
        Ok(db)
    }

    fn with_connection<T>(
        &self,
        operation: impl FnOnce(&mut Connection) -> Result<T, AtmError>,
    ) -> Result<T, AtmError> {
        let mut connection = Connection::open(self.path.as_ref()).map_err(|error| {
            sqlite_error(
                format!("failed to open sqlite database {}", self.path.display()),
                error,
            )
        })?;
        connection
            .busy_timeout(std::time::Duration::from_millis(5000))
            .map_err(|error| sqlite_error("failed to configure sqlite busy timeout", error))?;
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .map_err(|error| sqlite_error("failed to enable sqlite foreign keys", error))?;
        operation(&mut connection)
    }

    fn with_transaction<T>(
        &self,
        operation: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<T, AtmError>,
    ) -> Result<T, AtmError> {
        self.with_connection(|connection| {
            let transaction = connection
                .transaction()
                .map_err(|error| sqlite_error("failed to open sqlite transaction", error))?;
            let value = operation(&transaction)?;
            transaction
                .commit()
                .map_err(|error| sqlite_error("failed to commit sqlite transaction", error))?;
            Ok(value)
        })
    }
}

fn sqlite_error(message: impl Into<String>, source: RusqliteError) -> AtmError {
    AtmError::validation(message)
        .with_recovery("Retry the SQLite-backed boundary operation after the lock is released.")
        .with_source(source)
}

fn json_error(message: impl Into<String>, source: serde_json::Error) -> AtmError {
    AtmError::validation(message)
        .with_recovery("Repair the persisted ATM-owned JSON payload or rebuild it through the owning boundary.")
        .with_source(source)
}

fn serialize_json<T: serde::Serialize>(value: &T, what: &str) -> Result<String, AtmError> {
    serde_json::to_string(value)
        .map_err(|error| json_error(format!("failed to encode {what}"), error))
}

fn deserialize_json<T: serde::de::DeserializeOwned>(
    value: &str,
    what: &str,
) -> Result<T, AtmError> {
    serde_json::from_str(value)
        .map_err(|error| json_error(format!("failed to decode {what}"), error))
}

#[derive(Debug)]
struct SqliteRosterStore {
    db: Arc<SharedDb>,
}

impl SqliteRosterStore {
    fn new(db: Arc<SharedDb>) -> Self {
        Self { db }
    }
}

/// Internal assembly root for Phase R SQLite-backed boundary implementations.
#[derive(Debug)]
pub(crate) struct SqliteBoundaryAssembly {
    mail_store: Arc<SqliteMailStore>,
    task_store: Arc<SqliteTaskStore>,
    roster_store: Arc<SqliteRosterStore>,
}

impl SqliteBoundaryAssembly {
    pub(crate) fn new(path: impl AsRef<Path>) -> Result<Self, AtmError> {
        let db = Arc::new(SharedDb::open(path)?);
        Ok(Self {
            mail_store: Arc::new(SqliteMailStore::new(db.clone())),
            task_store: Arc::new(SqliteTaskStore::new(db.clone())),
            roster_store: Arc::new(SqliteRosterStore::new(db)),
        })
    }

    pub(crate) fn mail_store(&self) -> &dyn boundary::MailStore {
        self.mail_store.as_ref()
    }

    pub(crate) fn task_store(&self) -> &dyn boundary::TaskStore {
        self.task_store.as_ref()
    }

    pub(crate) fn roster_store(&self) -> &dyn boundary::RosterStore {
        self.roster_store.as_ref()
    }
}

pub(crate) fn assemble_boundary(
    path: impl AsRef<Path>,
) -> Result<SqliteBoundaryAssembly, AtmError> {
    SqliteBoundaryAssembly::new(path)
}

#[derive(Debug)]
struct SqliteMailStore {
    db: Arc<SharedDb>,
}

impl SqliteMailStore {
    fn new(db: Arc<SharedDb>) -> Self {
        Self { db }
    }
}

impl boundary::sealed::Sealed for SqliteMailStore {}

impl boundary::MailStore for SqliteMailStore {
    fn bootstrap(
        &self,
        request: boundary::MailStoreBootstrapRequest,
    ) -> Result<boundary::MailStoreBootstrapResponse, AtmError> {
        self.db.with_connection(|connection| {
            connection
                .execute_batch(DB_MIGRATIONS)
                .map_err(|error| sqlite_error("failed to bootstrap mail-store schema", error))?;
            Ok(boundary::MailStoreBootstrapResponse {
                team: request.team,
                bootstrapped: true,
                opened: true,
            })
        })
    }

    fn run_transaction(
        &self,
        request: boundary::MailStoreTransactionRequest,
    ) -> Result<boundary::MailStoreTransactionResponse, AtmError> {
        self.db.with_transaction(|_transaction| {
            Ok(boundary::MailStoreTransactionResponse {
                team: request.team,
                committed: true,
                operations_executed: request.requested_operations.len(),
            })
        })
    }

    fn upsert_message(
        &self,
        request: boundary::MailStoreUpsertMessageRequest,
    ) -> Result<boundary::MailStoreUpsertMessageResponse, AtmError> {
        let record = request.record;
        let envelope_json = serialize_json(&record.envelope, "mail-store envelope")?;
        let inserted = self.db.with_transaction(|transaction| {
            let existing: Option<i64> = transaction
                .query_row(
                    "SELECT 1 FROM mail_messages WHERE team = ?1 AND agent = ?2 AND message_key = ?3;",
                    params![record.team.as_str(), record.agent.as_str(), record.message_key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| sqlite_error("failed to probe existing mail-store message", error))?;
            transaction
                .execute(
                    "INSERT INTO mail_messages(team, agent, message_key, envelope_json, imported_from, recorded_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(team, agent, message_key) DO UPDATE SET
                       envelope_json = excluded.envelope_json,
                       imported_from = excluded.imported_from,
                       recorded_at = excluded.recorded_at;",
                    params![
                        record.team.as_str(),
                        record.agent.as_str(),
                        record.message_key,
                        envelope_json,
                        record.imported_from,
                        record.recorded_at.map(|value| value.into_inner().to_rfc3339()),
                    ],
                )
                .map_err(|error| sqlite_error("failed to upsert mail-store message", error))?;
            Ok(existing.is_none())
        })?;

        Ok(boundary::MailStoreUpsertMessageResponse { record, inserted })
    }

    fn load_message(
        &self,
        request: boundary::MailStoreLoadMessageRequest,
    ) -> Result<boundary::MailStoreLoadMessageResponse, AtmError> {
        let record = self.db.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT envelope_json, imported_from, recorded_at
                     FROM mail_messages
                     WHERE team = ?1 AND agent = ?2 AND message_key = ?3;",
                    params![
                        request.team.as_str(),
                        request.agent.as_str(),
                        request.message_key
                    ],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, Option<String>>(2)?,
                        ))
                    },
                )
                .optional()
                .map_err(|error| sqlite_error("failed to load mail-store message", error))
        })?;

        let record = if let Some((envelope_json, imported_from, recorded_at)) = record {
            let envelope = deserialize_json(&envelope_json, "mail-store envelope")?;
            let recorded_at = recorded_at
                .map(|value| value.parse::<chrono::DateTime<chrono::Utc>>())
                .transpose()
                .map_err(|error| {
                    AtmError::validation(format!(
                        "failed to parse mail-store recorded_at timestamp: {error}"
                    ))
                    .with_recovery("Repair the sqlite-backed mail-store row or rewrite it through the owning boundary.")
                    .with_source(error)
                })?
                .map(IsoTimestamp::from_datetime);
            Some(boundary::MailStoreMessageRecord {
                team: request.team.clone(),
                agent: request.agent.clone(),
                message_key: request.message_key.clone(),
                envelope,
                imported_from,
                recorded_at,
            })
        } else {
            None
        };

        Ok(boundary::MailStoreLoadMessageResponse { record })
    }

    fn upsert_visibility_state(
        &self,
        request: boundary::MailStoreUpsertVisibilityStateRequest,
    ) -> Result<boundary::MailStoreUpsertVisibilityStateResponse, AtmError> {
        let state_json = serialize_json(&request.state, "mail-store visibility state")?;
        self.db.with_transaction(|transaction| {
            transaction
                .execute(
                    "INSERT INTO mail_visibility_states(team, agent, message_key, state_json)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(team, agent, message_key) DO UPDATE SET
                       state_json = excluded.state_json;",
                    params![
                        request.team.as_str(),
                        request.agent.as_str(),
                        request.state.message_key.clone(),
                        state_json,
                    ],
                )
                .map_err(|error| {
                    sqlite_error("failed to upsert mail-store visibility state", error)
                })?;
            Ok(boundary::MailStoreUpsertVisibilityStateResponse {
                state: request.state,
            })
        })
    }

    fn load_visibility_state(
        &self,
        request: boundary::MailStoreLoadVisibilityStateRequest,
    ) -> Result<boundary::MailStoreLoadVisibilityStateResponse, AtmError> {
        let state_json = self.db.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT state_json
                     FROM mail_visibility_states
                     WHERE team = ?1 AND agent = ?2 AND message_key = ?3;",
                    params![
                        request.team.as_str(),
                        request.agent.as_str(),
                        request.message_key
                    ],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(|error| sqlite_error("failed to load mail-store visibility state", error))
        })?;

        let state = state_json
            .map(|value| deserialize_json(&value, "mail-store visibility state"))
            .transpose()?;

        Ok(boundary::MailStoreLoadVisibilityStateResponse { state })
    }

    fn record_ingest_replay_state(
        &self,
        request: boundary::MailStoreRecordIngestReplayStateRequest,
    ) -> Result<boundary::MailStoreRecordIngestReplayStateResponse, AtmError> {
        let state_json = serialize_json(&request.state, "mail-store ingest replay state")?;
        self.db.with_transaction(|transaction| {
            transaction
                .execute(
                    "INSERT INTO mail_ingest_replay_states(team, agent, source, state_json)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(team, agent, source) DO UPDATE SET
                       state_json = excluded.state_json;",
                    params![
                        request.team.as_str(),
                        request.agent.as_str(),
                        request.source,
                        state_json,
                    ],
                )
                .map_err(|error| {
                    sqlite_error("failed to record mail-store ingest replay state", error)
                })?;
            Ok(boundary::MailStoreRecordIngestReplayStateResponse {
                state: request.state,
            })
        })
    }

    fn load_ingest_replay_state(
        &self,
        request: boundary::MailStoreLoadIngestReplayStateRequest,
    ) -> Result<boundary::MailStoreLoadIngestReplayStateResponse, AtmError> {
        let state_json = self.db.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT state_json
                     FROM mail_ingest_replay_states
                     WHERE team = ?1 AND agent = ?2 AND source = ?3;",
                    params![
                        request.team.as_str(),
                        request.agent.as_str(),
                        request.source
                    ],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(|error| {
                    sqlite_error("failed to load mail-store ingest replay state", error)
                })
        })?;

        let state = state_json
            .map(|value| deserialize_json(&value, "mail-store ingest replay state"))
            .transpose()?;

        Ok(boundary::MailStoreLoadIngestReplayStateResponse { state })
    }

    fn health_snapshot(
        &self,
        request: boundary::MailStoreHealthSnapshotRequest,
    ) -> Result<boundary::MailStoreHealthSnapshotResponse, AtmError> {
        let rows = self.db.with_connection(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT envelope_json, recorded_at
                     FROM mail_messages
                     WHERE team = ?1 AND agent = ?2;",
                )
                .map_err(|error| {
                    sqlite_error("failed to prepare mail-store health query", error)
                })?;
            let mapped = statement
                .query_map(
                    params![request.team.as_str(), request.agent.as_str()],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
                )
                .map_err(|error| {
                    sqlite_error("failed to execute mail-store health query", error)
                })?;
            let mut rows = Vec::new();
            for row in mapped {
                rows.push(row.map_err(|error| {
                    sqlite_error("failed to read mail-store health row", error)
                })?);
            }
            Ok(rows)
        })?;

        let states = self.db.with_connection(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT state_json
                     FROM mail_visibility_states
                     WHERE team = ?1 AND agent = ?2;",
                )
                .map_err(|error| {
                    sqlite_error(
                        "failed to prepare mail-store visibility health query",
                        error,
                    )
                })?;
            let mapped = statement
                .query_map(
                    params![request.team.as_str(), request.agent.as_str()],
                    |row| row.get::<_, String>(0),
                )
                .map_err(|error| {
                    sqlite_error(
                        "failed to execute mail-store visibility health query",
                        error,
                    )
                })?;
            let mut rows = Vec::new();
            for row in mapped {
                rows.push(row.map_err(|error| {
                    sqlite_error("failed to read mail-store visibility health row", error)
                })?);
            }
            Ok(rows)
        })?;

        let total_messages = rows.len() as u64;
        let mut pending_ack_messages = 0_u64;
        let mut latest_message_timestamp = None;
        for (envelope_json, recorded_at) in rows {
            let envelope: atm_core::schema::MessageEnvelope =
                deserialize_json(&envelope_json, "mail-store envelope")?;
            if envelope.pending_ack_at.is_some() {
                pending_ack_messages += 1;
            }
            let candidate = recorded_at
                .as_deref()
                .map(str::parse::<chrono::DateTime<chrono::Utc>>)
                .transpose()
                .map_err(|error| {
                    AtmError::validation(format!(
                        "failed to parse mail-store health recorded_at timestamp: {error}"
                    ))
                    .with_recovery("Repair the sqlite-backed mail-store row or rewrite it through the owning boundary.")
                    .with_source(error)
                })?
                .map(IsoTimestamp::from_datetime)
                .or(Some(envelope.timestamp));
            if let Some(candidate) = candidate
                && latest_message_timestamp
                    .map(|current| candidate > current)
                    .unwrap_or(true)
            {
                latest_message_timestamp = Some(candidate);
            }
        }

        let mut read_messages = 0_u64;
        for state_json in states {
            let state: boundary::MailStoreVisibilityState =
                deserialize_json(&state_json, "mail-store visibility state")?;
            if state.read {
                read_messages += 1;
            }
        }

        Ok(boundary::MailStoreHealthSnapshotResponse {
            snapshot: boundary::MailStoreHealthSnapshot {
                team: request.team,
                agent: request.agent,
                total_messages,
                pending_ack_messages,
                read_messages,
                latest_message_timestamp,
            },
        })
    }
}

#[derive(Debug)]
struct SqliteTaskStore {
    db: Arc<SharedDb>,
}

impl SqliteTaskStore {
    fn new(db: Arc<SharedDb>) -> Self {
        Self { db }
    }

    fn load_record_in_connection(
        connection: &Connection,
        team: &TeamName,
        task_id: &atm_core::types::TaskId,
    ) -> Result<Option<boundary::TaskStoreTaskRecord>, AtmError> {
        let record_json = connection
            .query_row(
                "SELECT record_json FROM task_records WHERE team = ?1 AND task_id = ?2;",
                params![team.as_str(), task_id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| sqlite_error("failed to load task-store record", error))?;

        record_json
            .map(|value| deserialize_json(&value, "task-store record"))
            .transpose()
    }

    fn save_record_in_connection(
        connection: &Connection,
        record: &boundary::TaskStoreTaskRecord,
    ) -> Result<(), AtmError> {
        let record_json = serialize_json(record, "task-store record")?;
        connection
            .execute(
                "INSERT INTO task_records(team, task_id, record_json)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(team, task_id) DO UPDATE SET
                   record_json = excluded.record_json;",
                params![record.team.as_str(), record.task_id.as_str(), record_json],
            )
            .map_err(|error| sqlite_error("failed to save task-store record", error))?;
        Ok(())
    }

    fn load_record(
        &self,
        team: &TeamName,
        task_id: &atm_core::types::TaskId,
    ) -> Result<Option<boundary::TaskStoreTaskRecord>, AtmError> {
        self.db.with_connection(|connection| {
            Self::load_record_in_connection(connection, team, task_id)
        })
    }

    fn save_record(&self, record: &boundary::TaskStoreTaskRecord) -> Result<(), AtmError> {
        self.db
            .with_transaction(|transaction| Self::save_record_in_connection(transaction, record))
    }
}

impl boundary::sealed::Sealed for SqliteTaskStore {}

impl boundary::TaskStore for SqliteTaskStore {
    fn create_task(
        &self,
        request: boundary::TaskStoreCreateTaskRequest,
    ) -> Result<boundary::TaskStoreCreateTaskResponse, AtmError> {
        self.db.with_transaction(|transaction| {
            Self::save_record_in_connection(transaction, &request.record)?;
            Ok(())
        })?;
        Ok(boundary::TaskStoreCreateTaskResponse {
            record: request.record,
        })
    }

    fn load_task(
        &self,
        request: boundary::TaskStoreLoadTaskRequest,
    ) -> Result<boundary::TaskStoreLoadTaskResponse, AtmError> {
        Ok(boundary::TaskStoreLoadTaskResponse {
            record: self.load_record(&request.team, &request.task_id)?,
        })
    }

    fn update_task(
        &self,
        request: boundary::TaskStoreUpdateTaskRequest,
    ) -> Result<boundary::TaskStoreUpdateTaskResponse, AtmError> {
        self.db.with_transaction(|transaction| {
            let mut record =
                Self::load_record_in_connection(transaction, &request.team, &request.task_id)?
                    .ok_or_else(|| {
                        AtmError::validation(format!(
                            "task-store update failed because task {} does not exist in team {}",
                            request.task_id, request.team
                        ))
                        .with_recovery(
                            "Create the task through TaskStore::create_task before updating it.",
                        )
                    })?;
            if let Some(owner) = request.owner {
                record.owner = Some(owner);
            }
            if let Some(state) = request.state {
                record.state = state;
            }
            if let Some(metadata) = request.metadata {
                record.metadata = metadata;
            }
            for message_key in request.append_message_keys {
                if !record
                    .linked_message_keys
                    .iter()
                    .any(|existing| existing == &message_key)
                {
                    record.linked_message_keys.push(message_key);
                }
            }
            record.updated_at = Some(atm_core::types::IsoTimestamp::now());
            Self::save_record_in_connection(transaction, &record)?;
            Ok(boundary::TaskStoreUpdateTaskResponse { record })
        })
    }

    fn attach_message_link(
        &self,
        request: boundary::TaskStoreAttachMessageLinkRequest,
    ) -> Result<boundary::TaskStoreAttachMessageLinkResponse, AtmError> {
        self.db.with_transaction(|transaction| {
            let mut record = Self::load_record_in_connection(transaction, &request.team, &request.task_id)?
                .ok_or_else(|| {
                    AtmError::validation(format!(
                        "task-store attach-link failed because task {} does not exist in team {}",
                        request.task_id, request.team
                    ))
                    .with_recovery("Create the task through TaskStore::create_task before attaching message links.")
                })?;
            if !record
                .linked_message_keys
                .iter()
                .any(|existing| existing == &request.message_key)
            {
                record.linked_message_keys.push(request.message_key);
            }
            record.updated_at = Some(atm_core::types::IsoTimestamp::now());
            Self::save_record_in_connection(transaction, &record)?;
            Ok(boundary::TaskStoreAttachMessageLinkResponse { record })
        })
    }

    fn detach_message_link(
        &self,
        request: boundary::TaskStoreDetachMessageLinkRequest,
    ) -> Result<boundary::TaskStoreDetachMessageLinkResponse, AtmError> {
        self.db.with_transaction(|transaction| {
            let mut record = Self::load_record_in_connection(transaction, &request.team, &request.task_id)?
                .ok_or_else(|| {
                    AtmError::validation(format!(
                        "task-store detach-link failed because task {} does not exist in team {}",
                        request.task_id, request.team
                    ))
                    .with_recovery("Create the task through TaskStore::create_task before detaching message links.")
                })?;
            record
                .linked_message_keys
                .retain(|existing| existing != &request.message_key);
            record.updated_at = Some(atm_core::types::IsoTimestamp::now());
            Self::save_record_in_connection(transaction, &record)?;
            Ok(boundary::TaskStoreDetachMessageLinkResponse { record })
        })
    }

    fn record_ack_transition(
        &self,
        request: boundary::TaskStoreRecordAckTransitionRequest,
    ) -> Result<boundary::TaskStoreRecordAckTransitionResponse, AtmError> {
        let transition_json = serialize_json(
            &serde_json::json!({
                "message_key": request.message_key.clone(),
                "actor": request.actor.to_string(),
                "transitioned_at": request.transitioned_at.into_inner().to_rfc3339(),
                "transition": request.transition.clone(),
            }),
            "task ack transition",
        )?;
        self.db.with_transaction(|transaction| {
            let mut record = Self::load_record_in_connection(transaction, &request.team, &request.task_id)?
                .ok_or_else(|| {
                    AtmError::validation(format!(
                        "task-store ack transition failed because task {} does not exist in team {}",
                        request.task_id, request.team
                    ))
                    .with_recovery("Create the task through TaskStore::create_task before recording ack transitions.")
                })?;
            record.metadata.fields.insert(
                "last_ack_transition".to_string(),
                request.transition.clone(),
            );
            record
                .metadata
                .fields
                .insert("last_ack_actor".to_string(), request.actor.to_string());
            record.metadata.fields.insert(
                "last_ack_message_key".to_string(),
                request.message_key.clone(),
            );
            record.metadata.fields.insert(
                "last_ack_at".to_string(),
                request.transitioned_at.into_inner().to_rfc3339(),
            );
            record.updated_at = Some(atm_core::types::IsoTimestamp::now());
            Self::save_record_in_connection(transaction, &record)?;

            let next_index: i64 = transaction
                .query_row(
                    "SELECT COALESCE(MAX(transition_index), -1) + 1
                     FROM task_ack_transitions
                     WHERE team = ?1 AND task_id = ?2;",
                    params![record.team.as_str(), record.task_id.as_str()],
                    |row| row.get(0),
                )
                .map_err(|error| sqlite_error("failed to query next task ack transition index", error))?;
            transaction
                .execute(
                    "INSERT INTO task_ack_transitions(team, task_id, transition_index, transition_json)
                     VALUES (?1, ?2, ?3, ?4);",
                    params![
                        record.team.as_str(),
                        record.task_id.as_str(),
                        next_index,
                        transition_json,
                    ],
                )
                .map_err(|error| sqlite_error("failed to persist task ack transition", error))?;
            Ok(boundary::TaskStoreRecordAckTransitionResponse { record })
        })
    }

    fn query_task_metadata(
        &self,
        request: boundary::TaskStoreQueryTaskMetadataRequest,
    ) -> Result<boundary::TaskStoreQueryTaskMetadataResponse, AtmError> {
        let rows = self.db.with_connection(|connection| {
            let mut statement = connection
                .prepare("SELECT record_json FROM task_records WHERE team = ?1 ORDER BY task_id;")
                .map_err(|error| {
                    sqlite_error("failed to prepare task-store metadata query", error)
                })?;
            let mapped = statement
                .query_map(params![request.team.as_str()], |row| {
                    row.get::<_, String>(0)
                })
                .map_err(|error| {
                    sqlite_error("failed to execute task-store metadata query", error)
                })?;
            let mut rows = Vec::new();
            for row in mapped {
                rows.push(row.map_err(|error| {
                    sqlite_error("failed to read task-store metadata row", error)
                })?);
            }
            Ok(rows)
        })?;

        let mut records = Vec::new();
        for row in rows {
            let record: boundary::TaskStoreTaskRecord =
                deserialize_json(&row, "task-store record")?;
            if let Some(task_id) = request.task_id.as_ref()
                && &record.task_id != task_id
            {
                continue;
            }
            if let Some(message_key) = request.message_key.as_ref()
                && !record
                    .linked_message_keys
                    .iter()
                    .any(|existing| existing == message_key)
            {
                continue;
            }
            if let Some(state) = request.state.as_ref()
                && &record.state != state
            {
                continue;
            }
            records.push(record);
            if let Some(limit) = request.limit
                && records.len() >= limit
            {
                break;
            }
        }

        Ok(boundary::TaskStoreQueryTaskMetadataResponse { records })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atm_core::schema::TeamConfig;
    use atm_core::schema::{AgentMember, MessageEnvelope};
    use atm_core::types::{AgentName, IsoTimestamp, TaskId, TeamName};
    use tempfile::TempDir;

    fn temp_db() -> (TempDir, PathBuf) {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("phase-r.sqlite3");
        (tempdir, path)
    }

    fn team() -> TeamName {
        "test-team".parse().expect("team")
    }

    fn agent() -> AgentName {
        "test-agent".parse().expect("agent")
    }

    fn actor() -> AgentName {
        "test-actor".parse().expect("agent")
    }

    fn task_id() -> TaskId {
        "task-123".parse().expect("task id")
    }

    fn envelope() -> MessageEnvelope {
        MessageEnvelope {
            from: actor(),
            text: "phase-r message".to_string(),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: Some(team()),
            summary: Some("summary".to_string()),
            message_id: None,
            pending_ack_at: Some(IsoTimestamp::now()),
            acknowledged_at: None,
            acknowledges_message_id: None,
            task_id: Some(task_id()),
            extra: serde_json::Map::new(),
        }
    }

    #[test]
    fn mail_store_round_trips_message_visibility_and_health() {
        let (_tempdir, path) = temp_db();
        let assembly = assemble_boundary(&path).expect("assembly");
        let store = assembly.mail_store();

        store
            .bootstrap(boundary::MailStoreBootstrapRequest {
                team_dir: path.parent().expect("parent").to_path_buf(),
                team: team(),
                team_config: None,
            })
            .expect("bootstrap");

        let record = boundary::MailStoreMessageRecord {
            team: team(),
            agent: agent(),
            message_key: "atm:test-1".to_string(),
            envelope: envelope(),
            imported_from: Some("cli-send".to_string()),
            recorded_at: Some(IsoTimestamp::now()),
        };
        let upsert = store
            .upsert_message(boundary::MailStoreUpsertMessageRequest {
                record: record.clone(),
            })
            .expect("upsert");
        assert!(upsert.inserted);

        let loaded = store
            .load_message(boundary::MailStoreLoadMessageRequest {
                team: team(),
                agent: agent(),
                message_key: "atm:test-1".to_string(),
            })
            .expect("load");
        assert_eq!(loaded.record, Some(record.clone()));

        let visibility = boundary::MailStoreVisibilityState {
            team: team(),
            agent: agent(),
            actor: actor(),
            message_key: "atm:test-1".to_string(),
            read: true,
            pending_ack_at: record.envelope.pending_ack_at,
            acknowledged_at: None,
            updated_at: Some(IsoTimestamp::now()),
        };
        store
            .upsert_visibility_state(boundary::MailStoreUpsertVisibilityStateRequest {
                team: team(),
                agent: agent(),
                actor: actor(),
                state: visibility.clone(),
            })
            .expect("upsert visibility");
        let loaded_visibility = store
            .load_visibility_state(boundary::MailStoreLoadVisibilityStateRequest {
                team: team(),
                agent: agent(),
                actor: actor(),
                message_key: "atm:test-1".to_string(),
            })
            .expect("load visibility");
        assert_eq!(loaded_visibility.state, Some(visibility));

        let health = store
            .health_snapshot(boundary::MailStoreHealthSnapshotRequest {
                team: team(),
                agent: agent(),
            })
            .expect("health");
        assert_eq!(health.snapshot.total_messages, 1);
        assert_eq!(health.snapshot.pending_ack_messages, 1);
        assert_eq!(health.snapshot.read_messages, 1);
    }

    #[test]
    fn task_store_round_trips_records_and_metadata_queries() {
        let (_tempdir, path) = temp_db();
        let assembly = assemble_boundary(&path).expect("assembly");
        let store = assembly.task_store();

        let record = boundary::TaskStoreTaskRecord {
            team: team(),
            task_id: task_id(),
            state: "active".to_string(),
            owner: Some(agent()),
            linked_message_keys: vec!["atm:test-1".to_string()],
            metadata: boundary::TaskStoreTaskMetadata::default(),
            created_at: Some(IsoTimestamp::now()),
            updated_at: None,
        };
        store
            .create_task(boundary::TaskStoreCreateTaskRequest {
                team: team(),
                record: record.clone(),
            })
            .expect("create");

        let loaded = store
            .load_task(boundary::TaskStoreLoadTaskRequest {
                team: team(),
                task_id: task_id(),
            })
            .expect("load");
        assert_eq!(loaded.record, Some(record.clone()));

        let updated = store
            .update_task(boundary::TaskStoreUpdateTaskRequest {
                team: team(),
                task_id: task_id(),
                owner: None,
                state: Some("acknowledged".to_string()),
                metadata: None,
                append_message_keys: vec!["atm:test-2".to_string()],
            })
            .expect("update");
        assert_eq!(updated.record.state, "acknowledged");
        assert!(
            updated
                .record
                .linked_message_keys
                .iter()
                .any(|value| value == "atm:test-2")
        );

        let metadata = store
            .query_task_metadata(boundary::TaskStoreQueryTaskMetadataRequest {
                team: team(),
                task_id: Some(task_id()),
                message_key: None,
                state: Some("acknowledged".to_string()),
                limit: Some(10),
            })
            .expect("metadata query");
        assert_eq!(metadata.records.len(), 1);
    }

    #[test]
    fn roster_store_round_trips_roster_membership_and_health() {
        let (_tempdir, path) = temp_db();
        let assembly = assemble_boundary(&path).expect("assembly");
        let store = assembly.roster_store();

        let roster = TeamConfig {
            members: vec![AgentMember::with_name(agent())],
            extra: serde_json::Map::new(),
        };
        let replaced = store
            .replace_roster(boundary::RosterStoreReplaceRosterRequest {
                team: team(),
                roster: roster.clone(),
                source: Some("config.json".to_string()),
            })
            .expect("replace");
        assert!(replaced.replaced);

        let loaded = store
            .load_roster(boundary::RosterStoreLoadRosterRequest { team: team() })
            .expect("load roster");
        assert_eq!(loaded.roster, roster);

        let membership = store
            .query_membership(boundary::RosterStoreQueryMembershipRequest {
                team: team(),
                member: agent(),
            })
            .expect("membership");
        assert!(membership.is_member);

        let health = store
            .health_snapshot(boundary::RosterStoreHealthSnapshotRequest { team: team() })
            .expect("health");
        assert_eq!(health.snapshot.member_count, 1);
    }

    #[test]
    fn sqlite_boundary_enforces_wal_and_busy_timeout() {
        let (_tempdir, path) = temp_db();
        let assembly = assemble_boundary(&path).expect("assembly");

        assembly
            .mail_store
            .db
            .with_connection(|connection| {
                let journal_mode: String = connection
                    .pragma_query_value(None, "journal_mode", |row| row.get(0))
                    .map_err(|error| {
                        sqlite_error("failed to read sqlite journal_mode pragma", error)
                    })?;
                let busy_timeout_ms: i64 = connection
                    .pragma_query_value(None, "busy_timeout", |row| row.get(0))
                    .map_err(|error| {
                        sqlite_error("failed to read sqlite busy_timeout pragma", error)
                    })?;
                assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
                assert_eq!(busy_timeout_ms, 5000);
                Ok(())
            })
            .expect("pragma verification");
    }

    #[test]
    fn sqlite_transactions_roll_back_on_error() {
        let (_tempdir, path) = temp_db();
        let assembly = assemble_boundary(&path).expect("assembly");
        let db = assembly.mail_store.db.clone();
        let forced_failure_message = "force rollback";

        let result: Result<(), AtmError> = db.with_transaction(|transaction| {
            transaction
                .execute(
                    "INSERT INTO task_records(team, task_id, record_json) VALUES (?1, ?2, ?3);",
                    params![
                        team().as_str(),
                        task_id().as_str(),
                        "{\"state\":\"pending\"}"
                    ],
                )
                .map_err(|error| {
                    sqlite_error("failed to insert transactional rollback probe", error)
                })?;
            Err(AtmError::validation(forced_failure_message))
        });
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains(forced_failure_message)
        );

        let persisted: Option<String> = db
            .with_connection(|connection| {
                connection
                    .query_row(
                        "SELECT record_json FROM task_records WHERE team = ?1 AND task_id = ?2;",
                        params![team().as_str(), task_id().as_str()],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
                    .map_err(|error| {
                        sqlite_error("failed to verify transactional rollback probe", error)
                    })
            })
            .expect("rollback verification");
        assert!(persisted.is_none());
    }
}
