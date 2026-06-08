#![forbid(unsafe_code)]

//! SQLite-backed storage backend implementing the shared `atm-storage`
//! message and roster contracts.

mod mailbox_metadata;
mod roster_store;
mod shared_db;
mod writer;

use crate::mailbox_metadata::{query_mailbox_metadata_counts, query_mailbox_metadata_rows};
use atm_storage::{AtmError, IsoTimestamp, NullSqliteObservability, SqliteObservability};
use atm_storage::contract::{Message, MessageKey, MessageQuery, MessageStore, RosterStore};
use atm_storage::schema::{AtmMessageId, MessageEnvelope, ThreadMode};
use atm_storage::types::{AgentName, TeamName};
use rusqlite::{Connection, OptionalExtension, params};
use shared_db::{SharedDb, deserialize_json};
use std::path::Path;
use std::sync::Arc;

fn decode_sqlite_count(value: i64, field_name: &str) -> Result<u64, AtmError> {
    u64::try_from(value).map_err(|error| {
        AtmError::validation(format!(
            "sqlite count {field_name} must not be negative: {value}"
        ))
        .with_recovery(
            "Repair the malformed sqlite count row before retrying the health or metadata query.",
        )
        .with_source(error)
    })
}

#[derive(Debug)]
pub(crate) struct SqliteWriterLockGuard {
    connection: Connection,
}

impl Drop for SqliteWriterLockGuard {
    fn drop(&mut self) {
        let _ = self.connection.execute_batch("ROLLBACK;");
    }
}

#[doc(hidden)]
pub struct TestOnlySqliteWriterLockGuard {
    _guard: SqliteWriterLockGuard,
}

pub(crate) fn hold_sqlite_writer_lock(
    path: impl AsRef<Path>,
) -> Result<SqliteWriterLockGuard, AtmError> {
    let connection = Connection::open(path.as_ref()).map_err(|error| {
        AtmError::daemon_unavailable("failed to open sqlite writer lock connection")
            .with_recovery(
                "Repair the sqlite test runtime path before retrying the bounded sqlite writer-lock test.",
            )
            .with_source(error)
    })?;
    connection.execute_batch("BEGIN IMMEDIATE;").map_err(|error| {
        AtmError::daemon_unavailable("failed to begin sqlite writer lock transaction")
            .with_recovery(
                "Repair the sqlite test runtime path before retrying the bounded sqlite writer-lock test.",
            )
            .with_source(error)
    })?;
    Ok(SqliteWriterLockGuard { connection })
}

#[doc(hidden)]
pub fn hold_sqlite_writer_lock_for_test(
    path: impl AsRef<Path>,
) -> Result<TestOnlySqliteWriterLockGuard, AtmError> {
    hold_sqlite_writer_lock(path).map(|guard| TestOnlySqliteWriterLockGuard { _guard: guard })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqliteMessageStateRecord {
    pub team: TeamName,
    pub agent: AgentName,
    pub actor: AgentName,
    pub message_key: MessageKey,
    pub read: bool,
    pub pending_ack_at: Option<IsoTimestamp>,
    pub acknowledged_at: Option<IsoTimestamp>,
    pub expires_at: Option<IsoTimestamp>,
    pub deleted_at: Option<IsoTimestamp>,
    pub updated_at: Option<IsoTimestamp>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SqliteStoredMessageRecord {
    pub team: TeamName,
    pub agent: AgentName,
    pub message_key: MessageKey,
    pub envelope: MessageEnvelope,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SqliteIngestReplayStateRecord {
    pub team: TeamName,
    pub agent: AgentName,
    pub source: String,
    pub last_fingerprint: Option<String>,
    pub last_ingested_at: Option<IsoTimestamp>,
    pub ingested_rows: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqliteMailHealthSnapshot {
    pub team: TeamName,
    pub agent: AgentName,
    pub total_messages: u64,
    pub pending_ack_messages: u64,
    pub read_message_count: u64,
    pub latest_message_timestamp: Option<IsoTimestamp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqliteMailboxMetadataRow {
    pub message_key: MessageKey,
    pub message_id: Option<AtmMessageId>,
    pub parent_message_id: Option<AtmMessageId>,
    pub thread_mode: Option<ThreadMode>,
    pub from_agent: AgentName,
    pub summary: Option<String>,
    pub message_at: IsoTimestamp,
    pub read: bool,
    pub pending_ack: bool,
    pub acknowledged_at: Option<IsoTimestamp>,
    pub expires_at: Option<IsoTimestamp>,
    pub task_id: Option<atm_storage::types::TaskId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqliteMailboxMetadataCounts {
    pub total_messages: u64,
    pub unread_message_count: u64,
    pub pending_ack_messages: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqliteRosterHealthSnapshot {
    pub team: TeamName,
    pub member_count: u64,
    pub stale: bool,
    pub refreshed_at: Option<IsoTimestamp>,
}

#[derive(Debug)]
struct SqliteMessageStore {
    db: Arc<SharedDb>,
}

#[derive(Debug)]
struct SqliteRosterStore {
    db: Arc<SharedDb>,
}

#[derive(Debug, Clone)]
struct StoredMailMessageState {
    read: bool,
    pending_ack_at: Option<IsoTimestamp>,
    acknowledged_at: Option<IsoTimestamp>,
    expires_at: Option<IsoTimestamp>,
    deleted_at: Option<IsoTimestamp>,
    updated_at: Option<IsoTimestamp>,
}

impl SqliteMessageStore {
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
                    "Repair the sqlite-backed mail-store row or rewrite it through the owning backend.",
                )
                .with_source(error)
            })
            .map(|value| value.map(IsoTimestamp::from_datetime))
    }

    fn load_message_state_row(
        &self,
        connection: &Connection,
        team: &TeamName,
        agent: &AgentName,
        message_key: &MessageKey,
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
                    .error("failed to load sqlite message-state row", error)
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
        mut envelope: MessageEnvelope,
        state: Option<&StoredMailMessageState>,
    ) -> MessageEnvelope {
        if let Some(state) = state {
            envelope.read = state.read;
            envelope.pending_ack_at = state.pending_ack_at;
            envelope.acknowledged_at = state.acknowledged_at;
            envelope.expires_at = state.expires_at;
        }
        envelope
    }
}

impl SqliteRosterStore {
    fn new(db: Arc<SharedDb>) -> Self {
        Self { db }
    }
}

impl MessageStore for SqliteMessageStore {
    fn save_message(&self, message: &Message) -> Result<(), AtmError> {
        self.db.submit_upsert_message(message.clone())
    }

    fn load_message(&self, key: &MessageKey) -> Result<Option<Message>, AtmError> {
        let record = self.db.with_connection(|connection| {
            let loaded = connection
                .query_row(
                    "SELECT team, agent, envelope_json
                     FROM mail_messages
                     WHERE message_key = ?1;",
                    params![key.as_ref()],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()
                .map_err(|error| self.db.error("failed to load sqlite message", error))?;

            if let Some((team, agent, envelope_json)) = loaded {
                let team: TeamName = team.parse().map_err(|error| {
                    AtmError::validation(format!(
                        "failed to parse sqlite team for message {key}: {error}"
                    ))
                    .with_recovery("Repair the sqlite mail_messages row before retrying the read.")
                })?;
                let agent: AgentName = agent.parse().map_err(|error| {
                    AtmError::validation(format!(
                        "failed to parse sqlite agent for message {key}: {error}"
                    ))
                    .with_recovery("Repair the sqlite mail_messages row before retrying the read.")
                })?;
                let state = self.load_message_state_row(connection, &team, &agent, key)?;
                Ok(Some((team, agent, envelope_json, state)))
            } else {
                Ok(None)
            }
        })?;

        if let Some((team, agent, envelope_json, state)) = record {
            let envelope = deserialize_json(&envelope_json, "sqlite message envelope")?;
            let envelope = Self::apply_loaded_state(envelope, state.as_ref());
            Ok(Some(Message {
                team,
                agent,
                message_key: key.clone(),
                envelope,
            }))
        } else {
            Ok(None)
        }
    }

    fn list_messages(&self, query: &MessageQuery) -> Result<Vec<Message>, AtmError> {
        self.db.with_connection(|connection| {
            let limit = query
                .limit
                // The shared contract accepts usize, but SQLite LIMIT is i64.
                // Saturating oversize fuzz/test inputs at i64::MAX keeps the
                // query bounded without inventing a second caller-visible cap.
                .map(|value| i64::try_from(value).unwrap_or(i64::MAX))
                .unwrap_or(-1);
            let mut statement = connection
                .prepare(
                    "SELECT mail_messages.message_key, mail_messages.envelope_json
                     FROM mail_messages
                     LEFT JOIN mail_message_states
                       ON mail_message_states.team = mail_messages.team
                      AND mail_message_states.agent = mail_messages.agent
                      AND mail_message_states.message_key = mail_messages.message_key
                     WHERE mail_messages.team = ?1
                       AND mail_messages.agent = ?2
                       AND (?3 IS NULL OR mail_messages.from_agent = ?3)
                       AND (?4 IS NULL OR json_extract(mail_messages.envelope_json, '$.taskId') = ?4)
                       AND mail_message_states.deleted_at IS NULL
                       AND (
                            mail_message_states.expires_at IS NULL
                            OR mail_message_states.expires_at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                       )
                     ORDER BY mail_messages.message_at DESC, mail_messages.message_key DESC
                     LIMIT ?5;",
                )
                .map_err(|error| self.db.error("failed to prepare sqlite message list query", error))?;
            let rows = statement
                .query_map(
                    params![
                        query.team.as_str(),
                        query.agent.as_str(),
                        query.sender.as_ref().map(|value| value.as_str()),
                        query.task_id.as_ref().map(|value| value.as_str()),
                        limit,
                    ],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .map_err(|error| self.db.error("failed to execute sqlite message list query", error))?;

            let mut messages = Vec::new();
            for row in rows {
                let (message_key, envelope_json) =
                    row.map_err(|error| self.db.error("failed to decode sqlite message row", error))?;
                let message_key = MessageKey::new(message_key).map_err(|error| {
                    AtmError::validation(format!(
                        "failed to parse sqlite message key during list: {error}"
                    ))
                    .with_recovery("Repair the sqlite mail_messages row before retrying the list.")
                })?;
                let state =
                    self.load_message_state_row(connection, &query.team, &query.agent, &message_key)?;
                let envelope: MessageEnvelope =
                    deserialize_json(&envelope_json, "sqlite message envelope")?;
                let envelope = Self::apply_loaded_state(envelope, state.as_ref());
                messages.push(Message {
                    team: query.team.clone(),
                    agent: query.agent.clone(),
                    message_key,
                    envelope,
                });
            }
            Ok(messages)
        })
    }

    fn delete_message(&self, key: &MessageKey) -> Result<(), AtmError> {
        self.db.with_transaction(|transaction| {
            transaction
                .execute(
                    "DELETE FROM mail_message_states WHERE message_key = ?1;",
                    params![key.as_ref()],
                )
                .map_err(|error| {
                    self.db
                        .error("failed to delete sqlite message state", error)
                })?;
            transaction
                .execute(
                    "DELETE FROM mail_messages WHERE message_key = ?1;",
                    params![key.as_ref()],
                )
                .map_err(|error| self.db.error("failed to delete sqlite message", error))?;
            Ok(())
        })
    }
}

#[derive(Clone, Debug)]
pub struct SqliteStorageBackend {
    message_store: Arc<SqliteMessageStore>,
    roster_store: Arc<SqliteRosterStore>,
}

impl SqliteStorageBackend {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, AtmError> {
        Self::new_with_observability(path, Arc::new(NullSqliteObservability))
    }

    pub fn new_with_observability(
        path: impl AsRef<Path>,
        observability: Arc<dyn SqliteObservability>,
    ) -> Result<Self, AtmError> {
        let db = Arc::new(SharedDb::open_with_observability(path, observability)?);
        Ok(Self {
            message_store: Arc::new(SqliteMessageStore::new(Arc::clone(&db))),
            roster_store: Arc::new(SqliteRosterStore::new(db)),
        })
    }

    #[cfg(test)]
    fn in_memory_for_test() -> Result<Self, AtmError> {
        let db = Arc::new(SharedDb::open_in_memory_for_test()?);
        Ok(Self {
            message_store: Arc::new(SqliteMessageStore::new(Arc::clone(&db))),
            roster_store: Arc::new(SqliteRosterStore::new(db)),
        })
    }

    pub fn message_store(&self) -> Arc<dyn MessageStore + Send + Sync> {
        self.message_store.clone()
    }

    pub fn save_message_record(
        &self,
        team: TeamName,
        agent: AgentName,
        message_key: MessageKey,
        envelope: MessageEnvelope,
    ) -> Result<(), AtmError> {
        self.message_store().save_message(&Message {
            team,
            agent,
            message_key,
            envelope,
        })
    }

    pub fn load_message_record(
        &self,
        key: &MessageKey,
    ) -> Result<Option<SqliteStoredMessageRecord>, AtmError> {
        self.message_store().load_message(key).map(|record| {
            record.map(|record| SqliteStoredMessageRecord {
                team: record.team,
                agent: record.agent,
                message_key: record.message_key,
                envelope: record.envelope,
            })
        })
    }

    pub fn roster_store(&self) -> Arc<dyn RosterStore + Send + Sync> {
        self.roster_store.clone()
    }

    pub fn replace_roster(
        &self,
        team: TeamName,
        members: Vec<atm_storage::contract::RosterMember>,
    ) -> Result<(), AtmError> {
        self.roster_store()
            .save_roster(&atm_storage::contract::RosterSnapshot {
                team_name: team,
                members,
                refreshed_at: None,
            })
    }

    pub fn load_roster_members(
        &self,
        team: &TeamName,
    ) -> Result<Vec<atm_storage::contract::RosterMember>, AtmError> {
        self.roster_store()
            .load_roster(team)
            .map(|snapshot| snapshot.members)
    }

    pub fn checkpoint_wal(&self) -> Result<(), AtmError> {
        self.message_store.db.checkpoint_wal()
    }

    pub fn path(&self) -> Option<&Path> {
        match self.message_store.db.target_path() {
            Some(path) => Some(path.as_path()),
            None => None,
        }
    }

    pub fn upsert_message_state(&self, state: SqliteMessageStateRecord) -> Result<(), AtmError> {
        let updated_at = state
            .updated_at
            .unwrap_or_else(IsoTimestamp::now)
            .into_inner()
            .to_rfc3339();
        self.message_store.db.with_transaction(|transaction| {
            transaction
                .execute(
                    "INSERT INTO mail_message_states(
                        team,
                        agent,
                        message_key,
                        read,
                        pending_ack_at,
                        acknowledged_at,
                        expires_at,
                        deleted_at,
                        updated_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                     ON CONFLICT(team, agent, message_key) DO UPDATE SET
                        read = excluded.read,
                        pending_ack_at = excluded.pending_ack_at,
                        acknowledged_at = excluded.acknowledged_at,
                        expires_at = excluded.expires_at,
                        deleted_at = excluded.deleted_at,
                        updated_at = excluded.updated_at;",
                    params![
                        state.team.as_str(),
                        state.agent.as_str(),
                        state.message_key.as_ref(),
                        i64::from(state.read),
                        state
                            .pending_ack_at
                            .map(|value| value.into_inner().to_rfc3339()),
                        state
                            .acknowledged_at
                            .map(|value| value.into_inner().to_rfc3339()),
                        state
                            .expires_at
                            .map(|value| value.into_inner().to_rfc3339()),
                        state
                            .deleted_at
                            .map(|value| value.into_inner().to_rfc3339()),
                        updated_at,
                    ],
                )
                .map_err(|error| {
                    self.message_store
                        .db
                        .error("failed to upsert sqlite message state", error)
                })?;
            Ok(())
        })
    }

    pub fn load_message_state(
        &self,
        team: &TeamName,
        agent: &AgentName,
        actor: &AgentName,
        message_key: &MessageKey,
    ) -> Result<Option<SqliteMessageStateRecord>, AtmError> {
        self.message_store
            .db
            .with_connection(|connection| {
                self.message_store
                    .load_message_state_row(connection, team, agent, message_key)
            })
            .map(|state| {
                state.map(|state| SqliteMessageStateRecord {
                    team: team.clone(),
                    agent: agent.clone(),
                    actor: actor.clone(),
                    message_key: message_key.clone(),
                    read: state.read,
                    pending_ack_at: state.pending_ack_at,
                    acknowledged_at: state.acknowledged_at,
                    expires_at: state.expires_at,
                    deleted_at: state.deleted_at,
                    updated_at: state.updated_at,
                })
            })
    }

    pub fn query_mailbox_metadata(
        &self,
        team: &TeamName,
        agent: &AgentName,
        limit: Option<usize>,
    ) -> Result<Vec<SqliteMailboxMetadataRow>, AtmError> {
        query_mailbox_metadata_rows(&self.message_store.db, team, agent, limit)
    }

    pub fn query_mailbox_metadata_counts(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<SqliteMailboxMetadataCounts, AtmError> {
        query_mailbox_metadata_counts(&self.message_store.db, team, agent)
    }

    pub fn record_ingest_replay_state(
        &self,
        state: &SqliteIngestReplayStateRecord,
    ) -> Result<(), AtmError> {
        let state_json = serde_json::to_string(state).map_err(|error| {
            AtmError::validation(format!(
                "failed to serialize sqlite ingest replay state: {error}"
            ))
            .with_recovery(
                "Repair the ingest replay state payload before retrying the SQLite write.",
            )
            .with_source(error)
        })?;
        self.message_store.db.with_transaction(|transaction| {
            transaction
                .execute(
                    "INSERT INTO mail_ingest_replay_states(team, agent, source, state_json)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(team, agent, source) DO UPDATE SET
                       state_json = excluded.state_json;",
                    params![
                        state.team.as_str(),
                        state.agent.as_str(),
                        state.source,
                        state_json,
                    ],
                )
                .map_err(|error| {
                    self.message_store
                        .db
                        .error("failed to record sqlite ingest replay state", error)
                })?;
            Ok(())
        })
    }

    pub fn load_ingest_replay_state(
        &self,
        team: &TeamName,
        agent: &AgentName,
        source: &str,
    ) -> Result<Option<SqliteIngestReplayStateRecord>, AtmError> {
        let state_json = self.message_store.db.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT state_json
                     FROM mail_ingest_replay_states
                     WHERE team = ?1 AND agent = ?2 AND source = ?3;",
                    params![team.as_str(), agent.as_str(), source],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(|error| {
                    self.message_store
                        .db
                        .error("failed to load sqlite ingest replay state", error)
                })
        })?;

        state_json
            .map(|value| {
                serde_json::from_str::<SqliteIngestReplayStateRecord>(&value).map_err(|error| {
                    AtmError::validation(format!(
                        "failed to parse sqlite ingest replay state: {error}"
                    ))
                    .with_recovery(
                        "Repair the persisted ingest replay row before retrying the SQLite read.",
                    )
                    .with_source(error)
                })
            })
            .transpose()
    }

    pub fn mail_health_snapshot(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<SqliteMailHealthSnapshot, AtmError> {
        let (total_messages, pending_ack_messages, read_message_count, latest_message_timestamp) =
            self.message_store.db.with_connection(|connection| {
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
                        params![team.as_str(), agent.as_str()],
                        |row| {
                            Ok((
                                row.get::<_, i64>(0)?,
                                row.get::<_, i64>(1)?,
                                row.get::<_, i64>(2)?,
                                row.get::<_, Option<String>>(3)?,
                            ))
                        },
                    )
                    .map_err(|error| {
                        self.message_store
                            .db
                            .error("failed to query sqlite mail health summary", error)
                    })
            })?;

        Ok(SqliteMailHealthSnapshot {
            team: team.clone(),
            agent: agent.clone(),
            total_messages: decode_sqlite_count(total_messages, "total_messages")?,
            pending_ack_messages: decode_sqlite_count(
                pending_ack_messages,
                "pending_ack_messages",
            )?,
            read_message_count: decode_sqlite_count(read_message_count, "read_message_count")?,
            latest_message_timestamp: SqliteMessageStore::parse_optional_timestamp(
                latest_message_timestamp,
                "health latest_message",
            )?,
        })
    }

    pub fn inspect_mail_store(&self) -> Result<(), AtmError> {
        self.message_store.db.with_connection(|_| Ok(()))
    }

    pub fn roster_health_snapshot(
        &self,
        team: &TeamName,
    ) -> Result<SqliteRosterHealthSnapshot, AtmError> {
        let member_count = self.roster_store.db.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT COUNT(*) FROM team_roster WHERE team_name = ?1;",
                    params![team.as_str()],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(|error| {
                    self.roster_store
                        .db
                        .error("failed to query sqlite roster health summary", error)
                })
        })?;

        Ok(SqliteRosterHealthSnapshot {
            team: team.clone(),
            member_count: decode_sqlite_count(member_count, "roster_member_count")?,
            stale: false,
            refreshed_at: None,
        })
    }

    pub fn inspect_roster_store(&self) -> Result<(), AtmError> {
        self.roster_store.db.with_connection(|_| Ok(()))
    }

    pub fn record_remote_replay_state_json(
        &self,
        team: &TeamName,
        agent: &AgentName,
        message_key: &MessageKey,
        state_json: &str,
    ) -> Result<(), AtmError> {
        self.message_store.db.with_transaction(|transaction| {
            transaction
                .execute(
                    "INSERT INTO daemon_remote_replay_states(team, agent, message_key, state_json)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(team, agent, message_key) DO UPDATE SET
                       state_json = excluded.state_json;",
                    params![
                        team.as_str(),
                        agent.as_str(),
                        message_key.as_ref(),
                        state_json
                    ],
                )
                .map_err(|error| {
                    self.message_store
                        .db
                        .error("failed to record daemon remote replay state", error)
                })?;
            Ok(())
        })
    }

    pub fn load_remote_replay_state_json(&self) -> Result<Vec<String>, AtmError> {
        self.message_store.db.with_connection(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT state_json
                     FROM daemon_remote_replay_states
                     ORDER BY team, agent, message_key
                     LIMIT 10000;",
                )
                .map_err(|error| {
                    self.message_store
                        .db
                        .error("failed to prepare daemon remote replay query", error)
                })?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|error| {
                    self.message_store
                        .db
                        .error("failed to read daemon remote replay rows", error)
                })?;
            let mut records = Vec::new();
            for row in rows {
                records.push(row.map_err(|error| {
                    self.message_store
                        .db
                        .error("failed to decode daemon remote replay row", error)
                })?);
            }
            Ok(records)
        })
    }

    pub fn delete_remote_replay_state(
        &self,
        team: &TeamName,
        agent: &AgentName,
        message_key: &MessageKey,
    ) -> Result<(), AtmError> {
        self.message_store.db.with_transaction(|transaction| {
            transaction
                .execute(
                    "DELETE FROM daemon_remote_replay_states
                     WHERE team = ?1 AND agent = ?2 AND message_key = ?3;",
                    params![team.as_str(), agent.as_str(), message_key.as_ref()],
                )
                .map_err(|error| {
                    self.message_store
                        .db
                        .error("failed to delete daemon remote replay state", error)
                })?;
            Ok(())
        })
    }

    pub fn purge_expired_remote_replay_states(&self, now: IsoTimestamp) -> Result<usize, AtmError> {
        let now = now.into_inner().to_rfc3339();
        self.message_store.db.with_transaction(|transaction| {
            transaction
                .execute(
                    "DELETE FROM daemon_remote_replay_states
                     WHERE json_extract(state_json, '$.expires_at') <= ?1;",
                    params![now],
                )
                .map_err(|error| {
                    self.message_store
                        .db
                        .error("failed to purge expired daemon remote replay state", error)
                })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::SqliteStorageBackend;
    use atm_storage::contract::{
        AgentType, Message, MessageKey, MessageQuery, RosterHarness, RosterMember,
        RosterMemberKind, RosterSnapshot,
    };
    use atm_storage::schema::MessageEnvelope;
    use atm_storage::types::{AgentName, IsoTimestamp, ModelName, TeamName};
    use chrono::Utc;
    use rusqlite::params;
    use serde_json::Map;

    fn team() -> TeamName {
        "test-team".parse().expect("team")
    }

    fn agent() -> AgentName {
        "test-agent".parse().expect("agent")
    }

    fn message(key: &str, text: &str) -> Message {
        let team = team();
        let agent = agent();
        Message {
            team: team.clone(),
            agent: agent.clone(),
            message_key: MessageKey::new(key).expect("key"),
            envelope: MessageEnvelope {
                from: agent,
                text: text.to_string(),
                timestamp: IsoTimestamp::from_datetime(Utc::now()),
                read: false,
                source_team: Some(team),
                summary: None,
                message_id: None,
                pending_ack_at: None,
                acknowledged_at: None,
                acknowledges_message_id: None,
                parent_message_id: None,
                thread_mode: None,
                expires_at: None,
                task_id: None,
                extra: Map::new(),
            },
        }
    }

    #[test]
    fn sqlite_backend_saves_loads_lists_and_deletes_messages() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.message_store();
        let original = message("atm:test-1", "hello");

        store.save_message(&original).expect("save");
        let loaded = store
            .load_message(&original.message_key)
            .expect("load")
            .expect("message");
        assert_eq!(loaded.envelope.text, "hello");

        let listed = store
            .list_messages(&MessageQuery {
                team: original.team.clone(),
                agent: original.agent.clone(),
                sender: None,
                task_id: None,
                limit: Some(10),
            })
            .expect("list");
        assert_eq!(listed.len(), 1);

        store.delete_message(&original.message_key).expect("delete");
        assert!(
            store
                .load_message(&original.message_key)
                .expect("load after delete")
                .is_none()
        );
    }

    #[test]
    fn sqlite_backend_saves_loads_and_lists_rosters() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.roster_store();
        let team = team();
        let roster = RosterSnapshot {
            team_name: team.clone(),
            members: vec![RosterMember {
                team_name: team.clone(),
                agent_name: agent(),
                member_kind: RosterMemberKind::Permanent,
                harness: RosterHarness::ClaudeCode,
                agent_type: AgentType::Worker,
                model: ModelName::default(),
                recipient_pane_id: None,
                metadata_json: Map::new(),
            }],
            refreshed_at: None,
        };

        store.save_roster(&roster).expect("save roster");
        let loaded = store.load_roster(&team).expect("load roster");
        assert_eq!(loaded.members.len(), 1);
        assert_eq!(store.list_teams().expect("list teams"), vec![team]);
    }

    #[test]
    fn load_message_rejects_invalid_team_row() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        backend
            .message_store
            .db
            .with_transaction(|transaction| {
                transaction
                    .execute(
                        "INSERT INTO mail_messages(
                            team, agent, message_key, envelope_json, from_agent, message_text,
                            summary, message_at, message_id, parent_message_id, thread_mode, recorded_at
                         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, NULL, NULL, NULL, ?7);",
                        params![
                            "",
                            agent().as_str(),
                            "atm:test-invalid-team",
                            serde_json::to_string(&message("atm:test-invalid-team", "hello").envelope)
                                .expect("envelope json"),
                            agent().as_str(),
                            "hello",
                            IsoTimestamp::from_datetime(Utc::now()).into_inner().to_rfc3339(),
                        ],
                    )
                    .expect("insert invalid row");
                Ok(())
            })
            .expect("seed row");

        let error = backend
            .message_store()
            .load_message(&MessageKey::new("atm:test-invalid-team").expect("key"))
            .expect_err("invalid team should fail");
        assert!(error.message.contains("failed to parse sqlite team"));
    }

    #[test]
    fn load_ingest_replay_state_rejects_invalid_json() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let team = team();
        let agent = agent();
        backend
            .message_store
            .db
            .with_transaction(|transaction| {
                transaction
                    .execute(
                        "INSERT INTO mail_ingest_replay_states(team, agent, source, state_json)
                         VALUES (?1, ?2, ?3, ?4);",
                        params![team.as_str(), agent.as_str(), "bad-source", "{not-json"],
                    )
                    .expect("insert invalid replay state");
                Ok(())
            })
            .expect("seed row");

        let error = backend
            .load_ingest_replay_state(&team, &agent, "bad-source")
            .expect_err("invalid replay state should fail");
        assert!(
            error
                .message
                .contains("failed to parse sqlite ingest replay state")
        );
    }

    #[test]
    fn query_mailbox_metadata_rejects_invalid_task_id() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let original = message("atm:test-invalid-task", "hello");
        backend
            .message_store()
            .save_message(&original)
            .expect("save");
        backend
            .message_store
            .db
            .with_transaction(|transaction| {
                transaction
                    .execute(
                        "UPDATE mail_messages
                         SET envelope_json = json_set(envelope_json, '$.taskId', '   ')
                         WHERE team = ?1 AND agent = ?2 AND message_key = ?3;",
                        params![
                            original.team.as_str(),
                            original.agent.as_str(),
                            original.message_key.as_ref(),
                        ],
                    )
                    .expect("corrupt task id");
                Ok(())
            })
            .expect("update row");

        let error = backend
            .query_mailbox_metadata(&original.team, &original.agent, Some(10))
            .expect_err("invalid task id should fail");
        assert!(
            error
                .message
                .contains("failed to parse sqlite mailbox metadata task_id")
        );
    }

    #[test]
    fn load_roster_rejects_invalid_model() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let team = team();
        let invalid_model = "m".repeat(257);
        backend
            .roster_store
            .db
            .with_transaction(|transaction| {
                transaction
                    .execute(
                        "INSERT INTO team_roster(
                            team_name, agent_name, member_kind, harness, agent_type, model,
                            metadata_json, source, recipient_pane_id, updated_at
                         ) VALUES (?1, ?2, 'permanent', 'claude-code', 'worker', ?3, '{}', NULL, NULL, ?4);",
                        params![
                            team.as_str(),
                            agent().as_str(),
                            invalid_model,
                            IsoTimestamp::from_datetime(Utc::now()).into_inner().to_rfc3339(),
                        ],
                    )
                    .expect("insert invalid roster row");
                Ok(())
            })
            .expect("seed roster");

        let error = backend
            .roster_store()
            .load_roster(&team)
            .expect_err("invalid model should fail");
        assert!(
            error
                .message
                .contains("failed to parse canonical team-roster model")
        );
    }
}
