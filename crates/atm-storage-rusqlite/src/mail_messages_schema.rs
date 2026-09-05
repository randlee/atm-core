//! Legacy `mail_messages` schema rebuild and the canonical table DDL.
//!
//! SQLite cannot relax a `NOT NULL` constraint in place, so a database created
//! before `message_text` became nullable can never admit a decomposed template
//! message: the writer nulls that column out. This module owns the canonical
//! `mail_messages` DDL -- shared with the fresh-install migrations in
//! [`crate::shared_db`] so the two shapes cannot drift -- and the one-time,
//! self-detecting table rebuild that migrates those databases.

use crate::schema_support::{
    TableRebuildPlan, TableRebuildView, rebuild_table, run_within_foreign_keys_toggle,
};
use crate::shared_db::{SharedDbTarget, SqliteConnection, sqlite_error};
use atm_storage::error::AtmError;
use rusqlite::{Transaction, TransactionBehavior};
use std::time::Instant;

/// Canonical `mail_messages` table DDL, parameterized by its `CREATE` clause.
///
/// The fresh-database schema in [`DB_MIGRATIONS`] and the legacy
/// `message_text` rebuild both materialize their table from this single
/// definition, so the two shapes cannot drift apart. The column order here is
/// also the order a fresh database ends up with after the additive
/// `ALTER TABLE` migrations in `ensure_mail_message_columns` and
/// `template_catalog_schema::ensure_schema` have run.
macro_rules! mail_messages_table_ddl {
    ($create:literal) => {
        concat!(
            $create,
            r#" (
    team TEXT NOT NULL,
    agent TEXT NOT NULL,
    message_key TEXT NOT NULL,
    envelope_json TEXT NOT NULL,
    from_agent TEXT NOT NULL,
    source_chat_id TEXT NULL,
    destination_chat_id TEXT NULL,
    message_text TEXT NULL,
    template_sha TEXT NULL,
    vars_json TEXT NULL,
    category TEXT NULL,
    content_format TEXT NULL,
    tags_json TEXT NOT NULL DEFAULT '[]',
    summary TEXT NULL,
    message_at TEXT NOT NULL,
    message_id TEXT NULL,
    parent_message_id TEXT NULL,
    thread_mode TEXT NULL CHECK(thread_mode IS NULL OR thread_mode IN ('add-details', 'supersede')),
    recorded_at TEXT,
    workflow_scope_kind TEXT NULL,
    workflow_scope_id TEXT NULL,
    workflow_state TEXT NULL,
    workflow_stage TEXT NULL,
    workflow_transition TEXT NULL,
    workflow_iteration TEXT NULL,
    applied_template_tags_json TEXT NULL,
    effective_tags_json TEXT NULL,
    CHECK(message_key GLOB 'atm:*' OR message_key GLOB 'ext:*'),
    PRIMARY KEY (team, agent, message_key)
);
"#
        )
    };
}

/// Every index owned by `mail_messages`.
///
/// The rebuild drops the table (and therefore its indexes), so it replays this
/// exact definition to guarantee a migrated database is index-identical to a
/// fresh one.
macro_rules! mail_messages_index_ddl {
    () => {
        r#"
CREATE UNIQUE INDEX IF NOT EXISTS uq_mail_messages_single_successor
    ON mail_messages(team, agent, parent_message_id)
    WHERE parent_message_id IS NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS uq_mail_messages_message_id
    ON mail_messages(team, agent, message_id)
    WHERE message_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_mail_messages_mailbox
    ON mail_messages(team, agent);

-- Post-commit received-hook dispatch reloads an admitted record by its
-- immutable message key.  The mailbox primary key begins with team and agent,
-- so it cannot serve that global-key lookup without scanning a growing table.
CREATE INDEX IF NOT EXISTS idx_mail_messages_message_key
    ON mail_messages(message_key);
"#
    };
}

pub(crate) use mail_messages_index_ddl;
pub(crate) use mail_messages_table_ddl;

/// Staging table name used by the `message_text` nullability rebuild.
const MAIL_MESSAGES_REBUILD_TABLE: &str = "mail_messages_nullable_rebuild";

/// `CREATE TABLE` for the rebuild staging table, byte-identical in shape to
/// the fresh `mail_messages` definition.
const MAIL_MESSAGES_REBUILD_TABLE_DDL: &str =
    mail_messages_table_ddl!("CREATE TABLE mail_messages_nullable_rebuild");

/// Index DDL replayed after the rebuild swaps the tables.
const MAIL_MESSAGES_INDEX_DDL: &str = mail_messages_index_ddl!();

/// Relaxes a legacy `mail_messages.message_text NOT NULL` column to nullable.
///
/// Databases created before decomposed template admission shipped still carry
/// `message_text TEXT NOT NULL`. SQLite cannot relax a `NOT NULL` constraint
/// in place, and the additive `ensure_column` migrations only add missing
/// columns, so `atm send --template` fails on those databases with a
/// constraint violation when the decomposed writer nulls the column out.
///
/// The probe is self-detecting rather than version-stamped: it reads the live
/// column flag and returns without writing anything when the table is missing
/// or the column is already nullable, so correct databases pay one pragma
/// query per startup.
///
/// # Errors
/// Returns an error when the probe or any rebuild stage fails. Every failure
/// rolls the rebuild transaction back, so the legacy table is left untouched
/// and a later startup retries.
pub(crate) fn ensure_mail_messages_message_text_nullable(
    connection: &mut SqliteConnection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    use rusqlite::OptionalExtension;

    let legacy_not_null = connection
        .query_row(
            r#"SELECT "notnull" FROM pragma_table_info('mail_messages') WHERE name = 'message_text';"#,
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to probe mail_messages.message_text nullability",
                error,
            )
        })?;
    if legacy_not_null != Some(1) {
        return Ok(());
    }

    // `mail_message_states` has a foreign key onto `mail_messages`, and the
    // rebuild's drop/rename would otherwise cascade the state rows away; see
    // `run_within_foreign_keys_toggle`.
    run_within_foreign_keys_toggle(connection, target, "mail_messages", |connection| {
        rebuild_mail_messages_with_nullable_message_text(connection, target)
    })
}

/// Performs the SQLite table rebuild that relaxes `message_text` to nullable.
///
/// Callers must have disabled `PRAGMA foreign_keys` and must restore it
/// afterwards; see [`ensure_mail_messages_message_text_nullable`].
fn rebuild_mail_messages_with_nullable_message_text(
    connection: &mut SqliteConnection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    let started = Instant::now();
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to start the mail_messages message_text rebuild",
                error,
            )
        })?;

    let copied = rebuild_table(
        &transaction,
        target,
        &TableRebuildPlan {
            table: "mail_messages",
            staging_table: MAIL_MESSAGES_REBUILD_TABLE,
            staging_table_ddl: MAIL_MESSAGES_REBUILD_TABLE_DDL,
            index_ddl: MAIL_MESSAGES_INDEX_DDL,
            view: Some(TableRebuildView {
                name: "decomposed_messages",
                restore: restore_decomposed_messages_view,
            }),
            check_foreign_keys: true,
        },
    )?;

    transaction.commit().map_err(|error| {
        sqlite_error(
            target,
            "failed to commit the mail_messages message_text rebuild",
            error,
        )
    })?;
    tracing::info!(
        event = "sqlite.mail_messages.message_text_rebuild",
        db.namespace = %target.display(),
        db.rows_copied = copied,
        db.elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        "rebuilt mail_messages so message_text is nullable",
    );
    Ok(())
}

/// Recreates the `decomposed_messages` catalog view, along with whatever
/// template-catalog migrations it depends on, after the rebuild swap.
///
/// `decomposed_messages` selects from `mail_messages`, so it must have been
/// dropped before the swap's `ALTER TABLE ... RENAME`; this restores it
/// verbatim from its owning module.
fn restore_decomposed_messages_view(
    transaction: &Transaction<'_>,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    crate::template_catalog_schema::ensure_schema(transaction, target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema_support::table_column_names;
    use crate::shared_db::{NEXT_IN_MEMORY_DB_ID, ensure_schema, open_connection_for_target};
    use std::sync::atomic::Ordering;

    /// `mail_messages` exactly as databases created before the nullable
    /// `message_text` change carry it, including the foreign-key dependent
    /// state table and every index owned by the mailbox table.
    const LEGACY_MAIL_SCHEMA_DDL: &str = r#"
CREATE TABLE mail_messages (
    team TEXT NOT NULL, agent TEXT NOT NULL, message_key TEXT NOT NULL,
    envelope_json TEXT NOT NULL, from_agent TEXT NOT NULL,
    source_chat_id TEXT NULL, destination_chat_id TEXT NULL,
    message_text TEXT NOT NULL, summary TEXT NULL, message_at TEXT NOT NULL,
    message_id TEXT NULL, parent_message_id TEXT NULL,
    thread_mode TEXT NULL CHECK(thread_mode IS NULL OR thread_mode IN ('add-details', 'supersede')),
    recorded_at TEXT,
    CHECK(message_key GLOB 'atm:*' OR message_key GLOB 'ext:*'),
    PRIMARY KEY (team, agent, message_key)
);

CREATE TABLE mail_message_states (
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

CREATE UNIQUE INDEX uq_mail_messages_single_successor
    ON mail_messages(team, agent, parent_message_id)
    WHERE parent_message_id IS NOT NULL;

CREATE UNIQUE INDEX uq_mail_messages_message_id
    ON mail_messages(team, agent, message_id)
    WHERE message_id IS NOT NULL;

CREATE INDEX idx_mail_messages_mailbox
    ON mail_messages(team, agent);

CREATE INDEX idx_mail_messages_message_key
    ON mail_messages(message_key);
"#;

    const LEGACY_MAIL_ROWS_DDL: &str = r#"
INSERT INTO mail_messages(
    team, agent, message_key, envelope_json, from_agent, source_chat_id,
    destination_chat_id, message_text, summary, message_at, message_id,
    parent_message_id, thread_mode, recorded_at
) VALUES
 ('team-a', 'agent-a', 'atm:one', '{"one":1}', 'sender-one', 'src-1', 'dst-1',
  'body one', 'summary one', '2026-07-13T00:00:00Z', 'msg-1', NULL, NULL,
  '2026-07-13T00:00:01Z'),
 ('team-a', 'agent-a', 'atm:two', '{"two":2}', 'sender-two', NULL, NULL,
  'body two', NULL, '2026-07-13T00:00:02Z', 'msg-2', 'msg-1', 'supersede',
  '2026-07-13T00:00:03Z'),
 ('team-b', 'agent-b', 'ext:three', '{"three":3}', 'sender-three', NULL, NULL,
  'body three', 'summary three', '2026-07-13T00:00:04Z', NULL, NULL, NULL, NULL);

INSERT INTO mail_message_states(
    team, agent, message_key, read, acknowledged_at, updated_at
) VALUES ('team-a', 'agent-a', 'atm:two', 1, '2026-07-13T00:01:00Z', '2026-07-13T00:01:00Z');
"#;

    /// Full legacy row projection used to prove the rebuild is lossless.
    ///
    /// Every legacy column is TEXT, so one nullable-string vector per row keeps
    /// the comparison byte-for-byte without a bespoke row struct.
    type LegacyRow = Vec<Option<String>>;

    fn in_memory_target(label: &str) -> SharedDbTarget {
        SharedDbTarget::InMemory {
            uri: format!(
                "file:atm-storage-rusqlite-{label}-{}?mode=memory&cache=shared",
                NEXT_IN_MEMORY_DB_ID.fetch_add(1, Ordering::Relaxed)
            ),
        }
    }

    fn legacy_mail_messages_connection(label: &str) -> (SharedDbTarget, SqliteConnection) {
        let target = in_memory_target(label);
        let connection = open_connection_for_target(&target).expect("open connection");
        connection
            .execute_batch(LEGACY_MAIL_SCHEMA_DDL)
            .expect("create legacy mail schema");
        (target, connection)
    }

    fn fresh_schema_connection(label: &str) -> (SharedDbTarget, SqliteConnection) {
        let target = in_memory_target(label);
        let mut connection = open_connection_for_target(&target).expect("open connection");
        ensure_schema(&mut connection, &target).expect("fresh schema");
        (target, connection)
    }

    fn message_text_notnull(connection: &SqliteConnection) -> i64 {
        connection
            .query_row(
                r#"SELECT "notnull" FROM pragma_table_info('mail_messages') WHERE name = 'message_text';"#,
                [],
                |row| row.get(0),
            )
            .expect("probe message_text nullability")
    }

    fn foreign_keys_enabled(connection: &SqliteConnection) -> i64 {
        connection
            .query_row("PRAGMA foreign_keys;", [], |row| row.get(0))
            .expect("read foreign_keys pragma")
    }

    fn foreign_key_violations(connection: &SqliteConnection) -> i64 {
        connection
            .query_row(
                "SELECT COUNT(1) FROM pragma_foreign_key_check;",
                [],
                |row| row.get(0),
            )
            .expect("run foreign key check")
    }

    fn schema_objects(connection: &SqliteConnection, object_type: &str) -> Vec<(String, String)> {
        connection
            .prepare(
                "SELECT name, sql FROM sqlite_master
                 WHERE type = ?1 AND sql IS NOT NULL AND name LIKE '%mail_messages%'
                 ORDER BY name;",
            )
            .expect("prepare sqlite_master query")
            .query_map([object_type], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .expect("query sqlite_master")
            .collect::<Result<Vec<_>, _>>()
            .expect("decode sqlite_master rows")
    }

    fn decomposed_view_sql(connection: &SqliteConnection) -> String {
        connection
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'view' AND name = 'decomposed_messages';",
                [],
                |row| row.get(0),
            )
            .expect("read decomposed_messages view sql")
    }

    fn mail_messages_rootpage(connection: &SqliteConnection) -> i64 {
        connection
            .query_row(
                "SELECT rootpage FROM sqlite_master WHERE type = 'table' AND name = 'mail_messages';",
                [],
                |row| row.get(0),
            )
            .expect("read mail_messages rootpage")
    }

    fn legacy_rows(connection: &SqliteConnection) -> Vec<LegacyRow> {
        connection
            .prepare(
                "SELECT team, agent, message_key, envelope_json, from_agent,
                        source_chat_id, destination_chat_id, message_text, summary,
                        message_at, message_id, parent_message_id, thread_mode,
                        recorded_at
                 FROM mail_messages ORDER BY team, agent, message_key;",
            )
            .expect("prepare legacy row query")
            .query_map([], |row| {
                (0..14)
                    .map(|index| row.get(index))
                    .collect::<Result<_, _>>()
            })
            .expect("query legacy rows")
            .collect::<Result<Vec<_>, _>>()
            .expect("decode legacy rows")
    }

    #[test]
    fn legacy_not_null_message_text_is_rebuilt_without_losing_rows_or_schema_objects() {
        let (target, mut connection) = legacy_mail_messages_connection("message-text-rebuild");
        connection
            .execute_batch(LEGACY_MAIL_ROWS_DDL)
            .expect("seed legacy rows");
        assert_eq!(message_text_notnull(&connection), 1);
        let before = legacy_rows(&connection);

        ensure_schema(&mut connection, &target).expect("migrate legacy database");

        assert_eq!(
            message_text_notnull(&connection),
            0,
            "the rebuild must relax message_text to nullable"
        );
        assert_eq!(
            legacy_rows(&connection),
            before,
            "every legacy column value must survive the rebuild unchanged"
        );
        let state: (i64, Option<String>) = connection
            .query_row(
                "SELECT read, acknowledged_at FROM mail_message_states
                 WHERE team = 'team-a' AND agent = 'agent-a' AND message_key = 'atm:two';",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("dependent state row must survive the foreign-key detour");
        assert_eq!(state, (1, Some("2026-07-13T00:01:00Z".to_owned())));
        assert_eq!(foreign_key_violations(&connection), 0);
        assert_eq!(
            foreign_keys_enabled(&connection),
            1,
            "foreign key enforcement must be restored after the rebuild"
        );

        let (_fresh_target, fresh) = fresh_schema_connection("message-text-fresh");
        assert_eq!(
            schema_objects(&connection, "index"),
            schema_objects(&fresh, "index"),
            "a migrated database must be index-identical to a fresh one"
        );
        assert_eq!(
            decomposed_view_sql(&connection),
            decomposed_view_sql(&fresh),
            "the catalog view must be recreated exactly as a fresh database defines it"
        );
    }

    #[test]
    fn rebuilt_mail_messages_accepts_the_decomposed_admission_null_update() {
        let (target, mut connection) = legacy_mail_messages_connection("message-text-null-update");
        connection
            .execute_batch(LEGACY_MAIL_ROWS_DDL)
            .expect("seed legacy rows");
        ensure_schema(&mut connection, &target).expect("migrate legacy database");

        // Mirrors the decomposed admission write in `writer::ops`, which is the
        // exact statement that failed on legacy databases.
        let changed = connection
            .execute(
                "UPDATE mail_messages
                 SET template_sha = 'sha-1', vars_json = '{}', category = 'assignment',
                     tags_json = '[]', content_format = 'markdown', message_text = NULL
                 WHERE message_key = 'atm:one' AND template_sha IS NULL",
                [],
            )
            .expect("decomposed admission must be able to null message_text");
        assert_eq!(changed, 1);
        let stored: Option<String> = connection
            .query_row(
                "SELECT message_text FROM mail_messages WHERE message_key = 'atm:one';",
                [],
                |row| row.get(0),
            )
            .expect("read migrated row");
        assert_eq!(stored, None);
    }

    #[test]
    fn message_text_rebuild_is_skipped_once_the_column_is_already_nullable() {
        let (target, mut connection) = legacy_mail_messages_connection("message-text-idempotent");
        connection
            .execute_batch(LEGACY_MAIL_ROWS_DDL)
            .expect("seed legacy rows");
        ensure_schema(&mut connection, &target).expect("first migration");
        let migrated_rootpage = mail_messages_rootpage(&connection);

        ensure_schema(&mut connection, &target).expect("second migration");
        assert_eq!(
            mail_messages_rootpage(&connection),
            migrated_rootpage,
            "a migrated database must not be rebuilt a second time"
        );

        let (fresh_target, mut fresh) = fresh_schema_connection("message-text-fresh-idempotent");
        let fresh_rootpage = mail_messages_rootpage(&fresh);
        ensure_schema(&mut fresh, &fresh_target).expect("re-run fresh schema");
        assert_eq!(
            mail_messages_rootpage(&fresh),
            fresh_rootpage,
            "a fresh database must never be rebuilt"
        );
    }

    #[test]
    fn message_text_rebuild_preserves_the_message_id_uniqueness_guarantee() {
        let (target, mut connection) = legacy_mail_messages_connection("message-text-unique");
        connection
            .execute_batch(LEGACY_MAIL_ROWS_DDL)
            .expect("seed legacy rows");
        let duplicate = "INSERT INTO mail_messages(
                team, agent, message_key, envelope_json, from_agent, message_text,
                message_at, message_id
             ) VALUES ('team-a', 'agent-a', 'atm:duplicate', '{}', 'sender', 'body',
                       '2026-07-13T00:00:05Z', 'msg-1');";
        connection
            .execute_batch(duplicate)
            .expect_err("legacy unique index must reject a duplicate message_id");

        ensure_schema(&mut connection, &target).expect("migrate legacy database");

        connection
            .execute_batch(duplicate)
            .expect_err("the recreated unique index must still reject a duplicate message_id");
    }

    #[test]
    fn message_text_rebuild_handles_an_empty_legacy_table() {
        let (target, mut connection) = legacy_mail_messages_connection("message-text-empty");
        ensure_schema(&mut connection, &target).expect("migrate empty legacy database");

        assert_eq!(message_text_notnull(&connection), 0);
        assert_eq!(
            connection
                .query_row("SELECT COUNT(1) FROM mail_messages;", [], |row| row
                    .get::<_, i64>(0))
                .expect("count migrated rows"),
            0
        );
        assert_eq!(foreign_keys_enabled(&connection), 1);
    }

    #[test]
    fn message_text_rebuild_lands_the_fresh_column_list_in_the_fresh_order() {
        let (target, mut connection) = legacy_mail_messages_connection("message-text-columns");
        connection
            .execute_batch(LEGACY_MAIL_ROWS_DDL)
            .expect("seed legacy rows");
        assert!(
            !table_column_names(&connection, &target, "mail_messages")
                .expect("legacy columns")
                .contains(&"template_sha".to_owned()),
            "the legacy fixture must predate the additive template columns"
        );

        ensure_schema(&mut connection, &target).expect("migrate legacy database");

        let (fresh_target, fresh) = fresh_schema_connection("message-text-columns-fresh");
        assert_eq!(
            table_column_names(&connection, &target, "mail_messages").expect("migrated columns"),
            table_column_names(&fresh, &fresh_target, "mail_messages").expect("fresh columns"),
            "a migrated table must expose the fresh column list in the fresh order"
        );
    }

    #[test]
    fn a_failed_message_text_rebuild_rolls_back_and_restores_foreign_keys() {
        let (target, mut connection) = legacy_mail_messages_connection("message-text-rollback");
        connection
            .execute_batch(LEGACY_MAIL_ROWS_DDL)
            .expect("seed legacy rows");
        let before = legacy_rows(&connection);
        // Occupying the staging name makes the rebuild's `CREATE TABLE` fail
        // after the view drop, which is the riskiest point to roll back from.
        connection
            .execute_batch("CREATE TABLE mail_messages_nullable_rebuild (probe TEXT);")
            .expect("occupy the rebuild staging table name");

        let failure = ensure_schema(&mut connection, &target)
            .expect_err("a blocked rebuild must surface as an error");
        assert!(
            failure
                .message()
                .contains("failed to create the rebuilt mail_messages table"),
            "the failing stage must be identifiable: {}",
            failure.message()
        );

        assert_eq!(
            message_text_notnull(&connection),
            1,
            "the legacy table must be left untouched so a later startup retries"
        );
        assert_eq!(legacy_rows(&connection), before);
        assert_eq!(foreign_keys_enabled(&connection), 1);
        assert!(
            connection
                .query_row(
                    "SELECT COUNT(1) FROM sqlite_master WHERE type = 'view' AND name = 'decomposed_messages';",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .expect("count views")
                > 0,
            "the rollback must restore the dropped catalog view"
        );
    }

    #[test]
    fn a_legacy_table_with_an_unknown_column_is_rejected_instead_of_losing_data() {
        let (target, mut connection) = legacy_mail_messages_connection("message-text-extra-column");
        connection
            .execute_batch("ALTER TABLE mail_messages ADD COLUMN operator_annotation TEXT NULL;")
            .expect("add an unexpected legacy column");
        connection
            .execute_batch(LEGACY_MAIL_ROWS_DDL)
            .expect("seed legacy rows");
        let before = legacy_rows(&connection);

        let failure = ensure_schema(&mut connection, &target)
            .expect_err("an unknown legacy column must block the rebuild");
        assert!(
            failure.message().contains("operator_annotation"),
            "the error must name the column that would be dropped: {}",
            failure.message()
        );

        assert_eq!(message_text_notnull(&connection), 1);
        assert_eq!(legacy_rows(&connection), before);
        assert_eq!(foreign_keys_enabled(&connection), 1);
        assert!(
            table_column_names(&connection, &target, "mail_messages")
                .expect("columns")
                .contains(&"operator_annotation".to_owned())
        );
    }
}
