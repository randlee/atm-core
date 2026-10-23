//! Schema for the hook registration and cron schedule tables: additive,
//! idempotent, readable by the frozen pre-bk statements, and exposed through
//! the storage factory's handles.

use std::path::PathBuf;

use atm_storage::types::{AgentName, IsoTimestamp, TeamName};
use atm_storage::{
    HookName, HookRegistration, NewCronSchedule, PhaseId, StopCondition, StorageFactory,
};
use atm_storage_rusqlite::{STORAGE_SCHEMA_VERSION, SqliteStorageBackend, SqliteStorageFactory};
use chrono::{TimeZone, Utc};
use rusqlite::{Connection, params};

const PRE_BK_DB_MIGRATIONS: &str = include_str!("fixtures/pre_bk_db_migrations.sql");
const PRE_BK_TASK_SCHEMA: &str = include_str!("fixtures/pre_bk_task_schema.sql");

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

fn installed_tables(path: &std::path::Path) -> Vec<String> {
    let connection = Connection::open(path).expect("connection");
    let mut statement = connection
        .prepare(
            "SELECT name FROM sqlite_master
             WHERE type = 'table' AND name IN (
                'hook_registration', 'cron_schedule', 'tool_sender_registry',
                'tool_sender_nonce_ledger', 'cron_transition_event', 'cron_delivery_intent'
             )
             ORDER BY name",
        )
        .expect("prepare");
    statement
        .query_map(params![], |row| row.get(0))
        .expect("query")
        .collect::<rusqlite::Result<Vec<String>>>()
        .expect("rows")
}

fn storage_schema_version(path: &std::path::Path) -> String {
    Connection::open(path)
        .expect("connection")
        .query_row(
            "SELECT major || '.' || minor || '.' || patch
             FROM atm_storage_schema_version WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .expect("recorded storage schema version")
}

#[test]
fn pre_bk_ddl_set_reads_and_writes_after_hook_tables_created() {
    for seeded in [false, true] {
        let root = tempfile::tempdir().expect("tempdir");
        let path = root.path().join("mail.db");
        let connection = Connection::open(&path).expect("connection");
        if seeded {
            pre_bk_schema(&connection);
            write_pre_bk_rows(&connection, "before");
            assert_pre_bk_rows(&connection, &["before"]);
        }
        assert!(installed_tables(&path).is_empty());
        drop(connection);

        let backend = SqliteStorageBackend::new(&path).expect("backend");
        assert_eq!(
            installed_tables(&path),
            vec![
                "cron_delivery_intent",
                "cron_schedule",
                "cron_transition_event",
                "hook_registration",
                "tool_sender_nonce_ledger",
                "tool_sender_registry",
            ]
        );
        assert_eq!(storage_schema_version(&path), STORAGE_SCHEMA_VERSION);
        backend
            .hook_registration_store()
            .register(registration(stamp(1)))
            .expect("hook write");
        backend
            .cron_schedule_store()
            .start(cron("nightly"))
            .expect("cron write");
        drop(backend);

        let connection = Connection::open(&path).expect("pre-bk reader");
        assert_pre_bk_rows(&connection, if seeded { &["before"] } else { &[] });
        pre_bk_schema(&connection);
        write_pre_bk_rows(&connection, "after");
        assert_pre_bk_rows(
            &connection,
            if seeded {
                &["after", "before"]
            } else {
                &["after"]
            },
        );
        drop(connection);

        // The frozen previous-consumer DDL and writes leave the version record
        // untouched while continuing to read/write its supported tables.
        assert_eq!(storage_schema_version(&path), STORAGE_SCHEMA_VERSION);

        let backend = SqliteStorageBackend::new(&path).expect("reopen after old writes");
        assert_eq!(
            backend
                .hook_registration_store()
                .active_all()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(backend.cron_schedule_store().active_all().unwrap().len(), 1);
        assert_eq!(storage_schema_version(&path), STORAGE_SCHEMA_VERSION);
    }
}

fn pre_bk_schema(connection: &Connection) {
    connection
        .execute_batch(PRE_BK_DB_MIGRATIONS)
        .expect("frozen pre-bk DB_MIGRATIONS");
    connection
        .execute_batch(PRE_BK_TASK_SCHEMA)
        .expect("frozen pre-bk task and handoff schema");
}

fn write_pre_bk_rows(connection: &Connection, marker: &str) {
    for statement in [
        "INSERT INTO mail_messages
            (team, agent, message_key, envelope_json, from_agent, message_text, message_at)
         VALUES ('test-team', ?1, 'atm:' || ?1, '{}', 'lead', ?1, '2026-10-01T12:00:00Z')",
        "INSERT INTO mail_message_states (team, agent, message_key, read)
         VALUES ('test-team', ?1, 'atm:' || ?1, 1)",
        "INSERT INTO mail_seen_watermarks (team, agent, watermark)
         VALUES ('test-team', ?1, ?1)",
        "INSERT INTO team_roster (team_name, agent_name, member_kind, harness, updated_at)
         VALUES ('test-team', ?1, 'permanent', 'codex-cli', '2026-10-01T12:00:00Z')",
        "INSERT INTO tasks
            (team, task_id, assignee, assigner, state, position, assignment_message_id,
             description, assigned_at, updated_at)
         VALUES ('test-team', ?1, ?1, 'lead', 'assigned', 1, ?1,
             ?1, '2026-10-01T12:00:00Z', '2026-10-01T12:00:00Z')",
        "INSERT INTO task_events (team, task_id, assignee, seq, at, event, actor, detail)
         VALUES ('test-team', ?1, ?1, 1, '2026-10-01T12:00:00Z', 'assigned', 'lead', ?1)",
        "INSERT INTO prompt_handoffs (team, agent, message_key, kind, task_id, trigger, at)
         VALUES ('test-team', ?1, 'atm:' || ?1, 'task_ready', ?1, 'steer', '2026-10-01T12:00:00Z')",
    ] {
        assert_eq!(
            connection
                .execute(statement, params![marker])
                .expect("pre-bk write"),
            1
        );
    }
}

fn assert_pre_bk_rows(connection: &Connection, markers: &[&str]) {
    let mut statement = connection
        .prepare(
            "SELECT 'mail_messages', message_text FROM mail_messages WHERE team = 'test-team'
         UNION ALL SELECT 'mail_message_states', agent FROM mail_message_states
             WHERE team = 'test-team' AND read = 1
         UNION ALL SELECT 'mail_seen_watermarks', watermark FROM mail_seen_watermarks
             WHERE team = 'test-team'
         UNION ALL SELECT 'team_roster', agent_name FROM team_roster WHERE team_name = 'test-team'
         UNION ALL SELECT 'tasks', description FROM tasks WHERE team = 'test-team'
         UNION ALL SELECT 'task_events', detail FROM task_events WHERE team = 'test-team'
         UNION ALL SELECT 'prompt_handoffs', task_id FROM prompt_handoffs WHERE team = 'test-team'
         ORDER BY 1, 2",
        )
        .expect("pre-bk reads");
    let actual = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    let expected: Vec<_> = [
        "mail_message_states",
        "mail_messages",
        "mail_seen_watermarks",
        "prompt_handoffs",
        "task_events",
        "tasks",
        "team_roster",
    ]
    .into_iter()
    .flat_map(|table| {
        markers
            .iter()
            .map(move |marker| (table.to_owned(), (*marker).to_owned()))
    })
    .collect();
    assert_eq!(actual, expected);
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
        installed_tables(&path),
        vec![
            "cron_delivery_intent",
            "cron_schedule",
            "cron_transition_event",
            "hook_registration",
            "tool_sender_nonce_ledger",
            "tool_sender_registry",
        ]
    );
    assert_eq!(storage_schema_version(&path), STORAGE_SCHEMA_VERSION);
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
