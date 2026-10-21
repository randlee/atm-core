//! SQLite hook registration store: the bk-1 contract cases with fixed
//! timestamps, reopen durability, the upsert and the validation mapping.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use atm_storage::types::{AgentName, IsoTimestamp, TeamName};
use atm_storage::{AtmErrorCode, HookName, HookRegistration, HookRegistrationStore, PhaseId};
use atm_storage_rusqlite::SqliteStorageBackend;
use chrono::{TimeZone, Utc};
use rusqlite::{Connection, params};
use tempfile::TempDir;

// The primary-key index can incidentally return phase order for tied rows
// even when a query omits its explicit SQL tie-breaker. Keep the required
// ORDER BY terms pinned so the tests do not depend on SQLite's query plan.
const HOOK_REGISTRATION_STORE_SOURCE: &str = include_str!("../src/hook_registration_store.rs");

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

fn phase(value: &str) -> PhaseId {
    PhaseId::parse(value).expect("valid phase id")
}

fn hook(value: &str) -> HookName {
    HookName::parse(value).expect("valid hook name")
}

fn registration(
    phase_id: &str,
    hook_name: &str,
    module: &str,
    at: IsoTimestamp,
) -> HookRegistration {
    registration_for_team("test-team", phase_id, hook_name, module, at)
}

fn registration_for_team(
    team_name: &str,
    phase_id: &str,
    hook_name: &str,
    module: &str,
    at: IsoTimestamp,
) -> HookRegistration {
    HookRegistration {
        team: TeamName::from_validated(team_name),
        phase: phase(phase_id),
        hook: hook(hook_name),
        module_path: PathBuf::from(module),
        cwd: PathBuf::from("/repo"),
        timeout_ms: 2500,
        custom: serde_json::Map::new(),
        active: true,
        registered_by: AgentName::from_validated("test-agent"),
        registered_at: at,
        disabled_at: None,
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

    fn store(&self) -> Arc<dyn HookRegistrationStore + Send + Sync> {
        self.backend.hook_registration_store()
    }
}

/// Reads one stored `registered_at` by SQL, bypassing the store.
fn stored_registered_at(path: &Path, phase_id: &str) -> String {
    Connection::open(path)
        .expect("connection")
        .query_row(
            "SELECT registered_at FROM hook_registration WHERE phase = ?1",
            params![phase_id],
            |row| row.get(0),
        )
        .expect("stored row")
}

#[test]
fn disable_keeps_rows_inactive() {
    let h = Harness::new();
    let store = h.store();
    store
        .register(registration("bk", "task.close", "/a.py", stamp(1)))
        .expect("register");
    store
        .register(registration("bk", "task.assign", "/b.py", stamp(2)))
        .expect("register");
    let changed = store
        .disable(&team(), &phase("bk"), Some(&hook("task.close")), stamp(4))
        .expect("disable");
    assert_eq!(changed.len(), 1);
    assert!(!changed[0].active);
    assert_eq!(changed[0].disabled_at, Some(stamp(4)));
    let rows = store.list(&team(), Some(&phase("bk"))).expect("list");
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().any(|row| {
        row.hook.as_str() == "task.close" && !row.active && row.disabled_at == Some(stamp(4))
    }));
    assert!(
        store
            .active(&team(), &hook("task.close"))
            .expect("active")
            .is_empty()
    );
}

#[test]
fn disable_single_hook_leaves_other_hooks_of_phase_active() {
    let h = Harness::new();
    let store = h.store();
    store
        .register(registration("bj", "task.close", "/a.py", stamp(1)))
        .expect("close");
    store
        .register(registration("bj", "task.start", "/b.py", stamp(1)))
        .expect("start");
    store
        .disable(&team(), &phase("bj"), Some(&hook("task.close")), stamp(2))
        .expect("disable");
    assert!(
        store
            .active(&team(), &hook("task.close"))
            .expect("close")
            .is_empty()
    );
    assert_eq!(
        store
            .active(&team(), &hook("task.start"))
            .expect("start")
            .len(),
        1
    );
}

#[test]
fn disable_phase_with_no_rows_returns_empty() {
    let h = Harness::new();
    let changed = h
        .store()
        .disable(&team(), &phase("bz"), None, stamp(2))
        .expect("disable");
    assert!(changed.is_empty());
}

#[test]
fn register_second_phase_same_hook_succeeds() {
    let h = Harness::new();
    let store = h.store();
    store
        .register(registration("bk", "task.close", "/a.py", stamp(1)))
        .expect("first phase");
    store
        .register(registration("bj", "task.close", "/b.py", stamp(2)))
        .expect("second phase");
    let phases: Vec<String> = store
        .active(&team(), &hook("task.close"))
        .expect("active")
        .iter()
        .map(|row| row.phase.as_str().to_owned())
        .collect();
    assert_eq!(phases, vec!["bk", "bj"]);
}

#[test]
fn reregister_active_key_keeps_chain_position() {
    let h = Harness::new();
    let store = h.store();
    store
        .register(registration("bk", "task.close", "/old.py", stamp(1)))
        .expect("register");
    store
        .register(registration("bj", "task.close", "/other.py", stamp(2)))
        .expect("register other phase");
    let returned = store
        .register(registration("bk", "task.close", "/new.py", stamp(3)))
        .expect("re-register");
    assert_eq!(returned.registered_at, stamp(1));
    assert_eq!(returned.module_path, PathBuf::from("/new.py"));
    let phases: Vec<String> = store
        .active(&team(), &hook("task.close"))
        .expect("active")
        .iter()
        .map(|row| row.phase.as_str().to_owned())
        .collect();
    assert_eq!(phases, vec!["bk", "bj"]);
}

#[test]
fn reregister_disabled_key_lists_last() {
    let h = Harness::new();
    let store = h.store();
    store
        .register(registration("bk", "task.close", "/a.py", stamp(1)))
        .expect("register");
    store
        .disable(&team(), &phase("bk"), Some(&hook("task.close")), stamp(2))
        .expect("disable");
    store
        .register(registration("bj", "task.close", "/b.py", stamp(3)))
        .expect("register other phase");
    let returned = store
        .register(registration("bk", "task.close", "/a.py", stamp(4)))
        .expect("re-register disabled key");
    assert!(returned.active);
    assert_eq!(returned.disabled_at, None);
    assert_eq!(returned.registered_at, stamp(4));
    let phases: Vec<String> = store
        .active(&team(), &hook("task.close"))
        .expect("active")
        .iter()
        .map(|row| row.phase.as_str().to_owned())
        .collect();
    assert_eq!(phases, vec!["bj", "bk"]);
}

#[test]
fn list_orders_active_first_then_registered_at_then_phase() {
    assert!(HOOK_REGISTRATION_STORE_SOURCE.contains("ORDER BY registered_at, phase;"));
    assert!(HOOK_REGISTRATION_STORE_SOURCE.contains("ORDER BY active DESC, registered_at, phase;"));

    let h = Harness::new();
    let store = h.store();
    store
        .register(registration("bk", "task.close", "/a.py", stamp(2)))
        .expect("a");
    store
        .register(registration("bj", "task.close", "/b.py", stamp(1)))
        .expect("b");
    store
        .disable(&team(), &phase("bj"), Some(&hook("task.close")), stamp(3))
        .expect("disable");
    store
        .register(registration("bj", "task.assign", "/c.py", stamp(4)))
        .expect("c");
    let order: Vec<(String, bool)> = store
        .list(&team(), None)
        .expect("list")
        .iter()
        .map(|row| (format!("{}/{}", row.phase, row.hook), row.active))
        .collect();
    assert_eq!(
        order,
        vec![
            ("bk/task.close".to_owned(), true),
            ("bj/task.assign".to_owned(), true),
            ("bj/task.close".to_owned(), false),
        ]
    );

    let tied_h = Harness::new();
    let tied_store = tied_h.store();
    tied_store
        .register(registration("bj", "task.close", "/a.py", stamp(1)))
        .expect("bj");
    tied_store
        .register(registration("bl", "task.close", "/a.py", stamp(2)))
        .expect("bl");
    tied_store
        .register(registration("bk", "task.close", "/a.py", stamp(3)))
        .expect("bk");
    tied_store
        .disable(&team(), &phase("bj"), None, stamp(9))
        .expect("disable bj");
    tied_store
        .register(registration("bl", "task.start", "/c.py", stamp(4)))
        .expect("bl start");
    tied_store
        .register(registration("bk", "task.start", "/c.py", stamp(4)))
        .expect("bk start");
    let tied = tied_store
        .active(&team(), &hook("task.start"))
        .expect("tied active");
    assert_eq!(
        tied.iter()
            .map(|row| row.phase.as_str())
            .collect::<Vec<_>>(),
        ["bk", "bl"]
    );
    let listed = tied_store
        .list(&team(), None)
        .expect("list with tied times");
    assert_eq!(
        listed
            .iter()
            .map(|row| row.phase.as_str())
            .collect::<Vec<_>>(),
        ["bl", "bk", "bk", "bl", "bj"]
    );
}

#[test]
fn active_all_order_equals_active_order_for_every_key() {
    assert!(HOOK_REGISTRATION_STORE_SOURCE.contains("ORDER BY team, hook, registered_at, phase;"));

    let h = Harness::new();
    let store = h.store();
    store
        .register(registration_for_team(
            "test-team",
            "bk",
            "task.close",
            "/a.py",
            stamp(2),
        ))
        .expect("bk");
    store
        .register(registration_for_team(
            "test-team",
            "bj",
            "task.close",
            "/a.py",
            stamp(1),
        ))
        .expect("bj");
    store
        .register(registration_for_team(
            "test-team",
            "bl",
            "task.close",
            "/a.py",
            stamp(2),
        ))
        .expect("bl");
    for (team_name, hook_name) in [
        ("test-team", "task.start"),
        ("test-team-z", "task.close"),
        ("test-team-z", "task.start"),
    ] {
        for (phase_id, day) in [("bl", 2), ("bk", 2), ("bj", 1)] {
            store
                .register(registration_for_team(
                    team_name,
                    phase_id,
                    hook_name,
                    "/a.py",
                    stamp(day),
                ))
                .expect("register");
        }
    }
    let all = store.active_all().expect("all");
    for team_name in ["test-team", "test-team-z"] {
        for hook_name in ["task.close", "task.start"] {
            let team = TeamName::from_validated(team_name);
            let hook = hook(hook_name);
            let per_key = store.active(&team, &hook).expect("per key");
            assert_eq!(
                per_key
                    .iter()
                    .map(|row| row.phase.as_str())
                    .collect::<Vec<_>>(),
                ["bj", "bk", "bl"]
            );
            let filtered: Vec<HookRegistration> = all
                .iter()
                .filter(|row| row.team == team && row.hook == hook)
                .cloned()
                .collect();
            assert_eq!(filtered, per_key);
        }
    }
}

#[test]
fn active_all_spans_teams() {
    let h = Harness::new();
    let store = h.store();
    store
        .register(registration_for_team(
            "test-team",
            "bj",
            "task.close",
            "/a.py",
            stamp(1),
        ))
        .expect("a");
    store
        .register(registration_for_team(
            "test-team-z",
            "bj",
            "task.close",
            "/b.py",
            stamp(1),
        ))
        .expect("b");
    store
        .register(registration_for_team(
            "test-team-z",
            "bk",
            "task.start",
            "/c.py",
            stamp(2),
        ))
        .expect("c");
    store
        .disable(
            &TeamName::from_validated("test-team-z"),
            &phase("bk"),
            None,
            stamp(3),
        )
        .expect("disable");
    let rows = store.active_all().expect("active_all");
    let keys: Vec<(String, String)> = rows
        .iter()
        .map(|row| (row.team.as_str().to_owned(), row.hook.as_str().to_owned()))
        .collect();
    assert_eq!(
        keys,
        [
            ("test-team".to_owned(), "task.close".to_owned()),
            ("test-team-z".to_owned(), "task.close".to_owned())
        ]
    );
}

#[test]
fn disable_twice_keeps_first_disabled_at() {
    let h = Harness::new();
    let store = h.store();
    store
        .register(registration("bk", "task.close", "/a.py", stamp(1)))
        .expect("register");
    assert_eq!(
        store
            .disable(&team(), &phase("bk"), None, stamp(4))
            .expect("first disable")
            .len(),
        1
    );
    assert!(
        store
            .disable(&team(), &phase("bk"), None, stamp(5))
            .expect("second disable")
            .is_empty()
    );
    let rows = store.list(&team(), Some(&phase("bk"))).expect("list");
    assert_eq!(rows[0].disabled_at, Some(stamp(4)));
}

#[test]
fn registrations_survive_reopen() {
    let h = Harness::new();
    let first = h.store();
    let mut custom = serde_json::Map::new();
    custom.insert("goal".to_owned(), serde_json::json!("x"));
    let mut bj = registration("bj", "task.close", "/b.py", stamp(2));
    bj.custom = custom;
    first
        .register(registration("bk", "task.close", "/a.py", stamp(1)))
        .expect("bk");
    first.register(bj).expect("bj");
    drop(first);

    let reopened = SqliteStorageBackend::new(&h.path).expect("reopen");
    let rows = reopened
        .hook_registration_store()
        .active_all()
        .expect("active_all");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].phase.as_str(), "bk");
    assert_eq!(rows[0].module_path, PathBuf::from("/a.py"));
    assert_eq!(rows[0].cwd, PathBuf::from("/repo"));
    assert_eq!(rows[0].timeout_ms, 2500);
    assert_eq!(rows[0].registered_by.as_str(), "test-agent");
    assert_eq!(rows[0].registered_at, stamp(1));
    assert_eq!(rows[0].disabled_at, None);
    assert_eq!(rows[1].phase.as_str(), "bj");
    assert_eq!(rows[1].registered_at, stamp(2));
    assert_eq!(rows[1].custom.get("goal"), Some(&serde_json::json!("x")));
    let listed = reopened
        .hook_registration_store()
        .list(&team(), None)
        .expect("list");
    assert_eq!(listed, rows);
}

#[test]
fn register_is_an_upsert_not_a_replace() {
    let h = Harness::new();
    let store = h.store();
    store
        .register(registration("bk", "task.close", "/a.py", stamp(1)))
        .expect("first");
    let mut again = registration("bk", "task.close", "/b.py", stamp(3));
    again.timeout_ms = 3000;
    store.register(again).expect("second");
    assert_eq!(stored_registered_at(&h.path, "bk"), stamp_text(1));
    let rows = store.list(&team(), None).expect("list");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].module_path, PathBuf::from("/b.py"));
    assert_eq!(rows[0].timeout_ms, 3000);
}

#[test]
fn zero_timeout_fails_the_check() {
    let h = Harness::new();
    let mut zero = registration("bk", "task.close", "/a.py", stamp(1));
    zero.timeout_ms = 0;
    let error = h
        .store()
        .register(zero)
        .expect_err("zero timeout is refused");
    assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
    assert!(error.message().contains("hook_registration"));
    assert!(error.message().contains("timeout_ms"));
}

#[test]
fn custom_round_trips_and_defaults_to_empty_object() {
    let h = Harness::new();
    let store = h.store();
    let mut with_custom = registration("bk", "task.close", "/a.py", stamp(1));
    with_custom.custom = serde_json::json!({"goal": "x", "n": [1, {"y": null}]})
        .as_object()
        .expect("object literal")
        .clone();
    let expected = with_custom.custom.clone();
    store.register(with_custom).expect("register");
    let rows = store.list(&team(), Some(&phase("bk"))).expect("list");
    assert_eq!(rows[0].custom, expected);

    Connection::open(&h.path)
        .expect("connection")
        .execute(
            "INSERT INTO hook_registration
                (team, phase, hook, module_path, cwd, timeout_ms, active,
                 registered_by, registered_at)
             VALUES ('test-team', 'bh', 'task.close', '/c.py', '/repo', 1000, 1,
                 'test-agent', ?1)",
            params![stamp_text(2)],
        )
        .expect("insert without custom");
    let defaulted = store.list(&team(), Some(&phase("bh"))).expect("list");
    assert!(defaulted[0].custom.is_empty());
}

#[test]
fn malformed_stored_custom_is_an_internal_error_naming_the_row() {
    let h = Harness::new();
    h.store()
        .register(registration("bk", "task.close", "/a.py", stamp(1)))
        .expect("register");
    Connection::open(&h.path)
        .expect("connection")
        .execute(
            "UPDATE hook_registration SET custom = '[1]' WHERE phase = 'bk'",
            [],
        )
        .expect("corrupt custom");
    let error = h
        .store()
        .list(&team(), None)
        .expect_err("corrupt custom is refused");
    assert_eq!(error.code(), AtmErrorCode::InternalError);
    assert!(error.message().contains("test-team/bk/task.close"));
}
