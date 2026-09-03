//! Shared helpers for the legacy table rebuilds SQLite forces on this crate.
//!
//! SQLite cannot relax a `NOT NULL` constraint or widen a `CHECK` constraint in
//! place, so [`crate::mail_messages_schema`] and [`crate::team_roster_schema`]
//! both recreate their table with the documented rename/create/copy/drop
//! procedure. The helpers here own the parts those rebuilds must agree on:
//! column introspection, the guard that refuses to silently drop an unknown
//! legacy column, and compound failure reporting when cleanup itself fails.

use crate::shared_db::{SharedDbTarget, SqliteConnection, sqlite_error};
use atm_storage::error::AtmError;

/// Reads the declared column names of `table` in their declaration order.
///
/// # Errors
/// Returns an error when the table cannot be introspected.
pub(crate) fn table_column_names(
    connection: &SqliteConnection,
    target: &SharedDbTarget,
    table: &str,
) -> Result<Vec<String>, AtmError> {
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
    columns
        .map(|entry| {
            entry.map_err(|error| {
                sqlite_error(
                    target,
                    format!("failed to read sqlite column metadata for {table}"),
                    error,
                )
            })
        })
        .collect()
}

/// Fails unless the live columns of `table` are exactly `canonical`.
///
/// A rebuild copies rows by explicit column list, so a legacy column the
/// current schema does not define would be dropped without a trace. Refusing
/// to run keeps that operator data recoverable. A canonical column missing
/// from the legacy table means the additive `ensure_column` migrations did not
/// run first, which would equally lose data on copy.
///
/// # Errors
/// Returns a validation-style write error naming the offending columns, and
/// propagates introspection failures.
pub(crate) fn ensure_columns_match_canonical(
    connection: &SqliteConnection,
    target: &SharedDbTarget,
    table: &str,
    canonical: &[&str],
) -> Result<(), AtmError> {
    let live = table_column_names(connection, target, table)?;
    let unknown = live
        .iter()
        .map(String::as_str)
        .filter(|column| !canonical.contains(column))
        .collect::<Vec<_>>();
    if !unknown.is_empty() {
        return Err(AtmError::mailbox_write(format!(
            "refusing to rebuild {table} on {}: the legacy table carries column(s) {} that the current schema does not define",
            target.display(),
            unknown.join(", ")
        )));
    }
    let absent = canonical
        .iter()
        .copied()
        .filter(|column| !live.iter().any(|live_column| live_column == column))
        .collect::<Vec<_>>();
    if !absent.is_empty() {
        return Err(AtmError::mailbox_write(format!(
            "refusing to rebuild {table} on {}: the legacy table is missing column(s) {} that the additive migrations should have added",
            target.display(),
            absent.join(", ")
        )));
    }
    Ok(())
}

/// Folds a failed connection-state restore into the error that preceded it.
///
/// A rebuild toggles `PRAGMA foreign_keys` around its transaction. If both the
/// rebuild and the restore fail, the restore failure would otherwise be
/// dropped. The original error keeps its code and message -- it is the
/// actionable one -- while the restore failure rides along in the preserved
/// cause so an operator can see the connection was left with foreign keys off.
pub(crate) fn merge_restore_failure(failure: AtmError, restore: &AtmError) -> AtmError {
    let combined = format!(
        "{}; additionally failed to restore the connection pragma state: {}",
        failure.cause().unwrap_or_else(|| failure.message()),
        restore.cause().unwrap_or_else(|| restore.message())
    );
    failure.with_cause(combined)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared_db::{NEXT_IN_MEMORY_DB_ID, open_connection_for_target};
    use std::sync::atomic::Ordering;

    fn probe_connection() -> (SharedDbTarget, SqliteConnection) {
        let target = SharedDbTarget::InMemory {
            uri: format!(
                "file:atm-storage-rusqlite-schema-support-{}?mode=memory&cache=shared",
                NEXT_IN_MEMORY_DB_ID.fetch_add(1, Ordering::Relaxed)
            ),
        };
        let connection = open_connection_for_target(&target).expect("open connection");
        connection
            .execute_batch("CREATE TABLE probe (alpha TEXT, beta TEXT);")
            .expect("create probe table");
        (target, connection)
    }

    #[test]
    fn canonical_column_guard_accepts_an_exact_match() {
        let (target, connection) = probe_connection();
        ensure_columns_match_canonical(&connection, &target, "probe", &["alpha", "beta"])
            .expect("an exact column match must be accepted");
    }

    #[test]
    fn canonical_column_guard_names_unknown_and_missing_columns() {
        let (target, connection) = probe_connection();
        let unknown = ensure_columns_match_canonical(&connection, &target, "probe", &["alpha"])
            .expect_err("an unknown legacy column must be rejected");
        assert!(
            unknown.message().contains("beta"),
            "the error must name the column that would be dropped: {}",
            unknown.message()
        );
        let missing = ensure_columns_match_canonical(
            &connection,
            &target,
            "probe",
            &["alpha", "beta", "gamma"],
        )
        .expect_err("a missing canonical column must be rejected");
        assert!(
            missing.message().contains("gamma"),
            "the error must name the column that never got added: {}",
            missing.message()
        );
    }

    #[test]
    fn a_failed_restore_rides_along_in_the_original_errors_cause() {
        // Both halves of the compound failure are cheap to build directly;
        // provoking a real pragma-restore failure would need a connection that
        // is simultaneously usable enough to run a rebuild and broken enough to
        // reject `PRAGMA foreign_keys`, which SQLite does not offer.
        let failure = AtmError::mailbox_write("failed to swap in the rebuilt table")
            .with_cause("database is locked");
        let restore = AtmError::mailbox_write("failed to restore foreign keys")
            .with_cause("attempt to write a readonly database");
        let merged = merge_restore_failure(failure, &restore);
        assert!(
            merged
                .message()
                .starts_with("failed to swap in the rebuilt table"),
            "the original message must survive: {}",
            merged.message()
        );
        let cause = merged.cause().expect("merged cause");
        assert!(cause.starts_with("database is locked"), "{cause}");
        assert!(
            cause.contains("attempt to write a readonly database"),
            "the dropped restore failure must be visible: {cause}"
        );
    }
}
