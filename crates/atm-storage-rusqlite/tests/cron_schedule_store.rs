//! SQLite cron schedule store: the bk-1 contract cases with fixed timestamps,
//! generation fencing, the stop-condition encoding and reopen durability.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use atm_storage::types::{AgentName, IsoTimestamp, TeamName};
use atm_storage::{
    AtmErrorCode, CronSchedule, CronScheduleId, CronScheduleStore, NewCronSchedule, PhaseId,
    StopCondition,
};
use atm_storage_rusqlite::SqliteStorageBackend;
use chrono::{TimeZone, Utc};
use rusqlite::{Connection, params};
use tempfile::TempDir;

fn stamp(day: u32) -> IsoTimestamp {
    IsoTimestamp::from_datetime(
        Utc.with_ymd_and_hms(2026, 10, day, 12, 0, 0)
            .single()
            .expect("valid fixed timestamp"),
    )
}

/// The fixed-width text the store writes for `stamp(day)`.
fn stamp_text(day: u32) -> String {
    format!("2026-10-{day:02}T12:00:00.000000000Z")
}

fn team() -> TeamName {
    TeamName::from_validated("test-team")
}

fn phase() -> PhaseId {
    PhaseId::parse("bk").expect("phase")
}

fn cron_spec(name: &str, stop: StopCondition, created_at: IsoTimestamp) -> NewCronSchedule {
    NewCronSchedule {
        team: team(),
        phase: phase(),
        name: name.to_owned(),
        cron_expr: "*/5 * * * *".to_owned(),
        module_path: PathBuf::from("/hooks/cron.py"),
        cwd: PathBuf::from("/repo"),
        args: vec!["--once".to_owned()],
        timeout_ms: 30000,
        jitter_s: 1799,
        custom: serde_json::json!({"goal": "x", "n": [1, {"y": null}]})
            .as_object()
            .expect("object literal")
            .clone(),
        stop,
        created_by: AgentName::from_validated("test-agent"),
        created_at,
    }
}

struct Harness {
    _root: TempDir,
    path: PathBuf,
    backend: SqliteStorageBackend,
}

impl Harness {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("tempdir");
        let path = root.path().join("mail.db");
        let backend = SqliteStorageBackend::new(&path).expect("backend");
        Self {
            _root: root,
            path,
            backend,
        }
    }

    fn store(&self) -> Arc<dyn CronScheduleStore + Send + Sync> {
        self.backend.cron_schedule_store()
    }

    fn sql(&self) -> Connection {
        Connection::open(&self.path).expect("connection")
    }
}

/// Inserts one `cron_schedule` row by SQL with explicit stop and lifecycle
/// columns, for the CHECK tests.
fn sql_insert_cron(
    path: &Path,
    name: &str,
    active: i64,
    until_at: Option<&str>,
    stop_hook: i64,
    ended_at: Option<&str>,
) -> rusqlite::Result<usize> {
    Connection::open(path)?.execute(
        "INSERT INTO cron_schedule
            (team, phase, name, cron_expr, module_path, cwd, args, timeout_ms,
             created_by, created_at, active, until_at, stop_hook, ended_at)
         VALUES ('test-team', 'bk', ?1, '*/5 * * * *', '/h.py', '/repo', '[]', 1000,
             'test-agent', ?2, ?3, ?4, ?5, ?6)",
        params![name, stamp_text(1), active, until_at, stop_hook, ended_at],
    )
}

/// Asserts that the insert was refused by a table CHECK constraint, not by a
/// NOT NULL, a typo or schema drift.
fn assert_check_violation(result: rusqlite::Result<usize>, case: &str) {
    let error = result.expect_err(case);
    let message = error.to_string();
    assert!(
        message.contains("CHECK constraint failed"),
        "{case}: expected a CHECK constraint violation, got: {message}"
    );
}

fn stored_last_outcome(path: &Path, name: &str) -> Option<String> {
    Connection::open(path)
        .expect("connection")
        .query_row(
            "SELECT last_outcome FROM cron_schedule WHERE name = ?1",
            [name],
            |row| row.get(0),
        )
        .expect("stored row")
}

#[test]
fn active_all_survives_reopen() {
    let h = Harness::new();
    let store = h.store();
    let nightly = store
        .start(cron_spec("nightly", StopCondition::Hook, stamp(3)))
        .expect("nightly");
    let until = StopCondition::Until {
        at: stamp(9).into_inner(),
    };
    store
        .start(cron_spec("weekly", until, stamp(4)))
        .expect("weekly");
    store
        .end(&team(), &phase(), Some("weekly"), stamp(5))
        .expect("end");
    store
        .start(cron_spec("weekly", StopCondition::Hook, stamp(6)))
        .expect("restart weekly");
    let before = store.active_all().expect("active_all");
    drop(store);

    let reopened = SqliteStorageBackend::new(&h.path).expect("reopen");
    let after = reopened
        .cron_schedule_store()
        .active_all()
        .expect("reopened");
    assert_eq!(after, before);
    assert_eq!(after.len(), 2);
    let row: &CronSchedule = after
        .iter()
        .find(|row| row.name == "nightly")
        .expect("nightly survives");
    assert_eq!(row.id, nightly.id);
    assert_eq!(row.cron_expr, "*/5 * * * *");
    assert_eq!(row.jitter_s, 1799);
    assert_eq!(row.generation, 1);
    assert_eq!(row.custom, nightly.custom);
    assert_eq!(row.stop, StopCondition::Hook);
    let restarted = after
        .iter()
        .find(|row| row.name == "weekly")
        .expect("weekly");
    assert_eq!(restarted.generation, 2);
}

#[test]
fn stop_condition_check_and_round_trip() {
    let h = Harness::new();
    let until_text = stamp_text(9);
    // A valid Until row inserted by SQL proves the helper writes a storable row.
    assert_eq!(
        sql_insert_cron(&h.path, "ok-until", 1, Some(&until_text), 0, None).expect("valid until"),
        1
    );
    assert_check_violation(
        sql_insert_cron(&h.path, "no-stop", 1, None, 0, None),
        "no stop condition",
    );
    assert_check_violation(
        sql_insert_cron(&h.path, "both-stops", 1, Some(&until_text), 1, None),
        "both stop conditions",
    );

    let store = h.store();
    let until = store
        .start(cron_spec(
            "until-row",
            StopCondition::Until {
                at: stamp(9).into_inner(),
            },
            stamp(1),
        ))
        .expect("start until");
    assert_eq!(
        until.stop,
        StopCondition::Until {
            at: stamp(9).into_inner()
        }
    );
    let hooked = store
        .start(cron_spec("hook-row", StopCondition::Hook, stamp(1)))
        .expect("start hook");
    assert_eq!(hooked.stop, StopCondition::Hook);

    let connection = h.sql();
    let (until_at, stop_hook): (Option<String>, i64) = connection
        .query_row(
            "SELECT until_at, stop_hook FROM cron_schedule WHERE name = 'until-row'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("until row");
    assert_eq!(until_at.as_deref(), Some(until_text.as_str()));
    assert_eq!(stop_hook, 0);
    let (until_at, stop_hook): (Option<String>, i64) = connection
        .query_row(
            "SELECT until_at, stop_hook FROM cron_schedule WHERE name = 'hook-row'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("hook row");
    assert_eq!(until_at, None);
    assert_eq!(stop_hook, 1);
}

#[test]
fn start_after_end_updates_in_place_and_bumps_generation() {
    let h = Harness::new();
    let store = h.store();
    let first = store
        .start(cron_spec("nightly", StopCondition::Hook, stamp(3)))
        .expect("start");
    assert!(
        store
            .record_run(first.id, 1, stamp(4), "exit 0")
            .expect("record run")
    );
    assert_eq!(
        store
            .end(&team(), &phase(), Some("nightly"), stamp(4))
            .expect("end")
            .len(),
        1
    );
    let restarted = store
        .start(cron_spec("nightly", StopCondition::Hook, stamp(6)))
        .expect("restart");
    assert_eq!(restarted.id, first.id);
    assert_eq!(restarted.generation, 2);
    assert!(restarted.active);
    assert_eq!(restarted.created_at, stamp(6));
    assert_eq!(restarted.ended_at, None);
    assert_eq!(restarted.last_run_at, None);
    assert_eq!(restarted.last_outcome, None);
}

#[test]
fn start_refuses_active_duplicate() {
    let h = Harness::new();
    let store = h.store();
    store
        .start(cron_spec("nightly", StopCondition::Hook, stamp(3)))
        .expect("start");
    let error = store
        .start(cron_spec("nightly", StopCondition::Hook, stamp(4)))
        .expect_err("active duplicate refused");
    assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
    assert!(
        error
            .message()
            .starts_with("cron schedule nightly already active for phase bk"),
        "{}",
        error.message()
    );
    assert!(error.message().contains("atm cron end --phase bk nightly"));
    assert_eq!(store.list(None, None).expect("list").len(), 1);
}

#[test]
fn record_with_stale_generation_changes_nothing() {
    let h = Harness::new();
    let store = h.store();
    let first = store
        .start(cron_spec("nightly", StopCondition::Hook, stamp(3)))
        .expect("start");
    store
        .end(&team(), &phase(), Some("nightly"), stamp(4))
        .expect("end");
    store
        .start(cron_spec("nightly", StopCondition::Hook, stamp(6)))
        .expect("restart");
    assert!(
        !store
            .record_run(first.id, 1, stamp(7), "stale")
            .expect("stale run")
    );
    assert!(
        !store
            .record_stop(first.id, 1, stamp(7), "stale")
            .expect("stale stop")
    );
    assert_eq!(stored_last_outcome(&h.path, "nightly"), None);
    assert!(
        store
            .record_run(first.id, 2, stamp(8), "fresh")
            .expect("current run")
    );
    assert_eq!(
        stored_last_outcome(&h.path, "nightly").as_deref(),
        Some("fresh")
    );
}

#[test]
fn record_stop_ends_the_row_once() {
    let h = Harness::new();
    let store = h.store();
    let row = store
        .start(cron_spec("nightly", StopCondition::Hook, stamp(3)))
        .expect("start");
    assert!(
        store
            .record_stop(row.id, 1, stamp(4), "phase bead closed")
            .expect("stop")
    );
    assert!(
        !store
            .record_stop(row.id, 1, stamp(5), "again")
            .expect("second stop")
    );
    let ended = store.list(None, None).expect("list");
    assert!(!ended[0].active);
    assert_eq!(ended[0].ended_at, Some(stamp(4)));
    assert_eq!(
        ended[0].last_outcome.as_deref(),
        Some("stopped: phase bead closed")
    );
    assert!(store.active_all().expect("active").is_empty());
}

#[test]
fn record_run_on_ended_row_changes_nothing() {
    let h = Harness::new();
    let store = h.store();
    let row = store
        .start(cron_spec("nightly", StopCondition::Hook, stamp(3)))
        .expect("start");
    store.end(&team(), &phase(), None, stamp(4)).expect("end");
    assert!(
        !store
            .record_run(row.id, 1, stamp(5), "late")
            .expect("late run")
    );
    assert_eq!(stored_last_outcome(&h.path, "nightly"), None);
}

#[test]
fn record_with_unstorable_id_changes_nothing() {
    let h = Harness::new();
    let changed = h
        .store()
        .record_run(CronScheduleId(u64::MAX), 1, stamp(4), "none")
        .expect("no row can carry this id");
    assert!(!changed);
}

#[test]
fn lifecycle_checks_refuse_contradictory_rows() {
    let h = Harness::new();
    h.store()
        .start(cron_spec("seed", StopCondition::Hook, stamp(1)))
        .expect("seed table");
    assert_check_violation(
        sql_insert_cron(&h.path, "active-ended", 1, None, 1, Some(&stamp_text(2))),
        "active schedule with ended_at",
    );
    assert_check_violation(
        sql_insert_cron(&h.path, "inactive-open", 0, None, 1, None),
        "inactive schedule without ended_at",
    );
    assert_check_violation(
        Connection::open(&h.path).expect("connection").execute(
            "INSERT INTO hook_registration
                    (team, phase, hook, module_path, cwd, timeout_ms, active,
                     registered_by, registered_at, disabled_at)
                 VALUES ('test-team', 'bk', 'task.close', '/a.py', '/repo', 1000, 1,
                     'test-agent', ?1, ?1)",
            [stamp_text(1)],
        ),
        "active hook with disabled_at",
    );
    assert_check_violation(
        Connection::open(&h.path).expect("connection").execute(
            "INSERT INTO hook_registration
                    (team, phase, hook, module_path, cwd, timeout_ms, active,
                     registered_by, registered_at)
                 VALUES ('test-team', 'bk', 'task.assign', '/a.py', '/repo', 1000, 0,
                     'test-agent', ?1)",
            [stamp_text(1)],
        ),
        "disabled hook without disabled_at",
    );
}

#[test]
fn malformed_stored_args_is_an_internal_error_naming_the_row() {
    let h = Harness::new();
    h.store()
        .start(cron_spec("nightly", StopCondition::Hook, stamp(1)))
        .expect("start");
    h.sql()
        .execute("UPDATE cron_schedule SET args = '{\"x\":1}'", [])
        .expect("corrupt args");
    let error = h
        .store()
        .list(None, None)
        .expect_err("corrupt args is refused");
    assert_eq!(error.code(), AtmErrorCode::InternalError);
    assert!(error.message().contains("cron_schedule id 1"));
}
