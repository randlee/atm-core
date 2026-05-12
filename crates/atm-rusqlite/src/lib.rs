#![forbid(unsafe_code)]

//! SQLite-backed adapter implementations for the Phase R store boundaries.

mod mailbox_metadata;
mod roster_store;
mod shared_db;
mod writer;

use atm_core::boundary;
use atm_core::error::AtmError;
use atm_core::protocol::RequestEnvelope;
use atm_core::types::{AgentName, IsoTimestamp, TeamName};
use mailbox_metadata::{
    MailboxMetadataCounts, MailboxMetadataRow, query_mailbox_metadata_counts,
    query_mailbox_metadata_rows,
};
#[cfg(test)]
use rusqlite::Error as RusqliteError;
use rusqlite::{Connection, OptionalExtension, params};
use shared_db::{SharedDb, SharedDbTarget, deserialize_json, serialize_json, sqlite_error};
use std::net::SocketAddr;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug)]
struct SqliteRosterStore {
    db: Arc<SharedDb>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RemoteReplayStateRecord {
    pub team: TeamName,
    pub agent: AgentName,
    pub message_key: boundary::MessageKey,
    pub peer_addr: SocketAddr,
    pub request: RequestEnvelope,
    pub recorded_at: IsoTimestamp,
    pub expires_at: IsoTimestamp,
    #[serde(default)]
    pub attempt_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_attempt_at: Option<IsoTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl SqliteRosterStore {
    fn new(db: Arc<SharedDb>) -> Self {
        Self { db }
    }
}

/// Internal assembly root for Phase R SQLite-backed boundary implementations.
#[derive(Debug, Clone)]
pub struct SqliteBoundaryAssembly {
    mail_store: Arc<SqliteMailStore>,
    task_store: Arc<SqliteTaskStore>,
    roster_store: Arc<SqliteRosterStore>,
}

impl SqliteBoundaryAssembly {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, AtmError> {
        let db = Arc::new(SharedDb::open(path)?);
        Ok(Self {
            mail_store: Arc::new(SqliteMailStore::new(db.clone())),
            task_store: Arc::new(SqliteTaskStore::new(db.clone())),
            roster_store: Arc::new(SqliteRosterStore::new(db)),
        })
    }

    pub fn default_production() -> Result<Self, AtmError> {
        Self::new(SharedDb::production_path()?)
    }

    #[cfg(test)]
    fn in_memory_for_test() -> Result<Self, AtmError> {
        let db = Arc::new(SharedDb::open_in_memory()?);
        Ok(Self {
            mail_store: Arc::new(SqliteMailStore::new(db.clone())),
            task_store: Arc::new(SqliteTaskStore::new(db.clone())),
            roster_store: Arc::new(SqliteRosterStore::new(db)),
        })
    }

    pub fn mail_store(&self) -> &dyn boundary::MailStore {
        self.mail_store.as_ref()
    }

    pub fn task_store(&self) -> &dyn boundary::TaskStore {
        self.task_store.as_ref()
    }

    pub fn roster_store(&self) -> &dyn boundary::RosterStore {
        self.roster_store.as_ref()
    }

    pub fn checkpoint_wal(&self) -> Result<(), AtmError> {
        self.mail_store.db.checkpoint_wal()
    }

    pub fn query_mailbox_metadata_rows(
        &self,
        team: &TeamName,
        agent: &AgentName,
        limit: usize,
    ) -> Result<Vec<MailboxMetadataRow>, AtmError> {
        query_mailbox_metadata_rows(&self.mail_store.db, team, agent, limit)
    }

    pub fn query_mailbox_metadata_counts(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<MailboxMetadataCounts, AtmError> {
        query_mailbox_metadata_counts(&self.mail_store.db, team, agent)
    }

    pub fn record_remote_replay_state(
        &self,
        record: RemoteReplayStateRecord,
    ) -> Result<(), AtmError> {
        let state_json = serialize_json(&record, "daemon remote replay state")?;
        self.mail_store.db.with_transaction(|transaction| {
            transaction
                .execute(
                    "INSERT INTO daemon_remote_replay_states(team, agent, message_key, state_json)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(team, agent, message_key) DO UPDATE SET
                       state_json = excluded.state_json;",
                    params![
                        record.team.as_str(),
                        record.agent.as_str(),
                        record.message_key.as_ref(),
                        state_json,
                    ],
                )
                .map_err(|error| {
                    self.mail_store
                        .db
                        .error("failed to record daemon remote replay state", error)
                })?;
            Ok(())
        })
    }

    pub fn load_remote_replay_states(&self) -> Result<Vec<RemoteReplayStateRecord>, AtmError> {
        self.mail_store.db.with_connection(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT state_json
                     FROM daemon_remote_replay_states
                     ORDER BY team, agent, message_key;",
                )
                .map_err(|error| {
                    self.mail_store
                        .db
                        .error("failed to prepare daemon remote replay query", error)
                })?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|error| {
                    self.mail_store
                        .db
                        .error("failed to read daemon remote replay rows", error)
                })?;
            let mut records = Vec::new();
            for row in rows {
                let state_json = row.map_err(|error| {
                    self.mail_store
                        .db
                        .error("failed to decode daemon remote replay row", error)
                })?;
                records.push(deserialize_json(&state_json, "daemon remote replay state")?);
            }
            Ok(records)
        })
    }

    pub fn delete_remote_replay_state(
        &self,
        team: &TeamName,
        agent: &AgentName,
        message_key: &boundary::MessageKey,
    ) -> Result<(), AtmError> {
        self.mail_store.db.with_transaction(|transaction| {
            transaction
                .execute(
                    "DELETE FROM daemon_remote_replay_states
                     WHERE team = ?1 AND agent = ?2 AND message_key = ?3;",
                    params![team.as_str(), agent.as_str(), message_key.as_ref(),],
                )
                .map_err(|error| {
                    self.mail_store
                        .db
                        .error("failed to delete daemon remote replay state", error)
                })?;
            Ok(())
        })
    }

    pub fn purge_expired_remote_replay_states(&self, now: IsoTimestamp) -> Result<usize, AtmError> {
        let now = now.into_inner().to_rfc3339();
        self.mail_store.db.with_transaction(|transaction| {
            // Accepted risk: remote replay rows expire slowly and this purge
            // runs on a bounded maintenance path, so one unbounded DELETE is
            // acceptable until replay-volume data proves otherwise.
            transaction
                .execute(
                    "DELETE FROM daemon_remote_replay_states
                     WHERE json_extract(state_json, '$.expires_at') <= ?1;",
                    params![now],
                )
                .map_err(|error| {
                    self.mail_store
                        .db
                        .error("failed to purge expired daemon remote replay state", error)
                })
        })
    }
}

pub fn assemble_boundary(path: impl AsRef<Path>) -> Result<SqliteBoundaryAssembly, AtmError> {
    SqliteBoundaryAssembly::new(path)
}

pub fn assemble_default_boundary() -> Result<SqliteBoundaryAssembly, AtmError> {
    SqliteBoundaryAssembly::default_production()
}

#[derive(Debug)]
struct SqliteMailStore {
    db: Arc<SharedDb>,
}

#[derive(Debug, Clone)]
struct StoredMailMessageState {
    // SQLite stores booleans as INTEGER 0/1 values; reads normalize that
    // column back into Rust `bool` at the boundary edge.
    read: bool,
    pending_ack_at: Option<IsoTimestamp>,
    acknowledged_at: Option<IsoTimestamp>,
    expires_at: Option<IsoTimestamp>,
    deleted_at: Option<IsoTimestamp>,
    updated_at: Option<IsoTimestamp>,
}

impl SqliteMailStore {
    fn new(db: Arc<SharedDb>) -> Self {
        Self { db }
    }

    fn parse_optional_timestamp(
        raw: Option<String>,
        field_name: &str,
    ) -> Result<Option<IsoTimestamp>, AtmError> {
        raw.map(|value| value.parse::<chrono::DateTime<chrono::Utc>>())
            .transpose()
            .map_err(|error| {
                AtmError::validation(format!(
                    "failed to parse mail-store {field_name} timestamp: {error}"
                ))
                .with_recovery(
                    "Repair the sqlite-backed mail-store row or rewrite it through the owning boundary.",
                )
                .with_source(error)
            })
            .map(|value| value.map(IsoTimestamp::from_datetime))
    }

    fn load_message_state(
        &self,
        connection: &Connection,
        team: &TeamName,
        agent: &AgentName,
        message_key: &boundary::MessageKey,
    ) -> Result<Option<StoredMailMessageState>, AtmError> {
        connection
            .query_row(
                "SELECT read, pending_ack_at, acknowledged_at, expires_at, deleted_at, updated_at
                 FROM mail_message_states
                 WHERE team = ?1 AND agent = ?2 AND message_key = ?3;",
                params![team.as_str(), agent.as_str(), message_key.as_ref()],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| {
                self.db
                    .error("failed to load unified mail message state", error)
            })?
            .map(
                |(read, pending_ack_at, acknowledged_at, expires_at, deleted_at, updated_at)| {
                    Ok(StoredMailMessageState {
                        read: read != 0,
                        pending_ack_at: Self::parse_optional_timestamp(
                            pending_ack_at,
                            "pending_ack_at",
                        )?,
                        acknowledged_at: Self::parse_optional_timestamp(
                            acknowledged_at,
                            "acknowledged_at",
                        )?,
                        expires_at: Self::parse_optional_timestamp(expires_at, "expires_at")?,
                        deleted_at: Self::parse_optional_timestamp(deleted_at, "deleted_at")?,
                        updated_at: Self::parse_optional_timestamp(updated_at, "updated_at")?,
                    })
                },
            )
            .transpose()
    }

    fn apply_loaded_state(
        mut envelope: atm_core::schema::MessageEnvelope,
        state: Option<&StoredMailMessageState>,
    ) -> atm_core::schema::MessageEnvelope {
        if let Some(state) = state {
            envelope.read = state.read;
            envelope.pending_ack_at = state.pending_ack_at;
            envelope.acknowledged_at = state.acknowledged_at;
            envelope.expires_at = state.expires_at;
        }
        envelope
    }
}

impl boundary::sealed::Sealed for SqliteMailStore {}

impl boundary::MailStore for SqliteMailStore {
    fn bootstrap(
        &self,
        request: boundary::MailStoreBootstrapRequest,
    ) -> Result<boundary::MailStoreBootstrapResponse, AtmError> {
        self.db.with_connection(|_| Ok(()))?;
        Ok(boundary::MailStoreBootstrapResponse {
            team: request.team,
            bootstrapped: true,
            opened: true,
        })
    }

    fn run_transaction(
        &self,
        request: boundary::MailStoreTransactionRequest,
    ) -> Result<boundary::MailStoreTransactionResponse, AtmError> {
        if !request.requested_operations.is_empty() {
            return Err(AtmError::config(
                "sqlite mail-store ad-hoc requested_operations are not implemented",
            )
            .with_recovery(
                "Use the typed MailStore methods instead of run_transaction(requested_operations) until the generic transaction payload is specified.",
            ));
        }

        self.db.with_transaction(|_transaction| {
            Ok(boundary::MailStoreTransactionResponse {
                team: request.team,
                committed: true,
                operations_executed: 0,
            })
        })
    }

    fn upsert_message(
        &self,
        request: boundary::MailStoreUpsertMessageRequest,
    ) -> Result<boundary::MailStoreUpsertMessageResponse, AtmError> {
        self.db.submit_upsert_message(request)
    }

    fn load_message(
        &self,
        request: boundary::MailStoreLoadMessageRequest,
    ) -> Result<boundary::MailStoreLoadMessageResponse, AtmError> {
        let record = self.db.with_connection(|connection| {
            let loaded = connection
                .query_row(
                    "SELECT envelope_json, imported_from, recorded_at
                     FROM mail_messages
                     WHERE team = ?1 AND agent = ?2 AND message_key = ?3;",
                    params![
                        request.team.as_str(),
                        request.agent.as_str(),
                        request.message_key.as_ref()
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
                .map_err(|error| self.db.error("failed to load mail-store message", error))?;
            let state = self.load_message_state(
                connection,
                &request.team,
                &request.agent,
                &request.message_key,
            )?;
            Ok(loaded.map(|row| (row, state)))
        })?;

        let record = if let Some(((envelope_json, imported_from, recorded_at), state)) = record {
            let envelope = deserialize_json(&envelope_json, "mail-store envelope")?;
            let envelope = Self::apply_loaded_state(envelope, state.as_ref());
            let recorded_at = Self::parse_optional_timestamp(recorded_at, "recorded_at")?;
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

    fn upsert_message_state(
        &self,
        request: boundary::UpsertMailMessageStateRequest,
    ) -> Result<boundary::UpsertMailMessageStateResponse, AtmError> {
        self.db.submit_upsert_message_state(request)
    }

    fn load_message_state(
        &self,
        request: boundary::LoadMailMessageStateRequest,
    ) -> Result<boundary::LoadMailMessageStateResponse, AtmError> {
        let state = self.db.with_connection(|connection| {
            self.load_message_state(
                connection,
                &request.team,
                &request.agent,
                &request.message_key,
            )
        })?;
        let state = state.map(|state| boundary::MailMessageState {
            team: request.team.clone(),
            agent: request.agent.clone(),
            actor: request.actor.clone(),
            message_key: request.message_key.clone(),
            read: state.read,
            pending_ack_at: state.pending_ack_at,
            acknowledged_at: state.acknowledged_at,
            expires_at: state.expires_at,
            deleted_at: state.deleted_at,
            updated_at: state.updated_at,
        });

        Ok(boundary::LoadMailMessageStateResponse { state })
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
                        request.source.as_str(),
                        state_json,
                    ],
                )
                .map_err(|error| {
                    self.db
                        .error("failed to record mail-store ingest replay state", error)
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
                        request.source.as_str()
                    ],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(|error| {
                    self.db
                        .error("failed to load mail-store ingest replay state", error)
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
        let (total_messages, pending_ack_messages, read_messages, latest_message_timestamp) =
            self.db.with_connection(|connection| {
                connection
                    .query_row(
                        "SELECT
                             COUNT(*),
                             (
                                 SELECT COUNT(*)
                                 FROM mail_message_states
                                 WHERE team = ?1
                                   AND agent = ?2
                                   AND pending_ack_at IS NOT NULL
                                   AND acknowledged_at IS NULL
                             ),
                             (
                                 SELECT COUNT(*)
                                 FROM mail_message_states
                                 WHERE team = ?1
                                   AND agent = ?2
                                   AND read = 1
                             ),
                             MAX(COALESCE(recorded_at, message_at))
                         FROM mail_messages
                         WHERE team = ?1 AND agent = ?2;",
                        params![request.team.as_str(), request.agent.as_str()],
                        |row| {
                            Ok((
                                row.get::<_, i64>(0)? as u64,
                                row.get::<_, i64>(1)? as u64,
                                row.get::<_, i64>(2)? as u64,
                                row.get::<_, Option<String>>(3)?,
                            ))
                        },
                    )
                    .map_err(|error| {
                        self.db
                            .error("failed to query mail-store health summary", error)
                    })
            })?;

        let latest_message_timestamp =
            Self::parse_optional_timestamp(latest_message_timestamp, "health latest_message")?;

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
        target: &SharedDbTarget,
        team: &TeamName,
        task_id: &atm_core::types::TaskId,
    ) -> Result<Option<boundary::TaskStoreTaskRecord>, AtmError> {
        let record_json = connection
            .query_row(
                "SELECT record_json FROM tasks WHERE team = ?1 AND task_id = ?2;",
                params![team.as_str(), task_id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| sqlite_error(target, "failed to load task-store record", error))?;

        record_json
            .map(|value| deserialize_json(&value, "task-store record"))
            .transpose()
    }

    fn save_record_in_connection(
        connection: &Connection,
        target: &SharedDbTarget,
        record: &boundary::TaskStoreTaskRecord,
    ) -> Result<(), AtmError> {
        let record_json = serialize_json(record, "task-store record")?;
        connection
            .execute(
                "INSERT INTO tasks(team, task_id, record_json)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(team, task_id) DO UPDATE SET
                   record_json = excluded.record_json;",
                params![record.team.as_str(), record.task_id.as_str(), record_json],
            )
            .map_err(|error| sqlite_error(target, "failed to save task-store record", error))?;
        Ok(())
    }

    fn load_record(
        &self,
        team: &TeamName,
        task_id: &atm_core::types::TaskId,
    ) -> Result<Option<boundary::TaskStoreTaskRecord>, AtmError> {
        self.db.with_connection(|connection| {
            Self::load_record_in_connection(connection, self.db.target(), team, task_id)
        })
    }
}

impl boundary::sealed::Sealed for SqliteTaskStore {}

impl boundary::TaskStore for SqliteTaskStore {
    fn create_task(
        &self,
        request: boundary::TaskStoreCreateTaskRequest,
    ) -> Result<boundary::TaskStoreCreateTaskResponse, AtmError> {
        self.db.with_transaction(|transaction| {
            Self::save_record_in_connection(transaction, self.db.target(), &request.record)?;
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
            let mut record = Self::load_record_in_connection(
                transaction,
                self.db.target(),
                &request.team,
                &request.task_id,
            )?
            .ok_or_else(|| {
                AtmError::validation(format!(
                    "task-store update failed because task {} does not exist in team {}",
                    request.task_id, request.team
                ))
                .with_recovery("Create the task through TaskStore::create_task before updating it.")
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
            Self::save_record_in_connection(transaction, self.db.target(), &record)?;
            Ok(boundary::TaskStoreUpdateTaskResponse { record })
        })
    }

    fn attach_message_link(
        &self,
        request: boundary::TaskStoreAttachMessageLinkRequest,
    ) -> Result<boundary::TaskStoreAttachMessageLinkResponse, AtmError> {
        self.db.with_transaction(|transaction| {
            let mut record = Self::load_record_in_connection(
                transaction,
                self.db.target(),
                &request.team,
                &request.task_id,
            )?
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
            Self::save_record_in_connection(transaction, self.db.target(), &record)?;
            Ok(boundary::TaskStoreAttachMessageLinkResponse { record })
        })
    }

    fn detach_message_link(
        &self,
        request: boundary::TaskStoreDetachMessageLinkRequest,
    ) -> Result<boundary::TaskStoreDetachMessageLinkResponse, AtmError> {
        self.db.with_transaction(|transaction| {
            let mut record = Self::load_record_in_connection(
                transaction,
                self.db.target(),
                &request.team,
                &request.task_id,
            )?
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
            Self::save_record_in_connection(transaction, self.db.target(), &record)?;
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
            let mut record = Self::load_record_in_connection(
                transaction,
                self.db.target(),
                &request.team,
                &request.task_id,
            )?
                .ok_or_else(|| {
                    AtmError::validation(format!(
                        "task-store ack transition failed because task {} does not exist in team {}",
                        request.task_id, request.team
                    ))
                    .with_recovery("Create the task through TaskStore::create_task before recording ack transitions.")
                })?;
            record.metadata.fields.insert(
                "last_ack_transition".to_string(),
                request.transition.to_string(),
            );
            record
                .metadata
                .fields
                .insert("last_ack_actor".to_string(), request.actor.to_string());
            record.metadata.fields.insert(
                "last_ack_message_key".to_string(),
                request.message_key.to_string(),
            );
            record.metadata.fields.insert(
                "last_ack_at".to_string(),
                request.transitioned_at.into_inner().to_rfc3339(),
            );
            record.updated_at = Some(atm_core::types::IsoTimestamp::now());
            Self::save_record_in_connection(transaction, self.db.target(), &record)?;

            let next_index: i64 = transaction
                .query_row(
                    "SELECT COALESCE(MAX(transition_index), -1) + 1
                     FROM task_ack_transitions
                     WHERE team = ?1 AND task_id = ?2;",
                    params![record.team.as_str(), record.task_id.as_str()],
                    |row| row.get(0),
                )
                .map_err(|error| self.db.error("failed to query next task ack transition index", error))?;
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
                .map_err(|error| self.db.error("failed to persist task ack transition", error))?;
            Ok(boundary::TaskStoreRecordAckTransitionResponse { record })
        })
    }

    fn query_task_metadata(
        &self,
        request: boundary::TaskStoreQueryTaskMetadataRequest,
    ) -> Result<boundary::TaskStoreQueryTaskMetadataResponse, AtmError> {
        let rows = self.db.with_connection(|connection| {
            let mut statement = connection
                .prepare("SELECT record_json FROM tasks WHERE team = ?1 ORDER BY task_id;")
                .map_err(|error| {
                    self.db
                        .error("failed to prepare task-store metadata query", error)
                })?;
            let mapped = statement
                .query_map(params![request.team.as_str()], |row| {
                    row.get::<_, String>(0)
                })
                .map_err(|error| {
                    self.db
                        .error("failed to execute task-store metadata query", error)
                })?;
            let mut rows = Vec::new();
            for row in mapped {
                rows.push(row.map_err(|error| {
                    self.db
                        .error("failed to read task-store metadata row", error)
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
    use atm_core::MessageKey;
    use atm_core::boundary::MailStore as _;
    use atm_core::doctor::DoctorQuery;
    use atm_core::protocol::RequestEnvelope;
    use atm_core::schema::TeamConfig;
    use atm_core::schema::{AgentMember, MessageEnvelope};
    use atm_core::types::{AgentName, IsoTimestamp, TaskId, TeamName};
    use tempfile::TempDir;

    fn temp_disk_db() -> (TempDir, PathBuf) {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("phase-r.sqlite3");
        (tempdir, path)
    }

    fn in_memory_assembly() -> SqliteBoundaryAssembly {
        SqliteBoundaryAssembly::in_memory_for_test().expect("in-memory assembly")
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

    fn message_key(value: &str) -> MessageKey {
        MessageKey::new(value).expect("message key")
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
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: Some(task_id()),
            extra: serde_json::Map::new(),
        }
    }

    fn envelope_at(
        text: &str,
        timestamp: chrono::DateTime<chrono::Utc>,
        read: bool,
        pending_ack: bool,
        task_id_value: Option<TaskId>,
    ) -> MessageEnvelope {
        MessageEnvelope {
            from: actor(),
            text: text.to_string(),
            timestamp: IsoTimestamp::from_datetime(timestamp),
            read,
            source_team: Some(team()),
            summary: Some(format!("summary: {text}")),
            message_id: Some(atm_core::schema::AtmMessageId::new()),
            pending_ack_at: pending_ack.then_some(IsoTimestamp::from_datetime(timestamp)),
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: task_id_value,
            extra: serde_json::Map::new(),
        }
    }

    #[test]
    fn default_production_path_is_host_scoped_mail_db() {
        let tempdir = TempDir::new().expect("tempdir");
        assert_eq!(
            SharedDb::production_path_from_home(tempdir.path()),
            tempdir.path().join(".atm").join("db").join("mail.db")
        );
    }

    #[test]
    fn sqlite_error_maps_constraint_busy_and_open_failures() {
        use atm_core::error_codes::AtmErrorCode;
        use rusqlite::ffi::{Error, ErrorCode};
        let target = SharedDbTarget::Path(std::env::temp_dir().join("phase-r.sqlite3"));

        let constraint = sqlite_error(
            &target,
            "constraint failed",
            RusqliteError::SqliteFailure(
                Error {
                    code: ErrorCode::ConstraintViolation,
                    extended_code: 0,
                },
                None,
            ),
        );
        assert_eq!(constraint.code, AtmErrorCode::MessageValidationFailed);

        let busy = sqlite_error(
            &target,
            "database busy",
            RusqliteError::SqliteFailure(
                Error {
                    code: ErrorCode::DatabaseBusy,
                    extended_code: 0,
                },
                None,
            ),
        );
        assert_eq!(busy.code, AtmErrorCode::MailboxLockTimeout);

        let open = sqlite_error(
            &target,
            "open failed",
            RusqliteError::SqliteFailure(
                Error {
                    code: ErrorCode::CannotOpen,
                    extended_code: 0,
                },
                None,
            ),
        );
        assert_eq!(open.code, AtmErrorCode::MailboxWriteFailed);

        let corrupt = sqlite_error(
            &target,
            "corrupt failed",
            RusqliteError::SqliteFailure(
                Error {
                    code: ErrorCode::DatabaseCorrupt,
                    extended_code: 0,
                },
                None,
            ),
        );
        assert_eq!(corrupt.code, AtmErrorCode::MailboxReadFailed);

        let read_only = sqlite_error(
            &target,
            "read only failed",
            RusqliteError::SqliteFailure(
                Error {
                    code: ErrorCode::ReadOnly,
                    extended_code: 0,
                },
                None,
            ),
        );
        assert_eq!(read_only.code, AtmErrorCode::MailboxWriteFailed);

        let in_memory_busy = sqlite_error(
            &SharedDbTarget::InMemory {
                uri: "file:atm-rusqlite-test?mode=memory&cache=shared".to_string(),
            },
            "database busy",
            RusqliteError::SqliteFailure(
                Error {
                    code: ErrorCode::DatabaseBusy,
                    extended_code: 0,
                },
                None,
            ),
        );
        assert_eq!(in_memory_busy.code, AtmErrorCode::MailboxLockFailed);
    }

    #[test]
    fn in_memory_assembly_does_not_touch_production_root() {
        let tempdir = TempDir::new().expect("tempdir");
        let production_path = SharedDb::production_path_from_home(tempdir.path());

        let _assembly = in_memory_assembly();

        assert!(!production_path.exists());
    }

    #[test]
    fn on_disk_boundary_reopens_existing_database() {
        let (_tempdir, path) = temp_disk_db();
        let first = assemble_boundary(&path).expect("first assembly");
        first
            .mail_store()
            .upsert_message(boundary::MailStoreUpsertMessageRequest {
                record: boundary::MailStoreMessageRecord {
                    team: team(),
                    agent: agent(),
                    message_key: message_key("atm:test-reopen"),
                    envelope: envelope(),
                    imported_from: None,
                    recorded_at: Some(IsoTimestamp::now()),
                },
            })
            .expect("write");

        let second = assemble_boundary(&path).expect("second assembly");
        let loaded = second
            .mail_store()
            .load_message(boundary::MailStoreLoadMessageRequest {
                team: team(),
                agent: agent(),
                message_key: message_key("atm:test-reopen"),
            })
            .expect("reload");

        assert!(loaded.record.is_some());
    }

    #[test]
    fn mail_store_round_trips_message_visibility_and_health() {
        let assembly = in_memory_assembly();
        let store = assembly.mail_store();
        let expires_at =
            IsoTimestamp::from_datetime(chrono::Utc::now() + chrono::Duration::minutes(30));

        store
            .bootstrap(boundary::MailStoreBootstrapRequest {
                team_dir: std::env::temp_dir(),
                team: team(),
                team_config: None,
            })
            .expect("bootstrap");

        let record = boundary::MailStoreMessageRecord {
            team: team(),
            agent: agent(),
            message_key: message_key("atm:test-1"),
            envelope: MessageEnvelope {
                expires_at: Some(expires_at),
                ..envelope()
            },
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
                message_key: message_key("atm:test-1"),
            })
            .expect("load");
        assert_eq!(loaded.record, Some(record.clone()));

        let visibility = boundary::MailMessageState {
            team: team(),
            agent: agent(),
            actor: actor(),
            message_key: message_key("atm:test-1"),
            read: true,
            pending_ack_at: record.envelope.pending_ack_at,
            acknowledged_at: None,
            expires_at: record.envelope.expires_at,
            deleted_at: None,
            updated_at: Some(IsoTimestamp::now()),
        };
        store
            .upsert_message_state(boundary::UpsertMailMessageStateRequest {
                team: team(),
                agent: agent(),
                actor: actor(),
                state: visibility.clone(),
            })
            .expect("upsert visibility");
        let loaded_visibility = store
            .load_message_state(boundary::LoadMailMessageStateRequest {
                team: team(),
                agent: agent(),
                actor: actor(),
                message_key: message_key("atm:test-1"),
            })
            .expect("load visibility");
        assert_eq!(loaded_visibility.state, Some(visibility));
        assert_eq!(
            loaded
                .record
                .as_ref()
                .and_then(|record| record.envelope.expires_at),
            Some(expires_at)
        );

        let health = store
            .health_snapshot(boundary::MailStoreHealthSnapshotRequest {
                team: team(),
                agent: agent(),
            })
            .expect("health");
        assert_eq!(health.snapshot.total_messages, 1);
        assert_eq!(health.snapshot.pending_ack_messages, 1);
        assert_eq!(health.snapshot.read_messages, 1);

        assembly
            .mail_store
            .db
            .with_connection(|connection| {
                let envelope_json: String = connection
                    .query_row(
                        "SELECT envelope_json
                         FROM mail_messages
                         WHERE team = ?1 AND agent = ?2 AND message_key = ?3;",
                        params![team().as_str(), agent().as_str(), "atm:test-1"],
                        |row| row.get(0),
                    )
                    .map_err(|error| {
                        assembly
                            .mail_store
                            .db
                            .error("failed to inspect stored mail envelope", error)
                    })?;
                let stored: MessageEnvelope =
                    deserialize_json(&envelope_json, "stored mail envelope")?;
                assert!(!stored.read);
                assert!(stored.pending_ack_at.is_none());
                assert!(stored.acknowledged_at.is_none());
                assert!(stored.expires_at.is_none());
                Ok(())
            })
            .expect("content row strips mutable state");
    }

    #[test]
    fn bounded_mailbox_metadata_queries_return_limited_rows_and_counts() {
        let assembly = in_memory_assembly();
        let team = team();
        let agent = agent();
        let now = chrono::Utc::now();

        for (index, (text, read, pending_ack, task_id_value)) in [
            ("older", false, false, None),
            ("middle", true, true, Some(task_id())),
            ("newest", false, true, None),
        ]
        .into_iter()
        .enumerate()
        {
            let record = boundary::MailStoreMessageRecord {
                team: team.clone(),
                agent: agent.clone(),
                message_key: message_key(&format!("atm:test-{index}")),
                envelope: envelope_at(
                    text,
                    now + chrono::Duration::seconds(index as i64),
                    read,
                    pending_ack,
                    task_id_value,
                ),
                imported_from: None,
                recorded_at: Some(IsoTimestamp::from_datetime(
                    now + chrono::Duration::seconds(index as i64),
                )),
            };
            assembly
                .mail_store()
                .upsert_message(boundary::MailStoreUpsertMessageRequest { record })
                .expect("upsert metadata row");
        }

        let rows = assembly
            .query_mailbox_metadata_rows(&team, &agent, 2)
            .expect("bounded metadata rows");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].summary.as_deref(), Some("summary: newest"));
        assert_eq!(rows[1].summary.as_deref(), Some("summary: middle"));
        assert!(rows[0].pending_ack);
        assert!(rows[1].read);
        assert_eq!(
            rows[1].task_id.as_ref().map(TaskId::as_str),
            Some("task-123")
        );

        let counts = assembly
            .query_mailbox_metadata_counts(&team, &agent)
            .expect("bounded metadata counts");
        assert_eq!(
            counts,
            MailboxMetadataCounts {
                total_messages: 3,
                unread_messages: 2,
                pending_ack_messages: 2,
            }
        );
    }

    #[test]
    fn bounded_mailbox_metadata_queries_exclude_deleted_and_expired_rows() {
        let assembly = in_memory_assembly();
        let team = team();
        let agent = agent();
        let now = chrono::Utc::now();

        for (index, text) in ["active", "deleted", "expired"].into_iter().enumerate() {
            let record = boundary::MailStoreMessageRecord {
                team: team.clone(),
                agent: agent.clone(),
                message_key: message_key(&format!("atm:state-{index}")),
                envelope: envelope_at(
                    text,
                    now + chrono::Duration::seconds(index as i64),
                    false,
                    false,
                    None,
                ),
                imported_from: None,
                recorded_at: Some(IsoTimestamp::from_datetime(
                    now + chrono::Duration::seconds(index as i64),
                )),
            };
            assembly
                .mail_store()
                .upsert_message(boundary::MailStoreUpsertMessageRequest { record })
                .expect("upsert state row");
        }

        assembly
            .mail_store()
            .upsert_message_state(boundary::UpsertMailMessageStateRequest {
                team: team.clone(),
                agent: agent.clone(),
                actor: actor(),
                state: boundary::MailMessageState {
                    team: team.clone(),
                    agent: agent.clone(),
                    actor: actor(),
                    message_key: message_key("atm:state-1"),
                    read: true,
                    pending_ack_at: None,
                    acknowledged_at: None,
                    expires_at: None,
                    deleted_at: Some(IsoTimestamp::from_datetime(
                        now + chrono::Duration::minutes(1),
                    )),
                    updated_at: Some(IsoTimestamp::from_datetime(
                        now + chrono::Duration::minutes(1),
                    )),
                },
            })
            .expect("mark deleted");

        assembly
            .mail_store()
            .upsert_message_state(boundary::UpsertMailMessageStateRequest {
                team: team.clone(),
                agent: agent.clone(),
                actor: actor(),
                state: boundary::MailMessageState {
                    team: team.clone(),
                    agent: agent.clone(),
                    actor: actor(),
                    message_key: message_key("atm:state-2"),
                    read: false,
                    pending_ack_at: None,
                    acknowledged_at: None,
                    expires_at: Some(IsoTimestamp::from_datetime(
                        now - chrono::Duration::minutes(1),
                    )),
                    deleted_at: None,
                    updated_at: Some(IsoTimestamp::from_datetime(
                        now + chrono::Duration::minutes(1),
                    )),
                },
            })
            .expect("mark expired");

        let rows = assembly
            .query_mailbox_metadata_rows(&team, &agent, 10)
            .expect("bounded metadata rows");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].summary.as_deref(), Some("summary: active"));

        let counts = assembly
            .query_mailbox_metadata_counts(&team, &agent)
            .expect("bounded metadata counts");
        assert_eq!(
            counts,
            MailboxMetadataCounts {
                total_messages: 1,
                unread_messages: 1,
                pending_ack_messages: 0,
            }
        );
    }

    #[test]
    fn mail_message_state_json_round_trip_preserves_all_optional_fields() {
        let state = boundary::MailMessageState {
            team: team(),
            agent: agent(),
            actor: actor(),
            message_key: message_key("atm:serde-state"),
            read: true,
            pending_ack_at: Some(IsoTimestamp::from_datetime(
                chrono::Utc::now() + chrono::Duration::minutes(1),
            )),
            acknowledged_at: Some(IsoTimestamp::from_datetime(
                chrono::Utc::now() + chrono::Duration::minutes(2),
            )),
            expires_at: Some(IsoTimestamp::from_datetime(
                chrono::Utc::now() + chrono::Duration::minutes(3),
            )),
            deleted_at: Some(IsoTimestamp::from_datetime(
                chrono::Utc::now() + chrono::Duration::minutes(4),
            )),
            updated_at: Some(IsoTimestamp::from_datetime(
                chrono::Utc::now() + chrono::Duration::minutes(5),
            )),
        };

        let json = serde_json::to_string(&state).expect("serialize state");
        let decoded: boundary::MailMessageState =
            serde_json::from_str(&json).expect("deserialize state");
        assert_eq!(decoded, state);
    }

    #[test]
    fn load_message_state_keeps_deleted_rows_visible_for_admin_paths() {
        let assembly = in_memory_assembly();
        let store = assembly.mail_store();
        let now = chrono::Utc::now();

        store
            .upsert_message(boundary::MailStoreUpsertMessageRequest {
                record: boundary::MailStoreMessageRecord {
                    team: team(),
                    agent: agent(),
                    message_key: message_key("atm:deleted-state"),
                    envelope: envelope(),
                    imported_from: None,
                    recorded_at: Some(IsoTimestamp::from_datetime(now)),
                },
            })
            .expect("upsert message");

        let deleted_state = boundary::MailMessageState {
            team: team(),
            agent: agent(),
            actor: actor(),
            message_key: message_key("atm:deleted-state"),
            read: true,
            pending_ack_at: None,
            acknowledged_at: None,
            expires_at: None,
            deleted_at: Some(IsoTimestamp::from_datetime(
                now + chrono::Duration::minutes(1),
            )),
            updated_at: Some(IsoTimestamp::from_datetime(
                now + chrono::Duration::minutes(1),
            )),
        };

        store
            .upsert_message_state(boundary::UpsertMailMessageStateRequest {
                team: team(),
                agent: agent(),
                actor: actor(),
                state: deleted_state.clone(),
            })
            .expect("upsert deleted state");

        let loaded = store
            .load_message_state(boundary::LoadMailMessageStateRequest {
                team: team(),
                agent: agent(),
                actor: actor(),
                message_key: message_key("atm:deleted-state"),
            })
            .expect("load deleted state");

        assert_eq!(loaded.state, Some(deleted_state));
    }

    #[test]
    fn sqlite_schema_tracks_unified_message_state_and_roster_runtime_columns() {
        let assembly = in_memory_assembly();

        assembly
            .mail_store
            .db
            .with_connection(|connection| {
                let state_table_exists: i64 = connection
                    .query_row(
                        "SELECT COUNT(1) FROM sqlite_master WHERE type = 'table' AND name = 'mail_message_states';",
                        [],
                        |row| row.get(0),
                    )
                    .map_err(|error| assembly.mail_store.db.error("failed to inspect mail_message_states table", error))?;
                assert_eq!(state_table_exists, 1);

                let mut roster_columns = connection
                    .prepare("PRAGMA table_info(team_roster);")
                    .map_err(|error| assembly.mail_store.db.error("failed to inspect roster schema", error))?;
                let roster_columns = roster_columns
                    .query_map([], |row| row.get::<_, String>(1))
                    .map_err(|error| assembly.mail_store.db.error("failed to enumerate roster columns", error))?;
                let collected = roster_columns
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| assembly.mail_store.db.error("failed to read roster column metadata", error))?;
                assert!(collected.iter().any(|column| column == "recipient_pane_id"));
                assert!(collected.iter().any(|column| column == "pid"));

                let mut message_columns = connection
                    .prepare("PRAGMA table_info(mail_messages);")
                    .map_err(|error| assembly.mail_store.db.error("failed to inspect mail schema", error))?;
                let message_columns = message_columns
                    .query_map([], |row| row.get::<_, String>(1))
                    .map_err(|error| assembly.mail_store.db.error("failed to enumerate mail columns", error))?;
                let collected = message_columns
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| assembly.mail_store.db.error("failed to read mail column metadata", error))?;
                assert!(collected.iter().any(|column| column == "from_agent"));
                assert!(collected.iter().any(|column| column == "message_text"));
                assert!(collected.iter().any(|column| column == "message_at"));
                assert!(collected.iter().any(|column| column == "message_id"));
                assert!(!collected.iter().any(|column| column == "expires_at"));

                let mut state_columns = connection
                    .prepare("PRAGMA table_info(mail_message_states);")
                    .map_err(|error| assembly.mail_store.db.error("failed to inspect unified state schema", error))?;
                let state_columns = state_columns
                    .query_map([], |row| row.get::<_, String>(1))
                    .map_err(|error| assembly.mail_store.db.error("failed to enumerate unified state columns", error))?;
                let collected = state_columns
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| assembly.mail_store.db.error("failed to read unified state column metadata", error))?;
                assert!(collected.iter().any(|column| column == "read"));
                assert!(collected.iter().any(|column| column == "pending_ack_at"));
                assert!(collected.iter().any(|column| column == "acknowledged_at"));
                assert!(collected.iter().any(|column| column == "expires_at"));
                assert!(collected.iter().any(|column| column == "deleted_at"));
                assert!(!collected.iter().any(|column| column == "stale_at"));
                Ok(())
            })
            .expect("schema inspection");
    }

    #[test]
    fn mail_store_enforces_message_key_prefix_and_single_successor() {
        let assembly = in_memory_assembly();
        let store = assembly.mail_store();
        let root_id = atm_core::schema::AtmMessageId::new();

        let root_record = boundary::MailStoreMessageRecord {
            team: team(),
            agent: agent(),
            message_key: message_key("atm:root"),
            envelope: MessageEnvelope {
                message_id: Some(root_id),
                ..envelope()
            },
            imported_from: None,
            recorded_at: Some(IsoTimestamp::now()),
        };
        store
            .upsert_message(boundary::MailStoreUpsertMessageRequest {
                record: root_record,
            })
            .expect("root upsert");

        let invalid = store
            .upsert_message(boundary::MailStoreUpsertMessageRequest {
                record: boundary::MailStoreMessageRecord {
                    team: team(),
                    agent: agent(),
                    message_key: MessageKey::new("bad-key").expect("non-empty"),
                    envelope: envelope(),
                    imported_from: None,
                    recorded_at: Some(IsoTimestamp::now()),
                },
            })
            .expect_err("invalid key");
        assert!(invalid.is_validation());

        let first_successor = boundary::MailStoreMessageRecord {
            team: team(),
            agent: agent(),
            message_key: message_key("atm:successor-1"),
            envelope: MessageEnvelope {
                message_id: Some(atm_core::schema::AtmMessageId::new()),
                parent_message_id: Some(root_id),
                thread_mode: Some(atm_core::schema::ThreadMode::AddDetails),
                ..envelope()
            },
            imported_from: None,
            recorded_at: Some(IsoTimestamp::now()),
        };
        store
            .upsert_message(boundary::MailStoreUpsertMessageRequest {
                record: first_successor,
            })
            .expect("first successor");

        let duplicate_successor = store
            .upsert_message(boundary::MailStoreUpsertMessageRequest {
                record: boundary::MailStoreMessageRecord {
                    team: team(),
                    agent: agent(),
                    message_key: message_key("atm:successor-2"),
                    envelope: MessageEnvelope {
                        message_id: Some(atm_core::schema::AtmMessageId::new()),
                        parent_message_id: Some(root_id),
                        thread_mode: Some(atm_core::schema::ThreadMode::Supersede),
                        ..envelope()
                    },
                    imported_from: None,
                    recorded_at: Some(IsoTimestamp::now()),
                },
            })
            .expect_err("duplicate successor");
        assert!(duplicate_successor.is_validation());
        assert!(
            duplicate_successor
                .message
                .contains("single-successor invariant"),
            "duplicate successor should be rejected by crate-owned validation before sqlite constraint handling"
        );

        let duplicate_message_identity = store
            .upsert_message(boundary::MailStoreUpsertMessageRequest {
                record: boundary::MailStoreMessageRecord {
                    team: team(),
                    agent: agent(),
                    message_key: message_key("atm:dup-legacy"),
                    envelope: MessageEnvelope {
                        message_id: Some(root_id),
                        ..envelope()
                    },
                    imported_from: None,
                    recorded_at: Some(IsoTimestamp::now()),
                },
            })
            .expect_err("duplicate message identity");
        assert!(duplicate_message_identity.is_validation());
        assert!(
            duplicate_message_identity
                .message
                .contains("already owned by"),
            "message identity collision should be rejected by crate-owned validation before sqlite constraint handling"
        );
    }

    #[test]
    fn duplicate_upsert_reports_inserted_false_and_keeps_record_loadable() {
        let assembly = in_memory_assembly();
        let store = assembly.mail_store();
        let original = boundary::MailStoreMessageRecord {
            team: team(),
            agent: agent(),
            message_key: message_key("atm:duplicate"),
            envelope: envelope(),
            imported_from: None,
            recorded_at: Some(IsoTimestamp::now()),
        };
        let replacement = boundary::MailStoreMessageRecord {
            envelope: MessageEnvelope {
                text: "rewritten duplicate payload".to_string(),
                summary: Some("rewritten summary".to_string()),
                pending_ack_at: None,
                ..original.envelope.clone()
            },
            imported_from: Some("duplicate-rewrite-attempt".to_string()),
            recorded_at: Some(IsoTimestamp::now()),
            ..original.clone()
        };

        let first = store
            .upsert_message(boundary::MailStoreUpsertMessageRequest {
                record: original.clone(),
            })
            .expect("first upsert");
        let second = store
            .upsert_message(boundary::MailStoreUpsertMessageRequest {
                record: replacement,
            })
            .expect("second upsert");

        assert!(first.inserted);
        assert!(!second.inserted);
        let loaded = store
            .load_message(boundary::MailStoreLoadMessageRequest {
                team: team(),
                agent: agent(),
                message_key: message_key("atm:duplicate"),
            })
            .expect("load duplicate");
        assert_eq!(loaded.record, Some(original));
    }

    #[test]
    fn concurrent_hot_path_upserts_succeed_through_writer_lane() {
        let assembly = in_memory_assembly();
        let store = Arc::clone(&assembly.mail_store);
        let mut workers: Vec<std::thread::JoinHandle<bool>> = Vec::new();
        for index in 0..16 {
            let store = Arc::clone(&store);
            workers.push(std::thread::spawn(move || {
                let response = store.upsert_message(boundary::MailStoreUpsertMessageRequest {
                    record: boundary::MailStoreMessageRecord {
                        team: team(),
                        agent: agent(),
                        message_key: message_key(&format!("atm:concurrent-{index}")),
                        envelope: envelope(),
                        imported_from: Some("concurrency-test".to_string()),
                        recorded_at: Some(IsoTimestamp::now()),
                    },
                });
                response.expect("concurrent upsert").inserted
            }));
        }

        let inserted = workers
            .into_iter()
            .map(|worker| worker.join().expect("writer thread join"))
            .filter(|inserted| *inserted)
            .count();
        assert_eq!(inserted, 16);

        let health = assembly
            .mail_store()
            .health_snapshot(boundary::MailStoreHealthSnapshotRequest {
                team: team(),
                agent: agent(),
            })
            .expect("health after concurrent upserts");
        assert_eq!(health.snapshot.total_messages, 16);
    }

    #[test]
    fn task_store_round_trips_records_and_metadata_queries() {
        let assembly = in_memory_assembly();
        let store = assembly.task_store();

        let record = boundary::TaskStoreTaskRecord {
            team: team(),
            task_id: task_id(),
            state: "active".parse().expect("task state"),
            owner: Some(agent()),
            linked_message_keys: vec![message_key("atm:test-1")],
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
                state: Some("acknowledged".parse().expect("task state")),
                metadata: None,
                append_message_keys: vec![message_key("atm:test-2")],
            })
            .expect("update");
        assert_eq!(updated.record.state, "acknowledged");
        assert!(
            updated
                .record
                .linked_message_keys
                .iter()
                .any(|value| value.as_ref() == "atm:test-2")
        );

        let metadata = store
            .query_task_metadata(boundary::TaskStoreQueryTaskMetadataRequest {
                team: team(),
                task_id: Some(task_id()),
                message_key: None,
                state: Some("acknowledged".parse().expect("task state")),
                limit: Some(10),
            })
            .expect("metadata query");
        assert_eq!(metadata.records.len(), 1);
    }

    #[test]
    fn roster_store_round_trips_roster_membership_and_health() {
        let assembly = in_memory_assembly();
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
    fn remote_replay_state_round_trips_and_purges_expired_rows() {
        let assembly = in_memory_assembly();
        let now = IsoTimestamp::now();
        let live_record = RemoteReplayStateRecord {
            team: team(),
            agent: agent(),
            message_key: message_key("atm:remote-live"),
            peer_addr: "127.0.0.1:4310".parse().expect("peer addr"),
            request: RequestEnvelope::Doctor(DoctorQuery {
                home_dir: PathBuf::from("."),
                current_dir: PathBuf::from("."),
                team_override: None,
            }),
            recorded_at: now,
            expires_at: IsoTimestamp::from_datetime(
                now.into_inner() + chrono::Duration::minutes(1),
            ),
            attempt_count: 0,
            last_attempt_at: None,
            last_error: None,
        };
        let expired_record = RemoteReplayStateRecord {
            team: team(),
            agent: agent(),
            message_key: message_key("atm:remote-expired"),
            peer_addr: "127.0.0.1:4311".parse().expect("peer addr"),
            request: RequestEnvelope::Doctor(DoctorQuery {
                home_dir: PathBuf::from("."),
                current_dir: PathBuf::from("."),
                team_override: None,
            }),
            recorded_at: now,
            expires_at: IsoTimestamp::from_datetime(
                now.into_inner() - chrono::Duration::minutes(1),
            ),
            attempt_count: 1,
            last_attempt_at: Some(now),
            last_error: Some("ATM_DAEMON_UNAVAILABLE".to_string()),
        };

        assembly
            .record_remote_replay_state(live_record.clone())
            .expect("record live");
        assembly
            .record_remote_replay_state(expired_record)
            .expect("record expired");

        let loaded = assembly
            .load_remote_replay_states()
            .expect("load replay states");
        assert_eq!(loaded.len(), 2);

        let purged = assembly
            .purge_expired_remote_replay_states(now)
            .expect("purge expired");
        assert_eq!(purged, 1);

        let loaded = assembly
            .load_remote_replay_states()
            .expect("load replay states");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].message_key.as_ref(), "atm:remote-live");

        assembly
            .delete_remote_replay_state(&team(), &agent(), &live_record.message_key)
            .expect("delete live");
        assert!(
            assembly
                .load_remote_replay_states()
                .expect("load replay states")
                .is_empty()
        );
    }

    #[test]
    fn sqlite_boundary_enforces_wal_and_busy_timeout() {
        let (_tempdir, path) = temp_disk_db();
        let assembly = assemble_boundary(&path).expect("assembly");

        assembly
            .mail_store
            .db
            .with_connection(|connection| {
                let journal_mode: String = connection
                    .pragma_query_value(None, "journal_mode", |row| row.get(0))
                    .map_err(|error| {
                        assembly
                            .mail_store
                            .db
                            .error("failed to read sqlite journal_mode pragma", error)
                    })?;
                let busy_timeout_ms: i64 = connection
                    .pragma_query_value(None, "busy_timeout", |row| row.get(0))
                    .map_err(|error| {
                        assembly
                            .mail_store
                            .db
                            .error("failed to read sqlite busy_timeout pragma", error)
                    })?;
                assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
                assert_eq!(busy_timeout_ms, 5000);
                Ok(())
            })
            .expect("pragma verification");
    }

    #[test]
    fn sqlite_transactions_roll_back_on_error() {
        let assembly = in_memory_assembly();
        let db = assembly.mail_store.db.clone();
        let forced_failure_message = "force rollback";

        let result: Result<(), AtmError> = db.with_transaction(|transaction| {
            transaction
                .execute(
                    "INSERT INTO tasks(team, task_id, record_json) VALUES (?1, ?2, ?3);",
                    params![
                        team().as_str(),
                        task_id().as_str(),
                        "{\"state\":\"pending\"}"
                    ],
                )
                .map_err(|error| {
                    db.error("failed to insert transactional rollback probe", error)
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
                        "SELECT record_json FROM tasks WHERE team = ?1 AND task_id = ?2;",
                        params![team().as_str(), task_id().as_str()],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
                    .map_err(|error| {
                        db.error("failed to verify transactional rollback probe", error)
                    })
            })
            .expect("rollback verification");
        assert!(persisted.is_none());
    }
}
