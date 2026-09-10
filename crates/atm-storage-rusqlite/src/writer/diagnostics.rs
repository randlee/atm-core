use atm_storage::{AtmError, DiagnosticEvent};
use rusqlite::{Connection, params};

use super::ops::WriteOpResult;
use crate::shared_db::{SharedDbTarget, sqlite_error};

pub(super) fn execute_batch(
    events: &[DiagnosticEvent],
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<WriteOpResult, AtmError> {
    for event in events {
        connection
            .execute(
                "INSERT INTO diagnostic_events (ts_unix_ms, level, component, code, correlation_id, origin, message, detail) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![event.ts_unix_ms, event.level, event.component, event.code, event.correlation_id, event.origin, event.message, event.detail],
            )
            .map_err(|error| sqlite_error(target, "failed to record diagnostic timeline batch", error))?;
    }
    Ok(WriteOpResult::DiagnosticsRecorded)
}

pub(super) fn execute_prune(
    now_unix_ms: i64,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<WriteOpResult, AtmError> {
    let cutoff = now_unix_ms - crate::DIAGNOSTIC_MAX_AGE_DAYS * 24 * 60 * 60 * 1_000;
    let expired_rows = connection
        .execute(
            "DELETE FROM diagnostic_events WHERE id IN (SELECT id FROM diagnostic_events WHERE ts_unix_ms < ?1 ORDER BY id LIMIT ?2)",
            params![cutoff, crate::DIAGNOSTIC_PRUNE_BATCH],
        )
        .map_err(|error| sqlite_error(target, "failed to prune expired diagnostic events", error))?;
    if expired_rows > 0 {
        return Ok(WriteOpResult::DiagnosticsPruned(expired_rows as u64));
    }
    let excess_rows = connection
        .execute(
            "DELETE FROM diagnostic_events WHERE id IN (SELECT id FROM diagnostic_events ORDER BY ts_unix_ms DESC, id DESC LIMIT ?1 OFFSET ?2)",
            params![crate::DIAGNOSTIC_PRUNE_BATCH, crate::DIAGNOSTIC_MAX_ROWS],
        )
        .map_err(|error| sqlite_error(target, "failed to prune excess diagnostic events", error))?;
    Ok(WriteOpResult::DiagnosticsPruned(excess_rows as u64))
}
