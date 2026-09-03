//! Legacy `team_roster` harness `CHECK` constraint rebuild.
//!
//! SQLite cannot widen a `CHECK` constraint in place, so databases created
//! before the `hermes` and `python-graft` harnesses existed still reject those
//! roster rows. The rebuild here recreates the table with the current
//! constraint, following the same procedure as
//! [`crate::mail_messages_schema`].

use crate::schema_support::ensure_columns_match_canonical;
use crate::shared_db::{SharedDbTarget, sqlite_error};
use atm_storage::error::AtmError;
use rusqlite::{Connection, Transaction, TransactionBehavior};

/// Every column the rebuilt `team_roster` table defines, in declaration order.
///
/// The rebuild copies rows by explicit column list, so a legacy roster column
/// outside this set would be dropped silently; the shared guard rejects that.
const TEAM_ROSTER_CANONICAL_COLUMNS: &[&str] = &[
    "team_name",
    "agent_name",
    "member_kind",
    "harness",
    "agent_type",
    "model",
    "metadata_json",
    "source",
    "recipient_pane_id",
    "updated_at",
];

pub(crate) fn ensure_team_roster_harness_values(
    connection: &mut Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    let table_sql = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'team_roster';",
            [],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to inspect team_roster harness constraint",
                error,
            )
        })?;
    if table_sql.contains("'hermes'") && table_sql.contains("'python-graft'") {
        return Ok(());
    }
    ensure_columns_match_canonical(
        connection,
        target,
        "team_roster",
        TEAM_ROSTER_CANONICAL_COLUMNS,
    )?;

    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to start team_roster harness migration",
                error,
            )
        })?;
    rebuild_team_roster_with_current_harness_values(&transaction, target)?;
    transaction.commit().map_err(|error| {
        sqlite_error(
            target,
            "failed to commit team_roster harness migration",
            error,
        )
    })
}

/// Recreates `team_roster` with the current harness `CHECK` constraint.
///
/// The copied column list is derived from [`TEAM_ROSTER_CANONICAL_COLUMNS`]
/// rather than spelled out again, and the recreated table is checked back
/// against that same list so the `CREATE TABLE` body below cannot drift away
/// from what the copy assumes.
fn rebuild_team_roster_with_current_harness_values(
    transaction: &Transaction<'_>,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    let column_list = TEAM_ROSTER_CANONICAL_COLUMNS.join(", ");
    transaction
        .execute_batch(&format!(
            "DROP INDEX IF EXISTS idx_team_roster_team_name;
             ALTER TABLE team_roster RENAME TO team_roster_legacy_harness;
             CREATE TABLE team_roster (
                 team_name TEXT NOT NULL,
                 agent_name TEXT NOT NULL,
                 member_kind TEXT NOT NULL CHECK(member_kind IN ('permanent', 'ephemeral')),
                 harness TEXT NOT NULL CHECK(harness IN ('claude-code', 'codex-cli', 'gemini-cli', 'opencode', 'hermes', 'python-graft')),
                 agent_type TEXT NOT NULL DEFAULT '',
                 model TEXT NOT NULL DEFAULT '',
                 metadata_json TEXT NOT NULL DEFAULT '{{}}',
                 source TEXT,
                 recipient_pane_id TEXT NULL,
                 updated_at TEXT NOT NULL,
                 PRIMARY KEY (team_name, agent_name)
             );
             INSERT INTO team_roster({column_list})
             SELECT {column_list} FROM team_roster_legacy_harness;
             DROP TABLE team_roster_legacy_harness;
             CREATE INDEX idx_team_roster_team_name ON team_roster(team_name);"
        ))
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to migrate team_roster harness constraint",
                error,
            )
        })?;
    ensure_columns_match_canonical(
        transaction,
        target,
        "team_roster",
        TEAM_ROSTER_CANONICAL_COLUMNS,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared_db::{NEXT_IN_MEMORY_DB_ID, open_connection_for_target};
    use std::sync::atomic::Ordering;

    #[test]
    fn ensure_team_roster_harness_values_migrates_legacy_check_constraint() {
        let target = SharedDbTarget::InMemory {
            uri: format!(
                "file:atm-storage-rusqlite-roster-harness-test-{}?mode=memory&cache=shared",
                NEXT_IN_MEMORY_DB_ID.fetch_add(1, Ordering::Relaxed)
            ),
        };
        let mut connection = open_connection_for_target(&target).expect("open connection");
        connection
            .execute_batch(
                "CREATE TABLE team_roster (
                    team_name TEXT NOT NULL,
                    agent_name TEXT NOT NULL,
                    member_kind TEXT NOT NULL CHECK(member_kind IN ('permanent', 'ephemeral')),
                    harness TEXT NOT NULL CHECK(harness IN ('claude-code', 'codex-cli', 'gemini-cli', 'opencode')),
                    agent_type TEXT NOT NULL DEFAULT '',
                    model TEXT NOT NULL DEFAULT '',
                    metadata_json TEXT NOT NULL DEFAULT '{}',
                    source TEXT,
                    recipient_pane_id TEXT NULL,
                    updated_at TEXT NOT NULL,
                    PRIMARY KEY (team_name, agent_name)
                );
                CREATE INDEX idx_team_roster_team_name ON team_roster(team_name);
                INSERT INTO team_roster(
                    team_name, agent_name, member_kind, harness, updated_at
                ) VALUES ('hermes', 'skillrx', 'permanent', 'claude-code', 'now');",
            )
            .expect("create legacy roster table");

        ensure_team_roster_harness_values(&mut connection, &target).expect("migrate roster");
        connection
            .execute(
                "INSERT INTO team_roster(
                    team_name, agent_name, member_kind, harness, updated_at
                ) VALUES ('hermes', 'python-worker', 'permanent', 'python-graft', 'now');",
                [],
            )
            .expect("new harness should satisfy migrated check constraint");
        let harnesses: Vec<String> = connection
            .prepare(
                "SELECT harness FROM team_roster
                 WHERE team_name = 'hermes' ORDER BY agent_name;",
            )
            .expect("prepare harness query")
            .query_map([], |row| row.get(0))
            .expect("query harnesses")
            .collect::<Result<_, _>>()
            .expect("decode harnesses");
        assert_eq!(harnesses, vec!["python-graft", "claude-code"]);
    }

    #[test]
    fn team_roster_rebuild_rejects_an_unknown_legacy_column() {
        let target = SharedDbTarget::InMemory {
            uri: format!(
                "file:atm-storage-rusqlite-roster-unknown-column-{}?mode=memory&cache=shared",
                NEXT_IN_MEMORY_DB_ID.fetch_add(1, Ordering::Relaxed)
            ),
        };
        let mut connection = open_connection_for_target(&target).expect("open connection");
        connection
            .execute_batch(
                "CREATE TABLE team_roster (
                    team_name TEXT NOT NULL,
                    agent_name TEXT NOT NULL,
                    member_kind TEXT NOT NULL CHECK(member_kind IN ('permanent', 'ephemeral')),
                    harness TEXT NOT NULL CHECK(harness IN ('claude-code', 'codex-cli', 'gemini-cli', 'opencode')),
                    agent_type TEXT NOT NULL DEFAULT '',
                    model TEXT NOT NULL DEFAULT '',
                    metadata_json TEXT NOT NULL DEFAULT '{}',
                    source TEXT,
                    recipient_pane_id TEXT NULL,
                    operator_annotation TEXT NULL,
                    updated_at TEXT NOT NULL,
                    PRIMARY KEY (team_name, agent_name)
                );
                INSERT INTO team_roster(
                    team_name, agent_name, member_kind, harness, operator_annotation, updated_at
                ) VALUES ('hermes', 'skillrx', 'permanent', 'claude-code', 'keep me', 'now');",
            )
            .expect("create legacy roster table with an unknown column");

        let failure = ensure_team_roster_harness_values(&mut connection, &target)
            .expect_err("an unknown legacy column must block the roster rebuild");
        assert!(
            failure.message().contains("operator_annotation"),
            "the error must name the column that would be dropped: {}",
            failure.message()
        );

        let annotation: String = connection
            .query_row(
                "SELECT operator_annotation FROM team_roster WHERE agent_name = 'skillrx';",
                [],
                |row| row.get(0),
            )
            .expect("the legacy table must be left untouched");
        assert_eq!(annotation, "keep me");
        let table_sql: String = connection
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'team_roster';",
                [],
                |row| row.get(0),
            )
            .expect("read table sql");
        assert!(
            !table_sql.contains("'python-graft'"),
            "the rejected rebuild must not have altered the table"
        );
    }
}
