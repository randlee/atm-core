//! Database-wide schema version governed by ADR-061.

use crate::shared_db::{SharedDbTarget, SqliteConnection, sqlite_error};
use atm_storage::AtmError;

/// Semver for the SQLite schema installed by this crate.
///
/// The 2.2.0 minor adds durable authenticated-peer tool-sender replay bindings
/// after the hook, cron, and local signed-sender schema in 2.1.0.
pub const STORAGE_SCHEMA_VERSION: &str = "2.2.0";

const STORAGE_SCHEMA_MAJOR: i64 = 2;
const STORAGE_SCHEMA_MINOR: i64 = 2;
const STORAGE_SCHEMA_PATCH: i64 = 0;

const STORAGE_SCHEMA_VERSION_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS atm_storage_schema_version (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    major INTEGER NOT NULL CHECK(major >= 0),
    minor INTEGER NOT NULL CHECK(minor >= 0),
    patch INTEGER NOT NULL CHECK(patch >= 0)
);
"#;

pub(crate) fn ensure_schema_version(
    connection: &SqliteConnection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    connection
        .execute_batch(STORAGE_SCHEMA_VERSION_DDL)
        .map_err(|error| {
            sqlite_error(target, "failed to initialize SQLite schema version", error)
        })?;
    connection
        .execute(
            "INSERT INTO atm_storage_schema_version (singleton, major, minor, patch)
             VALUES (1, ?1, ?2, ?3)
             ON CONFLICT(singleton) DO UPDATE SET
                major = excluded.major,
                minor = excluded.minor,
                patch = excluded.patch
             WHERE excluded.major > atm_storage_schema_version.major
                OR (excluded.major = atm_storage_schema_version.major
                    AND excluded.minor > atm_storage_schema_version.minor)
                OR (excluded.major = atm_storage_schema_version.major
                    AND excluded.minor = atm_storage_schema_version.minor
                    AND excluded.patch > atm_storage_schema_version.patch)",
            (
                STORAGE_SCHEMA_MAJOR,
                STORAGE_SCHEMA_MINOR,
                STORAGE_SCHEMA_PATCH,
            ),
        )
        .map_err(|error| sqlite_error(target, "failed to record SQLite schema version", error))?;
    Ok(())
}
