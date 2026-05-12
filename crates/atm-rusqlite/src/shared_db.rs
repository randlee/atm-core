use crate::writer::{SqliteWriter, WriteOp, WriteOpResult, validate_upsert_message_request};
use atm_core::error::AtmError;
use atm_core::home;
#[cfg(test)]
use rusqlite::OpenFlags;
use rusqlite::{Connection, Error as RusqliteError};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Keep one permanent writer handle plus at most three concurrent reader handles so the
/// same-process SQLite budget stays explicit under WAL mode without turning read bursts into an
/// unbounded connection fan-out.
const MAX_SQLITE_READER_CONNECTIONS: usize = 3;
#[cfg(test)]
static NEXT_IN_MEMORY_DB_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) const DB_MIGRATIONS: &str = r#"
CREATE TABLE IF NOT EXISTS mail_messages (
    team TEXT NOT NULL,
    agent TEXT NOT NULL,
    message_key TEXT NOT NULL,
    envelope_json TEXT NOT NULL,
    from_agent TEXT NOT NULL,
    message_text TEXT NOT NULL,
    summary TEXT NULL,
    message_at TEXT NOT NULL,
    message_id TEXT NULL,
    parent_message_id TEXT NULL,
    thread_mode TEXT NULL CHECK(thread_mode IS NULL OR thread_mode IN ('add-details', 'supersede')),
    imported_from TEXT,
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

CREATE TABLE IF NOT EXISTS mail_ingest_replay_states (
    team TEXT NOT NULL,
    agent TEXT NOT NULL,
    source TEXT NOT NULL,
    state_json TEXT NOT NULL,
    PRIMARY KEY (team, agent, source)
);

CREATE TABLE IF NOT EXISTS daemon_remote_replay_states (
    team TEXT NOT NULL,
    agent TEXT NOT NULL,
    message_key TEXT NOT NULL,
    state_json TEXT NOT NULL,
    PRIMARY KEY (team, agent, message_key)
);

CREATE TABLE IF NOT EXISTS tasks (
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
    recipient_pane_id TEXT NULL,
    pid INTEGER NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS team_roster (
    team_name TEXT NOT NULL,
    agent_name TEXT NOT NULL,
    member_json TEXT NOT NULL,
    source TEXT,
    recipient_pane_id TEXT NULL,
    pid INTEGER NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (team_name, agent_name)
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

CREATE INDEX IF NOT EXISTS idx_mail_ingest_mailbox
    ON mail_ingest_replay_states(team, agent);

CREATE INDEX IF NOT EXISTS idx_daemon_remote_replay_mailbox
    ON daemon_remote_replay_states(team, agent);

CREATE INDEX IF NOT EXISTS idx_task_records_lookup
    ON tasks(team, task_id);

CREATE INDEX IF NOT EXISTS idx_team_roster_team_name
    ON team_roster(team_name);
"#;
// `rosters` remains the canonical per-team `TeamConfig` snapshot, while
// `team_roster` is the per-member durable projection that runtime lookup uses.

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
    // Reader handles are budgeted across cloned SharedDb adapters, so the
    // counter must be shared and synchronized independently of any one
    // connection instance.
    connection_count: Arc<Mutex<usize>>,
}

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
    pub(crate) fn production_path() -> Result<PathBuf, AtmError> {
        home::host_mail_db_path()
    }

    #[cfg(test)]
    pub(crate) fn production_path_from_home(home_dir: &Path) -> PathBuf {
        home::host_mail_db_path_from_home(home_dir)
    }

    #[cfg(test)]
    pub(crate) fn open_in_memory() -> Result<Self, AtmError> {
        let target = Arc::new(SharedDbTarget::InMemory {
            uri: format!(
                "file:atm-rusqlite-{}?mode=memory&cache=shared",
                NEXT_IN_MEMORY_DB_ID.fetch_add(1, Ordering::Relaxed)
            ),
        });
        let writer = Arc::new(SqliteWriter::start(Arc::clone(&target))?);
        Ok(Self {
            target,
            writer,
            connection_count: Arc::new(Mutex::new(0)),
        })
    }

    pub(crate) fn open(path: impl AsRef<Path>) -> Result<Self, AtmError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            // Accepted risk: durable-state root creation happens during boundary
            // assembly and is allowed to block on the host filesystem once.
            std::fs::create_dir_all(parent).map_err(|error| {
                AtmError::mailbox_write(format!(
                    "failed to create sqlite parent directory {}: {error}",
                    parent.display()
                ))
                .with_recovery(
                    "Check the sqlite database directory permissions or choose a different ATM durable-state root before retrying.",
                )
                .with_source(error)
            })?;
        }

        let target = Arc::new(SharedDbTarget::Path(path));
        let writer = Arc::new(SqliteWriter::start(Arc::clone(&target))?);
        tracing::debug!(
            writer_handles = 1,
            reader_budget = MAX_SQLITE_READER_CONNECTIONS,
            path = %target.display(),
            "sqlite boundary assembly opened"
        );
        Ok(Self {
            target,
            writer,
            connection_count: Arc::new(Mutex::new(0)),
        })
    }

    /// Call only from blocking code paths; async callers must enter
    /// `spawn_blocking` before borrowing a sqlite connection.
    pub(crate) fn with_connection<T>(
        &self,
        operation: impl FnOnce(&mut Connection) -> Result<T, AtmError>,
    ) -> Result<T, AtmError> {
        let _connection_guard = self.acquire_connection_guard()?;
        let mut connection = self.open_connection()?;
        operation(&mut connection)
    }

    /// Call only from blocking code paths; async callers must enter
    /// `spawn_blocking` before opening a sqlite transaction.
    pub(crate) fn with_transaction<T>(
        &self,
        operation: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<T, AtmError>,
    ) -> Result<T, AtmError> {
        self.with_connection(|connection| {
            let transaction = connection.transaction().map_err(|error| {
                sqlite_error(
                    self.target.as_ref(),
                    "failed to open sqlite transaction",
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

    pub(crate) fn submit_upsert_message(
        &self,
        request: atm_core::boundary::MailStoreUpsertMessageRequest,
    ) -> Result<atm_core::boundary::MailStoreUpsertMessageResponse, AtmError> {
        validate_upsert_message_request(&request)?;
        let record = request.record.clone();
        let result = self
            .writer
            .submit(WriteOp::UpsertMessage(Box::new(request)))?;
        match result {
            WriteOpResult::UpsertMessage { inserted } => {
                Ok(atm_core::boundary::MailStoreUpsertMessageResponse { record, inserted })
            }
            other => Err(AtmError::daemon_unavailable(format!(
                "sqlite writer returned unexpected result for upsert_message: {other:?}"
            ))
            .with_recovery(
                "Inspect the sqlite writer operation routing and retry after correcting the mismatched result contract.",
            )),
        }
    }

    pub(crate) fn submit_upsert_message_state(
        &self,
        request: atm_core::boundary::UpsertMailMessageStateRequest,
    ) -> Result<atm_core::boundary::UpsertMailMessageStateResponse, AtmError> {
        let state = request.state.clone();
        let result = self
            .writer
            .submit(WriteOp::UpsertMessageState(Box::new(request)))?;
        match result {
            WriteOpResult::Unit => Ok(atm_core::boundary::UpsertMailMessageStateResponse {
                state,
            }),
            other => Err(AtmError::daemon_unavailable(format!(
                "sqlite writer returned unexpected result for upsert_message_state: {other:?}"
            ))
            .with_recovery(
                "Inspect the sqlite writer operation routing and retry after correcting the mismatched result contract.",
            )),
        }
    }

    pub(crate) fn error(&self, message: impl Into<String>, source: RusqliteError) -> AtmError {
        sqlite_error(self.target.as_ref(), message, source)
    }

    pub(crate) fn target(&self) -> &SharedDbTarget {
        self.target.as_ref()
    }

    pub(crate) fn checkpoint_wal(&self) -> Result<(), AtmError> {
        #[cfg(test)]
        if matches!(self.target.as_ref(), SharedDbTarget::InMemory { .. }) {
            return Ok(());
        }

        self.with_connection(|connection| {
            connection
                .query_row("PRAGMA wal_checkpoint(PASSIVE);", [], |_row| Ok(()))
                .map_err(|error| {
                    sqlite_error(
                        self.target.as_ref(),
                        "failed to checkpoint sqlite wal during daemon shutdown",
                        error,
                    )
                })
        })
    }

    fn acquire_connection_guard(&self) -> Result<SharedDbConnectionGuard, AtmError> {
        let mut connection_count = self.connection_count.lock().map_err(|_| {
            AtmError::daemon_unavailable("sqlite connection budget state lock poisoned")
                .with_recovery(
                    "Restart the daemon or recreate the sqlite boundary assembly before retrying the shared connection budget path.",
                )
        })?;
        if *connection_count >= MAX_SQLITE_READER_CONNECTIONS {
            tracing::warn!(
                limit = MAX_SQLITE_READER_CONNECTIONS,
                current = %connection_count,
                "sqlite reader connection budget exhausted"
            );
            return Err(AtmError::daemon_unavailable(format!(
                "sqlite connection budget exceeded (max {MAX_SQLITE_READER_CONNECTIONS} concurrent reader handles with one permanent writer handle)"
            ))
            .with_recovery(
                "Reduce concurrent daemon SQLite work or raise the documented SQLite handle budget before retrying.",
            ));
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

impl std::fmt::Debug for SharedDb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedDb")
            .field("target", &self.target.display())
            .finish()
    }
}

pub(crate) fn configure_connection(
    connection: &mut Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    connection
        .busy_timeout(std::time::Duration::from_millis(5000))
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
    connection
        .execute_batch(DB_MIGRATIONS)
        .map_err(|error| sqlite_error(target, "failed to initialize sqlite schema", error))?;
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
    )?;
    ensure_column(
        connection,
        target,
        "rosters",
        "recipient_pane_id",
        "ALTER TABLE rosters ADD COLUMN recipient_pane_id TEXT NULL;",
    )?;
    ensure_column(
        connection,
        target,
        "rosters",
        "pid",
        "ALTER TABLE rosters ADD COLUMN pid INTEGER NULL;",
    )?;
    Ok(())
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
    let error = match &source {
        RusqliteError::SqliteFailure(error, _) => match error.code {
            rusqlite::ffi::ErrorCode::ConstraintViolation => AtmError::validation(message)
                .with_recovery(
                    "Correct the conflicting ATM message/thread/store input so it satisfies the SQLite-backed durability constraints, then retry.",
                ),
            rusqlite::ffi::ErrorCode::DatabaseBusy
            | rusqlite::ffi::ErrorCode::DatabaseLocked => match target {
                SharedDbTarget::Path(path) => AtmError::mailbox_lock_timeout(path),
                #[cfg(test)]
                SharedDbTarget::InMemory { .. } => AtmError::mailbox_lock(format!(
                    "timed out waiting for sqlite database lock on {}",
                    target.display()
                )),
            },
            rusqlite::ffi::ErrorCode::CannotOpen => AtmError::mailbox_write(message)
                .with_recovery(
                    "Check the SQLite durable-state path, parent directory creation, and filesystem permissions before retrying.",
                ),
            rusqlite::ffi::ErrorCode::ReadOnly => AtmError::mailbox_write(message)
                .with_recovery(
                    "Remount the SQLite durable-state root as writable or choose a writable ATM durable-state path before retrying.",
                ),
            rusqlite::ffi::ErrorCode::DatabaseCorrupt
            | rusqlite::ffi::ErrorCode::NotADatabase => AtmError::mailbox_read(message)
                .with_recovery(
                    "Inspect or rebuild the SQLite durable-state store because the current file is corrupt or not a valid database.",
                ),
            rusqlite::ffi::ErrorCode::SystemIoFailure
            | rusqlite::ffi::ErrorCode::DiskFull => AtmError::mailbox_write(message)
                .with_recovery(
                    "Check the host filesystem health, available disk space, and SQLite durable-state path before retrying.",
                ),
            _ => AtmError::mailbox_write(message).with_recovery(
                "Inspect the SQLite durable-state store for corruption or permission faults before retrying.",
            ),
        },
        _ => AtmError::mailbox_write(message).with_recovery(
            "Inspect the SQLite durable-state store for corruption or permission faults before retrying.",
        ),
    };
    error.with_source(source)
}

fn json_error(message: impl Into<String>, source: serde_json::Error) -> AtmError {
    AtmError::validation(message)
        .with_recovery("Repair the persisted ATM-owned JSON payload or rebuild it through the owning boundary.")
        .with_source(source)
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

pub(crate) fn sqlite_thread_mode(
    mode: Option<atm_core::schema::ThreadMode>,
) -> Option<&'static str> {
    match mode {
        Some(atm_core::schema::ThreadMode::AddDetails) => Some("add-details"),
        Some(atm_core::schema::ThreadMode::Supersede) => Some("supersede"),
        None => None,
    }
}
