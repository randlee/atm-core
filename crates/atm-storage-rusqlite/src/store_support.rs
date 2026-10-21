//! Helpers shared by the hook registration and cron schedule SQLite stores.
//!
//! Timestamps are stored as fixed-width UTC nanosecond RFC 3339 text, so SQL
//! `ORDER BY` on those columns matches the chronological order the traits
//! document.

use std::fmt::Display;

use atm_storage::types::IsoTimestamp;
use atm_storage::{AtmError, AtmErrorCode};
use chrono::SecondsFormat;
use rusqlite::types::Value;
use rusqlite::{Connection, Row, params_from_iter};

use crate::shared_db::SharedDb;

pub(crate) fn stamp(at: IsoTimestamp) -> String {
    at.into_inner().to_rfc3339_opts(SecondsFormat::Nanos, true)
}

pub(crate) fn internal(key: &str, detail: impl Display) -> AtmError {
    AtmError::new(
        AtmErrorCode::InternalError,
        format!("stored row {key}: {detail}"),
    )
}

pub(crate) fn parse_stamp(key: &str, value: &str) -> Result<IsoTimestamp, AtmError> {
    value.parse().map_err(|error| internal(key, error))
}

pub(crate) fn parse_optional_stamp(
    key: &str,
    value: Option<&str>,
) -> Result<Option<IsoTimestamp>, AtmError> {
    value.map(|value| parse_stamp(key, value)).transpose()
}

pub(crate) fn encode_custom(custom: serde_json::Map<String, serde_json::Value>) -> String {
    serde_json::Value::Object(custom).to_string()
}

pub(crate) fn parse_custom(
    key: &str,
    value: &str,
) -> Result<serde_json::Map<String, serde_json::Value>, AtmError> {
    match serde_json::from_str::<serde_json::Value>(value) {
        Ok(serde_json::Value::Object(map)) => Ok(map),
        _ => Err(internal(key, "custom is not a JSON object")),
    }
}

pub(crate) fn text(value: &str) -> Value {
    Value::Text(value.to_owned())
}

/// Maps a constraint violation to the validation error that names the table
/// and SQLite's constraint text; any other failure is an internal error.
pub(crate) fn write_error(db: &SharedDb, table: &str, error: rusqlite::Error) -> AtmError {
    match error {
        rusqlite::Error::SqliteFailure(failure, Some(message))
            if failure.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            AtmError::validation_with_recovery(
                format!("{table}: {message}"),
                "check the request fields against docs/atm-storage/boundaries.md",
            )
        }
        other => db.error(format!("failed to write {table}"), other),
    }
}

pub(crate) fn query_rows<T>(
    db: &SharedDb,
    connection: &Connection,
    label: &str,
    sql: &str,
    values: &[Value],
    map: fn(&Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>, AtmError> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|error| db.error(format!("failed to prepare {label} query"), error))?;
    statement
        .query_map(params_from_iter(values.iter()), map)
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|error| db.error(format!("failed to read {label}"), error))
}
