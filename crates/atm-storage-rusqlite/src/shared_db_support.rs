use atm_storage::error::AtmError;
use rusqlite::{Connection, Error as RusqliteError};
use std::path::PathBuf;

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

pub(crate) fn sqlite_open_error(target: &SharedDbTarget, source: RusqliteError) -> AtmError {
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
    let error =
        match &source {
            RusqliteError::SqliteFailure(error, _) => match error.code {
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
                rusqlite::ffi::ErrorCode::CannotOpen | rusqlite::ffi::ErrorCode::ReadOnly => {
                    AtmError::mailbox_write(message)
                }
                rusqlite::ffi::ErrorCode::DatabaseCorrupt
                | rusqlite::ffi::ErrorCode::NotADatabase => AtmError::mailbox_read(message),
                rusqlite::ffi::ErrorCode::SystemIoFailure | rusqlite::ffi::ErrorCode::DiskFull => {
                    AtmError::mailbox_write(message)
                }
                _ => AtmError::mailbox_write(message),
            },
            _ => AtmError::mailbox_write(message),
        };
    error.with_cause(source)
}

pub(crate) fn ensure_column(
    connection: &SqliteConnection,
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
