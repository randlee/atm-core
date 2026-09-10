//! Previous-binary task rows remain readable and writable through the v1/v2
//! coexistence window, while fresh and upgraded ledgers install one schema.

use atm_storage_rusqlite::SqliteStorageBackend;
use rusqlite::{Connection, params};

const LEGACY_TASK_SCHEMA: &str = r#"
CREATE TABLE tasks (
    team TEXT NOT NULL,
    task_id TEXT NOT NULL,
    assignee TEXT NOT NULL,
    assigner TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('assigned', 'active', 'complete')),
    assignment_message_id TEXT NOT NULL,
    description TEXT NOT NULL,
    assigned_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_reminded_at TEXT NULL,
    reminder_count INTEGER NOT NULL DEFAULT 0,
    lead_notified_count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (team, task_id, assignee)
);
CREATE TABLE task_events (
    team TEXT NOT NULL,
    task_id TEXT NOT NULL,
    assignee TEXT NOT NULL,
    seq INTEGER NOT NULL,
    at TEXT NOT NULL,
    event TEXT NOT NULL,
    from_state TEXT NULL,
    to_state TEXT NULL,
    actor TEXT NOT NULL,
    message_id TEXT NULL,
    outcome TEXT NULL,
    marker TEXT NULL,
    detail TEXT NULL,
    PRIMARY KEY (team, task_id, assignee, seq)
);
"#;

#[test]
fn fresh_and_v1_projection_task_ledgers_converge_without_dropping_v1() {
    let root = tempfile::tempdir().expect("temporary root");
    let fresh_path = root.path().join("fresh.db");
    let upgraded_path = root.path().join("upgraded.db");

    let fresh = SqliteStorageBackend::new(&fresh_path).expect("fresh backend");
    drop(fresh);

    let legacy = Connection::open(&upgraded_path).expect("legacy connection");
    legacy
        .execute_batch(LEGACY_TASK_SCHEMA)
        .expect("create retained v1 projection schema");
    insert_legacy_task(
        &legacy,
        "compat-task",
        "alpha",
        "assigned",
        "2026-01-01T00:00:00Z",
        "2026-01-01T00:00:00Z",
    );
    drop(legacy);

    let upgraded = SqliteStorageBackend::new(&upgraded_path).expect("upgrade previous binary");
    drop(upgraded);

    // Exercise supported v1 projection writes after migration. This crate-level
    // fixture deliberately uses direct v1 table writes; cross-binary executable
    // proof belongs to the Colima integration testbed.
    let v1_projection = Connection::open(&upgraded_path).expect("v1 projection connection");
    insert_legacy_task(
        &v1_projection,
        "compat-task",
        "beta",
        "active",
        "2026-01-02T00:00:00Z",
        "2026-01-02T00:00:00Z",
    );
    v1_projection
        .execute(
            "UPDATE tasks SET state = 'complete', updated_at = '2026-01-03T00:00:00Z'
             WHERE team = 'compat-team' AND task_id = 'compat-task' AND assignee = 'beta'",
            [],
        )
        .expect("v1 projection completion");
    drop(v1_projection);

    let reopened = SqliteStorageBackend::new(&upgraded_path).expect("reopen upgraded ledger");
    drop(reopened);
    let upgraded_sql = Connection::open(&upgraded_path).expect("inspect upgraded ledger");
    let v2: (String, String, u64, u64) = upgraded_sql
        .query_row(
            "SELECT current_assignee, state,
                    (SELECT COUNT(*) FROM task_assignment_attempts WHERE task_id = 'compat-task'),
                    (SELECT COUNT(*) FROM task_events_v2 WHERE task_id = 'compat-task')
             FROM tasks_v2 WHERE team = 'compat-team' AND task_id = 'compat-task'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("v2 projection after retained writes");
    assert_eq!(
        v2.0, "alpha",
        "completed beta yields the retained assigned attempt"
    );
    assert_eq!(v2.1, "assigned");
    assert_eq!(v2.2, 2, "both v1 assignee rows remain immutable attempts");
    assert!(v2.3 >= 3, "migration and retained writes remain auditable");
    let retained_v1: u64 = upgraded_sql
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('tasks') WHERE name = 'description'",
            [],
            |row| row.get(0),
        )
        .expect("retained description column");
    assert_eq!(retained_v1, 1, "coexistence retains the legacy projection");

    let fresh_sql = Connection::open(&fresh_path).expect("inspect fresh ledger");
    assert_eq!(
        task_schema_objects(&fresh_sql),
        task_schema_objects(&upgraded_sql)
    );
}

fn insert_legacy_task(
    connection: &Connection,
    task_id: &str,
    assignee: &str,
    state: &str,
    assigned_at: &str,
    updated_at: &str,
) {
    connection
        .execute(
            "INSERT INTO tasks(
                team, task_id, assignee, assigner, state, assignment_message_id,
                description, assigned_at, updated_at, reminder_count, lead_notified_count
             ) VALUES (
                'compat-team', ?1, ?2, 'lead', ?3, '01ARZ3NDEKTSV4RRFFQ69G5FAV',
                'retained v1 task body', ?4, ?5, 0, 0
             )",
            params![task_id, assignee, state, assigned_at, updated_at],
        )
        .expect("v1 projection task write");
}

fn task_schema_objects(connection: &Connection) -> Vec<(String, String, String)> {
    connection
        .prepare(
            "SELECT type, name, sql FROM sqlite_master
             WHERE name IN (
                 'tasks_v2', 'task_assignment_attempts', 'task_events_v2',
                 'task_operations', 'task_v2_projection_context',
                 'one_active_task_per_agent', 'task_list_order',
                 'task_v1_insert_bridge', 'task_v1_update_bridge',
                 'idx_mail_messages_task_id'
             ) ORDER BY type, name",
        )
        .expect("prepare schema object snapshot")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .expect("query schema object snapshot")
        .collect::<Result<_, _>>()
        .expect("decode schema object snapshot")
}
