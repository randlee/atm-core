//! Schema for the hook registration and cron schedule tables: additive,
//! idempotent, readable by the frozen pre-bk statements, and exposed through
//! the storage factory's handles.

use std::path::PathBuf;

use atm_storage::types::{AgentName, IsoTimestamp, TeamName};
use atm_storage::{
    HookName, HookRegistration, NewCronSchedule, PhaseId, StopCondition, StorageFactory,
};
use atm_storage_rusqlite::{SqliteStorageBackend, SqliteStorageFactory};
use chrono::{TimeZone, Utc};
use rusqlite::{Connection, params};

const PRE_BB_DDL: &str = include_str!("fixtures/pre_bb_task_and_mail.sql");

fn stamp(day: u32) -> IsoTimestamp {
    IsoTimestamp::from_datetime(
        Utc.with_ymd_and_hms(2026, 10, day, 12, 0, 0)
            .single()
            .expect("valid fixed timestamp"),
    )
}

fn team() -> TeamName {
    TeamName::from_validated("test-team")
}

fn registration(at: IsoTimestamp) -> HookRegistration {
    HookRegistration {
        team: team(),
        phase: PhaseId::parse("bk").expect("phase"),
        hook: HookName::parse("task.close").expect("hook"),
        module_path: PathBuf::from("/a.py"),
        cwd: PathBuf::from("/repo"),
        timeout_ms: 2500,
        custom: serde_json::Map::new(),
        active: true,
        registered_by: AgentName::from_validated("test-agent"),
        registered_at: at,
        disabled_at: None,
    }
}

fn cron(name: &str) -> NewCronSchedule {
    NewCronSchedule {
        team: team(),
        phase: PhaseId::parse("bk").expect("phase"),
        name: name.to_owned(),
        cron_expr: "*/5 * * * *".to_owned(),
        module_path: PathBuf::from("/hooks/cron.py"),
        cwd: PathBuf::from("/repo"),
        args: vec!["--once".to_owned()],
        timeout_ms: 30000,
        jitter_s: 0,
        custom: serde_json::Map::new(),
        stop: StopCondition::Hook,
        created_by: AgentName::from_validated("test-agent"),
        created_at: stamp(1),
    }
}

fn hook_tables(path: &std::path::Path) -> Vec<String> {
    let connection = Connection::open(path).expect("connection");
    let mut statement = connection
        .prepare(
            "SELECT name FROM sqlite_master
             WHERE type = 'table' AND name IN ('hook_registration', 'cron_schedule')
             ORDER BY name",
        )
        .expect("prepare");
    statement
        .query_map(params![], |row| row.get(0))
        .expect("query")
        .collect::<rusqlite::Result<Vec<String>>>()
        .expect("rows")
}

#[test]
fn pre_bk_ddl_set_reads_and_writes_after_hook_tables_created() {
    let root = tempfile::tempdir().expect("tempdir");
    let path = root.path().join("mail.db");
    let connection = Connection::open(&path).expect("connection");
    connection
        .execute_batch(PRE_BB_DDL)
        .expect("frozen pre-BB DDL");
    drop(connection);

    let backend = SqliteStorageBackend::new(&path).expect("backend");
    assert_eq!(
        hook_tables(&path),
        vec!["cron_schedule", "hook_registration"]
    );
    backend
        .hook_registration_store()
        .register(registration(stamp(1)))
        .expect("hook write");
    backend
        .cron_schedule_store()
        .start(cron("nightly"))
        .expect("cron write");
    drop(backend);

    let connection = Connection::open(&path).expect("pre-BB reader");
    let tasks: u32 = connection
        .query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get(0))
        .expect("frozen tasks read");
    let events: u32 = connection
        .query_row("SELECT COUNT(*) FROM task_events", [], |row| row.get(0))
        .expect("frozen task_events read");
    assert_eq!((tasks, events), (0, 0));
    connection
        .execute(
            "INSERT INTO tasks
                (team, task_id, assignee, assigner, state, position, assignment_message_id,
                 description, assigned_at, updated_at)
             VALUES ('test-team', 'BK6-COMPAT', 'worker', 'lead', 'assigned', 1, 'm1',
                 'compat', '2026-10-01T12:00:00Z', '2026-10-01T12:00:00Z')",
            [],
        )
        .expect("frozen tasks write");
}

#[test]
fn ensure_schema_twice_is_idempotent() {
    let root = tempfile::tempdir().expect("tempdir");
    let path = root.path().join("mail.db");
    let first = SqliteStorageBackend::new(&path).expect("first open");
    first
        .hook_registration_store()
        .register(registration(stamp(1)))
        .expect("register");
    first
        .cron_schedule_store()
        .start(cron("nightly"))
        .expect("start");
    drop(first);

    let second = SqliteStorageBackend::new(&path).expect("second open");
    assert_eq!(
        hook_tables(&path),
        vec!["cron_schedule", "hook_registration"]
    );
    assert_eq!(
        second
            .hook_registration_store()
            .active_all()
            .expect("hooks")
            .len(),
        1
    );
    assert_eq!(
        second
            .cron_schedule_store()
            .active_all()
            .expect("crons")
            .len(),
        1
    );
}

#[test]
fn hook_tables_have_no_sha_column() {
    let root = tempfile::tempdir().expect("tempdir");
    let path = root.path().join("mail.db");
    SqliteStorageBackend::new(&path).expect("backend");
    let connection = Connection::open(&path).expect("connection");
    for table in ["hook_registration", "cron_schedule"] {
        let mut statement = connection
            .prepare(&format!("PRAGMA table_info({table})"))
            .expect("prepare");
        let columns: Vec<String> = statement
            .query_map(params![], |row| row.get(1))
            .expect("query")
            .collect::<rusqlite::Result<Vec<String>>>()
            .expect("columns");
        assert!(!columns.is_empty(), "{table} exists");
        assert!(
            columns.iter().all(|name| !name.contains("sha")),
            "{table} has a sha column: {columns:?}"
        );
    }
}

#[test]
fn storage_handles_expose_both_stores() {
    let root = tempfile::tempdir().expect("tempdir");
    let handles = SqliteStorageFactory::host_scoped()
        .open(root.path())
        .expect("open handles");
    handles
        .hook_registration_store()
        .register(registration(stamp(1)))
        .expect("register through handles");
    handles
        .cron_schedule_store()
        .start(cron("nightly"))
        .expect("start through handles");
    let hooks = handles
        .hook_registration_store()
        .active_all()
        .expect("hooks");
    let crons = handles.cron_schedule_store().active_all().expect("crons");
    assert_eq!(hooks.len(), 1);
    assert_eq!(crons.len(), 1);
    assert_eq!(hooks[0].registered_at, stamp(1));
    assert_eq!(crons[0].name, "nightly");
}
