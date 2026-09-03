//! Legacy `team_roster` harness `CHECK` constraint rebuild.
//!
//! SQLite cannot widen a `CHECK` constraint in place, so databases created
//! before the `hermes` and `python-graft` harnesses existed still reject those
//! roster rows. The rebuild here recreates the table with the current
//! constraint, following the same procedure as
//! [`crate::mail_messages_schema`].

use crate::schema_support::{TableRebuildPlan, rebuild_table};
use crate::shared_db::{SharedDbTarget, SqliteConnection, sqlite_error};
use atm_storage::error::AtmError;
use rusqlite::TransactionBehavior;

/// Staging table name used by the harness `CHECK` constraint rebuild.
const TEAM_ROSTER_REBUILD_TABLE: &str = "team_roster_harness_rebuild";

/// `CREATE TABLE` for the rebuild staging table, carrying the current
/// harness `CHECK` constraint.
const TEAM_ROSTER_REBUILD_TABLE_DDL: &str = r#"
CREATE TABLE team_roster_harness_rebuild (
    team_name TEXT NOT NULL,
    agent_name TEXT NOT NULL,
    member_kind TEXT NOT NULL CHECK(member_kind IN ('permanent', 'ephemeral')),
    harness TEXT NOT NULL CHECK(harness IN ('claude-code', 'codex-cli', 'gemini-cli', 'opencode', 'hermes', 'python-graft')),
    agent_type TEXT NOT NULL DEFAULT '',
    model TEXT NOT NULL DEFAULT '',
    metadata_json TEXT NOT NULL DEFAULT '{}',
    source TEXT,
    recipient_pane_id TEXT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (team_name, agent_name)
);
"#;

/// Index DDL replayed after the rebuild swaps the tables.
const TEAM_ROSTER_INDEX_DDL: &str =
    "CREATE INDEX idx_team_roster_team_name ON team_roster(team_name);";

pub(crate) fn ensure_team_roster_harness_values(
    connection: &mut SqliteConnection,
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

    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to start team_roster harness migration",
                error,
            )
        })?;
    rebuild_table(
        &transaction,
        target,
        &TableRebuildPlan {
            table: "team_roster",
            staging_table: TEAM_ROSTER_REBUILD_TABLE,
            staging_table_ddl: TEAM_ROSTER_REBUILD_TABLE_DDL,
            index_ddl: TEAM_ROSTER_INDEX_DDL,
            view: None,
            check_foreign_keys: false,
        },
    )?;
    transaction.commit().map_err(|error| {
        sqlite_error(
            target,
            "failed to commit team_roster harness migration",
            error,
        )
    })
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
                ) VALUES ('test-team', 'test-agent', 'permanent', 'claude-code', 'now');",
            )
            .expect("create legacy roster table");

        ensure_team_roster_harness_values(&mut connection, &target).expect("migrate roster");
        connection
            .execute(
                "INSERT INTO team_roster(
                    team_name, agent_name, member_kind, harness, updated_at
                ) VALUES ('test-team', 'python-worker', 'permanent', 'python-graft', 'now');",
                [],
            )
            .expect("new harness should satisfy migrated check constraint");
        let harnesses: Vec<String> = connection
            .prepare(
                "SELECT harness FROM team_roster
                 WHERE team_name = 'test-team' ORDER BY agent_name;",
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
                ) VALUES ('test-team', 'test-agent', 'permanent', 'claude-code', 'keep me', 'now');",
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
                "SELECT operator_annotation FROM team_roster WHERE agent_name = 'test-agent';",
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
