#[cfg(test)]
use crate::observability::NullSqliteObservability;
use crate::observability::{
    SqliteObservability, SqliteObservabilityEvent, SqliteObservabilityOutcome,
};
use crate::writer::{SqliteWriter, WriteOp, WriteOpResult, validate_upsert_message_request};
use atm_storage::contract::{
    AcknowledgementCommit, AcknowledgementReplyBuilder, AcknowledgementSource, Message,
    MessageQuery,
};
use atm_storage::error::AtmError;
use atm_storage::schema::ThreadMode;
#[cfg(test)]
use rusqlite::OpenFlags;
use rusqlite::{Connection, Error as RusqliteError, TransactionBehavior};
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};

const SQLITE_BUSY_TIMEOUT_MS: u64 = 5_000;
#[cfg(test)]
static NEXT_IN_MEMORY_DB_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) const DB_MIGRATIONS: &str = r#"
CREATE TABLE IF NOT EXISTS mail_messages (
    team TEXT NOT NULL,
    agent TEXT NOT NULL,
    message_key TEXT NOT NULL,
    envelope_json TEXT NOT NULL,
    from_agent TEXT NOT NULL,
    source_chat_id TEXT NULL,
    destination_chat_id TEXT NULL,
    message_text TEXT NOT NULL,
    summary TEXT NULL,
    message_at TEXT NOT NULL,
    message_id TEXT NULL,
    parent_message_id TEXT NULL,
    thread_mode TEXT NULL CHECK(thread_mode IS NULL OR thread_mode IN ('add-details', 'supersede')),
    recorded_at TEXT,
    CHECK(message_key GLOB 'atm:*' OR message_key GLOB 'ext:*'),
    PRIMARY KEY (team, agent, message_key)
);

CREATE TABLE IF NOT EXISTS mail_message_states (
    team TEXT NOT NULL,
    agent TEXT NOT NULL,
    message_key TEXT NOT NULL,
    read INTEGER NOT NULL DEFAULT 0 CHECK(read IN (0, 1)),
    pending_ack_at TEXT NULL,
    acknowledged_at TEXT NULL,
    expires_at TEXT NULL,
    deleted_at TEXT NULL,
    updated_at TEXT NULL,
    PRIMARY KEY (team, agent, message_key),
    FOREIGN KEY (team, agent, message_key)
        REFERENCES mail_messages(team, agent, message_key)
        ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS team_roster (
    team_name TEXT NOT NULL,
    agent_name TEXT NOT NULL,
    member_kind TEXT NOT NULL CHECK(member_kind IN ('permanent', 'ephemeral')),
    harness TEXT NOT NULL CHECK(harness IN ('claude-code', 'codex-cli', 'gemini-cli', 'opencode', 'hermes', 'python-graft')),
    agent_type TEXT NOT NULL DEFAULT '',
    model TEXT NOT NULL DEFAULT '',
    metadata_json TEXT NOT NULL DEFAULT '{}',
    source TEXT,
    recipient_pane_id TEXT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (team_name, agent_name)
);

CREATE TABLE IF NOT EXISTS team_nudge_template_overrides (
    team_name TEXT NOT NULL,
    template_kind TEXT NOT NULL
        CHECK(template_kind IN (
            'delivery',
            'delivery_ack',
            'delivery_task',
            'delivery_task_ack',
            'acknowledge',
            'acknowledge_task'
        )),
    mode TEXT NOT NULL DEFAULT 'override'
        CHECK(mode IN ('override', 'disabled')),
    template_body TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (team_name, template_kind)
);

CREATE TABLE IF NOT EXISTS peer_https_interfaces (
    bind_addr TEXT NOT NULL PRIMARY KEY,
    advertise_host TEXT NOT NULL,
    enabled INTEGER NOT NULL CHECK(enabled IN (0, 1))
);

CREATE TABLE IF NOT EXISTS peer_local_certificate (
    singleton INTEGER NOT NULL PRIMARY KEY CHECK(singleton = 1),
    fingerprint TEXT NOT NULL,
    private_key_ref TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS peer_trusted_peers (
    host TEXT NOT NULL PRIMARY KEY,
    fingerprint TEXT NOT NULL,
    enabled INTEGER NOT NULL CHECK(enabled IN (0, 1)),
    https_port INTEGER NOT NULL DEFAULT 43101 CHECK(https_port BETWEEN 1 AND 65535)
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_mail_messages_single_successor
    ON mail_messages(team, agent, parent_message_id)
    WHERE parent_message_id IS NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS uq_mail_messages_message_id
    ON mail_messages(team, agent, message_id)
    WHERE message_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_mail_messages_mailbox
    ON mail_messages(team, agent);

-- Post-commit received-hook dispatch reloads an admitted record by its
-- immutable message key.  The mailbox primary key begins with team and agent,
-- so it cannot serve that global-key lookup without scanning a growing table.
CREATE INDEX IF NOT EXISTS idx_mail_messages_message_key
    ON mail_messages(message_key);

CREATE INDEX IF NOT EXISTS idx_mail_message_states_mailbox
    ON mail_message_states(team, agent);

CREATE INDEX IF NOT EXISTS idx_team_roster_team_name
    ON team_roster(team_name);

CREATE INDEX IF NOT EXISTS idx_team_nudge_template_overrides_team_name
    ON team_nudge_template_overrides(team_name);

CREATE INDEX IF NOT EXISTS idx_peer_https_interfaces_enabled
    ON peer_https_interfaces(enabled);

CREATE INDEX IF NOT EXISTS idx_peer_trusted_peers_enabled
    ON peer_trusted_peers(enabled);

-- AK.2 retires the worker-only reconciliation policy.  `IF EXISTS` makes the
-- migration safe for both historical databases and fresh installs.
DROP TABLE IF EXISTS peer_sync_policies;
"#;
// `team_roster` is the single canonical durable roster truth. Runtime pid
// continuity is transient daemon-owned state and must not be persisted here.

pub(crate) type SqliteConnection = Connection;

#[derive(Debug, Clone)]
pub(crate) enum SharedDbTarget {
    Path(PathBuf),
    #[cfg(test)]
    InMemory {
        uri: String,
    },
}

impl SharedDbTarget {
    pub(crate) fn display(&self) -> String {
        match self {
            Self::Path(path) => path.display().to_string(),
            #[cfg(test)]
            Self::InMemory { uri } => uri.clone(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct SharedDb {
    target: Arc<SharedDbTarget>,
    writer: Arc<SqliteWriter>,
    observability: Arc<dyn SqliteObservability>,
}

impl SharedDb {
    #[cfg(test)]
    pub(crate) fn open_in_memory_for_test() -> Result<Self, AtmError> {
        Self::open_in_memory_with_observability(Arc::new(NullSqliteObservability))
    }

    #[cfg(test)]
    pub(crate) fn open_in_memory_with_observability(
        observability: Arc<dyn SqliteObservability>,
    ) -> Result<Self, AtmError> {
        let target = Arc::new(SharedDbTarget::InMemory {
            uri: format!(
                "file:atm-storage-rusqlite-{}?mode=memory&cache=shared",
                NEXT_IN_MEMORY_DB_ID.fetch_add(1, Ordering::Relaxed)
            ),
        });
        let writer = Arc::new(SqliteWriter::start(
            Arc::clone(&target),
            Arc::clone(&observability),
        )?);
        Ok(Self {
            target,
            writer,
            observability,
        })
    }

    pub(crate) fn open_with_observability(
        path: impl AsRef<Path>,
        observability: Arc<dyn SqliteObservability>,
    ) -> Result<Self, AtmError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            // Accepted risk: durable-state root creation happens during boundary
            // assembly and is allowed to block on the host filesystem once.
            std::fs::create_dir_all(parent).map_err(|error| {
                AtmError::mailbox_write(format!(
                    "failed to create sqlite parent directory {}: {error}",
                    parent.display()
                ))
            })?;
        }

        let target = Arc::new(SharedDbTarget::Path(path));
        let writer = Arc::new(SqliteWriter::start(
            Arc::clone(&target),
            Arc::clone(&observability),
        )?);
        tracing::debug!(
            writer_handles = 1,
            path = %target.display(),
            "sqlite boundary assembly opened"
        );
        Ok(Self {
            target,
            writer,
            observability,
        })
    }

    /// Call only from blocking code paths; async callers must enter
    /// `spawn_blocking` before borrowing a sqlite connection.
    ///
    /// Accepted risk: this is enforced as a crate-internal contract rather
    /// than a runtime assert because `SharedDb` is only called from owned
    /// blocking code paths inside `atm-storage-rusqlite`.
    pub(crate) fn with_connection<T>(
        &self,
        operation: impl FnOnce(&mut Connection) -> Result<T, AtmError>,
    ) -> Result<T, AtmError> {
        debug_assert_blocking_only("SharedDb::with_connection");
        let mut connection = self.open_connection()?;
        operation(&mut connection)
    }

    /// Call only from blocking code paths; async callers must enter
    /// `spawn_blocking` before opening a sqlite transaction.
    ///
    /// Accepted risk: this is enforced as a crate-internal contract rather
    /// than a runtime assert because `SharedDb` is only called from owned
    /// blocking code paths inside `atm-storage-rusqlite`.
    pub(crate) fn with_transaction<T>(
        &self,
        operation: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<T, AtmError>,
    ) -> Result<T, AtmError> {
        debug_assert_blocking_only("SharedDb::with_transaction");
        self.with_connection(|connection| {
            // Acquire the SQLite writer lock up front so concurrent write paths
            // wait under the configured busy_timeout instead of failing during
            // deferred lock escalation on slower Windows schedulers.
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|error| {
                    sqlite_error(
                        self.target.as_ref(),
                        "failed to open sqlite immediate transaction",
                        error,
                    )
                })?;
            let value = operation(&transaction)?;
            transaction.commit().map_err(|error| {
                sqlite_error(
                    self.target.as_ref(),
                    "failed to commit sqlite transaction",
                    error,
                )
            })?;
            Ok(value)
        })
    }

    pub(crate) fn submit_upsert_message(&self, record: Message) -> Result<bool, AtmError> {
        validate_upsert_message_request(&record)?;
        let result = self
            .writer
            .submit(WriteOp::UpsertMessage(Box::new(record)))?;
        match result {
            WriteOpResult::UpsertMessage { inserted, .. } => Ok(inserted),
            WriteOpResult::Messages(_)
            | WriteOpResult::UpsertMessages
            | WriteOpResult::Acknowledged(_) => Err(AtmError::daemon_unavailable(
                "sqlite writer returned the wrong result for message upsert",
            )),
        }
    }

    pub(crate) async fn submit_upsert_message_async(
        &self,
        record: Message,
    ) -> Result<Option<Message>, AtmError> {
        validate_upsert_message_request(&record)?;
        match self
            .writer
            .submit_async(WriteOp::UpsertMessage(Box::new(record)))
            .await?
        {
            WriteOpResult::UpsertMessage { inserted: true, .. } => Ok(None),
            WriteOpResult::UpsertMessage {
                inserted: false,
                existing: Some(existing),
            } => Ok(Some(*existing)),
            WriteOpResult::UpsertMessage {
                inserted: false,
                existing: None,
            } => Err(AtmError::daemon_unavailable(
                "sqlite writer reported a duplicate without its retained record",
            )),
            WriteOpResult::Messages(_)
            | WriteOpResult::UpsertMessages
            | WriteOpResult::Acknowledged(_) => Err(AtmError::daemon_unavailable(
                "sqlite writer returned the wrong result for async message upsert",
            )),
        }
    }

    pub(crate) fn submit_upsert_messages_atomically(
        &self,
        records: Vec<Message>,
    ) -> Result<(), AtmError> {
        if records.is_empty() {
            return Ok(());
        }
        for record in &records {
            validate_upsert_message_request(record)?;
        }
        let result = self.writer.submit(WriteOp::UpsertMessages(records))?;
        match result {
            WriteOpResult::UpsertMessages => Ok(()),
            WriteOpResult::Messages(_)
            | WriteOpResult::UpsertMessage { .. }
            | WriteOpResult::Acknowledged(_) => Err(AtmError::daemon_unavailable(
                "sqlite writer returned the wrong result for atomic message commit",
            )),
        }
    }

    pub(crate) fn submit_acknowledgement(
        &self,
        source: AcknowledgementSource,
        builder: std::sync::Arc<dyn AcknowledgementReplyBuilder>,
    ) -> Result<AcknowledgementCommit, AtmError> {
        match self
            .writer
            .submit(WriteOp::Acknowledge { source, builder })?
        {
            WriteOpResult::Acknowledged(commit) => Ok(*commit),
            WriteOpResult::Messages(_)
            | WriteOpResult::UpsertMessage { .. }
            | WriteOpResult::UpsertMessages => Err(AtmError::daemon_unavailable(
                "sqlite writer returned the wrong result for acknowledgement admission",
            )),
        }
    }

    pub(crate) async fn submit_acknowledgement_async(
        &self,
        source: AcknowledgementSource,
        builder: std::sync::Arc<dyn AcknowledgementReplyBuilder>,
    ) -> Result<AcknowledgementCommit, AtmError> {
        match self
            .writer
            .submit_async(WriteOp::Acknowledge { source, builder })
            .await?
        {
            WriteOpResult::Acknowledged(commit) => Ok(*commit),
            WriteOpResult::Messages(_)
            | WriteOpResult::UpsertMessage { .. }
            | WriteOpResult::UpsertMessages => Err(AtmError::daemon_unavailable(
                "sqlite writer returned the wrong result for async acknowledgement admission",
            )),
        }
    }

    pub(crate) async fn submit_list_messages_async(
        &self,
        query: MessageQuery,
    ) -> Result<Vec<Message>, AtmError> {
        match self
            .writer
            .submit_async(WriteOp::ListMessages(query))
            .await?
        {
            WriteOpResult::Messages(messages) => Ok(messages),
            WriteOpResult::UpsertMessage { .. }
            | WriteOpResult::UpsertMessages
            | WriteOpResult::Acknowledged(_) => Err(AtmError::daemon_unavailable(
                "sqlite writer returned the wrong result for async mailbox projection",
            )),
        }
    }

    pub(crate) fn error(&self, message: impl Into<String>, source: RusqliteError) -> AtmError {
        sqlite_error(self.target.as_ref(), message, source)
    }

    pub(crate) fn target_path(&self) -> Option<&PathBuf> {
        match self.target.as_ref() {
            SharedDbTarget::Path(path) => Some(path),
            #[cfg(test)]
            SharedDbTarget::InMemory { .. } => None,
        }
    }

    pub(crate) fn checkpoint_wal(&self) -> Result<(), AtmError> {
        #[cfg(test)]
        if matches!(self.target.as_ref(), SharedDbTarget::InMemory { .. }) {
            return Ok(());
        }

        let result = self.with_connection(|connection| {
            connection
                .query_row("PRAGMA wal_checkpoint(PASSIVE);", [], |_row| Ok(()))
                .map_err(|error| {
                    sqlite_error(
                        self.target.as_ref(),
                        "failed to checkpoint sqlite wal during daemon shutdown",
                        error,
                    )
                })
        });
        match &result {
            Ok(()) => {}
            Err(error) => self
                .observability
                .emit_or_warn(SqliteObservabilityEvent::new(
                    "wal_checkpoint",
                    SqliteObservabilityOutcome::Failed,
                    error.message().to_owned(),
                    Some(error.code()),
                )),
        }
        result
    }

    fn open_connection(&self) -> Result<Connection, AtmError> {
        open_connection_for_target(self.target.as_ref())
    }
}

impl std::fmt::Debug for SharedDb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedDb")
            .field("target", &self.target.display())
            .finish()
    }
}

fn debug_assert_blocking_only(method: &str) {
    #[cfg(not(debug_assertions))]
    let _ = method;
    #[cfg(debug_assertions)]
    {
        let current_thread = std::thread::current();
        let thread_name = current_thread.name().unwrap_or("<unnamed>");
        debug_assert!(
            !thread_name.starts_with("tokio-runtime-worker"),
            "{method} must run from a blocking code path; enter spawn_blocking before borrowing sqlite state"
        );
    }
}

pub(crate) fn configure_connection(
    connection: &mut Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    // Bound SQLite lock waits so contention returns an actionable ATM timeout
    // instead of hanging an adapter thread indefinitely.
    connection
        .busy_timeout(std::time::Duration::from_millis(SQLITE_BUSY_TIMEOUT_MS))
        .map_err(|error| sqlite_error(target, "failed to configure sqlite busy timeout", error))?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(|error| sqlite_error(target, "failed to enable sqlite foreign keys", error))?;
    Ok(())
}

/// WAL is durable database state, not per-connection request setup. Configure
/// it once when the sole writer owns startup; readers only need their local
/// timeout and foreign-key settings.
fn enable_write_ahead_log(
    connection: &mut Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    #[cfg(test)]
    let enable_wal = !matches!(target, SharedDbTarget::InMemory { .. });
    #[cfg(not(test))]
    let enable_wal = true;
    if enable_wal {
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(|error| {
                sqlite_error(target, "failed to enable sqlite wal journal mode", error)
            })?;
    }
    Ok(())
}

pub(crate) fn open_connection_for_target(target: &SharedDbTarget) -> Result<Connection, AtmError> {
    let mut connection = match target {
        SharedDbTarget::Path(path) => {
            // Accepted risk: sqlite connection open is a process-init boundary
            // operation and may block on the host filesystem before runtime
            // request handling starts.
            Connection::open(path).map_err(|error| sqlite_open_error(target, error))?
        }
        #[cfg(test)]
        SharedDbTarget::InMemory { uri } => Connection::open_with_flags(
            uri,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        )
        .map_err(|error| sqlite_open_error(target, error))?,
    };
    configure_connection(&mut connection, target)?;
    Ok(connection)
}

pub(crate) fn open_writer_connection_for_target(
    target: &SharedDbTarget,
) -> Result<Connection, AtmError> {
    let mut connection = open_connection_for_target(target)?;
    enable_write_ahead_log(&mut connection, target)?;
    Ok(connection)
}

pub(crate) fn ensure_schema(
    connection: &mut Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    ensure_mail_messages_message_id_compat(connection, target)?;
    connection
        .execute_batch(DB_MIGRATIONS)
        .map_err(|error| sqlite_error(target, "failed to initialize sqlite schema", error))?;
    ensure_mail_message_columns(connection, target)?;
    ensure_team_roster_columns(connection, target)?;
    ensure_team_roster_harness_values(connection, target)?;
    ensure_team_nudge_template_override_columns(connection, target)?;
    ensure_column(
        connection,
        target,
        "peer_trusted_peers",
        "https_port",
        "ALTER TABLE peer_trusted_peers ADD COLUMN https_port INTEGER NOT NULL DEFAULT 43101 CHECK(https_port BETWEEN 1 AND 65535);",
    )?;
    Ok(())
}

fn ensure_mail_message_columns(
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    ensure_column(
        connection,
        target,
        "mail_messages",
        "from_agent",
        "ALTER TABLE mail_messages ADD COLUMN from_agent TEXT NULL;",
    )?;
    ensure_column(
        connection,
        target,
        "mail_messages",
        "source_chat_id",
        "ALTER TABLE mail_messages ADD COLUMN source_chat_id TEXT NULL;",
    )?;
    ensure_column(
        connection,
        target,
        "mail_messages",
        "destination_chat_id",
        "ALTER TABLE mail_messages ADD COLUMN destination_chat_id TEXT NULL;",
    )?;
    ensure_column(
        connection,
        target,
        "mail_messages",
        "message_text",
        "ALTER TABLE mail_messages ADD COLUMN message_text TEXT NULL;",
    )?;
    ensure_column(
        connection,
        target,
        "mail_messages",
        "summary",
        "ALTER TABLE mail_messages ADD COLUMN summary TEXT NULL;",
    )?;
    ensure_column(
        connection,
        target,
        "mail_messages",
        "message_at",
        "ALTER TABLE mail_messages ADD COLUMN message_at TEXT NULL;",
    )?;
    ensure_column(
        connection,
        target,
        "mail_messages",
        "message_id",
        "ALTER TABLE mail_messages ADD COLUMN message_id TEXT NULL;",
    )
}

fn ensure_team_roster_columns(
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    ensure_column(
        connection,
        target,
        "team_roster",
        "member_kind",
        "ALTER TABLE team_roster ADD COLUMN member_kind TEXT NOT NULL DEFAULT 'permanent';",
    )?;
    ensure_column(
        connection,
        target,
        "team_roster",
        "harness",
        "ALTER TABLE team_roster ADD COLUMN harness TEXT NOT NULL DEFAULT 'claude-code';",
    )?;
    ensure_column(
        connection,
        target,
        "team_roster",
        "agent_type",
        "ALTER TABLE team_roster ADD COLUMN agent_type TEXT NOT NULL DEFAULT '';",
    )?;
    ensure_column(
        connection,
        target,
        "team_roster",
        "model",
        "ALTER TABLE team_roster ADD COLUMN model TEXT NOT NULL DEFAULT '';",
    )?;
    ensure_column(
        connection,
        target,
        "team_roster",
        "metadata_json",
        "ALTER TABLE team_roster ADD COLUMN metadata_json TEXT NOT NULL DEFAULT '{}';",
    )
}

fn ensure_team_roster_harness_values(
    connection: &mut Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    let table_sql = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'team_roster';",
            [],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to inspect team_roster harness constraint",
                error,
            )
        })?;
    if table_sql.contains("'hermes'") && table_sql.contains("'python-graft'") {
        return Ok(());
    }

    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to start team_roster harness migration",
                error,
            )
        })?;
    transaction
        .execute_batch(
            "DROP INDEX IF EXISTS idx_team_roster_team_name;
             ALTER TABLE team_roster RENAME TO team_roster_legacy_harness;
             CREATE TABLE team_roster (
                 team_name TEXT NOT NULL,
                 agent_name TEXT NOT NULL,
                 member_kind TEXT NOT NULL CHECK(member_kind IN ('permanent', 'ephemeral')),
                 harness TEXT NOT NULL CHECK(harness IN ('claude-code', 'codex-cli', 'gemini-cli', 'opencode', 'hermes', 'python-graft')),
                 agent_type TEXT NOT NULL DEFAULT '',
                 model TEXT NOT NULL DEFAULT '',
                 metadata_json TEXT NOT NULL DEFAULT '{}',
                 source TEXT,
                 recipient_pane_id TEXT NULL,
                 updated_at TEXT NOT NULL,
                 PRIMARY KEY (team_name, agent_name)
             );
             INSERT INTO team_roster(
                 team_name, agent_name, member_kind, harness, agent_type, model,
                 metadata_json, source, recipient_pane_id, updated_at
             )
             SELECT
                 team_name, agent_name, member_kind, harness, agent_type, model,
                 metadata_json, source, recipient_pane_id, updated_at
             FROM team_roster_legacy_harness;
             DROP TABLE team_roster_legacy_harness;
             CREATE INDEX idx_team_roster_team_name ON team_roster(team_name);",
        )
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to migrate team_roster harness constraint",
                error,
            )
        })?;
    transaction.commit().map_err(|error| {
        sqlite_error(
            target,
            "failed to commit team_roster harness migration",
            error,
        )
    })
}

fn ensure_team_nudge_template_override_columns(
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    ensure_column(
        connection,
        target,
        "team_nudge_template_overrides",
        "mode",
        "ALTER TABLE team_nudge_template_overrides ADD COLUMN mode TEXT NOT NULL DEFAULT 'override';",
    )?;
    connection
        .execute(
            "UPDATE team_nudge_template_overrides
             SET mode = 'disabled'
             WHERE mode = 'override' AND template_body = '';",
            [],
        )
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to normalize legacy empty nudge-template override rows",
                error,
            )
        })?;
    Ok(())
}

fn ensure_mail_messages_message_id_compat(
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    if !table_exists(connection, target, "mail_messages")? {
        return Ok(());
    }
    ensure_column(
        connection,
        target,
        "mail_messages",
        "message_id",
        "ALTER TABLE mail_messages ADD COLUMN message_id TEXT NULL;",
    )
}

fn ensure_column(
    connection: &Connection,
    target: &SharedDbTarget,
    table: &str,
    column: &str,
    ddl: &str,
) -> Result<(), AtmError> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table});"))
        .map_err(|error| {
            sqlite_error(
                target,
                format!("failed to inspect sqlite table {table}"),
                error,
            )
        })?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| {
            sqlite_error(
                target,
                format!("failed to enumerate sqlite columns for {table}"),
                error,
            )
        })?;
    let collected = columns
        .into_iter()
        .map(|entry| {
            entry.map_err(|error| {
                sqlite_error(
                    target,
                    format!("failed to read sqlite column metadata for {table}"),
                    error,
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if collected.into_iter().any(|value| value == column) {
        return Ok(());
    }
    connection.execute_batch(ddl).map_err(|error| {
        sqlite_error(
            target,
            format!("failed to migrate sqlite table {table}"),
            error,
        )
    })
}

fn table_exists(
    connection: &Connection,
    target: &SharedDbTarget,
    table: &str,
) -> Result<bool, AtmError> {
    connection
        .query_row(
            "SELECT COUNT(1) FROM sqlite_master WHERE type = 'table' AND name = ?1;",
            [table],
            |row| row.get::<_, i64>(0),
        )
        .map(|count| count > 0)
        .map_err(|error| {
            sqlite_error(
                target,
                format!("failed to inspect sqlite table existence for {table}"),
                error,
            )
        })
}

fn sqlite_open_error(target: &SharedDbTarget, source: RusqliteError) -> AtmError {
    sqlite_error(
        target,
        format!("failed to open sqlite database {}", target.display()),
        source,
    )
}

pub(crate) fn sqlite_error(
    target: &SharedDbTarget,
    message: impl Into<String>,
    source: RusqliteError,
) -> AtmError {
    let message = message.into();
    match &source {
        RusqliteError::SqliteFailure(error, _) => {
            match error.code {
                rusqlite::ffi::ErrorCode::ConstraintViolation => AtmError::validation(message),
                rusqlite::ffi::ErrorCode::DatabaseBusy
                | rusqlite::ffi::ErrorCode::DatabaseLocked => match target {
                    SharedDbTarget::Path(path) => AtmError::mailbox_lock_timeout(path),
                    #[cfg(test)]
                    SharedDbTarget::InMemory { .. } => AtmError::mailbox_lock(format!(
                        "timed out waiting for sqlite database lock on {}",
                        target.display()
                    )),
                },
                rusqlite::ffi::ErrorCode::OperationInterrupted => match target {
                    SharedDbTarget::Path(path) => AtmError::mailbox_lock_timeout(path),
                    #[cfg(test)]
                    SharedDbTarget::InMemory { .. } => AtmError::mailbox_lock(
                        "sqlite query exceeded its caller-provided execution budget",
                    ),
                },
                rusqlite::ffi::ErrorCode::CannotOpen => AtmError::mailbox_write(message),
                rusqlite::ffi::ErrorCode::ReadOnly => AtmError::mailbox_write(message),
                rusqlite::ffi::ErrorCode::DatabaseCorrupt
                | rusqlite::ffi::ErrorCode::NotADatabase => AtmError::mailbox_read(message),
                rusqlite::ffi::ErrorCode::SystemIoFailure | rusqlite::ffi::ErrorCode::DiskFull => {
                    AtmError::mailbox_write(message)
                }
                _ => AtmError::mailbox_write(message),
            }
        }
        _ => AtmError::mailbox_write(message),
    }
}

fn json_error(message: impl Into<String>, _source: serde_json::Error) -> AtmError {
    AtmError::validation(message)
}

pub(crate) fn serialize_json<T: serde::Serialize>(
    value: &T,
    what: &str,
) -> Result<String, AtmError> {
    serde_json::to_string(value)
        .map_err(|error| json_error(format!("failed to encode {what}"), error))
}

pub(crate) fn deserialize_json<T: serde::de::DeserializeOwned>(
    value: &str,
    what: &str,
) -> Result<T, AtmError> {
    serde_json::from_str(value)
        .map_err(|error| json_error(format!("failed to decode {what}"), error))
}

pub(crate) fn sqlite_thread_mode(mode: Option<ThreadMode>) -> Option<&'static str> {
    match mode {
        Some(ThreadMode::AddDetails) => Some("add-details"),
        Some(ThreadMode::Supersede) => Some("supersede"),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_schema_drops_the_retired_peer_sync_policy_table() {
        let target = SharedDbTarget::InMemory {
            uri: format!(
                "file:atm-storage-rusqlite-peer-sync-retirement-{}?mode=memory&cache=shared",
                NEXT_IN_MEMORY_DB_ID.fetch_add(1, Ordering::Relaxed)
            ),
        };
        let mut connection = open_connection_for_target(&target).expect("open connection");
        connection
            .execute_batch(
                "CREATE TABLE peer_sync_policies (
                    host TEXT NOT NULL PRIMARY KEY,
                    max_message_age_seconds INTEGER NOT NULL,
                    max_batch_messages INTEGER NOT NULL
                 );
                 CREATE INDEX idx_peer_sync_policies_host ON peer_sync_policies(host);",
            )
            .expect("create legacy peer sync configuration");

        ensure_schema(&mut connection, &target).expect("migrate schema");

        assert!(
            !table_exists(&connection, &target, "peer_sync_policies")
                .expect("inspect retired table"),
            "AK.2 must remove the obsolete worker policy from existing databases"
        );
    }

    #[test]
    fn ensure_schema_adds_the_message_key_lookup_index_for_post_commit_dispatch() {
        let target = SharedDbTarget::InMemory {
            uri: format!(
                "file:atm-storage-rusqlite-message-key-index-{}?mode=memory&cache=shared",
                NEXT_IN_MEMORY_DB_ID.fetch_add(1, Ordering::Relaxed)
            ),
        };
        let mut connection = open_connection_for_target(&target).expect("open connection");
        ensure_schema(&mut connection, &target).expect("initialize schema");
        connection
            .execute_batch("DROP INDEX idx_mail_messages_message_key;")
            .expect("simulate database created before the lookup index");

        ensure_schema(&mut connection, &target).expect("upgrade existing schema");

        let plan: String = connection
            .query_row(
                "EXPLAIN QUERY PLAN
                 SELECT team, agent, envelope_json
                 FROM mail_messages
                 WHERE message_key = ?1;",
                ["atm:01J00000000000000000000000"],
                |row| row.get(3),
            )
            .expect("explain message-key lookup");
        assert!(
            plan.contains("idx_mail_messages_message_key"),
            "post-commit message lookup must remain indexed instead of scanning a growing mailbox: {plan}"
        );
    }
    use std::sync::Barrier;
    use std::thread;

    #[test]
    fn writer_initialization_persists_wal_for_later_reader_connections() {
        let tempdir = tempfile::tempdir().expect("create temporary database directory");
        let path = tempdir.path().join("wal.db");
        let target = SharedDbTarget::Path(path.clone());
        {
            let writer =
                open_writer_connection_for_target(&target).expect("open writer connection");
            let writer_mode: String = writer
                .query_row("PRAGMA journal_mode;", [], |row| row.get(0))
                .expect("read writer journal mode");
            assert_eq!(writer_mode.to_ascii_lowercase(), "wal");

            let reader = open_connection_for_target(&target).expect("open reader connection");
            let reader_mode: String = reader
                .query_row("PRAGMA journal_mode;", [], |row| row.get(0))
                .expect("read persisted journal mode");
            assert_eq!(reader_mode.to_ascii_lowercase(), "wal");
        }
    }

    #[test]
    fn concurrent_read_handles_do_not_fail_fast_at_an_arbitrary_reader_cap() {
        let db = Arc::new(SharedDb::open_in_memory_for_test().expect("open database"));
        let start = Arc::new(Barrier::new(9));
        thread::scope(|scope| {
            let mut workers = Vec::new();
            for _ in 0..8 {
                let db = Arc::clone(&db);
                let start = Arc::clone(&start);
                workers.push(scope.spawn(move || {
                    start.wait();
                    db.with_connection(|connection| {
                        connection
                            .query_row("SELECT 1;", [], |row| row.get::<_, i64>(0))
                            .map_err(|error| db.error("concurrent reader failed", error))
                            .map(|_| ())
                    })
                }));
            }
            start.wait();
            for worker in workers {
                worker
                    .join()
                    .expect("reader thread panicked")
                    .expect("reader should succeed");
            }
        });
    }

    #[test]
    fn ensure_team_roster_harness_values_migrates_legacy_check_constraint() {
        let target = SharedDbTarget::InMemory {
            uri: format!(
                "file:atm-storage-rusqlite-roster-harness-test-{}?mode=memory&cache=shared",
                NEXT_IN_MEMORY_DB_ID.fetch_add(1, Ordering::Relaxed)
            ),
        };
        let mut connection = open_connection_for_target(&target).expect("open connection");
        connection
            .execute_batch(
                "CREATE TABLE team_roster (
                    team_name TEXT NOT NULL,
                    agent_name TEXT NOT NULL,
                    member_kind TEXT NOT NULL CHECK(member_kind IN ('permanent', 'ephemeral')),
                    harness TEXT NOT NULL CHECK(harness IN ('claude-code', 'codex-cli', 'gemini-cli', 'opencode')),
                    agent_type TEXT NOT NULL DEFAULT '',
                    model TEXT NOT NULL DEFAULT '',
                    metadata_json TEXT NOT NULL DEFAULT '{}',
                    source TEXT,
                    recipient_pane_id TEXT NULL,
                    updated_at TEXT NOT NULL,
                    PRIMARY KEY (team_name, agent_name)
                );
                CREATE INDEX idx_team_roster_team_name ON team_roster(team_name);
                INSERT INTO team_roster(
                    team_name, agent_name, member_kind, harness, updated_at
                ) VALUES ('hermes', 'skillrx', 'permanent', 'claude-code', 'now');",
            )
            .expect("create legacy roster table");

        ensure_team_roster_harness_values(&mut connection, &target).expect("migrate roster");
        connection
            .execute(
                "INSERT INTO team_roster(
                    team_name, agent_name, member_kind, harness, updated_at
                ) VALUES ('hermes', 'python-worker', 'permanent', 'python-graft', 'now');",
                [],
            )
            .expect("new harness should satisfy migrated check constraint");
        let harnesses: Vec<String> = connection
            .prepare(
                "SELECT harness FROM team_roster
                 WHERE team_name = 'hermes' ORDER BY agent_name;",
            )
            .expect("prepare harness query")
            .query_map([], |row| row.get(0))
            .expect("query harnesses")
            .collect::<Result<_, _>>()
            .expect("decode harnesses");
        assert_eq!(harnesses, vec!["python-graft", "claude-code"]);
    }

    #[test]
    fn ensure_team_nudge_template_override_columns_migrates_legacy_empty_rows_to_disabled() {
        let target = SharedDbTarget::InMemory {
            uri: format!(
                "file:atm-storage-rusqlite-shared-db-test-{}?mode=memory&cache=shared",
                NEXT_IN_MEMORY_DB_ID.fetch_add(1, Ordering::Relaxed)
            ),
        };
        let connection = open_connection_for_target(&target).expect("open connection");
        connection
            .execute_batch(
                "CREATE TABLE team_nudge_template_overrides (
                    team_name TEXT NOT NULL,
                    template_kind TEXT NOT NULL,
                    template_body TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    PRIMARY KEY (team_name, template_kind)
                );",
            )
            .expect("create pre-migration table");
        connection
            .execute(
                "INSERT INTO team_nudge_template_overrides(
                    team_name, template_kind, template_body, updated_at
                 ) VALUES (?1, ?2, ?3, ?4);",
                rusqlite::params![
                    "test-team",
                    "delivery_ack",
                    "",
                    atm_storage::types::IsoTimestamp::now().to_string()
                ],
            )
            .expect("insert legacy empty-body row");

        ensure_team_nudge_template_override_columns(&connection, &target)
            .expect("migrate override table");

        let mode_exists = connection
            .prepare("PRAGMA table_info(team_nudge_template_overrides);")
            .expect("prepare pragma")
            .query_map([], |row| row.get::<_, String>(1))
            .expect("query pragma")
            .filter_map(Result::ok)
            .any(|column| column == "mode");
        assert!(mode_exists, "schema upgrade should add the mode column");

        let mode: String = connection
            .query_row(
                "SELECT mode FROM team_nudge_template_overrides
                 WHERE team_name = ?1 AND template_kind = ?2;",
                rusqlite::params!["test-team", "delivery_ack"],
                |row| row.get(0),
            )
            .expect("query migrated mode");
        assert_eq!(mode, "disabled");
    }
}
