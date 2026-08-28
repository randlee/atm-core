//! SQLite-owned schema and column migrations for durable graft receiver
//! endpoint leases.
//!
//! Keeping this beside the graft-receiver store adapter prevents the generic
//! shared-database root from accumulating feature-specific DDL (RULE-003:
//! `shared_db.rs` stays under the file-length threshold as new storage
//! features land their own schema modules, mirroring
//! `template_catalog_schema` and `search_schema`).

use crate::shared_db::{SharedDbTarget, SqliteConnection, ensure_column, sqlite_error};
use atm_storage::error::AtmError;

const GRAFT_RECEIVER_ENDPOINTS_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS graft_receiver_endpoints (
    team TEXT NOT NULL,
    agent TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    capability TEXT NOT NULL,
    owner_generation TEXT NOT NULL,
    registered_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL,
    unreachable_at TEXT NULL,
    PRIMARY KEY (team, agent)
);
"#;

pub(crate) fn ensure_schema(
    connection: &SqliteConnection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    connection
        .execute_batch(GRAFT_RECEIVER_ENDPOINTS_DDL)
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to initialize graft receiver endpoint schema",
                error,
            )
        })?;
    ensure_graft_receiver_endpoint_columns(connection, target)
}

pub(crate) fn ensure_graft_receiver_endpoint_columns(
    connection: &SqliteConnection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    ensure_column(
        connection,
        target,
        "graft_receiver_endpoints",
        "endpoint",
        "ALTER TABLE graft_receiver_endpoints ADD COLUMN endpoint TEXT NOT NULL DEFAULT '127.0.0.1:0';",
    )?;
    ensure_column(
        connection,
        target,
        "graft_receiver_endpoints",
        "capability",
        "ALTER TABLE graft_receiver_endpoints ADD COLUMN capability TEXT NOT NULL DEFAULT '';",
    )?;
    ensure_column(
        connection,
        target,
        "graft_receiver_endpoints",
        "owner_generation",
        "ALTER TABLE graft_receiver_endpoints ADD COLUMN owner_generation TEXT NOT NULL DEFAULT '';",
    )?;
    ensure_column(
        connection,
        target,
        "graft_receiver_endpoints",
        "registered_at",
        "ALTER TABLE graft_receiver_endpoints ADD COLUMN registered_at TEXT NOT NULL DEFAULT '';",
    )?;
    ensure_column(
        connection,
        target,
        "graft_receiver_endpoints",
        "last_seen_at",
        "ALTER TABLE graft_receiver_endpoints ADD COLUMN last_seen_at TEXT NOT NULL DEFAULT '';",
    )?;
    ensure_column(
        connection,
        target,
        "graft_receiver_endpoints",
        "unreachable_at",
        "ALTER TABLE graft_receiver_endpoints ADD COLUMN unreachable_at TEXT NULL;",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn ensure_graft_receiver_endpoint_columns_is_idempotent() {
        let connection = Connection::open_in_memory().expect("open sqlite database");
        let target = SharedDbTarget::InMemory {
            uri: "file:atm-storage-rusqlite-graft-endpoint-schema-module?mode=memory&cache=shared"
                .to_owned(),
        };
        ensure_schema(&connection, &target).expect("initialise graft endpoint schema");
        ensure_graft_receiver_endpoint_columns(&connection, &target)
            .expect("re-run graft endpoint migration idempotently");
        let columns: Vec<String> = connection
            .prepare("PRAGMA table_info(graft_receiver_endpoints);")
            .expect("prepare columns")
            .query_map([], |row| row.get(1))
            .expect("query columns")
            .collect::<Result<_, _>>()
            .expect("decode columns");
        for expected in [
            "team",
            "agent",
            "endpoint",
            "capability",
            "owner_generation",
            "registered_at",
            "last_seen_at",
            "unreachable_at",
        ] {
            assert!(
                columns.iter().any(|column| column == expected),
                "missing {expected}"
            );
        }
    }
}
