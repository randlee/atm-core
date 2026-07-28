#[cfg(test)]
use crate::observability::NullSqliteObservability;
use crate::observability::{
    SqliteObservability, SqliteObservabilityEvent, SqliteObservabilityOutcome,
};
use crate::writer::{SqliteWriter, WriteOp, WriteOpResult, validate_upsert_message_request};
use atm_storage::contract::Message;
use atm_storage::error::AtmError;
use atm_storage::schema::ThreadMode;
#[cfg(test)]
use rusqlite::OpenFlags;
use rusqlite::{Connection, Error as RusqliteError, TransactionBehavior};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Keep one permanent writer handle plus at most three concurrent reader handles so the
/// same-process SQLite budget stays explicit under WAL mode without turning read bursts into an
/// unbounded connection fan-out.
const MAX_SQLITE_READER_CONNECTIONS: usize = 3;
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

CREATE TABLE IF NOT EXISTS peer_sync_policies (
    host TEXT NOT NULL PRIMARY KEY,
    max_message_age_seconds INTEGER NOT NULL CHECK(max_message_age_seconds >= 0),
    max_batch_messages INTEGER NOT NULL CHECK(max_batch_messages BETWEEN 1 AND 65535),
    FOREIGN KEY(host) REFERENCES peer_trusted_peers(host) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_mail_messages_single_successor
    ON mail_messages(team, agent, parent_message_id)
    WHERE parent_message_id IS NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS uq_mail_messages_message_id
    ON mail_messages(team, agent, message_id)
    WHERE message_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_mail_messages_mailbox
    ON mail_messages(team, agent);

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

CREATE INDEX IF NOT EXISTS idx_peer_sync_policies_host
    ON peer_sync_policies(host);
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
    // Reader handles are budgeted across cloned SharedDb adapters, so the
    // counter must be shared and synchronized independently of any one
    // connection instance.
    connection_count: Arc<Mutex<usize>>,
}

#[derive(Debug)]
struct SharedDbConnectionGuard {
    // Connection release can happen on whichever clone drops the guard, so the
    // shared counter uses the same synchronized ownership model as SharedDb.
    connection_count: Arc<Mutex<usize>>,
}

impl Drop for SharedDbConnectionGuard {
    fn drop(&mut self) {
        if let Ok(mut connection_count) = self.connection_count.lock() {
            *connection_count = connection_count.saturating_sub(1);
        }
    }
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
            connection_count: Arc::new(Mutex::new(0)),
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
            reader_budget = MAX_SQLITE_READER_CONNECTIONS,
            path = %target.display(),
            "sqlite boundary assembly opened"
        );
        Ok(Self {
            target,
            writer,
            observability,
            connection_count: Arc::new(Mutex::new(0)),
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
        let _connection_guard = self.acquire_connection_guard()?;
        let mut connection = self.open_connection()?;
        operation(&mut connection)
    }

    /// Runs one read with a caller-owned execution budget. SQLite's progress
    /// callback interrupts VM execution when the budget expires, while the
    /// matching busy timeout bounds lock acquisition. This is deliberately a
    /// separate path so ordinary storage reads retain their existing policy.
    pub(crate) fn with_connection_budget<T>(
        &self,
        budget: Duration,
        operation: impl FnOnce(&mut Connection) -> Result<T, AtmError>,
    ) -> Result<T, AtmError> {
        debug_assert_blocking_only("SharedDb::with_connection_budget");
        if budget.is_zero() {
            return Err(AtmError::mailbox_lock(
                "sqlite query budget expired before the read started",
            ));
        }
        let _connection_guard = self.acquire_connection_guard()?;
        let mut connection = self.open_connection()?;
        connection.busy_timeout(budget).map_err(|error| {
            sqlite_error(
                self.target.as_ref(),
                "failed to configure sqlite query budget",
                error,
            )
        })?;
        let expires_at = Instant::now() + budget;
        connection.progress_handler(1_000, Some(move || Instant::now() >= expires_at));
        let result = operation(&mut connection);
        connection.progress_handler(0, None::<fn() -> bool>);
        result
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

    pub(crate) fn submit_upsert_message(&self, record: Message) -> Result<(), AtmError> {
        validate_upsert_message_request(&record)?;
        let result = self
            .writer
            .submit(WriteOp::UpsertMessage(Box::new(record)))?;
        match result {
            WriteOpResult::UpsertMessage { .. } => Ok(()),
            WriteOpResult::UpsertMessages => Err(AtmError::daemon_unavailable(
                "sqlite writer returned the wrong result for message upsert",
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
            WriteOpResult::UpsertMessage { .. } => Err(AtmError::daemon_unavailable(
                "sqlite writer returned the wrong result for atomic message commit",
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

    fn acquire_connection_guard(&self) -> Result<SharedDbConnectionGuard, AtmError> {
        let mut connection_count = self.connection_count.lock().map_err(|_| {
            let error =
                AtmError::daemon_unavailable("sqlite connection budget state lock poisoned");
            self.observability
                .emit_or_warn(SqliteObservabilityEvent::new(
                    "reader_budget_state",
                    SqliteObservabilityOutcome::Failed,
                    error.message().to_owned(),
                    Some(error.code()),
                ));
            error
        })?;
        if *connection_count >= MAX_SQLITE_READER_CONNECTIONS {
            tracing::warn!(
                limit = MAX_SQLITE_READER_CONNECTIONS,
                current = %connection_count,
                "sqlite reader connection budget exhausted"
            );
            let error = reader_budget_exceeded_error();
            self.observability
                .emit_or_warn(SqliteObservabilityEvent::new(
                    "reader_budget_acquire",
                    SqliteObservabilityOutcome::Failed,
                    error.message().to_owned(),
                    Some(error.code()),
                ));
            return Err(error);
        }
        *connection_count += 1;
        Ok(SharedDbConnectionGuard {
            connection_count: Arc::clone(&self.connection_count),
        })
    }

    fn open_connection(&self) -> Result<Connection, AtmError> {
        open_connection_for_target(self.target.as_ref())
    }
}

fn reader_budget_exceeded_error() -> AtmError {
    AtmError::daemon_unavailable(format!(
        "sqlite connection budget exceeded (max {MAX_SQLITE_READER_CONNECTIONS} concurrent reader handles with one permanent writer handle)"
    ))
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
