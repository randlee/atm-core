mod mail;
mod roster;
mod send;
mod task;

#[cfg(test)]
mod tests;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use atm_core::home::mail_db_path_from_home;
use atm_core::store::{
    BusyTimeoutMs, SqliteHandleBudget, StoreBootstrapReport, StoreBoundary, StoreError,
    StoreErrorKind, StoreHealth,
};
use atm_core::types::TeamName;
use rusqlite::{Connection, OptionalExtension, Transaction};

const SCHEMA_VERSION: i64 = 1;

const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS messages (
    message_key TEXT PRIMARY KEY,
    team_name TEXT NOT NULL,
    recipient_agent TEXT NOT NULL,
    sender_display TEXT NOT NULL,
    sender_canonical TEXT NULL,
    sender_team TEXT NULL,
    body TEXT NOT NULL,
    summary TEXT NULL,
    created_at TEXT NOT NULL,
    source_kind TEXT NOT NULL,
    legacy_message_id TEXT NULL UNIQUE,
    atm_message_id TEXT NULL UNIQUE,
    raw_metadata_json TEXT NULL
);

CREATE INDEX IF NOT EXISTS idx_messages_team_recipient_created
    ON messages(team_name, recipient_agent, created_at);

CREATE TABLE IF NOT EXISTS inbox_ingest (
    team_name TEXT NOT NULL,
    recipient_agent TEXT NOT NULL,
    source_path TEXT NOT NULL,
    source_fingerprint TEXT NOT NULL,
    message_key TEXT NOT NULL,
    imported_at TEXT NOT NULL,
    PRIMARY KEY (team_name, recipient_agent, source_fingerprint),
    FOREIGN KEY (message_key) REFERENCES messages(message_key) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS ack_state (
    message_key TEXT PRIMARY KEY,
    pending_ack_at TEXT NULL,
    acknowledged_at TEXT NULL,
    ack_reply_message_key TEXT NULL,
    ack_reply_team TEXT NULL,
    ack_reply_agent TEXT NULL,
    FOREIGN KEY (message_key) REFERENCES messages(message_key) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS tasks (
    task_id TEXT PRIMARY KEY,
    message_key TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    acknowledged_at TEXT NULL,
    metadata_json TEXT NULL,
    FOREIGN KEY (message_key) REFERENCES messages(message_key) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_tasks_message_key ON tasks(message_key);

CREATE TABLE IF NOT EXISTS team_roster (
    team_name TEXT NOT NULL,
    agent_name TEXT NOT NULL,
    role TEXT NOT NULL,
    transport_kind TEXT NOT NULL,
    host_name TEXT NOT NULL,
    recipient_pane_id TEXT NULL,
    pid INTEGER NULL,
    metadata_json TEXT NULL,
    PRIMARY KEY (team_name, agent_name)
);

CREATE TABLE IF NOT EXISTS message_visibility (
    message_key TEXT PRIMARY KEY,
    read_at TEXT NULL,
    cleared_at TEXT NULL,
    FOREIGN KEY (message_key) REFERENCES messages(message_key) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS pending_exports (
    message_key TEXT PRIMARY KEY,
    export_target_team TEXT NOT NULL,
    export_target_agent TEXT NOT NULL,
    recipient_pane_id TEXT NULL,
    attempt_count INTEGER NOT NULL,
    next_attempt_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    FOREIGN KEY (message_key) REFERENCES messages(message_key) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_pending_exports_next_attempt
    ON pending_exports(next_attempt_at);
"#;

#[derive(Debug)]
pub struct RusqliteStore {
    database_path: PathBuf,
    /// A single mutex-guarded `Connection` keeps write ordering explicit and
    /// matches the Q.1 single-writer store design without pretending reads are
    /// independent from the shared transaction lifecycle.
    connection: Mutex<Connection>,
    busy_timeout_ms: BusyTimeoutMs,
    handle_budget: SqliteHandleBudget,
}

impl RusqliteStore {
    pub fn open_for_team_home(home_dir: &Path, team_name: &TeamName) -> Result<Self, StoreError> {
        let database_path =
            mail_db_path_from_home(home_dir, team_name.as_str()).map_err(|error| {
                StoreError::open("failed to resolve team mail.db path").with_source(error)
            })?;
        Self::open_path(database_path)
    }

    pub fn open_path(database_path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Self::open_path_with_options(
            database_path.as_ref(),
            BusyTimeoutMs::DEFAULT,
            SqliteHandleBudget::DEFAULT,
        )
    }

    fn open_path_with_options(
        database_path: &Path,
        busy_timeout_ms: BusyTimeoutMs,
        handle_budget: SqliteHandleBudget,
    ) -> Result<Self, StoreError> {
        if let Some(parent) = database_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                StoreError::open(format!(
                    "failed to create SQLite store directory {}",
                    parent.display()
                ))
                .with_source(error)
            })?;
        }

        for attempt in 0..3 {
            let mut connection = Connection::open(database_path).map_err(|error| {
                StoreError::open(format!(
                    "failed to open SQLite store {}",
                    database_path.display()
                ))
                .with_source(error)
            })?;

            match configure_connection(&connection, busy_timeout_ms)
                .and_then(|()| bootstrap_schema(&mut connection))
            {
                Ok(()) => {
                    return Ok(Self {
                        database_path: database_path.to_path_buf(),
                        connection: Mutex::new(connection),
                        busy_timeout_ms,
                        handle_budget,
                    });
                }
                Err(error) if error.kind == StoreErrorKind::Bootstrap && attempt < 2 => {
                    thread::sleep(Duration::from_millis(25 * (attempt + 1) as u64));
                }
                Err(error) => return Err(error),
            }
        }

        unreachable!("bootstrap retry loop either returns success or the terminal error")
    }

    pub(crate) fn lock_connection(&self) -> Result<MutexGuard<'_, Connection>, StoreError> {
        self.connection
            .lock()
            .map_err(|_| StoreError::transaction("SQLite store mutex poisoned"))
    }

    pub(crate) fn with_transaction<T>(
        &self,
        action: impl FnOnce(&Transaction<'_>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction().map_err(|error| {
            StoreError::transaction("failed to start SQLite transaction").with_source(error)
        })?;
        let result = action(&transaction)?;
        transaction.commit().map_err(|error| {
            StoreError::transaction("failed to commit SQLite transaction").with_source(error)
        })?;
        Ok(result)
    }
}

impl StoreBoundary for RusqliteStore {
    fn bootstrap_report(&self) -> Result<StoreBootstrapReport, StoreError> {
        let connection = self.lock_connection()?;
        Ok(StoreBootstrapReport {
            database_path: self.database_path.clone(),
            schema_version: query_user_version(&connection)?,
            wal_enabled: query_journal_mode(&connection)?.eq_ignore_ascii_case("wal"),
            foreign_keys_enabled: query_foreign_keys(&connection)?,
            busy_timeout_ms: self.busy_timeout_ms,
            handle_budget: self.handle_budget,
        })
    }

    fn health(&self) -> Result<StoreHealth, StoreError> {
        let connection = self.lock_connection()?;
        Ok(StoreHealth {
            database_path: self.database_path.clone(),
            ready: true,
            schema_version: query_user_version(&connection)?,
        })
    }
}

pub(crate) fn configure_connection(
    connection: &Connection,
    busy_timeout_ms: BusyTimeoutMs,
) -> Result<(), StoreError> {
    connection
        .busy_timeout(Duration::from_millis(u64::from(busy_timeout_ms.get())))
        .map_err(|error| {
            StoreError::bootstrap("failed to set SQLite busy timeout").with_source(error)
        })?;
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(|error| {
            StoreError::bootstrap("failed to enable SQLite WAL mode").with_source(error)
        })?;
    connection
        .pragma_update(None, "foreign_keys", 1)
        .map_err(|error| {
            StoreError::bootstrap("failed to enable SQLite foreign_keys").with_source(error)
        })?;
    Ok(())
}

pub(crate) fn bootstrap_schema(connection: &mut Connection) -> Result<(), StoreError> {
    let transaction = connection.transaction().map_err(|error| {
        StoreError::bootstrap("failed to start schema bootstrap transaction").with_source(error)
    })?;
    transaction.execute_batch(SCHEMA_SQL).map_err(|error| {
        StoreError::bootstrap("failed to apply SQLite schema bootstrap").with_source(error)
    })?;
    transaction
        .pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(|error| {
            StoreError::migration("failed to persist SQLite schema version").with_source(error)
        })?;
    transaction.commit().map_err(|error| {
        StoreError::bootstrap("failed to commit SQLite schema bootstrap").with_source(error)
    })?;
    Ok(())
}

pub(crate) fn query_user_version(connection: &Connection) -> Result<i64, StoreError> {
    connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|error| {
            StoreError::query("failed to query SQLite user_version").with_source(error)
        })
}

pub(crate) fn query_journal_mode(connection: &Connection) -> Result<String, StoreError> {
    connection
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .map_err(|error| {
            StoreError::query("failed to query SQLite journal_mode").with_source(error)
        })
}

pub(crate) fn query_foreign_keys(connection: &Connection) -> Result<bool, StoreError> {
    let enabled: i64 = connection
        .pragma_query_value(None, "foreign_keys", |row| row.get(0))
        .map_err(|error| {
            StoreError::query("failed to query SQLite foreign_keys").with_source(error)
        })?;
    Ok(enabled == 1)
}

pub(crate) fn table_exists(connection: &Connection, table_name: &str) -> Result<bool, StoreError> {
    let exists: Option<i64> = connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table_name],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| classify_store_error(error, "failed to query sqlite_master"))?;
    Ok(exists.is_some())
}

pub(crate) fn classify_store_error(error: rusqlite::Error, context: &str) -> StoreError {
    if let rusqlite::Error::SqliteFailure(code, message) = &error {
        if code.code == rusqlite::ErrorCode::DatabaseBusy
            || code.code == rusqlite::ErrorCode::DatabaseLocked
        {
            return StoreError::busy(format!(
                "{context}: {}",
                message.as_deref().unwrap_or("database busy")
            ))
            .with_source(error);
        }
        if code.code == rusqlite::ErrorCode::ConstraintViolation
            || message
                .as_deref()
                .unwrap_or_default()
                .contains("constraint failed")
        {
            return StoreError::constraint(format!(
                "{context}: {}",
                message.as_deref().unwrap_or("constraint violation")
            ))
            .with_source(error);
        }
    }
    StoreError::query(format!("{context}: {error}")).with_source(error)
}

pub(crate) fn sqlite_error_detail(error: &rusqlite::Error) -> Option<&str> {
    match error {
        rusqlite::Error::SqliteFailure(_, Some(message)) => Some(message.as_str()),
        _ => None,
    }
}

pub(crate) fn parse_required<T>(value: String, field: &str) -> Result<T, StoreError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    value
        .parse::<T>()
        .map_err(|error| invalid_store_data(field, error))
}

pub(crate) fn parse_optional<T>(value: Option<String>, field: &str) -> Result<Option<T>, StoreError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    value.map(|value| parse_required(value, field)).transpose()
}

pub(crate) fn invalid_store_data(field: &str, error: impl std::fmt::Display) -> StoreError {
    StoreError::query(format!("invalid store data for {field}: {error}"))
}
