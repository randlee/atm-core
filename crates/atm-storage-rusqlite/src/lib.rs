#![forbid(unsafe_code)]

//! SQLite-backed storage backend implementing the shared `atm-storage`
//! message and roster contracts.

mod observability;
mod roster_store;
mod shared_db;
mod writer;

use atm_storage::contract::{Message, MessageKey, MessageQuery, MessageStore, RosterStore};
use atm_storage::schema::MessageEnvelope;
use atm_storage::types::{AgentName, IsoTimestamp, TeamName};
use atm_storage::AtmError;
use rusqlite::{Connection, OptionalExtension, params};
use shared_db::{SharedDb, deserialize_json};
use std::path::Path;
use std::sync::Arc;

pub use observability::{
    NullSqliteObservability, SqliteObservability, SqliteObservabilityEvent,
    SqliteObservabilityOutcome,
};

#[derive(Debug)]
pub struct SqliteWriterLockGuard {
    connection: Connection,
}

impl Drop for SqliteWriterLockGuard {
    fn drop(&mut self) {
        let _ = self.connection.execute_batch("ROLLBACK;");
    }
}

pub fn hold_sqlite_writer_lock(path: impl AsRef<Path>) -> Result<SqliteWriterLockGuard, AtmError> {
    let connection = Connection::open(path.as_ref()).map_err(|error| {
        AtmError::daemon_unavailable("failed to open sqlite writer lock connection")
            .with_recovery(
                "Repair the sqlite test runtime path before retrying the bounded mailbox lock test.",
            )
            .with_source(error)
    })?;
    connection.execute_batch("BEGIN IMMEDIATE;").map_err(|error| {
        AtmError::daemon_unavailable("failed to begin sqlite writer lock transaction")
            .with_recovery(
                "Repair the sqlite test runtime path before retrying the bounded mailbox lock test.",
            )
            .with_source(error)
    })?;
    Ok(SqliteWriterLockGuard { connection })
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

    fn load_message_state(
        &self,
        connection: &Connection,
        team: &TeamName,
        agent: &AgentName,
        message_key: &MessageKey,
    ) -> Result<Option<StoredMailMessageState>, AtmError> {
        connection
            .query_row(
                "SELECT read, pending_ack_at, acknowledged_at, expires_at
                 FROM mail_message_states
                 WHERE team = ?1 AND agent = ?2 AND message_key = ?3;",
                params![team.as_str(), agent.as_str(), message_key.as_ref()],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| {
                self.db
                    .error("failed to load sqlite message-state row", error)
            })?
            .map(|(read, pending_ack_at, acknowledged_at, expires_at)| {
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
                })
            })
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
                let state = self.load_message_state(connection, &team, &agent, key)?;
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
                    self.load_message_state(connection, &query.team, &query.agent, &message_key)?;
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
                .map_err(|error| self.db.error("failed to delete sqlite message state", error))?;
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
        let db = Arc::new(SharedDb::open(path)?);
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

    pub fn roster_store(&self) -> Arc<dyn RosterStore + Send + Sync> {
        self.roster_store.clone()
    }

    pub fn checkpoint_wal(&self) -> Result<(), AtmError> {
        self.message_store.db.checkpoint_wal()
    }

    pub fn upsert_remote_replay_state(
        &self,
        team: &str,
        agent: &str,
        message_key: &str,
        state_json: &str,
    ) -> Result<(), AtmError> {
        self.message_store.db.with_connection(|connection| {
            connection
                .execute(
                    "INSERT INTO daemon_remote_replay_states (team, agent, message_key, state_json)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(team, agent, message_key)
                     DO UPDATE SET state_json = excluded.state_json;",
                    params![team, agent, message_key, state_json],
                )
                .map_err(|error| {
                    self.message_store
                        .db
                        .error("failed to persist daemon remote replay state", error)
                })?;
            Ok(())
        })
    }

    pub fn load_all_remote_replay_states(&self) -> Result<Vec<String>, AtmError> {
        self.message_store.db.with_connection(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT state_json
                     FROM daemon_remote_replay_states
                     ORDER BY team, agent, message_key;",
                )
                .map_err(|error| {
                    self.message_store
                        .db
                        .error("failed to prepare daemon remote replay load", error)
                })?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|error| {
                    self.message_store
                        .db
                        .error("failed to query daemon remote replay rows", error)
                })?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|error| {
                self.message_store
                    .db
                    .error("failed to decode daemon remote replay row", error)
            })
        })
    }

    pub fn delete_remote_replay_state(
        &self,
        team: &str,
        agent: &str,
        message_key: &str,
    ) -> Result<(), AtmError> {
        self.message_store.db.with_connection(|connection| {
            connection
                .execute(
                    "DELETE FROM daemon_remote_replay_states
                     WHERE team = ?1 AND agent = ?2 AND message_key = ?3;",
                    params![team, agent, message_key],
                )
                .map_err(|error| {
                    self.message_store
                        .db
                        .error("failed to delete daemon remote replay row", error)
                })?;
            Ok(())
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
}
