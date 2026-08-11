#![forbid(unsafe_code)]
#![allow(
    deprecated,
    reason = "Phase AC keeps the shared storage traits as a transitional contract while the backend boundary settles"
)]

//! SQLite-backed storage backend implementing the shared `atm-storage`
//! message and roster contracts.

#[cfg(test)]
mod mailbox_metadata;
mod nudge_template_override_store;
mod observability;
mod peer_config_store;
mod roster_store;
mod shared_db;
mod template_catalog_schema;
mod template_catalog_store;
mod writer;

#[cfg(test)]
use crate::mailbox_metadata::query_mailbox_metadata_rows;
pub use crate::observability::{
    NullSqliteObservability, SqliteObservability, SqliteObservabilityEvent,
    SqliteObservabilityOutcome,
};
use atm_storage::TemplateCatalogStore;
use atm_storage::contract::{
    AcknowledgementCommit, AcknowledgementReplyBuilder, AcknowledgementSource, AsyncMessageStore,
    MailboxBucketCounts, Message, MessageKey, MessageQuery, MessageStore, PeerConfigStore,
    RosterStore,
};
use atm_storage::schema::MessageEnvelope;
#[cfg(test)]
use atm_storage::schema::{AtmMessageId, ThreadMode};
use atm_storage::types::{AgentName, TeamName};
use atm_storage::{AtmError, IsoTimestamp, StorageFactory, StorageHandles};
use rusqlite::{Connection, OptionalExtension, params};
use shared_db::{SharedDb, deserialize_json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use template_catalog_store::SqliteTemplateCatalogStore;

#[cfg(any(test, feature = "test-support"))]
#[derive(Debug)]
pub(crate) struct SqliteWriterLockGuard {
    connection: Connection,
}

#[cfg(any(test, feature = "test-support"))]
impl Drop for SqliteWriterLockGuard {
    fn drop(&mut self) {
        let _ = self.connection.execute_batch("ROLLBACK;");
    }
}

#[doc(hidden)]
#[cfg(any(test, feature = "test-support"))]
pub struct TestOnlySqliteWriterLockGuard {
    _guard: SqliteWriterLockGuard,
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn hold_sqlite_writer_lock(
    path: impl AsRef<Path>,
) -> Result<SqliteWriterLockGuard, AtmError> {
    let connection = Connection::open(path.as_ref()).map_err(|_error| {
        AtmError::daemon_unavailable("failed to open sqlite writer lock connection")
    })?;
    connection
        .execute_batch("BEGIN IMMEDIATE;")
        .map_err(|_error| {
            AtmError::daemon_unavailable("failed to begin sqlite writer lock transaction")
        })?;
    Ok(SqliteWriterLockGuard { connection })
}

#[doc(hidden)]
#[cfg(any(test, feature = "test-support"))]
pub fn hold_sqlite_writer_lock_for_test(
    path: impl AsRef<Path>,
) -> Result<TestOnlySqliteWriterLockGuard, AtmError> {
    hold_sqlite_writer_lock(path).map(|guard| TestOnlySqliteWriterLockGuard { _guard: guard })
}

/// Installs a test-only SQLite trigger that deterministically rejects mailbox
/// inserts.  Unlike a competing writer lock, this exercises the real writer
/// error path without depending on platform-specific busy-timeout scheduling.
#[doc(hidden)]
#[cfg(any(test, feature = "test-support"))]
pub fn install_message_write_failure_for_test(path: impl AsRef<Path>) -> Result<(), AtmError> {
    let connection = Connection::open(path.as_ref()).map_err(|_error| {
        AtmError::daemon_unavailable("failed to open sqlite storage-failure test connection")
    })?;
    connection
        .execute_batch(
            r#"
            CREATE TRIGGER fail_test_mail_message_insert
            BEFORE INSERT ON mail_messages
            BEGIN
                SELECT RAISE(ABORT, 'intentional test mailbox write failure');
            END;
            "#,
        )
        .map_err(|_error| {
            AtmError::daemon_unavailable("failed to install sqlite storage-failure test trigger")
        })
}

#[cfg(test)]
#[allow(
    dead_code,
    reason = "metadata positive-path fields are owned by the query DTO while current tests exercise malformed-row validation"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SqliteMailboxMetadataRow {
    pub message_key: MessageKey,
    pub message_id: Option<AtmMessageId>,
    pub parent_message_id: Option<AtmMessageId>,
    pub thread_mode: Option<ThreadMode>,
    pub from_agent: AgentName,
    pub source_chat_id: Option<atm_storage::types::ChatId>,
    pub destination_chat_id: Option<atm_storage::types::ChatId>,
    pub summary: Option<String>,
    pub message_at: IsoTimestamp,
    pub read: bool,
    pub requires_ack: bool,
    pub pending_ack: bool,
    pub acknowledged_at: Option<IsoTimestamp>,
    pub expires_at: Option<IsoTimestamp>,
    pub task_id: Option<atm_storage::types::TaskId>,
}

#[derive(Debug)]
struct SqliteMessageStore {
    db: Arc<SharedDb>,
}

#[derive(Debug)]
struct SqliteRosterStore {
    db: Arc<SharedDb>,
}

#[derive(Debug)]
struct SqliteNudgeTemplateOverrideStore {
    db: Arc<SharedDb>,
}

#[derive(Debug)]
struct SqlitePeerConfigStore {
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
        raw.map(|value| value.parse::<IsoTimestamp>())
            .transpose()
            .map_err(|error| {
                AtmError::validation(format!(
                    "failed to parse mail-store {field_name} timestamp: {error}"
                ))
            })
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

impl atm_storage::contract::sealed::Sealed for SqliteMessageStore {}
impl atm_storage::contract::sealed::Sealed for SqliteRosterStore {}

impl MessageStore for SqliteMessageStore {
    fn save_message(&self, message: &Message) -> Result<(), AtmError> {
        self.db.submit_upsert_message(message.clone()).map(|_| ())
    }

    fn save_message_if_absent(&self, message: &Message) -> Result<Option<Message>, AtmError> {
        if self.db.submit_upsert_message(message.clone())? {
            return Ok(None);
        }
        self.load_message(&message.message_key)?.ok_or_else(|| {
            AtmError::daemon_unavailable(
                "sqlite writer reported an existing message key but the retained record could not be loaded",
            )
        }).map(Some)
    }

    fn save_messages_atomically(&self, messages: &[Message]) -> Result<(), AtmError> {
        self.db.submit_upsert_messages_atomically(messages.to_vec())
    }

    fn acknowledge_message_atomically(
        &self,
        source: &AcknowledgementSource,
        builder: Arc<dyn AcknowledgementReplyBuilder>,
    ) -> Result<AcknowledgementCommit, AtmError> {
        self.db.submit_acknowledgement(source.clone(), builder)
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
                })?;
                let agent: AgentName = agent.parse().map_err(|error| {
                    AtmError::validation(format!(
                        "failed to parse sqlite agent for message {key}: {error}"
                    ))
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

    fn mailbox_bucket_counts(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Option<MailboxBucketCounts>, AtmError> {
        self.db.with_connection(|connection| {
            let sql = "WITH visible AS (
                    SELECT
                        mail_messages.message_id,
                        mail_messages.parent_message_id,
                        COALESCE(
                            mail_message_states.read,
                            json_extract(mail_messages.envelope_json, '$.read'),
                            0
                        ) AS is_read,
                        mail_message_states.pending_ack_at,
                        mail_message_states.acknowledged_at,
                        mail_message_states.expires_at
                    FROM mail_messages
                    LEFT JOIN mail_message_states
                      ON mail_message_states.team = mail_messages.team
                     AND mail_message_states.agent = mail_messages.agent
                     AND mail_message_states.message_key = mail_messages.message_key
                    WHERE mail_messages.team = ?1
                      AND mail_messages.agent = ?2
                      AND mail_message_states.deleted_at IS NULL
                      AND (
                           mail_message_states.expires_at IS NULL
                           OR mail_message_states.expires_at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                      )
                 ), terminal AS (
                    SELECT visible.*
                    FROM visible
                    WHERE visible.message_id IS NULL
                       OR NOT EXISTS (
                            SELECT 1
                            FROM visible AS successor
                            WHERE successor.parent_message_id = visible.message_id
                       )
                 ), displayable AS (
                    SELECT *
                    FROM terminal
                    WHERE expires_at IS NULL OR is_read = 0
                 )
                 SELECT
                    COALESCE(SUM(CASE
                        WHEN pending_ack_at IS NOT NULL AND acknowledged_at IS NULL THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE
                        WHEN NOT (pending_ack_at IS NOT NULL AND acknowledged_at IS NULL)
                         AND is_read = 0 THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE
                        WHEN NOT (pending_ack_at IS NOT NULL AND acknowledged_at IS NULL)
                         AND is_read != 0 THEN 1 ELSE 0 END), 0)
                 FROM displayable";
            let (pending_ack, unread, history): (i64, i64, i64) = connection
                .query_row(sql, params![team.as_str(), agent.as_str()], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })
                .map_err(|error| {
                    self.db
                        .error("failed to aggregate sqlite mailbox bucket counts", error)
                })?;
            Ok(Some(MailboxBucketCounts {
                unread: usize::try_from(unread).map_err(|_| {
                    AtmError::validation("sqlite unread mailbox count exceeds usize range")
                })?,
                pending_ack: usize::try_from(pending_ack).map_err(|_| {
                    AtmError::validation("sqlite pending-ack mailbox count exceeds usize range")
                })?,
                history: usize::try_from(history).map_err(|_| {
                    AtmError::validation("sqlite history mailbox count exceeds usize range")
                })?,
            }))
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

#[async_trait::async_trait]
impl AsyncMessageStore for SqliteMessageStore {
    async fn list_messages_async(&self, query: MessageQuery) -> Result<Vec<Message>, AtmError> {
        self.db.submit_list_messages_async(query).await
    }

    async fn save_message_if_absent_async(
        &self,
        message: Message,
    ) -> Result<Option<Message>, AtmError> {
        self.db.submit_upsert_message_async(message).await
    }

    async fn acknowledge_message_atomically_async(
        &self,
        source: AcknowledgementSource,
        builder: Arc<dyn AcknowledgementReplyBuilder>,
    ) -> Result<AcknowledgementCommit, AtmError> {
        self.db.submit_acknowledgement_async(source, builder).await
    }
}

#[derive(Clone, Debug)]
pub struct SqliteStorageBackend {
    message_store: Arc<SqliteMessageStore>,
    roster_store: Arc<SqliteRosterStore>,
    nudge_template_override_store: Arc<SqliteNudgeTemplateOverrideStore>,
    peer_config_store: Arc<SqlitePeerConfigStore>,
    template_catalog_store: Arc<SqliteTemplateCatalogStore>,
}

/// Concrete SQLite selection owned by the SQLite backend and consumed only at
/// an executable composition root through [`StorageFactory`].
#[derive(Debug, Clone, Default)]
pub struct SqliteStorageFactory {
    database_path: Option<PathBuf>,
}

impl SqliteStorageFactory {
    pub fn host_scoped() -> Self {
        Self::default()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn at_path(path: impl Into<PathBuf>) -> Self {
        Self {
            database_path: Some(path.into()),
        }
    }

    fn database_path(&self, durable_state_root: &Path) -> PathBuf {
        self.database_path
            .clone()
            .unwrap_or_else(|| durable_state_root.join("mail.db"))
    }
}

impl StorageFactory for SqliteStorageFactory {
    fn open(&self, durable_state_root: &Path) -> Result<StorageHandles, AtmError> {
        let backend = SqliteStorageBackend::new(self.database_path(durable_state_root))?;
        Ok(StorageHandles::new(
            backend.message_store(),
            backend.async_message_store(),
            backend.roster_store(),
            backend.nudge_template_override_store(),
            backend.peer_config_store(),
            backend.template_catalog_store(),
        ))
    }
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
            roster_store: Arc::new(SqliteRosterStore::new(Arc::clone(&db))),
            nudge_template_override_store: Arc::new(SqliteNudgeTemplateOverrideStore::new(
                Arc::clone(&db),
            )),
            peer_config_store: Arc::new(SqlitePeerConfigStore::new(Arc::clone(&db))),
            template_catalog_store: Arc::new(SqliteTemplateCatalogStore::new(Arc::clone(&db))),
        })
    }

    #[cfg(test)]
    fn in_memory_for_test() -> Result<Self, AtmError> {
        let db = Arc::new(SharedDb::open_in_memory_for_test()?);
        Ok(Self {
            message_store: Arc::new(SqliteMessageStore::new(Arc::clone(&db))),
            roster_store: Arc::new(SqliteRosterStore::new(Arc::clone(&db))),
            nudge_template_override_store: Arc::new(SqliteNudgeTemplateOverrideStore::new(
                Arc::clone(&db),
            )),
            peer_config_store: Arc::new(SqlitePeerConfigStore::new(Arc::clone(&db))),
            template_catalog_store: Arc::new(SqliteTemplateCatalogStore::new(Arc::clone(&db))),
        })
    }

    pub fn message_store(&self) -> Arc<dyn MessageStore + Send + Sync> {
        self.message_store.clone()
    }

    pub fn async_message_store(&self) -> Arc<dyn AsyncMessageStore + Send + Sync> {
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

    pub fn roster_store(&self) -> Arc<dyn RosterStore + Send + Sync> {
        self.roster_store.clone()
    }

    pub fn nudge_template_override_store(
        &self,
    ) -> Arc<dyn atm_storage::NudgeTemplateOverrideStore + Send + Sync> {
        self.nudge_template_override_store.clone()
    }

    pub fn peer_config_store(&self) -> Arc<dyn PeerConfigStore + Send + Sync> {
        self.peer_config_store.clone()
    }

    pub fn template_catalog_store(&self) -> Arc<dyn TemplateCatalogStore + Send + Sync> {
        self.template_catalog_store.clone()
    }

    #[cfg(test)]
    pub(crate) fn shared_db_for_test(&self) -> Arc<SharedDb> {
        Arc::clone(&self.nudge_template_override_store.db)
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

    #[cfg(test)]
    pub(crate) fn query_mailbox_metadata(
        &self,
        team: &TeamName,
        agent: &AgentName,
        limit: Option<usize>,
    ) -> Result<Vec<SqliteMailboxMetadataRow>, AtmError> {
        query_mailbox_metadata_rows(&self.message_store.db, team, agent, limit)
    }

    pub fn inspect_mail_store(&self) -> Result<(), AtmError> {
        self.message_store.db.with_connection(|_| Ok(()))
    }

    pub fn inspect_roster_store(&self) -> Result<(), AtmError> {
        self.roster_store.db.with_connection(|_| Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::SqliteStorageBackend;
    use atm_storage::contract::{
        AcknowledgementReplyBuilder, AcknowledgementSource, AgentType, Message, MessageKey,
        MessageQuery, RosterHarness, RosterMember, RosterMemberKind, RosterSnapshot,
    };
    use atm_storage::schema::{AtmMessageId, MessageEnvelope};
    use atm_storage::types::{AgentName, IsoTimestamp, ModelName, TeamName};
    use atm_storage::{
        AtmError, DecomposedMessageAdmission, DecomposedMessageAdmissionOutcome,
        DecomposedMessageRecord, MergedVarsJson, TemplateFirstSeen, TemplateFrontmatter,
        TemplateRegistration, TemplateRegistrationOutcome, TemplateSha,
    };
    use chrono::Utc;
    use rusqlite::{Connection, params};
    use serde_json::Map;
    use std::sync::Arc;

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
                source_chat_id: None,
                text: text.to_string(),
                timestamp: IsoTimestamp::from_datetime(Utc::now()),
                read: false,
                source_team: Some(team),
                destination_chat_id: None,
                summary: None,
                message_id: None,
                requires_ack: false,
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

    fn template_registration(sha_seed: char) -> TemplateRegistration {
        let content_bytes = b"---\nmetadata:\n  type: task\n---\nhello {{ name }}\n".to_vec();
        TemplateRegistration {
            sha: TemplateSha::new(sha_seed.to_string().repeat(64)).expect("template sha"),
            template_type: Some("task".to_string()),
            template_name: Some("example".to_string()),
            content_text: String::from_utf8(content_bytes.clone()).expect("utf8 fixture"),
            content_bytes,
            frontmatter: TemplateFrontmatter::default(),
            first_seen: TemplateFirstSeen::new(IsoTimestamp::now(), "test-agent")
                .expect("first seen"),
        }
    }

    #[test]
    fn bundled_sqlite_exposes_fts5_for_the_template_catalog_gate() {
        let connection = Connection::open_in_memory().expect("open temporary SQLite database");
        connection
            .execute_batch(
                "CREATE VIRTUAL TABLE template_catalog_fts_gate USING fts5(template_text);",
            )
            .expect("atm template catalog requires bundled SQLite FTS5 support");
    }

    #[test]
    fn sqlite_template_catalog_round_trips_bytes_and_admits_a_decomposed_row_atomically() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let message = message("atm:decomposed-template", "inline before decomposition");
        backend
            .message_store()
            .save_message(&message)
            .expect("seed canonical message");
        let catalog = backend.template_catalog_store();
        let template = template_registration('a');

        assert_eq!(
            catalog.register(template.clone()).expect("register"),
            TemplateRegistrationOutcome::Inserted
        );
        assert_eq!(
            catalog
                .register(template.clone())
                .expect("idempotent register"),
            TemplateRegistrationOutcome::AlreadyRegistered
        );
        let loaded = catalog
            .load(&template.sha)
            .expect("load")
            .expect("template exists");
        assert_eq!(loaded.content_bytes, template.content_bytes);
        assert_eq!(loaded.content_text, template.content_text);

        let vars = MergedVarsJson::try_from_merged_object(
            [("name".to_string(), serde_json::json!("Rand"))]
                .into_iter()
                .collect(),
        )
        .expect("vars");
        let outcome = catalog
            .admit_decomposed_message(DecomposedMessageAdmission {
                template: template.clone(),
                message: DecomposedMessageRecord {
                    key: message.message_key.clone(),
                    template_sha: template.sha.clone(),
                    vars,
                    category: Some("assignment".to_string()),
                    tags: vec!["phase-an".to_string()],
                    content_format: Some("markdown".to_string()),
                },
            })
            .expect("atomic admission");
        assert_eq!(
            outcome,
            DecomposedMessageAdmissionOutcome::Inserted {
                template: TemplateRegistrationOutcome::AlreadyRegistered
            }
        );
        backend
            .shared_db_for_test()
            .with_connection(|connection| {
                let row = connection
                    .query_row(
                        "SELECT template_sha, vars_json FROM decomposed_messages
                         WHERE team = ?1 AND agent = ?2",
                        params![message.team.as_str(), message.agent.as_str()],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                    )
                    .map_err(|error| {
                        AtmError::mailbox_read(format!("view query failed: {error}"))
                    })?;
                assert_eq!(row.0, template.sha.as_str());
                assert_eq!(row.1, r#"{"name":"Rand"}"#);
                let message_text = connection
                    .query_row(
                        "SELECT message_text FROM mail_messages WHERE message_key = ?1",
                        params![message.message_key.as_str()],
                        |row| row.get::<_, Option<String>>(0),
                    )
                    .map_err(|error| {
                        AtmError::mailbox_read(format!("mail row query failed: {error}"))
                    })?;
                assert_eq!(message_text, None);
                Ok(())
            })
            .expect("view exposes decomposed state");
    }

    #[test]
    fn failed_decomposed_update_rolls_back_its_new_template_registration() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let message = message("atm:decomposed-rollback", "inline");
        backend
            .message_store()
            .save_message(&message)
            .expect("seed canonical message");
        backend
            .shared_db_for_test()
            .with_connection(|connection| {
                connection
                    .execute_batch(
                        "CREATE TRIGGER fail_decomposed_update
                         BEFORE UPDATE OF template_sha ON mail_messages
                         BEGIN SELECT RAISE(ABORT, 'intentional decomposed failure'); END;",
                    )
                    .map_err(|error| {
                        AtmError::mailbox_write(format!("install trigger failed: {error}"))
                    })
            })
            .expect("install failure trigger");
        let catalog = backend.template_catalog_store();
        let template = template_registration('b');
        let _error = catalog
            .admit_decomposed_message(DecomposedMessageAdmission {
                template: template.clone(),
                message: DecomposedMessageRecord {
                    key: message.message_key,
                    template_sha: template.sha.clone(),
                    vars: MergedVarsJson::try_from_merged_object(Default::default()).expect("vars"),
                    category: None,
                    tags: vec![],
                    content_format: None,
                },
            })
            .expect_err("update trigger rejects admission");
        assert!(catalog.load(&template.sha).expect("load").is_none());
    }

    #[test]
    fn shared_inbox_envelope_does_not_gain_template_storage_fields() {
        let envelope = message("atm:shared-inbox-guard", "inline").envelope;
        let serialized = serde_json::to_value(envelope).expect("serialize shared envelope");
        for field in [
            "template_sha",
            "vars_json",
            "category",
            "content_format",
            "tags_json",
        ] {
            assert!(
                serialized.get(field).is_none(),
                "{field} must remain storage-only"
            );
        }
    }

    #[test]
    fn fresh_and_historical_databases_expose_the_same_decomposed_query_surface() {
        let fresh_root = tempfile::tempdir().expect("fresh root");
        let fresh_path = fresh_root.path().join("fresh.db");
        let _fresh = SqliteStorageBackend::new(&fresh_path).expect("fresh backend");

        let historical_root = tempfile::tempdir().expect("historical root");
        let historical_path = historical_root.path().join("historical.db");
        Connection::open(&historical_path)
            .expect("open historical fixture")
            .execute_batch(
                "CREATE TABLE mail_messages (
                    team TEXT NOT NULL, agent TEXT NOT NULL, message_key TEXT NOT NULL,
                    envelope_json TEXT NOT NULL, from_agent TEXT NOT NULL,
                    source_chat_id TEXT NULL, destination_chat_id TEXT NULL,
                    message_text TEXT NULL, summary TEXT NULL, message_at TEXT NOT NULL,
                    message_id TEXT NULL, parent_message_id TEXT NULL, thread_mode TEXT NULL,
                    recorded_at TEXT NULL,
                    PRIMARY KEY (team, agent, message_key)
                 );",
            )
            .expect("historical nullable-message-text fixture");
        let _historical = SqliteStorageBackend::new(&historical_path).expect("migrate fixture");

        let surface = |path: &std::path::Path| {
            let connection = Connection::open(path).expect("inspect schema");
            let columns = connection
                .prepare("PRAGMA table_info(decomposed_messages)")
                .expect("prepare view info")
                .query_map([], |row| row.get::<_, String>(1))
                .expect("query view info")
                .collect::<Result<Vec<_>, _>>()
                .expect("decode view columns");
            let message_text_not_null = connection
                .query_row(
                    "SELECT \"notnull\" FROM pragma_table_info('mail_messages') WHERE name = 'message_text'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .expect("message_text metadata");
            (columns, message_text_not_null)
        };
        let expected_columns = vec![
            "team",
            "agent",
            "from_agent",
            "message_at",
            "message_id",
            "template_sha",
            "template_type",
            "vars_json",
            "category",
            "tags_json",
            "summary",
            "read",
            "acknowledged_at",
            "pending_ack_at",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
        assert_eq!(surface(&fresh_path), surface(&historical_path));
        assert_eq!(surface(&fresh_path).0, expected_columns);
        assert_eq!(
            surface(&fresh_path).1,
            0,
            "fresh DDL keeps message_text nullable"
        );
    }

    #[test]
    fn mailbox_bucket_counts_aggregate_without_loading_message_bodies() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.message_store();
        let mut unread = message("atm:unread", "unread");
        let mut pending_ack = message("atm:pending", "pending");
        let mut history = message("atm:history", "history");
        pending_ack.envelope.requires_ack = true;
        pending_ack.envelope.pending_ack_at = Some(IsoTimestamp::from_datetime(Utc::now()));
        history.envelope.read = true;
        for record in [&mut unread, &mut pending_ack, &mut history] {
            store.save_message(record).expect("save message");
        }

        let counts = store
            .mailbox_bucket_counts(&team(), &agent())
            .expect("aggregate counts")
            .expect("sqlite aggregate is available");

        assert_eq!(counts.unread, 1);
        assert_eq!(counts.pending_ack, 1);
        assert_eq!(counts.history, 1);
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
    fn sqlite_backend_enforces_message_id_uniqueness_at_the_indexed_insert() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.message_store();
        let message_id = AtmMessageId::new();
        let mut first = message("atm:unique-id-first", "first");
        first.envelope.message_id = Some(message_id);
        let mut conflicting = message("atm:unique-id-conflicting", "conflicting");
        conflicting.envelope.message_id = Some(message_id);

        store.save_message(&first).expect("save first message");
        let error = store
            .save_message(&conflicting)
            .expect_err("unique message id must reject a distinct immutable key");
        assert_eq!(
            error.code(),
            atm_storage::AtmErrorCode::MessageValidationFailed
        );
        assert!(error.message().contains("uniqueness invariant"));
    }

    #[test]
    fn sqlite_backend_admits_a_message_once_and_returns_the_existing_record() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.message_store();
        let original = message("atm:admit-once", "immutable payload");

        assert_eq!(
            store
                .save_message_if_absent(&original)
                .expect("first atomic admission"),
            None,
            "the first atomic admission writes the message"
        );
        assert_eq!(
            store
                .save_message_if_absent(&original)
                .expect("duplicate atomic admission"),
            Some(original.clone()),
            "a duplicate returns the record that won the immutable key"
        );
        assert_eq!(
            store
                .load_message(&original.message_key)
                .expect("load stored message"),
            Some(original),
            "a duplicate admission does not replace the original immutable record"
        );
    }

    #[tokio::test]
    async fn sqlite_backend_async_admission_is_idempotent_and_uses_the_writer_lane() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.async_message_store();
        let original = message("atm:async-admit-once", "immutable payload");

        assert_eq!(
            store
                .save_message_if_absent_async(original.clone())
                .await
                .expect("first async admission"),
            None,
            "first admission is durable through the writer lane"
        );
        assert_eq!(
            store
                .save_message_if_absent_async(original.clone())
                .await
                .expect("duplicate async admission"),
            Some(original),
            "duplicate admission receives the existing immutable record"
        );
    }

    #[tokio::test]
    async fn sqlite_backend_async_mailbox_projection_uses_the_writer_lane() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.async_message_store();
        let first = message("atm:async-projection-first", "first");
        let second = message("atm:async-projection-second", "second");
        backend
            .message_store()
            .save_messages_atomically(&[first.clone(), second.clone()])
            .expect("seed mailbox");

        let projection = store
            .list_messages_async(MessageQuery {
                team: team(),
                agent: agent(),
                sender: None,
                task_id: None,
                limit: None,
            })
            .await
            .expect("async writer-owned mailbox projection");

        assert_eq!(projection.len(), 2);
        assert!(projection.contains(&first));
        assert!(projection.contains(&second));
    }

    #[test]
    fn sqlite_backend_commits_related_messages_through_one_writer_operation() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.message_store();
        let reply = message("atm:ack-reply", "reply");
        let mut source = message("atm:ack-source", "source");
        source.envelope.read = true;
        source.envelope.pending_ack_at = None;
        source.envelope.acknowledged_at = Some(IsoTimestamp::now());

        store
            .save_messages_atomically(&[reply.clone(), source.clone()])
            .expect("commit acknowledgement reply and source together");

        assert_eq!(
            store.load_message(&reply.message_key).expect("load reply"),
            Some(reply),
            "the immutable acknowledgement reply is durable"
        );
        assert_eq!(
            store
                .load_message(&source.message_key)
                .expect("load source"),
            Some(source),
            "the acknowledged source state is durable in the same commit"
        );
    }

    #[test]
    fn sqlite_acknowledgement_resolves_source_and_commits_pair_in_one_writer_operation() {
        struct ReplyBuilder;

        impl AcknowledgementReplyBuilder for ReplyBuilder {
            fn build_reply(&self, source: &Message) -> Result<Message, atm_storage::AtmError> {
                let source_id = source
                    .envelope
                    .message_id
                    .ok_or_else(|| atm_storage::AtmError::validation("test source has no id"))?;
                let mut reply = source.clone();
                let reply_id = AtmMessageId::new();
                reply.message_key = MessageKey::new(format!("atm:{reply_id}"))?;
                reply.envelope.message_id = Some(reply_id);
                reply.envelope.text = "acknowledged".to_string();
                reply.envelope.read = false;
                reply.envelope.requires_ack = false;
                reply.envelope.pending_ack_at = None;
                reply.envelope.acknowledged_at = None;
                reply.envelope.acknowledges_message_id = Some(source_id);
                reply.envelope.parent_message_id = Some(source_id);
                Ok(reply)
            }
        }

        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.message_store();
        let source_id = AtmMessageId::new();
        let mut source = message(&format!("atm:{source_id}"), "needs acknowledgement");
        source.envelope.message_id = Some(source_id);
        source.envelope.requires_ack = true;
        source.envelope.pending_ack_at = Some(IsoTimestamp::now());
        store.save_message(&source).expect("save pending source");

        let committed = store
            .acknowledge_message_atomically(
                &AcknowledgementSource {
                    team: source.team.clone(),
                    agent: source.agent.clone(),
                    message_id: source_id,
                },
                Arc::new(ReplyBuilder),
            )
            .expect("atomic acknowledgement");

        assert!(committed.source.envelope.read);
        assert!(committed.source.envelope.pending_ack_at.is_none());
        assert!(committed.source.envelope.acknowledged_at.is_some());
        assert_eq!(
            store
                .load_message(&source.message_key)
                .expect("load source"),
            Some(committed.source),
            "the source transition is durable with the reply"
        );
        assert_eq!(
            store
                .load_message(&committed.reply.message_key)
                .expect("load reply"),
            Some(committed.reply),
            "the reply derived from the transaction-loaded source is durable"
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
        assert!(error.message().contains("failed to parse sqlite team"));
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
                .message()
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
                .message()
                .contains("failed to parse canonical team-roster model")
        );
    }
}
