use std::collections::BTreeSet;
use std::path::Path;

use atm_storage_rusqlite::SqliteStorageBackend;
use rusqlite::Connection;
use serde::Deserialize;

#[derive(Deserialize)]
struct ResetPolicy {
    clear: BTreeSet<String>,
    clear_via_owner: BTreeSet<String>,
    keep: BTreeSet<String>,
}

#[test]
fn colima_reset_policy_classifies_every_migrated_table() {
    let directory = tempfile::tempdir().expect("temporary database directory");
    let database = directory.path().join("mail.db");
    let backend = SqliteStorageBackend::new(&database).expect("initialize migrated database");
    drop(backend);

    let connection = Connection::open(database).expect("reopen migrated database");
    let mut statement = connection
        .prepare(
            "SELECT name FROM sqlite_master \
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .expect("prepare schema inventory");
    let actual = statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query migrated schema")
        .collect::<Result<BTreeSet<_>, _>>()
        .expect("read migrated schema");

    let policy_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/integration/colima_reset_tables.json");
    let policy: ResetPolicy = serde_json::from_str(
        &std::fs::read_to_string(policy_path).expect("read colima reset policy"),
    )
    .expect("parse colima reset policy");
    let classified = policy
        .clear
        .union(&policy.clear_via_owner)
        .cloned()
        .collect::<BTreeSet<_>>()
        .union(&policy.keep)
        .cloned()
        .collect::<BTreeSet<_>>();

    assert_eq!(
        actual, classified,
        "every migrated table must be cleared or kept"
    );
}
