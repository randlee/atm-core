use std::fs;
use std::path::Path;

use atm_storage_rusqlite::SqliteStorageBackend;
use rusqlite::{Connection, params};
use tempfile::TempDir;

const LEGACY_DDL: &str = r#"
CREATE TABLE tasks (
    team TEXT NOT NULL, task_id TEXT NOT NULL, assignee TEXT NOT NULL,
    assigner TEXT NOT NULL, state TEXT NOT NULL,
    assignment_message_id TEXT NOT NULL, description TEXT NOT NULL,
    assigned_at TEXT NOT NULL, updated_at TEXT NOT NULL,
    last_reminded_at TEXT NULL, reminder_count INTEGER NOT NULL DEFAULT 0,
    lead_notified_count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (team, task_id, assignee)
);
CREATE TABLE task_events (
    team TEXT NOT NULL, task_id TEXT NOT NULL, assignee TEXT NOT NULL,
    seq INTEGER NOT NULL, at TEXT NOT NULL, event TEXT NOT NULL,
    from_state TEXT NULL, to_state TEXT NULL, actor TEXT NOT NULL,
    message_id TEXT NULL, outcome TEXT NULL, marker TEXT NULL, detail TEXT NULL,
    PRIMARY KEY (team, task_id, assignee, seq)
);
"#;

fn fixture() -> (TempDir, std::path::PathBuf, Connection) {
    let root = tempfile::tempdir().expect("tempdir");
    let path = root.path().join("mail.db");
    let connection = Connection::open(&path).expect("legacy database");
    connection.execute_batch(LEGACY_DDL).expect("legacy schema");
    (root, path, connection)
}

fn task(connection: &Connection, id: &str, assignee: &str, state: &str, assigned: &str) {
    connection.execute(
        "INSERT INTO tasks(team, task_id, assignee, assigner, state, assignment_message_id,
                           description, assigned_at, updated_at, reminder_count, lead_notified_count)
         VALUES ('team', ?1, ?2, 'lead', ?3, ?1 || '-' || ?2, 'task', ?4, ?4, 0, 0)",
        params![id, assignee, state, assigned],
    ).expect("legacy task");
}

#[allow(clippy::too_many_arguments)]
fn event(
    connection: &Connection,
    id: &str,
    assignee: &str,
    seq: u32,
    at: &str,
    kind: &str,
    from: Option<&str>,
    to: Option<&str>,
) {
    connection.execute(
        "INSERT INTO task_events(team, task_id, assignee, seq, at, event, from_state, to_state, actor)
         VALUES ('team', ?1, ?2, ?3, ?4, ?5, ?6, ?7, 'lead')",
        params![id, assignee, seq, at, kind, from, to],
    ).expect("legacy event");
}

fn migrate(path: &Path) -> SqliteStorageBackend {
    SqliteStorageBackend::new(path).expect("migrate task ledger")
}

fn backup_count(root: &Path) -> usize {
    fs::read_dir(root)
        .expect("read fixture directory")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains("pre-ba2"))
        .count()
}

#[test]
fn fresh_and_already_migrated_databases_skip_migration() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("mail.db");
    drop(migrate(&path));
    drop(migrate(&path));
    assert_eq!(backup_count(root.path()), 0);
    let connection = Connection::open(path).unwrap();
    let pk: u32 = connection
        .query_row(
            "SELECT pk FROM pragma_table_info('tasks') WHERE name = 'assignee'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(pk, 0);
}

#[test]
fn single_row_tasks_migrate_with_positions_by_assigned_at() {
    let (_root, path, connection) = fixture();
    task(&connection, "T2", "a", "assigned", "2026-01-02T00:00:00Z");
    event(
        &connection,
        "T2",
        "a",
        1,
        "2026-01-02T00:00:00Z",
        "assigned",
        None,
        Some("assigned"),
    );
    task(&connection, "T1", "a", "assigned", "2026-01-01T00:00:00Z");
    event(
        &connection,
        "T1",
        "a",
        1,
        "2026-01-01T00:00:00Z",
        "assigned",
        None,
        Some("assigned"),
    );
    drop(connection);
    let _backend = migrate(&path);
    let connection = Connection::open(path).unwrap();
    let rows: Vec<(String, u32)> = connection
        .prepare("SELECT task_id, position FROM tasks ORDER BY position")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(rows, vec![("T1".into(), 1), ("T2".into(), 2)]);
}

#[test]
fn duplicate_group_winner_precedence() {
    let (_root, path, connection) = fixture();
    task(&connection, "T1", "a", "complete", "2026-01-01T00:00:00Z");
    event(
        &connection,
        "T1",
        "a",
        1,
        "2026-01-01T00:00:00Z",
        "completed",
        Some("assigned"),
        Some("complete"),
    );
    task(&connection, "T1", "b", "active", "2026-01-02T00:00:00Z");
    connection
        .execute(
            "UPDATE tasks SET reminder_count = 587 WHERE assignee = 'b'",
            [],
        )
        .unwrap();
    event(
        &connection,
        "T1",
        "b",
        1,
        "2026-01-02T00:00:00Z",
        "started",
        Some("assigned"),
        Some("active"),
    );
    task(&connection, "T2", "a", "active", "2026-01-03T00:00:00Z");
    event(
        &connection,
        "T2",
        "a",
        1,
        "2026-01-03T00:00:00Z",
        "started",
        Some("assigned"),
        Some("active"),
    );
    task(&connection, "T2", "b", "assigned", "2026-01-04T00:00:00Z");
    event(
        &connection,
        "T2",
        "b",
        1,
        "2026-01-04T00:00:00Z",
        "assigned",
        None,
        Some("assigned"),
    );
    drop(connection);
    let _backend = migrate(&path);
    let connection = Connection::open(path).unwrap();
    let first: (String, String, String) = connection
        .query_row(
            "SELECT assignee, state, close_outcome FROM tasks WHERE task_id = 'T1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(first, ("a".into(), "complete".into(), "completed".into()));
    let second: (String, String) = connection
        .query_row(
            "SELECT assignee, state FROM tasks WHERE task_id = 'T2'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(second, ("a".into(), "active".into()));
}

#[test]
fn migration_inserts_open_winners_with_contiguous_positions() {
    let (_root, path, connection) = fixture();
    for (id, assignee, state, at) in [
        ("A1", "a", "assigned", "2026-01-02T00:00:00Z"),
        ("A2", "a", "active", "2026-01-03T00:00:00Z"),
        ("A3", "a", "assigned", "2026-01-01T00:00:00Z"),
        ("B1", "b", "assigned", "2026-01-01T00:00:00Z"),
        ("C1", "a", "complete", "2025-01-01T00:00:00Z"),
    ] {
        task(&connection, id, assignee, state, at);
        let (kind, from) = if state == "active" {
            ("started", Some("assigned"))
        } else if state == "complete" {
            ("completed", Some("assigned"))
        } else {
            ("assigned", None)
        };
        event(&connection, id, assignee, 1, at, kind, from, Some(state));
    }
    drop(connection);
    let _backend = migrate(&path);
    let connection = Connection::open(path).unwrap();
    let rows: Vec<(String, Option<u32>)> = connection.prepare(
        "SELECT task_id, position FROM tasks WHERE assignee = 'a' ORDER BY COALESCE(position, 99)"
    ).unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap()
        .collect::<Result<_, _>>().unwrap();
    assert_eq!(
        rows,
        vec![
            ("A2".into(), Some(1)),
            ("A3".into(), Some(2)),
            ("A1".into(), Some(3)),
            ("C1".into(), None)
        ]
    );
}

#[test]
fn two_active_tasks_same_member_demotes_later_one() {
    let (_root, path, connection) = fixture();
    for (id, at) in [
        ("T1", "2026-01-01T00:00:00Z"),
        ("T2", "2026-01-02T00:00:00Z"),
    ] {
        task(&connection, id, "a", "active", at);
        event(
            &connection,
            id,
            "a",
            1,
            at,
            "started",
            Some("assigned"),
            Some("active"),
        );
    }
    drop(connection);
    let _backend = migrate(&path);
    let connection = Connection::open(path).unwrap();
    let rows: Vec<(String, String, u32)> = connection
        .prepare("SELECT task_id, state, position FROM tasks ORDER BY position")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        rows,
        vec![
            ("T1".into(), "active".into(), 1),
            ("T2".into(), "assigned".into(), 2)
        ]
    );
}

#[test]
fn legacy_events_renumbered_and_acked_rows_survive() {
    let (_root, path, connection) = fixture();
    task(&connection, "T", "a", "assigned", "2026-01-01T00:00:00Z");
    event(
        &connection,
        "T",
        "a",
        9,
        "2026-01-02T00:00:00Z",
        "acked",
        Some("assigned"),
        Some("assigned"),
    );
    event(
        &connection,
        "T",
        "a",
        4,
        "2026-01-01T00:00:00Z",
        "assigned",
        None,
        Some("assigned"),
    );
    drop(connection);
    let _backend = migrate(&path);
    let connection = Connection::open(path).unwrap();
    let rows: Vec<(u32, String)> = connection
        .prepare("SELECT seq, event FROM task_events ORDER BY seq")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(rows, vec![(1, "assigned".into()), (2, "acked".into())]);
}

#[test]
fn migrated_rows_without_history_events_get_synthesized_ones() {
    let (_root, path, connection) = fixture();
    task(&connection, "A", "a", "active", "2026-01-01T00:00:00Z");
    task(&connection, "C", "b", "complete", "2026-01-02T00:00:00Z");
    drop(connection);
    let _backend = migrate(&path);
    let connection = Connection::open(path).unwrap();
    let events: Vec<(String, String)> = connection
        .prepare("SELECT task_id, event FROM task_events ORDER BY task_id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        events,
        vec![
            ("A".into(), "started".into()),
            ("C".into(), "completed".into())
        ]
    );
}

#[test]
fn replay_of_migrated_history_reproduces_row_state() {
    let (_root, path, connection) = fixture();
    for (id, assignee, state, kind, from) in [
        ("A", "a", "assigned", "assigned", None),
        ("B", "b", "active", "started", Some("assigned")),
        ("C", "c", "complete", "completed", Some("assigned")),
    ] {
        task(&connection, id, assignee, state, "2026-01-01T00:00:00Z");
        event(
            &connection,
            id,
            assignee,
            1,
            "2026-01-01T00:00:00Z",
            kind,
            from,
            Some(state),
        );
    }
    drop(connection);
    let _backend = migrate(&path);
    let connection = Connection::open(path).unwrap();
    let mismatches: u32 = connection.query_row(
        "SELECT COUNT(*) FROM tasks task JOIN task_events event ON event.team=task.team AND event.task_id=task.task_id WHERE event.seq=(SELECT MAX(seq) FROM task_events e WHERE e.team=task.team AND e.task_id=task.task_id AND e.to_state IS NOT NULL) AND event.to_state<>task.state",
        [], |row| row.get(0)
    ).unwrap();
    assert_eq!(mismatches, 0);
}

#[test]
fn migration_failure_rolls_back_and_leaves_backup() {
    let (root, path, connection) = fixture();
    task(&connection, "T", "a", "assigned", "2026-01-01T00:00:00Z");
    event(
        &connection,
        "T",
        "a",
        1,
        "2026-01-01T00:00:00Z",
        "completed",
        Some("assigned"),
        Some("complete"),
    );
    drop(connection);
    assert!(SqliteStorageBackend::new(&path).is_err());
    let connection = Connection::open(&path).unwrap();
    let pk: u32 = connection
        .query_row(
            "SELECT pk FROM pragma_table_info('tasks') WHERE name='assignee'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(pk, 3);
    assert_eq!(backup_count(root.path()), 1);
}

#[test]
fn pre_ba_task_insert_against_migrated_schema_fails_with_check_constraint() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("mail.db");
    let _backend = migrate(&path);
    let connection = Connection::open(path).unwrap();
    let error = connection
        .execute(
            "INSERT INTO tasks(team, task_id, assignee, assigner, state, assignment_message_id,
                           description, assigned_at, updated_at)
         VALUES ('team', 'T', 'a', 'lead', 'assigned', 'M', 'task', 'now', 'now')",
            [],
        )
        .expect_err("legacy insert must omit required position");
    assert!(error.to_string().contains("CHECK constraint failed"));
    let mail: u32 = connection
        .query_row("SELECT COUNT(*) FROM mail_messages", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mail, 0);
}

#[test]
fn live_fixture_snapshot_migrates() {
    let (_root, path, connection) = fixture();
    for number in 0..14 {
        let id = format!("T{number}");
        task(
            &connection,
            &id,
            "winner",
            "complete",
            "2026-01-01T00:00:00Z",
        );
        event(
            &connection,
            &id,
            "winner",
            1,
            "2026-01-01T00:00:00Z",
            "completed",
            Some("assigned"),
            Some("complete"),
        );
        task(
            &connection,
            &id,
            "loser",
            "assigned",
            "2026-01-02T00:00:00Z",
        );
        event(
            &connection,
            &id,
            "loser",
            1,
            "2026-01-02T00:00:00Z",
            "assigned",
            None,
            Some("assigned"),
        );
    }
    drop(connection);
    let _backend = migrate(&path);
    let connection = Connection::open(path).unwrap();
    let merged: u32 = connection.query_row(
        "SELECT COUNT(*) FROM task_events WHERE event='migrated' AND detail LIKE 'migrated source row:%'",
        [], |row| row.get(0)
    ).unwrap();
    assert_eq!(
        merged, 14,
        "synthetic anonymised copy of the live duplicate shape"
    );
}
