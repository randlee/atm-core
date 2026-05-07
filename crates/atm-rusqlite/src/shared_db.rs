use atm_core::boundary;
use atm_core::error::AtmError;
use atm_core::home;
#[cfg(test)]
use rusqlite::OpenFlags;
use rusqlite::{Connection, Error as RusqliteError};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(crate) const DB_MIGRATIONS: &str = r#"
PRAGMA journal_mode=WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS mail_messages (
    team TEXT NOT NULL,
    agent TEXT NOT NULL,
    message_key TEXT NOT NULL,
    envelope_json TEXT NOT NULL,
    parent_message_id TEXT NULL,
    thread_mode TEXT NULL CHECK(thread_mode IS NULL OR thread_mode IN ('add-details', 'supersede')),
    stale_at TEXT NULL,
    imported_from TEXT,
    recorded_at TEXT,
    CHECK(message_key GLOB 'atm:*' OR message_key GLOB 'ext:*'),
    PRIMARY KEY (team, agent, message_key)
);

CREATE TABLE IF NOT EXISTS ack_state (
    team TEXT NOT NULL,
    agent TEXT NOT NULL,
    message_key TEXT NOT NULL,
    pending_ack_at TEXT NULL,
    acknowledged_at TEXT NULL,
    updated_at TEXT NULL,
    PRIMARY KEY (team, agent, message_key),
    FOREIGN KEY (team, agent, message_key)
        REFERENCES mail_messages(team, agent, message_key)
        ON DELETE CASCADE
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
    recipient_pane_id TEXT NULL,
    pid INTEGER NULL,
    updated_at TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_mail_messages_single_successor
    ON mail_messages(team, agent, parent_message_id)
    WHERE parent_message_id IS NOT NULL;
"#;

#[derive(Debug, Clone)]
pub(crate) enum SharedDbTarget {
    Path(PathBuf),
    #[cfg(test)]
    InMemory,
}

impl SharedDbTarget {
    pub(crate) fn display(&self) -> String {
        match self {
            Self::Path(path) => path.display().to_string(),
            #[cfg(test)]
            Self::InMemory => ":memory:".to_string(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct SharedDb {
    target: Arc<SharedDbTarget>,
    #[cfg(test)]
    test_connection: Option<Arc<std::sync::Mutex<Connection>>>,
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
        let mut connection = Connection::open_in_memory()
            .map_err(|error| sqlite_open_error(&SharedDbTarget::InMemory, error))?;
        initialize_connection(&mut connection, &SharedDbTarget::InMemory)?;
        Ok(Self {
            target: Arc::new(SharedDbTarget::InMemory),
            test_connection: Some(Arc::new(std::sync::Mutex::new(connection))),
        })
    }

    pub(crate) fn open(path: impl AsRef<Path>) -> Result<Self, AtmError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
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

        let db = Self {
            target: Arc::new(SharedDbTarget::Path(path)),
            #[cfg(test)]
            test_connection: None,
        };
        db.with_connection(|_| Ok(()))?;
        Ok(db)
    }

    pub(crate) fn with_connection<T>(
        &self,
        operation: impl FnOnce(&mut Connection) -> Result<T, AtmError>,
    ) -> Result<T, AtmError> {
        #[cfg(test)]
        if let Some(connection) = &self.test_connection {
            let mut connection = connection.lock().map_err(|_| {
                AtmError::daemon_unavailable("sqlite test connection lock poisoned")
            })?;
            initialize_connection(&mut connection, self.target.as_ref())?;
            return operation(&mut connection);
        }

        let mut connection = match self.target.as_ref() {
            SharedDbTarget::Path(path) => Connection::open(path)
                .map_err(|error| sqlite_open_error(self.target.as_ref(), error))?,
            #[cfg(test)]
            SharedDbTarget::InMemory => Connection::open_with_flags(
                "file:atm-rusqlite?mode=memory&cache=shared",
                OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
            )
            .map_err(|error| sqlite_open_error(self.target.as_ref(), error))?,
        };
        initialize_connection(&mut connection, self.target.as_ref())?;
        operation(&mut connection)
    }

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

    pub(crate) fn error(&self, message: impl Into<String>, source: RusqliteError) -> AtmError {
        sqlite_error(self.target.as_ref(), message, source)
    }

    pub(crate) fn target(&self) -> &SharedDbTarget {
        self.target.as_ref()
    }
}

impl std::fmt::Debug for SharedDb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedDb")
            .field("target", &self.target.display())
            .finish()
    }
}

pub(crate) fn initialize_connection(
    connection: &mut Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    connection
        .busy_timeout(std::time::Duration::from_millis(5000))
        .map_err(|error| sqlite_error(target, "failed to configure sqlite busy timeout", error))?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(|error| sqlite_error(target, "failed to enable sqlite foreign keys", error))?;
    connection
        .execute_batch(DB_MIGRATIONS)
        .map_err(|error| sqlite_error(target, "failed to initialize sqlite schema", error))?;
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
    for entry in columns {
        if entry.map_err(|error| {
            sqlite_error(
                target,
                format!("failed to read sqlite column metadata for {table}"),
                error,
            )
        })? == column
        {
            return Ok(());
        }
    }
    connection.execute_batch(ddl).map_err(|error| {
        sqlite_error(
            target,
            format!("failed to migrate sqlite table {table}"),
            error,
        )
    })
}

pub(crate) fn validate_message_key_contract(
    message_key: &boundary::MessageKey,
) -> Result<(), AtmError> {
    let value = message_key.as_ref();
    if value.starts_with("atm:") || value.starts_with("ext:") {
        return Ok(());
    }

    Err(AtmError::validation(format!(
        "message key `{value}` must use an `atm:` or `ext:` prefix"
    ))
    .with_recovery(
        "Generate a canonical ATM message key with the required source-family prefix before writing it to SQLite.",
    ))
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
                SharedDbTarget::InMemory => AtmError::mailbox_lock(format!(
                    "timed out waiting for sqlite database lock on {}",
                    target.display()
                )),
            },
            rusqlite::ffi::ErrorCode::CannotOpen | rusqlite::ffi::ErrorCode::ReadOnly => {
                AtmError::mailbox_write(message).with_recovery(
                    "Check the SQLite durable-state path, filesystem permissions, and available disk space before retrying.",
                )
            }
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
