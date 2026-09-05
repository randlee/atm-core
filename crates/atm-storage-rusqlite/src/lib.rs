#![forbid(unsafe_code)]
#![allow(
    deprecated,
    reason = "Phase AC keeps the shared storage traits as a transitional contract while the backend boundary settles"
)]

//! SQLite-backed storage backend implementing the shared `atm-storage`
//! message and roster contracts.

mod analyst_query;
mod control_path_pool;
mod diagnostic_timeline;
mod graft_receiver_endpoint_schema;
mod graft_receiver_endpoint_store;
mod mail_messages_schema;
#[cfg(test)]
mod mailbox_metadata;
mod mailbox_reader;
mod nudge_template_override_store;
mod observability;
mod peer_config_store;
mod pending_nudge_store;
mod reader_pool;
mod roster_store;
mod schema_support;
mod search_reader;
mod search_schema;
mod search_store;
mod shared_db;
mod shared_db_diagnostics;
mod shared_db_reader_lanes;
mod team_roster_schema;
mod template_catalog_schema;
mod template_catalog_store;
mod writer;

pub use reader_pool::{ReaderLaneMetricsSnapshot, ReaderLanesMetricsSnapshot};

pub use crate::analyst_query::open_analyst_query_store;
#[cfg(feature = "test-support")]
pub use crate::analyst_query::{
    create_an8_analyst_query_fixture_for_test, create_an12_workflow_query_fixture_for_test,
    create_analyst_query_fixture_for_test,
};
#[cfg(test)]
use crate::mailbox_metadata::query_mailbox_metadata_rows;
pub use crate::observability::{
    NullSqliteObservability, SqliteObservability, SqliteObservabilityEvent,
    SqliteObservabilityOutcome,
};
use atm_storage::contract::{
    AcknowledgementCommit, AcknowledgementReplyBuilder, AcknowledgementSource, AsyncMailboxReader,
    AsyncMessageStore, GraftReceiverEndpointStore, MailboxBucketCounts, MailboxScope, Message,
    MessageKey, MessageQuery, MessageStore, PeerConfigStore, RosterStore,
};
use atm_storage::schema::MessageEnvelope;
#[cfg(test)]
use atm_storage::schema::{AtmMessageId, ThreadMode};
use atm_storage::types::{AgentName, TeamName};
use atm_storage::{AsyncMessageSearchStore, MessageSearchStore, TemplateCatalogStore};
use atm_storage::{
    AtmError, EffectiveReaderLane, EffectiveReaderLanes, IsoTimestamp, StorageFactory,
    StorageHandleParts, StorageHandles,
};
pub use diagnostic_timeline::{
    DIAGNOSTIC_DETAIL_MAX_BYTES, DIAGNOSTIC_MAX_AGE_DAYS, DIAGNOSTIC_MAX_ROWS,
    DIAGNOSTIC_PRUNE_BATCH, SqliteDiagnosticTimeline,
};
use graft_receiver_endpoint_store::SqliteGraftReceiverEndpointStore;
use rusqlite::{Connection, OptionalExtension, params};
use search_schema::delete_message_projection;
use search_store::{async_search_store, search_store};
use shared_db::{SharedDb, deserialize_json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use template_catalog_store::template_catalog_store;

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

/// Test-only projection of template-admission state.
///
/// Keeping this probe in SQLite test support lets the replacement HTTP runtime
/// prove durable rows without importing SQLite or opening a database itself.
#[doc(hidden)]
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateAdmissionSnapshot {
    pub template_count: usize,
    pub decomposed_count: usize,
    pub messages: Vec<TemplateAdmissionMessage>,
}

/// One durable message projection returned by [`TemplateAdmissionSnapshot`].
#[doc(hidden)]
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateAdmissionMessage {
    pub message_key: String,
    pub template_sha: Option<String>,
    pub vars_json: Option<String>,
    pub category: Option<String>,
    pub content_format: Option<String>,
    pub tags_json: String,
    pub message_text: Option<String>,
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

/// Reads only durable template-admission projections for black-box tests.
///
/// Production code must use the sealed storage contracts. This helper is
/// test-support-only so the Tokio HTTP runtime never owns a database handle.
#[doc(hidden)]
#[cfg(any(test, feature = "test-support"))]
pub fn inspect_template_admission_for_test(
    path: impl AsRef<Path>,
    message_keys: &[String],
) -> Result<TemplateAdmissionSnapshot, AtmError> {
    let connection = Connection::open(path.as_ref()).map_err(|error| {
        AtmError::mailbox_read(format!(
            "failed to inspect template-admission fixture: {error}"
        ))
    })?;
    let (template_count, decomposed_count): (i64, i64) = connection
        .query_row(
            "SELECT
                (SELECT COUNT(*) FROM message_templates),
                (SELECT COUNT(*) FROM decomposed_messages)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| {
            AtmError::mailbox_read(format!(
                "failed to count template-admission fixture rows: {error}"
            ))
        })?;
    let messages = message_keys
        .iter()
        .map(|message_key| {
            connection
                .query_row(
                    "SELECT message_key, template_sha, vars_json, category, content_format,
                            tags_json, message_text
                     FROM mail_messages WHERE message_key = ?1",
                    params![message_key],
                    |row| {
                        Ok(TemplateAdmissionMessage {
                            message_key: row.get(0)?,
                            template_sha: row.get(1)?,
                            vars_json: row.get(2)?,
                            category: row.get(3)?,
                            content_format: row.get(4)?,
                            tags_json: row.get(5)?,
                            message_text: row.get(6)?,
                        })
                    },
                )
                .map_err(|error| {
                    AtmError::mailbox_read(format!(
                        "failed to inspect template-admission message '{message_key}': {error}"
                    ))
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(TemplateAdmissionSnapshot {
        template_count: usize::try_from(template_count).map_err(|_| {
            AtmError::mailbox_read("template-admission fixture count exceeds usize range")
        })?,
        decomposed_count: usize::try_from(decomposed_count).map_err(|_| {
            AtmError::mailbox_read("template-admission fixture count exceeds usize range")
        })?,
        messages,
    })
}

/// Installs a test-only SQLite trigger that deterministically rejects mailbox
/// inserts.  Unlike a competing writer lock, this exercises the real writer
/// error path without depending on platform-specific busy-timeout scheduling.
#[doc(hidden)]
#[cfg(any(test, feature = "test-support"))]
pub fn install_message_write_failure_for_test(path: impl AsRef<Path>) -> Result<(), AtmError> {
    let connection = Connection::open(path.as_ref()).map_err(|error| {
        AtmError::daemon_unavailable("failed to open sqlite storage-failure test connection")
            .with_cause(error)
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
        .map_err(|error| {
            AtmError::daemon_unavailable("failed to install sqlite storage-failure test trigger")
                .with_cause(error)
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
struct SqlitePendingNudgeStore {
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
            delete_message_projection(transaction, self.db.target(), key.as_str())?;
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
    async fn apply_read_display_state_async(
        &self,
        scope: MailboxScope,
        message_ids: Vec<MessageKey>,
        seen_watermark: Option<IsoTimestamp>,
    ) -> Result<(), AtmError> {
        self.db
            .submit_read_display_state_async(scope, message_ids, seen_watermark)
            .await
    }

    async fn save_message_if_absent_async(
        &self,
        message: Message,
    ) -> Result<Option<Message>, AtmError> {
        self.db.submit_upsert_message_async(message).await
    }

    async fn admit_template_message_async(
        &self,
        admission: atm_storage::TemplateMessageAdmission,
    ) -> Result<Option<Message>, AtmError> {
        self.db
            .submit_template_message_admission_async(admission)
            .await
    }

    async fn acknowledge_message_atomically_async(
        &self,
        source: AcknowledgementSource,
        builder: Arc<dyn AcknowledgementReplyBuilder>,
    ) -> Result<AcknowledgementCommit, AtmError> {
        self.db.submit_acknowledgement_async(source, builder).await
    }
}

#[derive(Clone)]
pub struct SqliteStorageBackend {
    message_store: Arc<SqliteMessageStore>,
    roster_store: Arc<SqliteRosterStore>,
    nudge_template_override_store: Arc<SqliteNudgeTemplateOverrideStore>,
    pending_nudge_store: Arc<SqlitePendingNudgeStore>,
    graft_receiver_endpoint_store: Arc<SqliteGraftReceiverEndpointStore>,
    peer_config_store: Arc<SqlitePeerConfigStore>,
    template_catalog_store: Arc<dyn TemplateCatalogStore>,
    message_search_store: Arc<dyn MessageSearchStore>,
    async_message_search_store: Arc<dyn AsyncMessageSearchStore>,
    async_mailbox_reader: Arc<dyn AsyncMailboxReader>,
    diagnostic_timeline: Arc<SqliteDiagnosticTimeline>,
}

impl std::fmt::Debug for SqliteStorageBackend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SqliteStorageBackend")
            .finish_non_exhaustive()
    }
}

/// Concrete SQLite selection owned by the SQLite backend and consumed only at
/// an executable composition root through [`StorageFactory`].
#[derive(Debug, Clone, Default)]
pub struct SqliteStorageFactory {
    database_path: Option<PathBuf>,
    reader_lanes: reader_pool::ReaderLanesConfig,
}

impl SqliteStorageFactory {
    pub fn host_scoped() -> Self {
        Self::default()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn at_path(path: impl Into<PathBuf>) -> Self {
        Self {
            database_path: Some(path.into()),
            reader_lanes: reader_pool::ReaderLanesConfig::default(),
        }
    }

    #[allow(
        dead_code,
        reason = "The composition root uses default ReaderLanesConfig until AV.1b exposes config-file parsing; this builder seam is intentionally testable now."
    )]
    pub(crate) fn with_reader_lanes(
        mut self,
        reader_lanes: reader_pool::ReaderLanesConfig,
    ) -> Self {
        self.reader_lanes = reader_lanes;
        self
    }

    fn database_path(&self, durable_state_root: &Path) -> PathBuf {
        self.database_path
            .clone()
            .unwrap_or_else(|| durable_state_root.join("mail.db"))
    }
}

impl StorageFactory for SqliteStorageFactory {
    fn open(&self, durable_state_root: &Path) -> Result<StorageHandles, AtmError> {
        let effective_reader_lanes = EffectiveReaderLanes {
            mailbox: EffectiveReaderLane {
                pool_size: self.reader_lanes.mailbox.pool_size.get(),
                queue_depth: self.reader_lanes.mailbox.queue_depth.get(),
            },
            search: EffectiveReaderLane {
                pool_size: self.reader_lanes.search.pool_size.get(),
                queue_depth: self.reader_lanes.search.queue_depth.get(),
            },
        };
        let backend = SqliteStorageBackend::new_with_reader_lanes(
            self.database_path(durable_state_root),
            self.reader_lanes,
        )?;
        Ok(StorageHandles::from_parts(StorageHandleParts {
            message_store: backend.message_store(),
            async_message_store: backend.async_message_store(),
            async_mailbox_reader: backend.async_mailbox_reader(),
            roster_store: backend.roster_store(),
            nudge_template_override_store: backend.nudge_template_override_store(),
            pending_nudge_store: backend.pending_nudge_store(),
            graft_receiver_endpoint_store: backend.graft_receiver_endpoint_store(),
            peer_config_store: backend.peer_config_store(),
            template_catalog_store: backend.template_catalog_store(),
            message_search_store: backend.message_search_store(),
            async_message_search_store: backend.async_message_search_store(),
            effective_reader_lanes: Some(effective_reader_lanes),
        }))
    }
}

impl SqliteStorageBackend {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, AtmError> {
        Self::new_with_observability(path, Arc::new(NullSqliteObservability))
    }

    pub(crate) fn new_with_reader_lanes(
        path: impl AsRef<Path>,
        reader_lanes: reader_pool::ReaderLanesConfig,
    ) -> Result<Self, AtmError> {
        Self::new_with_observability_and_reader_lanes(
            path,
            Arc::new(NullSqliteObservability),
            reader_lanes,
        )
    }

    pub fn new_with_observability(
        path: impl AsRef<Path>,
        observability: Arc<dyn SqliteObservability>,
    ) -> Result<Self, AtmError> {
        Self::new_with_observability_and_reader_lanes(
            path,
            observability,
            reader_pool::ReaderLanesConfig::default(),
        )
    }

    fn new_with_observability_and_reader_lanes(
        path: impl AsRef<Path>,
        observability: Arc<dyn SqliteObservability>,
        reader_lanes: reader_pool::ReaderLanesConfig,
    ) -> Result<Self, AtmError> {
        let db = Arc::new(SharedDb::open_with_reader_lanes(
            path,
            observability,
            reader_lanes,
        )?);
        Ok(Self {
            diagnostic_timeline: Arc::new(SqliteDiagnosticTimeline::from_shared_db(Arc::clone(
                &db,
            ))),
            message_store: Arc::new(SqliteMessageStore::new(Arc::clone(&db))),
            roster_store: Arc::new(SqliteRosterStore::new(Arc::clone(&db))),
            nudge_template_override_store: Arc::new(SqliteNudgeTemplateOverrideStore::new(
                Arc::clone(&db),
            )),
            pending_nudge_store: Arc::new(SqlitePendingNudgeStore::new(Arc::clone(&db))),
            graft_receiver_endpoint_store: Arc::new(SqliteGraftReceiverEndpointStore::new(
                Arc::clone(&db),
            )),
            peer_config_store: Arc::new(SqlitePeerConfigStore::new(Arc::clone(&db))),
            template_catalog_store: template_catalog_store(Arc::clone(&db)),
            message_search_store: search_store(Arc::clone(&db)),
            async_message_search_store: async_search_store(Arc::clone(&db)),
            async_mailbox_reader: db.mailbox_reader(),
        })
    }

    #[cfg(test)]
    pub(crate) fn in_memory_for_test() -> Result<Self, AtmError> {
        let db = Arc::new(SharedDb::open_in_memory_for_test()?);
        Ok(Self {
            diagnostic_timeline: Arc::new(SqliteDiagnosticTimeline::from_shared_db(Arc::clone(
                &db,
            ))),
            message_store: Arc::new(SqliteMessageStore::new(Arc::clone(&db))),
            roster_store: Arc::new(SqliteRosterStore::new(Arc::clone(&db))),
            nudge_template_override_store: Arc::new(SqliteNudgeTemplateOverrideStore::new(
                Arc::clone(&db),
            )),
            pending_nudge_store: Arc::new(SqlitePendingNudgeStore::new(Arc::clone(&db))),
            graft_receiver_endpoint_store: Arc::new(SqliteGraftReceiverEndpointStore::new(
                Arc::clone(&db),
            )),
            peer_config_store: Arc::new(SqlitePeerConfigStore::new(Arc::clone(&db))),
            template_catalog_store: template_catalog_store(Arc::clone(&db)),
            message_search_store: search_store(Arc::clone(&db)),
            async_message_search_store: async_search_store(Arc::clone(&db)),
            async_mailbox_reader: db.mailbox_reader(),
        })
    }

    pub fn message_store(&self) -> Arc<dyn MessageStore + Send + Sync> {
        self.message_store.clone()
    }

    pub fn async_message_store(&self) -> Arc<dyn AsyncMessageStore + Send + Sync> {
        self.message_store.clone()
    }

    pub fn async_mailbox_reader(&self) -> Arc<dyn AsyncMailboxReader + Send + Sync> {
        self.async_mailbox_reader.clone()
    }

    /// Best-effort diagnostic timeline sharing this backend's sole writer.
    pub fn diagnostic_timeline(&self) -> Arc<SqliteDiagnosticTimeline> {
        Arc::clone(&self.diagnostic_timeline)
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

    pub fn pending_nudge_store(&self) -> Arc<dyn atm_storage::PendingNudgeStore + Send + Sync> {
        self.pending_nudge_store.clone()
    }

    pub fn graft_receiver_endpoint_store(
        &self,
    ) -> Arc<dyn GraftReceiverEndpointStore + Send + Sync> {
        self.graft_receiver_endpoint_store.clone()
    }

    pub fn peer_config_store(&self) -> Arc<dyn PeerConfigStore + Send + Sync> {
        self.peer_config_store.clone()
    }

    pub fn template_catalog_store(&self) -> Arc<dyn TemplateCatalogStore + Send + Sync> {
        self.template_catalog_store.clone()
    }

    pub fn message_search_store(&self) -> Arc<dyn MessageSearchStore + Send + Sync> {
        self.message_search_store.clone()
    }

    pub fn async_message_search_store(&self) -> Arc<dyn AsyncMessageSearchStore + Send + Sync> {
        self.async_message_search_store.clone()
    }

    /// Rebuilds both external-content FTS projections from canonical durable
    /// rows. This is the backend half of `atm admin reindex-search`; the CLI
    /// command is deliberately added with AN.6's public command surface.
    pub fn reindex_search(&self) -> Result<(), AtmError> {
        self.message_store.db.with_transaction(|transaction| {
            search_schema::rebuild(transaction, self.message_store.db.target())
        })
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

    /// Samples mailbox/search reader-lane diagnostics without scheduling a
    /// reader operation. AV.4 records these values with benchmark campaigns.
    #[must_use]
    pub fn reader_lane_metrics(&self) -> ReaderLanesMetricsSnapshot {
        self.message_store.db.reader_lane_metrics()
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
    use super::{SqliteStorageBackend, SqliteStorageFactory};
    use crate::reader_pool::ReaderLanesConfig;
    use atm_storage::contract::{
        AcknowledgementReplyBuilder, AcknowledgementSource, AgentType, MailboxScope, Message,
        MessageKey, MessageQuery, ReadDeadline, ReadLaneError, RosterHarness, RosterMember,
        RosterMemberKind, RosterSnapshot,
    };
    use atm_storage::schema::{AtmMessageId, MessageEnvelope};
    use atm_storage::types::{AgentName, IsoTimestamp, ModelName, TeamName};
    use atm_storage::{
        AtmError, DecomposedMessageAdmission, DecomposedMessageAdmissionOutcome,
        DecomposedMessageRecord, InstanceTag, MergedVarsJson, MessageSearchQuery, SearchAtom,
        SearchDeadline, SearchExpression, SearchGroupBy, SearchGroupField, SearchKey, SearchLimit,
        SearchMetadataMatch, SearchValue, SimpleAggregate, StorageFactory, TemplateFirstSeen,
        TemplateFrontmatter, TemplateMessageAdmission, TemplateOutputFormat, TemplateRegistration,
        TemplateRegistrationOutcome, TemplateSha, WorkflowAdmission, WorkflowScopeId,
    };
    use chrono::Utc;
    use rusqlite::{Connection, OptionalExtension, params};
    use serde_json::Map;
    use std::sync::Arc;

    type OrdinaryMessageColumns = (
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    );

    fn team() -> TeamName {
        "test-team".parse().expect("team")
    }

    fn agent() -> AgentName {
        "test-agent".parse().expect("agent")
    }

    #[test]
    fn reader_lane_configuration_is_threaded_through_storage_factory_and_validated() {
        let root = tempfile::tempdir().expect("temporary storage root");
        let config = ReaderLanesConfig {
            max_connections: std::num::NonZeroUsize::new(21).expect("non-zero maximum connections"),
            ..ReaderLanesConfig::default()
        };
        let error = SqliteStorageFactory::at_path(root.path().join("mail.db"))
            .with_reader_lanes(config)
            .open(root.path())
            .expect_err("over-budget reader lane configuration must fail startup");
        let message = error.message();
        assert!(message.contains("mailbox_pool=4"));
        assert!(message.contains("max_connections=21"));
    }

    #[tokio::test]
    async fn reader_lane_metrics_export_wal_health_after_a_bounded_read() {
        let root = tempfile::tempdir().expect("temporary storage root");
        let backend = SqliteStorageBackend::new(root.path().join("mail.db")).expect("backend");
        backend
            .message_store()
            .save_message(&message("atm:metrics", "metrics"))
            .expect("writer admission");
        let scope = MailboxScope::new(team(), agent());
        backend
            .async_mailbox_reader()
            .list_messages(
                scope.clone(),
                MessageQuery {
                    team: scope.team,
                    agent: scope.agent,
                    sender: None,
                    task_id: None,
                    limit: None,
                },
                ReadDeadline::new(std::time::Duration::from_secs(1)).expect("deadline"),
            )
            .await
            .expect("bounded read");
        backend.checkpoint_wal().expect("checkpoint");
        let metrics = backend.reader_lane_metrics();
        let mailbox = metrics.lane("mailbox").expect("mailbox lane metrics");
        let search = metrics.lane("search").expect("search lane metrics");
        assert_eq!(metrics.iter().count(), 2);
        assert!(metrics.lane("doctor").is_none());
        assert_eq!(mailbox.lane, "mailbox");
        assert_eq!(search.lane, "search");
        assert_eq!(mailbox.last_checkpoint_succeeded, Some(true));
        assert!(mailbox.current_wal_frames.is_some());
        assert_eq!(search.last_checkpoint_succeeded, Some(true));
    }

    #[tokio::test]
    async fn sustained_reader_and_writer_load_releases_read_transactions_for_wal_checkpointing() {
        let root = tempfile::tempdir().expect("temporary storage root");
        let backend =
            Arc::new(SqliteStorageBackend::new(root.path().join("mail.db")).expect("backend"));
        let writer = backend.async_message_store();
        let reader = backend.async_mailbox_reader();
        let search = backend.async_message_search_store();
        let scope = MailboxScope::new(team(), agent());
        let (checkpoint_tx, mut checkpoint_rx) = tokio::sync::mpsc::unbounded_channel();
        let writer_task = tokio::spawn(async move {
            for index in 0..24 {
                writer
                    .save_message_if_absent_async(message(
                        &format!("atm:wal-{index}"),
                        "sustained writer",
                    ))
                    .await
                    .expect("writer admission");
                checkpoint_tx
                    .send(())
                    .expect("checkpoint sampler remains live");
            }
        });
        let reader_task = {
            let scope = scope.clone();
            tokio::spawn(async move {
                for _ in 0..24 {
                    reader
                        .list_messages(
                            scope.clone(),
                            MessageQuery {
                                team: scope.team.clone(),
                                agent: scope.agent.clone(),
                                sender: None,
                                task_id: None,
                                limit: Some(8),
                            },
                            ReadDeadline::new(std::time::Duration::from_secs(1)).expect("deadline"),
                        )
                        .await
                        .expect("reader response");
                }
            })
        };
        let search_task = tokio::spawn(async move {
            for _ in 0..24 {
                search
                    .search_async(
                        MessageSearchQuery::default(),
                        SearchDeadline::new(std::time::Duration::from_secs(1))
                            .expect("search deadline"),
                    )
                    .await
                    .expect("search reader response");
            }
        });
        let sampler_backend = Arc::clone(&backend);
        let checkpoint_task = tokio::spawn(async move {
            let mut samples = Vec::new();
            while checkpoint_rx.recv().await.is_some() {
                sampler_backend
                    .checkpoint_wal()
                    .expect("progressing checkpoint");
                let metrics = sampler_backend.reader_lane_metrics();
                let frames = metrics
                    .lane("mailbox")
                    .expect("mailbox lane metrics")
                    .current_wal_frames
                    .expect("checkpoint records mailbox WAL frames");
                assert!(frames <= 512, "WAL frames stay bounded during load");
                samples.push(frames);
            }
            samples
        });
        writer_task.await.expect("writer join");
        reader_task.await.expect("reader join");
        search_task.await.expect("search join");
        let samples = checkpoint_task.await.expect("checkpoint sampler join");
        assert_eq!(samples.len(), 24, "sample every writer commit during load");
        let metrics = backend.reader_lane_metrics();
        let mailbox = metrics.lane("mailbox").expect("mailbox lane metrics");
        let search = metrics.lane("search").expect("search lane metrics");
        assert_eq!(mailbox.current_quarantined_workers, 0);
        assert_eq!(mailbox.last_checkpoint_succeeded, Some(true));
        assert!(mailbox.current_wal_frames.is_some());
        assert_eq!(search.last_checkpoint_succeeded, Some(true));
        assert!(search.current_wal_frames.is_some());
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
            output_format: TemplateOutputFormat::Text,
            frontmatter: TemplateFrontmatter {
                metadata: [("kind".to_owned(), serde_json::json!("assignment"))]
                    .into_iter()
                    .collect(),
                ..TemplateFrontmatter::default()
            },
            first_seen: TemplateFirstSeen::new(IsoTimestamp::now(), "test-agent")
                .expect("first seen"),
        }
    }

    fn instance_tags(values: &[&str]) -> Vec<InstanceTag> {
        values
            .iter()
            .map(|value| InstanceTag::new(*value).expect("valid instance tag"))
            .collect()
    }

    fn workflow_template_registration(sha_seed: char, tags: &[&str]) -> TemplateRegistration {
        let mut template = template_registration(sha_seed);
        template.frontmatter.metadata = [
            ("tags".to_owned(), serde_json::json!(tags)),
            (
                "workflow".to_owned(),
                serde_json::json!({
                    "scope": { "kind": "sprint", "variable": "sprint" },
                    "state": "dev-start",
                    "stage": "development",
                    "transition": "started",
                    "iteration_variable": "iteration"
                }),
            ),
        ]
        .into_iter()
        .collect();
        template
            .into_normalized_workflow_metadata()
            .expect("valid workflow template")
    }

    fn workflow_admission(
        record: &Message,
        template: TemplateRegistration,
        instance: &[&str],
    ) -> DecomposedMessageAdmission {
        let vars = MergedVarsJson::from_merged_object(
            [
                ("sprint".to_owned(), serde_json::json!("an-10")),
                ("iteration".to_owned(), serde_json::json!(2)),
            ]
            .into_iter()
            .collect(),
        );
        let snapshot = atm_storage::WorkflowSnapshot {
            scope_kind: template
                .frontmatter
                .workflow
                .as_ref()
                .expect("workflow")
                .scope_kind
                .clone(),
            scope_id: WorkflowScopeId::new("an-10").expect("scope id"),
            state: template
                .frontmatter
                .workflow
                .as_ref()
                .expect("workflow")
                .state
                .clone(),
            stage: template
                .frontmatter
                .workflow
                .as_ref()
                .expect("workflow")
                .stage
                .clone(),
            transition: template
                .frontmatter
                .workflow
                .as_ref()
                .expect("workflow")
                .transition
                .clone(),
            iteration: Some(atm_storage::WorkflowIteration::new("2").expect("iteration")),
        };
        let mut admission = DecomposedMessageAdmission {
            template: template.clone(),
            message: DecomposedMessageRecord {
                key: record.message_key.clone(),
                template_sha: template.sha.clone(),
                vars,
                category: Some("assignment".to_owned()),
                tags: instance_tags(instance),
                content_format: Some("markdown".to_owned()),
                workflow: None,
            },
        };
        let tag_provenance = admission
            .expected_tag_provenance(&snapshot)
            .expect("canonical provenance");
        admission.message.workflow = Some(WorkflowAdmission {
            snapshot,
            tag_provenance,
        });
        admission
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
    fn workflow_admission_persists_canonical_snapshot_and_tag_provenance() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let record = message("atm:workflow-snapshot", "rendered body");
        backend
            .message_store()
            .save_message(&record)
            .expect("seed canonical message");
        let template = workflow_template_registration('a', &["phase:an", "component:catalog"]);
        let admission = workflow_admission(&record, template.clone(), &["audience:engineering"]);
        backend
            .template_catalog_store()
            .admit_decomposed_message(admission)
            .expect("atomic workflow admission");

        let stored = backend
            .template_catalog_store()
            .load_decomposed_message(&record.message_key)
            .expect("load")
            .expect("decomposed row");
        let workflow = stored.workflow.expect("workflow");
        let snapshot = &workflow.snapshot;
        assert_eq!(snapshot.scope_kind.as_str(), "sprint");
        assert_eq!(snapshot.scope_id.as_str(), "an-10");
        assert_eq!(
            snapshot.iteration.as_ref().expect("iteration").as_str(),
            "2"
        );
        let provenance = &workflow.tag_provenance;
        assert_eq!(
            provenance
                .applied_template_tags
                .iter()
                .map(|tag| tag.as_str())
                .collect::<Vec<_>>(),
            ["component:catalog", "phase:an"]
        );
        assert_eq!(
            provenance
                .effective_tags
                .iter()
                .map(|tag| tag.as_str())
                .collect::<Vec<_>>(),
            [
                "audience:engineering",
                "component:catalog",
                "content-format:markdown",
                "phase:an",
                "template-type:task",
                "workflow-scope-kind:sprint",
                "workflow-stage:development",
                "workflow-state:dev-start",
                "workflow-transition:started",
            ]
        );
        backend
            .shared_db_for_test()
            .with_connection(|connection| {
                let row: (Option<String>, Option<String>, Option<String>) = connection
                    .query_row(
                        "SELECT applied_template_tags_json, effective_tags_json,
                                (SELECT derived_tags_json FROM decomposed_messages
                                 WHERE template_sha = ?2)
                         FROM mail_messages WHERE message_key = ?1",
                        params![record.message_key.as_str(), template.sha.as_str()],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )
                    .map_err(|error| AtmError::mailbox_read(error.to_string()))?;
                assert_eq!(
                    row.0.as_deref(),
                    Some(r#"["component:catalog","phase:an"]"#)
                );
                assert!(row.1.is_some());
                let derived_json =
                    serde_json::to_string(&provenance.derived_tags).expect("derived tags JSON");
                assert_eq!(row.2.as_deref(), Some(derived_json.as_str()));
                Ok(())
            })
            .expect("durable workflow projection");
    }

    #[test]
    fn workflow_admission_rejects_mismatched_projection_without_mutation() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let record = message("atm:workflow-mismatch", "rendered body");
        backend
            .message_store()
            .save_message(&record)
            .expect("seed canonical message");
        let template = workflow_template_registration('b', &["phase:an"]);
        let mut admission =
            workflow_admission(&record, template.clone(), &["audience:engineering"]);
        admission
            .message
            .workflow
            .as_mut()
            .expect("workflow")
            .tag_provenance
            .effective_tags
            .clear();
        let error = backend
            .template_catalog_store()
            .admit_decomposed_message(admission)
            .expect_err("mismatch must reject before writer mutation");
        assert_eq!(error.code().as_str(), "ATM_MESSAGE_VALIDATION_FAILED");
        assert!(
            backend
                .template_catalog_store()
                .load(&template.sha)
                .expect("load")
                .is_none()
        );
        backend
            .shared_db_for_test()
            .with_connection(|connection| {
                let value: Option<String> = connection
                    .query_row(
                        "SELECT template_sha FROM mail_messages WHERE message_key = ?1",
                        params![record.message_key.as_str()],
                        |row| row.get(0),
                    )
                    .map_err(|error| AtmError::mailbox_read(error.to_string()))?;
                assert!(value.is_none());
                Ok(())
            })
            .expect("canonical row unchanged");
    }

    #[test]
    fn workflow_template_revisions_preserve_prior_snapshot_and_provenance() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let first_record = message("atm:workflow-revision-one", "first rendered body");
        let second_record = message("atm:workflow-revision-two", "second rendered body");
        backend
            .message_store()
            .save_message(&first_record)
            .expect("seed first message");
        backend
            .message_store()
            .save_message(&second_record)
            .expect("seed second message");
        let first_template = workflow_template_registration('c', &["phase:an"]);
        let second_template = workflow_template_registration('d', &["phase:ao"]);
        backend
            .template_catalog_store()
            .admit_decomposed_message(workflow_admission(
                &first_record,
                first_template.clone(),
                &["audience:engineering"],
            ))
            .expect("first workflow admission");
        backend
            .template_catalog_store()
            .admit_decomposed_message(workflow_admission(
                &second_record,
                second_template.clone(),
                &["audience:operations"],
            ))
            .expect("second workflow admission");

        let first = backend
            .template_catalog_store()
            .load_decomposed_message(&first_record.message_key)
            .expect("load first")
            .expect("first decomposition");
        let second = backend
            .template_catalog_store()
            .load_decomposed_message(&second_record.message_key)
            .expect("load second")
            .expect("second decomposition");
        assert_eq!(first.template_sha, first_template.sha);
        assert_eq!(second.template_sha, second_template.sha);
        assert_eq!(
            first
                .workflow
                .as_ref()
                .expect("workflow")
                .tag_provenance
                .applied_template_tags
                .iter()
                .map(|tag| tag.as_str())
                .collect::<Vec<_>>(),
            ["phase:an"]
        );
        assert_eq!(
            second
                .workflow
                .as_ref()
                .expect("workflow")
                .tag_provenance
                .applied_template_tags
                .iter()
                .map(|tag| tag.as_str())
                .collect::<Vec<_>>(),
            ["phase:ao"]
        );
    }

    #[test]
    fn retained_an12_fixture_admits_two_unrelated_workflow_vocabularies() {
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../../docs/plans/phase-an/fixtures/workflow-metadata-evidence.json"
        ))
        .expect("retained fixture corpus");
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let families = corpus["families"].as_array().expect("fixture families");
        let expected_aggregate_counts: std::collections::BTreeMap<
            String,
            std::collections::BTreeMap<String, u64>,
        > = serde_json::from_value(corpus["expected_aggregate_counts"].clone())
            .expect("expected aggregate counts");
        let mut seeded = 0_usize;

        for family in families {
            let name = family["name"].as_str().expect("family name");
            let scope_kind = family["scope_kind"].as_str().expect("scope kind");
            let scope_id = family["scope_id"].as_str().expect("scope id");
            let template_tags = family["template_tags"].clone();
            let fixture_instance_tags = family["instance_tags"]
                .as_array()
                .expect("instance tags")
                .iter()
                .map(|tag| tag.as_str().expect("tag"))
                .collect::<Vec<_>>();
            for event in family["events"].as_array().expect("events") {
                let id = event["id"].as_str().expect("event id");
                let mut record = message(&format!("atm:an12:{id}"), "retained rendered body");
                record.envelope.timestamp = event["at"]
                    .as_str()
                    .expect("timestamp")
                    .parse()
                    .expect("timestamp");
                backend
                    .message_store()
                    .save_message(&record)
                    .expect("seed canonical message");

                let mut template = template_registration(
                    ["a", "b", "c", "d", "e"][seeded]
                        .chars()
                        .next()
                        .expect("hex seed"),
                );
                template.template_type = Some(
                    family["template_type"]
                        .as_str()
                        .expect("template type")
                        .to_owned(),
                );
                template.frontmatter.metadata = [
                    ("tags".to_owned(), template_tags.clone()),
                    (
                        "workflow".to_owned(),
                        serde_json::json!({
                            "scope": { "kind": scope_kind, "variable": "scope" },
                            "state": event["state"],
                            "stage": event["stage"],
                            "transition": event["transition"],
                            "iteration_variable": "iteration"
                        }),
                    ),
                ]
                .into_iter()
                .collect();
                let template = template
                    .into_normalized_workflow_metadata()
                    .expect("normalize fixture template");
                let snapshot = atm_storage::WorkflowSnapshot {
                    scope_kind: atm_storage::WorkflowScopeKind::new(scope_kind)
                        .expect("scope kind"),
                    scope_id: WorkflowScopeId::new(scope_id).expect("scope id"),
                    state: atm_storage::WorkflowState::new(event["state"].as_str().expect("state"))
                        .expect("state"),
                    stage: atm_storage::WorkflowStage::new(event["stage"].as_str().expect("stage"))
                        .expect("stage"),
                    transition: atm_storage::WorkflowTransition::new(
                        event["transition"].as_str().expect("transition"),
                    )
                    .expect("transition"),
                    iteration: event["iteration"]
                        .as_str()
                        .map(atm_storage::WorkflowIteration::new)
                        .transpose()
                        .expect("iteration"),
                };
                let mut admission = DecomposedMessageAdmission {
                    template: template.clone(),
                    message: DecomposedMessageRecord {
                        key: record.message_key.clone(),
                        template_sha: template.sha.clone(),
                        vars: MergedVarsJson::from_merged_object(
                            [
                                ("scope".to_owned(), serde_json::json!(scope_id)),
                                ("iteration".to_owned(), event["iteration"].clone()),
                            ]
                            .into_iter()
                            .collect(),
                        ),
                        category: Some(name.to_owned()),
                        tags: instance_tags(&fixture_instance_tags),
                        content_format: Some(
                            family["content_format"]
                                .as_str()
                                .expect("content format")
                                .to_owned(),
                        ),
                        workflow: None,
                    },
                };
                admission.message.workflow = Some(WorkflowAdmission {
                    tag_provenance: admission
                        .expected_tag_provenance(&snapshot)
                        .expect("canonical provenance"),
                    snapshot,
                });
                backend
                    .template_catalog_store()
                    .admit_decomposed_message(admission)
                    .expect("atomic fixture admission");
                seeded += 1;
            }
        }
        assert_eq!(seeded, 5, "all retained events admitted");
        let query = MessageSearchQuery::default();
        assert_eq!(
            backend
                .message_search_store()
                .search(&query)
                .expect("generic search")
                .matches
                .len(),
            seeded
        );

        for (name, field) in [
            ("scope_kind", SearchGroupField::WorkflowScopeKind),
            ("state", SearchGroupField::WorkflowState),
            ("stage", SearchGroupField::WorkflowStage),
            ("transition", SearchGroupField::WorkflowTransition),
        ] {
            let page = backend
                .message_search_store()
                .search(&MessageSearchQuery {
                    aggregate: Some(SimpleAggregate::GroupBy(SearchGroupBy::Field(field))),
                    ..MessageSearchQuery::default()
                })
                .expect("bounded workflow aggregate");
            let atm_storage::SearchAggregate::Groups { groups, .. } =
                page.aggregate.expect("aggregate result")
            else {
                panic!("expected grouped aggregate");
            };
            let actual = groups
                .into_iter()
                .map(|group| (group.key, group.count))
                .collect::<std::collections::BTreeMap<_, _>>();
            assert_eq!(
                actual, expected_aggregate_counts[name],
                "{name} aggregation"
            );
        }

        let scope_id_query = MessageSearchQuery {
            filters: atm_storage::SearchFilters {
                workflow_scope_id: Some(WorkflowScopeId::new("train-42").expect("scope id")),
                ..atm_storage::SearchFilters::default()
            },
            ..MessageSearchQuery::default()
        };
        assert_eq!(
            backend
                .message_search_store()
                .search(&scope_id_query)
                .expect("scope-id filter")
                .matches
                .len(),
            3,
            "scope_id remains an exact filter, never an aggregate group key"
        );
        let iteration_query = MessageSearchQuery {
            filters: atm_storage::SearchFilters {
                workflow_iteration: Some(
                    atm_storage::WorkflowIteration::new("1").expect("iteration"),
                ),
                ..atm_storage::SearchFilters::default()
            },
            ..MessageSearchQuery::default()
        };
        assert_eq!(
            backend
                .message_search_store()
                .search(&iteration_query)
                .expect("iteration filter")
                .matches
                .len(),
            2,
            "iteration remains an exact filter, never an aggregate group key"
        );
    }

    #[test]
    fn external_content_message_search_returns_a_compound_key_and_highlight() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let record = message("atm:search-alpha", "alpha beta gamma");
        backend
            .message_store()
            .save_message(&record)
            .expect("seed message");
        let query = MessageSearchQuery {
            expression: Some(SearchExpression::Atom(
                SearchAtom::term("beta").expect("atom"),
            )),
            ..MessageSearchQuery::default()
        };
        let page = backend
            .message_search_store()
            .search(&query)
            .expect("search");
        assert_eq!(page.matches.len(), 1);
        assert_eq!(page.matches[0].key.message_key, record.message_key);
        assert!(
            page.matches[0]
                .snippet
                .as_deref()
                .is_some_and(|snippet| snippet.contains("\u{1}beta\u{2}")),
            "typed search results must carry backend FTS context"
        );
        backend
            .shared_db_for_test()
            .with_connection(|connection| {
                let highlight: String = connection
                    .query_row(
                        "SELECT highlight(mail_messages_fts, 0, '[', ']')
                 FROM mail_messages_fts WHERE mail_messages_fts MATCH 'beta'",
                        [],
                        |row| row.get(0),
                    )
                    .map_err(|error| {
                        AtmError::mailbox_read(format!("search highlight query failed: {error}"))
                    })?;
                assert_eq!(highlight, "alpha [beta] gamma");
                Ok(())
            })
            .expect("external-content highlight");
    }

    #[test]
    fn search_reindex_matches_transactional_projection_and_delete_removes_hit() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let record = message("atm:search-rebuild", "needle before rebuild");
        let store = backend.message_store();
        store.save_message(&record).expect("seed message");
        let query = MessageSearchQuery {
            expression: Some(SearchExpression::Atom(
                SearchAtom::term("needle").expect("atom"),
            )),
            ..MessageSearchQuery::default()
        };
        assert_eq!(
            backend
                .message_search_store()
                .search(&query)
                .expect("before rebuild")
                .matches
                .len(),
            1
        );
        backend.reindex_search().expect("rebuild");
        assert_eq!(
            backend
                .message_search_store()
                .search(&query)
                .expect("after rebuild")
                .matches
                .len(),
            1
        );
        store
            .delete_message(&record.message_key)
            .expect("delete message");
        assert!(
            backend
                .message_search_store()
                .search(&query)
                .expect("after delete")
                .matches
                .is_empty()
        );
    }

    #[test]
    fn search_filters_and_template_projection_follow_decomposed_admission() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let record = message("atm:search-decomposed", "plain body must disappear");
        backend
            .message_store()
            .save_message(&record)
            .expect("seed canonical message");
        let template = template_registration('c');
        backend
            .template_catalog_store()
            .admit_decomposed_message(DecomposedMessageAdmission {
                template: template.clone(),
                message: DecomposedMessageRecord {
                    key: record.message_key.clone(),
                    template_sha: template.sha,
                    vars: MergedVarsJson::from_merged_object(
                        [("assignee".to_owned(), serde_json::json!("Rand"))]
                            .into_iter()
                            .collect(),
                    ),
                    category: Some("sprint-fix".to_owned()),
                    tags: instance_tags(&["phase-an"]),
                    content_format: Some("markdown".to_owned()),
                    workflow: None,
                },
            })
            .expect("decompose");

        let store = backend.message_search_store();
        let by_var = MessageSearchQuery {
            expression: Some(SearchExpression::Atom(
                SearchAtom::term("Rand").expect("atom"),
            )),
            filters: atm_storage::SearchFilters {
                vars: vec![(
                    SearchKey::new("assignee").expect("key"),
                    SearchValue::new("Rand").expect("value"),
                )],
                ..atm_storage::SearchFilters::default()
            },
            ..MessageSearchQuery::default()
        };
        let page = store.search(&by_var).expect("var search");
        assert_eq!(page.matches.len(), 1);
        assert_eq!(page.matches[0].key.message_key, record.message_key);
        assert_eq!(
            page.matches[0].match_fields,
            vec![atm_storage::SearchMatchField::VarValue]
        );

        let mut by_type_prefix = MessageSearchQuery::default();
        by_type_prefix.filters.template_metadata = vec![(
            SearchKey::new("kind").expect("key"),
            SearchMetadataMatch::prefix("assign").expect("prefix"),
        )];
        assert_eq!(
            store
                .search(&by_type_prefix)
                .expect("prefix metadata search")
                .matches
                .len(),
            1
        );

        let mut by_tag = MessageSearchQuery::default();
        by_tag.filters.tags = vec!["phase-an".to_owned()];
        by_tag.filters.template_metadata = vec![(
            SearchKey::new("kind").expect("key"),
            atm_storage::SearchMetadataMatch::exact("assignment").expect("value"),
        )];
        assert_eq!(
            store
                .search(&by_tag)
                .expect("structured filters")
                .matches
                .len(),
            1
        );

        let template_content = MessageSearchQuery {
            expression: Some(SearchExpression::Atom(
                SearchAtom::term("hello").expect("atom"),
            )),
            ..MessageSearchQuery::default()
        };
        let page = store
            .search(&template_content)
            .expect("template FTS search");
        assert_eq!(page.matches.len(), 1);
        assert_eq!(
            page.matches[0].match_fields,
            vec![atm_storage::SearchMatchField::TemplateContent]
        );

        let plain_body = MessageSearchQuery {
            expression: Some(SearchExpression::Atom(
                SearchAtom::term("plain").expect("atom"),
            )),
            ..MessageSearchQuery::default()
        };
        assert!(
            store
                .search(&plain_body)
                .expect("body search")
                .matches
                .is_empty()
        );
    }

    #[test]
    fn search_type_and_variables_include_derivative_template_revisions() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let first = message("atm:revision-one", "first body");
        let second = message("atm:revision-two", "second body");
        backend
            .message_store()
            .save_message(&first)
            .expect("first message");
        backend
            .message_store()
            .save_message(&second)
            .expect("second message");

        let first_template = template_registration('e');
        let second_template = template_registration('f');
        for (record, template) in [(&first, first_template), (&second, second_template)] {
            backend
                .template_catalog_store()
                .admit_decomposed_message(DecomposedMessageAdmission {
                    template: template.clone(),
                    message: DecomposedMessageRecord {
                        key: record.message_key.clone(),
                        template_sha: template.sha,
                        vars: MergedVarsJson::from_merged_object(
                            [("phase".to_owned(), serde_json::json!("an"))]
                                .into_iter()
                                .collect(),
                        ),
                        category: Some("assignment".to_owned()),
                        tags: instance_tags(&["phase-an"]),
                        content_format: Some("markdown".to_owned()),
                        workflow: None,
                    },
                })
                .expect("decomposed admission");
        }

        let mut query = MessageSearchQuery::default();
        query.filters.template_metadata = vec![(
            SearchKey::new("kind").expect("key"),
            SearchMetadataMatch::exact("assignment").expect("metadata"),
        )];
        query.filters.vars = vec![(
            SearchKey::new("phase").expect("key"),
            SearchValue::new("an").expect("value"),
        )];
        let page = backend
            .message_search_store()
            .search(&query)
            .expect("cross-revision search");
        assert_eq!(page.matches.len(), 2);
        assert_eq!(
            page.matches
                .iter()
                .map(|hit| hit.template_type.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("task"), Some("task")]
        );
    }

    #[test]
    fn search_cursor_is_bound_to_its_typed_query() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let timestamp: IsoTimestamp = "2026-08-12T00:00:00Z".parse().expect("timestamp");
        for (key, message_id) in [
            ("atm:cursor-one", Some(AtmMessageId::new())),
            ("atm:cursor-two", Some(AtmMessageId::new())),
            ("atm:cursor-three", Some(AtmMessageId::new())),
        ] {
            let mut record = message(key, "cursor needle");
            record.envelope.timestamp = timestamp;
            record.envelope.message_id = message_id;
            backend
                .message_store()
                .save_message(&record)
                .expect("seed message");
        }
        let mut query = MessageSearchQuery {
            expression: Some(SearchExpression::Atom(
                SearchAtom::term("needle").expect("atom"),
            )),
            page: atm_storage::SearchPageRequest {
                limit: SearchLimit::new(1).expect("limit"),
                cursor: None,
            },
            ..MessageSearchQuery::default()
        };
        let first = backend
            .message_search_store()
            .search(&query)
            .expect("first page");
        let cursor = first.next_cursor.expect("next cursor");
        query.page.cursor = Some(cursor.clone());
        let continuation = backend
            .message_search_store()
            .search(&query)
            .expect("continuation page");
        assert_eq!(continuation.matches.len(), 1);
        assert_ne!(
            continuation.matches[0].key.message_key,
            first.matches[0].key.message_key
        );
        query.filters.category = Some("different-query".to_owned());
        query.page.cursor = Some(cursor);
        assert!(backend.message_search_store().search(&query).is_err());
    }

    #[test]
    fn sqlite_search_default_dedup_happens_before_cursor_continuation() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let timestamp: IsoTimestamp = "2026-08-12T00:00:00Z".parse().expect("timestamp");
        let message_id = AtmMessageId::new();
        for (key, team_name) in [
            ("atm:dedup-first", "a-team"),
            ("atm:dedup-duplicate", "b-team"),
            ("atm:dedup-third", "c-team"),
        ] {
            let mut record = message(key, "dedup needle");
            record.team = team_name.parse().expect("team");
            record.agent = "test-agent".parse().expect("agent");
            record.envelope.from = record.agent.clone();
            record.envelope.source_team = Some(record.team.clone());
            record.envelope.timestamp = timestamp;
            record.envelope.message_id = if key == "atm:dedup-third" {
                Some(AtmMessageId::new())
            } else {
                Some(message_id)
            };
            backend
                .message_store()
                .save_message(&record)
                .expect("seed message");
        }
        let mut query = MessageSearchQuery {
            expression: Some(SearchExpression::Atom(
                SearchAtom::term("needle").expect("atom"),
            )),
            page: atm_storage::SearchPageRequest {
                limit: SearchLimit::new(1).expect("limit"),
                cursor: None,
            },
            ..MessageSearchQuery::default()
        };
        let first = backend
            .message_search_store()
            .search(&query)
            .expect("first page");
        query.page.cursor = first.next_cursor;
        let second = backend
            .message_search_store()
            .search(&query)
            .expect("second page");
        assert_eq!(second.matches.len(), 1);
        assert_eq!(
            second.matches[0].key.message_key.as_str(),
            "atm:dedup-third"
        );
        assert!(second.next_cursor.is_none());
    }

    #[test]
    fn search_projection_mutations_match_a_from_scratch_rebuild() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.message_store();
        let first = message("atm:projection-one", "first inline payload");
        let second = message("atm:projection-two", "second inline payload");
        store.save_message(&first).expect("insert first");
        store.save_message(&second).expect("insert second");

        let template = template_registration('d');
        backend
            .template_catalog_store()
            .admit_decomposed_message(DecomposedMessageAdmission {
                template: template.clone(),
                message: DecomposedMessageRecord {
                    key: first.message_key.clone(),
                    template_sha: template.sha,
                    vars: MergedVarsJson::from_merged_object(
                        [("cycle".to_owned(), serde_json::json!(["one", "two"]))]
                            .into_iter()
                            .collect(),
                    ),
                    category: Some("repair".to_owned()),
                    tags: instance_tags(&["phase-an", "search"]),
                    content_format: Some("markdown".to_owned()),
                    workflow: None,
                },
            })
            .expect("decompose first");
        store
            .delete_message(&second.message_key)
            .expect("delete second");

        let before = backend
            .shared_db_for_test()
            .with_connection(|connection| {
                crate::search_schema::projection_snapshot(
                    connection,
                    backend.shared_db_for_test().target(),
                )
            })
            .expect("transactional snapshot");
        let template_before = backend
            .shared_db_for_test()
            .with_connection(|connection| {
                crate::search_schema::template_projection_snapshot(
                    connection,
                    backend.shared_db_for_test().target(),
                )
            })
            .expect("transactional template snapshot");
        backend.reindex_search().expect("rebuild");
        let after = backend
            .shared_db_for_test()
            .with_connection(|connection| {
                crate::search_schema::projection_snapshot(
                    connection,
                    backend.shared_db_for_test().target(),
                )
            })
            .expect("rebuilt snapshot");
        let template_after = backend
            .shared_db_for_test()
            .with_connection(|connection| {
                crate::search_schema::template_projection_snapshot(
                    connection,
                    backend.shared_db_for_test().target(),
                )
            })
            .expect("rebuilt template snapshot");
        assert_eq!(before, after);
        assert_eq!(template_before, template_after);
        assert_eq!(template_after.len(), 1);
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].4, "");
        assert_eq!(after[0].7, "one two");
    }

    #[tokio::test]
    async fn async_search_uses_the_backend_owned_lane() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let record = message("atm:async-search", "async needle");
        backend
            .message_store()
            .save_message(&record)
            .expect("seed message");
        let query = MessageSearchQuery {
            expression: Some(SearchExpression::Atom(
                SearchAtom::term("needle").expect("atom"),
            )),
            ..MessageSearchQuery::default()
        };
        let page = backend
            .async_message_search_store()
            .search_async(
                query,
                SearchDeadline::new(std::time::Duration::from_secs(1)).expect("deadline"),
            )
            .await
            .expect("async search");
        assert_eq!(page.matches[0].key.message_key, record.message_key);
        assert!(SearchDeadline::new(std::time::Duration::ZERO).is_err());
    }

    #[tokio::test]
    async fn async_search_reader_rejects_work_that_expired_before_execution() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let error = backend
            .shared_db_for_test()
            .submit_expired_search_for_test(MessageSearchQuery::default())
            .await
            .expect_err("expired queued request must not execute");
        assert!(
            error.to_string().contains("expired before execution"),
            "the reader lane must reject a request after its absolute deadline"
        );
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
        let mut template = template_registration('a');
        template.frontmatter.metadata.extend([
            (
                "tags".to_owned(),
                serde_json::json!(["domain:workflow", "audience:engineering"]),
            ),
            (
                "workflow".to_owned(),
                serde_json::json!({
                    "scope": { "kind": "release-train", "variable": "train_id" },
                    "state": "dev-start",
                    "stage": "implementation",
                    "transition": "entered",
                    "iteration_variable": "attempt"
                }),
            ),
        ]);

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
        assert_eq!(
            loaded
                .frontmatter
                .template_tags
                .iter()
                .map(|tag| tag.as_str())
                .collect::<Vec<_>>(),
            vec!["audience:engineering", "domain:workflow"]
        );
        assert_eq!(
            loaded
                .frontmatter
                .workflow
                .as_ref()
                .expect("canonical workflow")
                .scope_variable
                .as_str(),
            "train_id"
        );

        let vars = MergedVarsJson::from_merged_object(
            [("name".to_string(), serde_json::json!("Rand"))]
                .into_iter()
                .collect(),
        );
        let outcome = catalog
            .admit_decomposed_message(DecomposedMessageAdmission {
                template: template.clone(),
                message: DecomposedMessageRecord {
                    key: message.message_key.clone(),
                    template_sha: template.sha.clone(),
                    vars,
                    category: Some("assignment".to_string()),
                    tags: instance_tags(&["phase-an"]),
                    content_format: Some("markdown".to_string()),
                    workflow: None,
                },
            })
            .expect("atomic admission");
        assert_eq!(
            outcome,
            DecomposedMessageAdmissionOutcome::Inserted {
                template: TemplateRegistrationOutcome::AlreadyRegistered
            }
        );
        let decomposed = catalog
            .load_decomposed_message(&message.message_key)
            .expect("load decomposition")
            .expect("decomposed row exists");
        assert_eq!(decomposed.key, message.message_key);
        assert_eq!(decomposed.template_sha, template.sha);
        assert_eq!(decomposed.category.as_deref(), Some("assignment"));
        assert_eq!(decomposed.tags, instance_tags(&["phase-an"]));
        assert_eq!(decomposed.content_format.as_deref(), Some("markdown"));
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
    fn an15_differential_probe_preserves_immutable_revision_metadata_and_idempotency() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let catalog = backend.template_catalog_store();

        for case_index in 0..100_u64 {
            let mut original = template_registration('a');
            original.sha = TemplateSha::new(format!("{case_index:064x}")).expect("original SHA");
            original.content_text = format!(
                "---\nmetadata:\n  revision: original-{case_index}\n---\nhello {{ name }}\n"
            );
            original.content_bytes = original.content_text.as_bytes().to_vec();
            original.frontmatter.metadata.insert(
                "revision".to_owned(),
                serde_json::json!(format!("original-{case_index}")),
            );
            assert_eq!(
                catalog
                    .register(original.clone())
                    .expect("register original revision"),
                TemplateRegistrationOutcome::Inserted
            );

            let mut revision = template_registration('a');
            revision.sha =
                TemplateSha::new(format!("{:064x}", case_index + 10_000)).expect("edited SHA");
            revision.content_text =
                format!("---\nmetadata:\n  revision: {case_index}\n---\nhello {{ name }}\n");
            revision.content_bytes = revision.content_text.as_bytes().to_vec();
            revision
                .frontmatter
                .metadata
                .insert("revision".to_owned(), serde_json::json!(case_index));

            assert_eq!(
                catalog
                    .register(revision.clone())
                    .expect("register immutable revision"),
                TemplateRegistrationOutcome::Inserted
            );
            assert_eq!(
                catalog
                    .register(revision.clone())
                    .expect("idempotent re-admission"),
                TemplateRegistrationOutcome::AlreadyRegistered
            );
            let loaded = catalog
                .load(&revision.sha)
                .expect("load revision")
                .expect("registered revision");
            assert_eq!(loaded.content_bytes, revision.content_bytes);
            assert_eq!(
                loaded.frontmatter.metadata.get("revision"),
                Some(&serde_json::json!(case_index))
            );
            assert_eq!(
                loaded.frontmatter.metadata.get("kind"),
                Some(&serde_json::json!("assignment"))
            );
            let loaded_original = catalog
                .load(&original.sha)
                .expect("load original")
                .expect("original remains registered");
            assert_ne!(
                loaded_original.sha, loaded.sha,
                "one-byte revision needs a new SHA"
            );
            assert_eq!(loaded_original.content_bytes, original.content_bytes);
            assert_eq!(
                loaded_original.frontmatter.metadata.get("revision"),
                Some(&serde_json::json!(format!("original-{case_index}")))
            );
        }
    }

    #[test]
    fn an15_differential_probe_legacy_rows_survive_reopen_without_reclassification() {
        let root = tempfile::tempdir().expect("temporary catalog root");
        let path = root.path().join("catalog.db");
        let legacy_sha = "e".repeat(64);
        let first_seen = IsoTimestamp::now().to_string();
        let connection = Connection::open(&path).expect("open pre-AN.13 catalog fixture");
        connection
            .execute_batch(
                "CREATE TABLE message_templates (
                    template_sha TEXT NOT NULL PRIMARY KEY,
                    template_type TEXT NULL,
                    template_name TEXT NULL,
                    content_bytes BLOB NOT NULL,
                    content_text TEXT NOT NULL,
                    schema_json TEXT NOT NULL DEFAULT '{}',
                    first_seen_at TEXT NOT NULL,
                    first_seen_by TEXT NOT NULL
                 );",
            )
            .expect("create pre-AN.13 catalog schema");
        connection
            .execute(
                "INSERT INTO message_templates(
                    template_sha, content_bytes, content_text, schema_json,
                    first_seen_at, first_seen_by
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    legacy_sha,
                    b"legacy source".as_slice(),
                    "legacy source",
                    serde_json::to_string(&TemplateFrontmatter::default())
                        .expect("serialize legacy frontmatter"),
                    first_seen,
                    "historical-agent",
                ],
            )
            .expect("seed legacy catalog row");
        drop(connection);

        let backend = SqliteStorageBackend::new(&path).expect("migrate catalog schema");
        let catalog = backend.template_catalog_store();
        let legacy = catalog
            .load(&legacy_sha.parse().expect("legacy SHA"))
            .expect("load legacy row")
            .expect("legacy row exists");
        assert_eq!(
            legacy.output_format, None,
            "migration must not guess a format"
        );

        let mut classified = template_registration('f');
        classified.output_format = TemplateOutputFormat::Json;
        assert_eq!(
            catalog
                .register(classified.clone())
                .expect("admit classified row"),
            TemplateRegistrationOutcome::Inserted
        );
        assert_eq!(
            catalog
                .load(&classified.sha)
                .expect("load newly admitted row")
                .expect("classified row exists")
                .output_format,
            Some(TemplateOutputFormat::Json)
        );
        drop(catalog);
        drop(backend);

        let reopened = SqliteStorageBackend::new(&path).expect("reopen catalog");
        let catalog = reopened.template_catalog_store();
        assert_eq!(
            catalog
                .load(&legacy_sha.parse().expect("legacy SHA"))
                .expect("load legacy after reopen")
                .expect("legacy row exists")
                .output_format,
            None
        );
        assert_eq!(
            catalog
                .load(&classified.sha)
                .expect("load classified after reopen")
                .expect("classified row exists")
                .output_format,
            Some(TemplateOutputFormat::Json)
        );
    }

    #[test]
    fn invalid_workflow_metadata_rejects_before_sqlite_catalog_or_message_mutation() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let message = message("atm:invalid-workflow", "canonical body");
        backend
            .message_store()
            .save_message(&message)
            .expect("seed canonical message");
        let mut template = template_registration('a');
        template.frontmatter.metadata.insert(
            "workflow".to_owned(),
            serde_json::json!({ "scope": { "kind": "sprint" } }),
        );

        let catalog = backend.template_catalog_store();
        let error = catalog
            .admit_decomposed_message(DecomposedMessageAdmission {
                template: template.clone(),
                message: DecomposedMessageRecord {
                    key: message.message_key.clone(),
                    template_sha: template.sha.clone(),
                    vars: MergedVarsJson::default(),
                    category: None,
                    tags: vec![],
                    content_format: None,
                    workflow: None,
                },
            })
            .expect_err("partial workflow must fail before mutation");
        assert_eq!(error.code().as_str(), "TEMPLATE_WORKFLOW_INVALID");
        assert!(catalog.load(&template.sha).expect("load").is_none());
        backend
            .shared_db_for_test()
            .with_connection(|connection| {
                let template_sha: Option<String> = connection
                    .query_row(
                        "SELECT template_sha FROM mail_messages WHERE message_key = ?1",
                        params![message.message_key.as_str()],
                        |row| row.get(0),
                    )
                    .map_err(|error| AtmError::mailbox_read(error.to_string()))?;
                assert!(template_sha.is_none());
                Ok(())
            })
            .expect("canonical row remains unmodified");
    }

    #[tokio::test]
    async fn async_template_message_admission_is_atomic_and_idempotent() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let message = message("atm:template-async", "rendered fallback is not persisted");
        let template = template_registration('b');
        let admission = TemplateMessageAdmission {
            record: message.clone(),
            decomposition: DecomposedMessageAdmission {
                template: template.clone(),
                message: DecomposedMessageRecord {
                    key: message.message_key.clone(),
                    template_sha: template.sha.clone(),
                    vars: MergedVarsJson::from_merged_object(
                        [("name".to_owned(), serde_json::json!("captured"))]
                            .into_iter()
                            .collect(),
                    ),
                    category: Some("assignment".to_owned()),
                    tags: instance_tags(&["phase-an"]),
                    content_format: Some("markdown".to_owned()),
                    workflow: None,
                },
            },
        };
        assert!(
            backend
                .async_message_store()
                .admit_template_message_async(admission.clone())
                .await
                .expect("first admission")
                .is_none()
        );
        assert!(
            backend
                .async_message_store()
                .admit_template_message_async(admission)
                .await
                .expect("idempotent admission")
                .is_some()
        );
        backend
            .shared_db_for_test()
            .with_connection(|connection| {
                let counts: (i64, i64, Option<String>, Option<String>) = connection
                    .query_row(
                        "SELECT (SELECT COUNT(*) FROM message_templates),
                            (SELECT COUNT(*) FROM decomposed_messages),
                            template_sha, vars_json
                     FROM mail_messages WHERE message_key = ?1",
                        params![message.message_key.as_str()],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                    )
                    .map_err(|error| AtmError::mailbox_read(error.to_string()))?;
                assert_eq!(counts.0, 1);
                assert_eq!(counts.1, 1);
                assert_eq!(counts.2.as_deref(), Some(template.sha.as_str()));
                assert_eq!(counts.3.as_deref(), Some(r#"{"name":"captured"}"#));
                Ok(())
            })
            .expect("stored decomposed row");
    }

    #[test]
    fn ordinary_message_classification_projects_to_normal_message_columns() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let mut message = message("atm:template-plain-fallback", "verified rendered body");
        message.envelope.summary = Some("ordinary summary beacon".to_owned());
        message.envelope.extra = Map::from_iter([
            ("category".to_owned(), serde_json::json!("assignment")),
            (
                "tags".to_owned(),
                serde_json::json!(["phase-an", "fallback"]),
            ),
            ("content_format".to_owned(), serde_json::json!("markdown")),
        ]);

        backend
            .message_store()
            .save_message(&message)
            .expect("ordinary classified message is stored");
        backend
            .shared_db_for_test()
            .with_connection(|connection| {
                let row: OrdinaryMessageColumns = connection
                    .query_row(
                        "SELECT template_sha, tags_json, category, content_format, message_text,
                                workflow_scope_kind, workflow_scope_id, workflow_state,
                                workflow_stage, workflow_transition, workflow_iteration,
                                applied_template_tags_json, effective_tags_json
                         FROM mail_messages WHERE message_key = ?1",
                        params![message.message_key.as_str()],
                        |row| {
                            Ok((
                                row.get(0)?,
                                row.get(1)?,
                                row.get(2)?,
                                row.get(3)?,
                                row.get(4)?,
                                row.get(5)?,
                                row.get(6)?,
                                row.get(7)?,
                                row.get(8)?,
                                row.get(9)?,
                                row.get(10)?,
                                row.get(11)?,
                                row.get(12)?,
                            ))
                        },
                    )
                    .map_err(|error| AtmError::mailbox_read(error.to_string()))?;
                assert_eq!(row.0, None, "ordinary message has no template reference");
                assert_eq!(row.1, r#"["phase-an","fallback"]"#);
                assert_eq!(row.2.as_deref(), Some("assignment"));
                assert_eq!(row.3.as_deref(), Some("markdown"));
                assert_eq!(row.4.as_deref(), Some("verified rendered body"));
                assert!(
                    [
                        row.5.as_ref(),
                        row.6.as_ref(),
                        row.7.as_ref(),
                        row.8.as_ref(),
                        row.9.as_ref(),
                        row.10.as_ref(),
                        row.11.as_ref(),
                        row.12.as_ref(),
                    ]
                    .into_iter()
                    .all(|value| value.is_none()),
                    "plain fallback must not invent workflow snapshots or tag projections"
                );
                Ok(())
            })
            .expect("inspect ordinary classified row");

        for (term, expected_field) in [
            ("verified", atm_storage::SearchMatchField::BodyText),
            ("beacon", atm_storage::SearchMatchField::Summary),
            ("fallback", atm_storage::SearchMatchField::Tag),
            ("test-agent", atm_storage::SearchMatchField::FromAgent),
        ] {
            let query = MessageSearchQuery {
                expression: Some(SearchExpression::Atom(
                    SearchAtom::term(term).expect("FTS term"),
                )),
                ..MessageSearchQuery::default()
            };
            let page = backend
                .message_search_store()
                .search(&query)
                .expect("ordinary row FTS query");
            assert_eq!(page.matches.len(), 1, "term {term:?} must be indexed");
            assert_eq!(page.matches[0].match_fields, vec![expected_field]);
        }
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
                    vars: MergedVarsJson::from_merged_object(Default::default()),
                    category: None,
                    tags: vec![],
                    content_format: None,
                    workflow: None,
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
    fn legacy_not_null_message_text_databases_admit_decomposed_templates() {
        // Databases created before the nullable `message_text` change failed
        // every decomposed admission because the writer nulls the column out.
        let root = tempfile::tempdir().expect("legacy root");
        let path = root.path().join("legacy.db");
        Connection::open(&path)
            .expect("open legacy fixture")
            .execute_batch(
                "CREATE TABLE mail_messages (
                    team TEXT NOT NULL, agent TEXT NOT NULL, message_key TEXT NOT NULL,
                    envelope_json TEXT NOT NULL, from_agent TEXT NOT NULL,
                    source_chat_id TEXT NULL, destination_chat_id TEXT NULL,
                    message_text TEXT NOT NULL, summary TEXT NULL, message_at TEXT NOT NULL,
                    message_id TEXT NULL, parent_message_id TEXT NULL,
                    thread_mode TEXT NULL CHECK(thread_mode IS NULL OR thread_mode IN ('add-details', 'supersede')),
                    recorded_at TEXT,
                    CHECK(message_key GLOB 'atm:*' OR message_key GLOB 'ext:*'),
                    PRIMARY KEY (team, agent, message_key)
                 );",
            )
            .expect("legacy not-null message_text fixture");

        let backend = SqliteStorageBackend::new(&path).expect("migrate legacy fixture");
        let record = message("atm:legacy-decomposed", "inline before decomposition");
        backend
            .message_store()
            .save_message(&record)
            .expect("seed canonical message");
        let catalog = backend.template_catalog_store();
        let template = template_registration('b');
        catalog
            .register(template.clone())
            .expect("register template");

        let outcome = catalog
            .admit_decomposed_message(DecomposedMessageAdmission {
                template: template.clone(),
                message: DecomposedMessageRecord {
                    key: record.message_key.clone(),
                    template_sha: template.sha.clone(),
                    vars: MergedVarsJson::from_merged_object(
                        [("name".to_string(), serde_json::json!("Rand"))]
                            .into_iter()
                            .collect(),
                    ),
                    category: Some("assignment".to_string()),
                    tags: instance_tags(&["phase-av"]),
                    content_format: Some("markdown".to_string()),
                    workflow: None,
                },
            })
            .expect("decomposed admission must succeed on a migrated legacy database");
        assert_eq!(
            outcome,
            DecomposedMessageAdmissionOutcome::Inserted {
                template: TemplateRegistrationOutcome::AlreadyRegistered
            }
        );
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
        let historical_record = message("atm:historical-search", "historical backfill needle");
        let historical_connection =
            Connection::open(&historical_path).expect("reopen historical fixture");
        historical_connection
            .execute(
                "INSERT INTO mail_messages(
                    team, agent, message_key, envelope_json, from_agent, message_text,
                    summary, message_at, message_id
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    historical_record.team.as_str(),
                    historical_record.agent.as_str(),
                    historical_record.message_key.as_str(),
                    serde_json::to_string(&historical_record.envelope).expect("envelope JSON"),
                    historical_record.envelope.from.as_str(),
                    historical_record.envelope.text.as_str(),
                    historical_record.envelope.summary.as_deref(),
                    historical_record.envelope.timestamp.to_string(),
                    historical_record
                        .envelope
                        .message_id
                        .as_ref()
                        .map(ToString::to_string),
                ],
            )
            .expect("seed historical message");
        drop(historical_connection);
        let historical = SqliteStorageBackend::new(&historical_path).expect("migrate fixture");

        let historical_query = MessageSearchQuery {
            expression: Some(SearchExpression::Atom(
                SearchAtom::term("needle").expect("atom"),
            )),
            ..MessageSearchQuery::default()
        };
        assert_eq!(
            historical
                .message_search_store()
                .search(&historical_query)
                .expect("historical backfill search")
                .matches
                .len(),
            1
        );
        let historical_projection_before_reindex = historical
            .shared_db_for_test()
            .with_connection(|connection| {
                crate::search_schema::projection_snapshot(
                    connection,
                    historical.shared_db_for_test().target(),
                )
            })
            .expect("historical backfill projection");
        let historical_templates_before_reindex = historical
            .shared_db_for_test()
            .with_connection(|connection| {
                crate::search_schema::template_projection_snapshot(
                    connection,
                    historical.shared_db_for_test().target(),
                )
            })
            .expect("historical template backfill projection");
        historical.reindex_search().expect("historical rebuild");
        assert_eq!(
            historical_projection_before_reindex,
            historical
                .shared_db_for_test()
                .with_connection(|connection| {
                    crate::search_schema::projection_snapshot(
                        connection,
                        historical.shared_db_for_test().target(),
                    )
                })
                .expect("historical rebuilt projection")
        );
        assert_eq!(
            historical_templates_before_reindex,
            historical
                .shared_db_for_test()
                .with_connection(|connection| {
                    crate::search_schema::template_projection_snapshot(
                        connection,
                        historical.shared_db_for_test().target(),
                    )
                })
                .expect("historical rebuilt template projection")
        );

        // Model an AN.8 decomposed row after the additive migration: it has
        // the historic decomposition values, but none of AN.9's columns.
        // This comes after the search-reindex equivalence proof because it is
        // intentionally a post-AN.8 decomposition rather than a legacy plain
        // message.
        let historical_template = template_registration('a');
        historical
            .template_catalog_store()
            .register(historical_template.clone())
            .expect("register historical template");
        historical
            .shared_db_for_test()
            .with_connection(|connection| {
                connection
                    .execute(
                        "UPDATE mail_messages
                         SET template_sha = ?1, vars_json = ?2, category = ?3,
                             tags_json = ?4, content_format = ?5, message_text = NULL
                         WHERE message_key = ?6",
                        params![
                            historical_template.sha.as_str(),
                            "{}",
                            "assignment",
                            "[\"phase-an\"]",
                            "markdown",
                            historical_record.message_key.as_str(),
                        ],
                    )
                    .map_err(|error| AtmError::mailbox_write(error.to_string()))?;
                Ok(())
            })
            .expect("seed historical decomposed row");

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
            let workflow_columns_are_null = connection
                .query_row(
                    "SELECT workflow_scope_kind IS NULL
                          AND workflow_scope_id IS NULL
                          AND workflow_state IS NULL
                          AND workflow_stage IS NULL
                          AND workflow_transition IS NULL
                          AND workflow_iteration IS NULL
                          AND applied_template_tags_json IS NULL
                          AND effective_tags_json IS NULL
                       FROM mail_messages WHERE template_sha IS NOT NULL LIMIT 1",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .expect("inspect workflow projection values");
            (columns, message_text_not_null, workflow_columns_are_null)
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
            "instance_tags_json",
            "summary",
            "workflow_scope_kind",
            "workflow_scope_id",
            "workflow_state",
            "workflow_stage",
            "workflow_transition",
            "workflow_iteration",
            "applied_template_tags_json",
            "derived_tags_json",
            "effective_tags_json",
            "read",
            "acknowledged_at",
            "pending_ack_at",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
        let fresh_surface = surface(&fresh_path);
        let historical_surface = surface(&historical_path);
        assert_eq!(fresh_surface.0, historical_surface.0);
        assert_eq!(fresh_surface.0, expected_columns);
        assert_eq!(fresh_surface.1, 0, "fresh DDL keeps message_text nullable");
        assert_eq!(
            historical_surface.2,
            Some(1),
            "historical decomposed rows retain NULL workflow/projection fields"
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
    async fn read_display_state_is_the_only_async_reader_followup_admitted_to_writer() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.async_message_store();
        let original = message("atm:read-state", "read state");
        backend
            .message_store()
            .save_message(&original)
            .expect("seed mailbox");

        store
            .apply_read_display_state_async(
                MailboxScope::new(team(), agent()),
                vec![original.message_key.clone()],
                Some(IsoTimestamp::now()),
            )
            .await
            .expect("writer applies the explicit read state transition");

        let stored = backend
            .message_store()
            .load_message(&original.message_key)
            .expect("load transitioned message")
            .expect("message remains durable");
        assert!(stored.envelope.read);
        assert_eq!(stored.envelope.text, original.envelope.text);
    }

    #[tokio::test]
    async fn dedicated_mailbox_reader_enforces_scope_and_reads_without_writer_submission() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let first = message("atm:reader-first", "first");
        let second = message("atm:reader-second", "second");
        backend
            .message_store()
            .save_messages_atomically(&[first.clone(), second.clone()])
            .expect("seed mailbox");
        let scope = MailboxScope::new(team(), agent());
        let reader = backend.async_mailbox_reader();
        let deadline = ReadDeadline::new(std::time::Duration::from_secs(1)).expect("deadline");

        let listed = reader
            .list_messages(
                scope.clone(),
                MessageQuery {
                    team: team(),
                    agent: agent(),
                    sender: None,
                    task_id: None,
                    limit: None,
                },
                deadline,
            )
            .await
            .expect("reader list");
        assert_eq!(listed.len(), 2);
        assert!(listed.contains(&first));
        assert!(listed.contains(&second));
        assert_eq!(
            reader
                .load_message(scope.clone(), first.message_key.clone(), deadline)
                .await
                .expect("reader load"),
            Some(first.clone())
        );

        let denied = reader
            .list_messages(
                MailboxScope::new("other-team".parse().expect("team"), agent()),
                MessageQuery {
                    team: team(),
                    agent: agent(),
                    sender: None,
                    task_id: None,
                    limit: None,
                },
                deadline,
            )
            .await
            .expect_err("mismatched scope must fail at the boundary");
        assert_eq!(denied, ReadLaneError::UnauthorizedScope);

        let denied_load = reader
            .load_message(
                MailboxScope::new("other-team".parse().expect("team"), agent()),
                first.message_key.clone(),
                deadline,
            )
            .await
            .expect_err("cross-team load must fail at the storage boundary");
        assert_eq!(denied_load, ReadLaneError::UnauthorizedScope);

        let other_agent: AgentName = "other-agent".parse().expect("agent");
        let denied_list = reader
            .list_messages(
                scope.clone(),
                MessageQuery {
                    team: team(),
                    agent: other_agent.clone(),
                    sender: None,
                    task_id: None,
                    limit: None,
                },
                deadline,
            )
            .await
            .expect_err("cross-agent list must fail at the storage boundary");
        assert_eq!(denied_list, ReadLaneError::UnauthorizedScope);
        let denied_cross_agent_load = reader
            .load_message(
                MailboxScope::new(team(), other_agent),
                second.message_key.clone(),
                deadline,
            )
            .await
            .expect_err("cross-agent load must fail at the storage boundary");
        assert_eq!(denied_cross_agent_load, ReadLaneError::UnauthorizedScope);
    }

    #[tokio::test]
    async fn dedicated_mailbox_reader_projects_roster_and_durable_seen_state() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let scope = MailboxScope::new(team(), agent());
        backend
            .roster_store()
            .save_roster(&RosterSnapshot {
                team_name: team(),
                members: vec![RosterMember {
                    team_name: team(),
                    agent_name: agent(),
                    member_kind: RosterMemberKind::Permanent,
                    harness: RosterHarness::ClaudeCode,
                    agent_type: AgentType::Worker,
                    model: ModelName::default(),
                    recipient_pane_id: None,
                    metadata_json: Map::new(),
                }],
                refreshed_at: None,
            })
            .expect("seed roster");
        let original = message("atm:reader-seen", "read state");
        let watermark = IsoTimestamp::now();
        backend
            .message_store()
            .save_message(&original)
            .expect("seed mailbox");
        backend
            .async_message_store()
            .apply_read_display_state_async(
                scope.clone(),
                vec![original.message_key],
                Some(watermark),
            )
            .await
            .expect("write seen state");

        let reader = backend.async_mailbox_reader();
        let deadline = ReadDeadline::new(std::time::Duration::from_secs(1)).expect("deadline");
        assert!(
            reader
                .mailbox_member_exists(scope.clone(), deadline)
                .await
                .expect("read-only roster projection")
        );
        assert_eq!(
            reader
                .load_seen_watermark(scope, deadline)
                .await
                .expect("read-only seen projection"),
            Some(watermark)
        );
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
