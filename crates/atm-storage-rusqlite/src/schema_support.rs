//! Shared helpers for the legacy table rebuilds SQLite forces on this crate.
//!
//! SQLite cannot relax a `NOT NULL` constraint or widen a `CHECK` constraint in
//! place, so [`crate::mail_messages_schema`] and [`crate::team_roster_schema`]
//! both recreate their table with the documented rename/create/copy/drop
//! procedure. The helpers here own every stage of that procedure -- column
//! introspection, the guard that refuses to silently drop an unknown legacy
//! column, the drop-staging-copy-swap-restore rebuild itself, the optional
//! post-rebuild foreign-key check, and the `PRAGMA foreign_keys` toggle
//! around a rebuild -- so the two callers cannot let the procedure itself
//! drift apart, and only their table-specific shape (DDL, index DDL, and an
//! optional dependent view) varies.

use crate::shared_db::{SharedDbTarget, SqliteConnection, sqlite_error};
use atm_storage::error::AtmError;
use rusqlite::Transaction;

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

/// A dependent view that must be dropped before a table rebuild and
/// recreated afterward.
///
/// The restore is a callback rather than raw DDL because recreating a view
/// can require running other schema-owning migrations first (see
/// `mail_messages_schema`'s `decomposed_messages`, whose columns depend on
/// the template-catalog migrations in `template_catalog_schema::ensure_schema`).
pub(crate) struct TableRebuildView<'a> {
    pub(crate) name: &'a str,
    pub(crate) restore: fn(&Transaction<'_>, &SharedDbTarget) -> Result<(), AtmError>,
}

/// Table-specific shape for one legacy-table rebuild.
///
/// [`rebuild_table`] performs the documented drop-view/create-staging/copy/
/// swap/restore-indexes/restore-view/check-foreign-keys procedure; this plan
/// supplies only what varies per table.
pub(crate) struct TableRebuildPlan<'a> {
    /// The live table being rebuilt. Also names the table in error text.
    pub(crate) table: &'a str,
    /// Name of the staging table created under the canonical DDL.
    pub(crate) staging_table: &'a str,
    /// `CREATE TABLE <staging_table> (...)` statement for the canonical shape.
    pub(crate) staging_table_ddl: &'a str,
    /// Index DDL replayed on `table` after the swap.
    pub(crate) index_ddl: &'a str,
    /// The table's dependent view, if any, to drop before and restore after.
    pub(crate) view: Option<TableRebuildView<'a>>,
    /// Whether to reject the rebuild when it leaves a dangling foreign key.
    pub(crate) check_foreign_keys: bool,
}

/// Rebuilds `plan.table` under the SQLite-documented ALTER TABLE procedure:
/// drop its dependent view (if any), create a staging table under the
/// canonical DDL, copy every legacy row into it (refusing an unknown or
/// missing column), swap the staging table into place, replay its indexes,
/// restore its dependent view (if any), and optionally reject a dangling
/// foreign key left by the swap. Returns the number of rows copied.
///
/// # Errors
/// Returns an error naming the failing stage; the caller's transaction is
/// left uncommitted so it rolls back on drop.
pub(crate) fn rebuild_table(
    transaction: &Transaction<'_>,
    target: &SharedDbTarget,
    plan: &TableRebuildPlan<'_>,
) -> Result<usize, AtmError> {
    if let Some(view) = &plan.view {
        drop_rebuild_view(transaction, target, plan.table, view.name)?;
    }
    let columns = create_staging_table(
        transaction,
        target,
        plan.table,
        plan.staging_table,
        plan.staging_table_ddl,
    )?;
    let copied = copy_rows_into_staging_table(
        transaction,
        target,
        plan.table,
        plan.staging_table,
        &columns,
    )?;
    swap_in_staging_table(transaction, target, plan.table, plan.staging_table)?;
    restore_table_indexes(transaction, target, plan.table, plan.index_ddl)?;
    if let Some(view) = &plan.view {
        (view.restore)(transaction, target)?;
    }
    if plan.check_foreign_keys {
        reject_foreign_key_violations(transaction, target, plan.table)?;
    }
    Ok(copied)
}

/// Drops `view` so a later `ALTER TABLE ... RENAME` on `table` cannot fail
/// schema validation against a view body that selects from it.
fn drop_rebuild_view(
    transaction: &Transaction<'_>,
    target: &SharedDbTarget,
    table: &str,
    view: &str,
) -> Result<(), AtmError> {
    transaction
        .execute_batch(&format!("DROP VIEW IF EXISTS {view};"))
        .map_err(|error| {
            sqlite_error(
                target,
                format!("failed to drop the {view} view for the {table} rebuild"),
                error,
            )
        })
}

/// Creates the staging table and returns its canonical column list.
///
/// The staging table has the canonical shape by construction, so its columns
/// are the authority `table`'s live columns are checked against; a legacy
/// table with an unknown or missing column is rejected rather than silently
/// truncated.
fn create_staging_table(
    transaction: &Transaction<'_>,
    target: &SharedDbTarget,
    table: &str,
    staging_table: &str,
    staging_table_ddl: &str,
) -> Result<Vec<String>, AtmError> {
    transaction
        .execute_batch(staging_table_ddl)
        .map_err(|error| {
            sqlite_error(
                target,
                format!("failed to create the rebuilt {table} table"),
                error,
            )
        })?;
    let columns = table_column_names(transaction, target, staging_table)?;
    ensure_columns_match_canonical(
        transaction,
        target,
        table,
        &columns.iter().map(String::as_str).collect::<Vec<_>>(),
    )?;
    Ok(columns)
}

/// Copies every live row of `table` into `staging_table`, returning the row
/// count.
fn copy_rows_into_staging_table(
    transaction: &Transaction<'_>,
    target: &SharedDbTarget,
    table: &str,
    staging_table: &str,
    columns: &[String],
) -> Result<usize, AtmError> {
    let column_list = columns
        .iter()
        .map(|column| format!("\"{column}\""))
        .collect::<Vec<_>>()
        .join(", ");
    transaction
        .execute(
            &format!(
                "INSERT INTO {staging_table} ({column_list}) SELECT {column_list} FROM {table};"
            ),
            [],
        )
        .map_err(|error| {
            sqlite_error(
                target,
                format!("failed to copy rows into the rebuilt {table} table"),
                error,
            )
        })
}

/// Drops the live table and renames the staging table into its place.
fn swap_in_staging_table(
    transaction: &Transaction<'_>,
    target: &SharedDbTarget,
    table: &str,
    staging_table: &str,
) -> Result<(), AtmError> {
    transaction
        .execute_batch(&format!(
            "DROP TABLE {table};
             ALTER TABLE {staging_table} RENAME TO {table};"
        ))
        .map_err(|error| {
            sqlite_error(
                target,
                format!("failed to swap in the rebuilt {table} table"),
                error,
            )
        })
}

/// Replays `index_ddl` on `table` after the swap dropped its indexes.
fn restore_table_indexes(
    transaction: &Transaction<'_>,
    target: &SharedDbTarget,
    table: &str,
    index_ddl: &str,
) -> Result<(), AtmError> {
    transaction.execute_batch(index_ddl).map_err(|error| {
        sqlite_error(
            target,
            format!("failed to recreate {table} indexes after the rebuild"),
            error,
        )
    })
}

/// Fails the transaction when the rebuild left a dangling foreign key.
fn reject_foreign_key_violations(
    transaction: &Transaction<'_>,
    target: &SharedDbTarget,
    table: &str,
) -> Result<(), AtmError> {
    let violations = transaction
        .query_row(
            "SELECT COUNT(1) FROM pragma_foreign_key_check;",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| {
            sqlite_error(
                target,
                format!("failed to run the foreign-key check after the {table} rebuild"),
                error,
            )
        })?;
    if violations > 0 {
        return Err(AtmError::mailbox_write(format!(
            "{table} rebuild on {} left {violations} foreign key violation(s); rolling back",
            target.display()
        )));
    }
    Ok(())
}

/// Runs `work` with `PRAGMA foreign_keys` disabled, then restores it.
///
/// A rebuild that drops and recreates `table` would otherwise cascade away
/// any row referencing it via a foreign key (`PRAGMA foreign_keys` is a
/// no-op inside a transaction, so it must be toggled around one, per
/// SQLite's documented ALTER TABLE procedure). `work`'s success or failure
/// always takes priority in the returned error over a restore failure; a
/// restore failure is folded into a successful `work`'s failure via
/// [`merge_restore_failure`], and reported alone when `work` itself
/// succeeded.
///
/// # Errors
/// Returns `work`'s error (augmented with any restore failure), or the
/// restore failure alone when `work` succeeded but the pragma could not be
/// restored.
pub(crate) fn run_within_foreign_keys_toggle<T>(
    connection: &mut SqliteConnection,
    target: &SharedDbTarget,
    table: &str,
    work: impl FnOnce(&mut SqliteConnection) -> Result<T, AtmError>,
) -> Result<T, AtmError> {
    connection
        .pragma_update(None, "foreign_keys", "OFF")
        .map_err(|error| {
            sqlite_error(
                target,
                format!("failed to disable foreign keys for the {table} rebuild"),
                error,
            )
        })?;
    let outcome = work(connection);
    let restored = connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(|error| {
            sqlite_error(
                target,
                format!("failed to restore foreign keys after the {table} rebuild"),
                error,
            )
        });
    match (outcome, restored) {
        (Ok(value), Ok(())) => Ok(value),
        (Ok(_value), Err(restore_failure)) => Err(restore_failure),
        (Err(failure), Ok(())) => Err(failure),
        // The connection is left with foreign keys disabled; surface that
        // alongside the original failure instead of dropping it.
        (Err(failure), Err(restore)) => Err(merge_restore_failure(failure, &restore)),
    }
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
