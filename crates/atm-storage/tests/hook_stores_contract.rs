//! Contract tests for the hook registration and cron schedule stores.
//!
//! Run against the in-memory doubles with fixed timestamps; bk-6 runs the
//! same cases against SQLite.

#![cfg(feature = "test-utils")]

use std::path::PathBuf;

use atm_storage::request_budget::{
    CRON_TIMEOUT_MAX_MS, HOOK_ACTION_RESERVE_MS, HOOK_TIMEOUT_MAX_MS,
};
use atm_storage::testing::{InMemoryCronScheduleStore, InMemoryHookRegistrationStore};
use atm_storage::types::{AgentName, IsoTimestamp, TeamName};
use atm_storage::{
    CronSchedule, CronScheduleStore, HookName, HookRegistration, HookRegistrationStore,
    NewCronSchedule, PhaseId, StopCondition,
};
use chrono::{TimeZone, Utc};

fn stamp(day: u32) -> IsoTimestamp {
    IsoTimestamp::from_datetime(
        Utc.with_ymd_and_hms(2026, 10, day, 12, 0, 0)
            .single()
            .expect("valid fixed timestamp"),
    )
}

fn team() -> TeamName {
    TeamName::from_validated("atm-dev")
}

fn phase(value: &str) -> PhaseId {
    PhaseId::parse(value).expect("valid phase id")
}

fn hook(value: &str) -> HookName {
    HookName::parse(value).expect("valid hook name")
}

fn registration(
    team: &str,
    phase_id: &str,
    hook_name: &str,
    module: &str,
    at: IsoTimestamp,
) -> HookRegistration {
    HookRegistration {
        team: TeamName::from_validated(team),
        phase: phase(phase_id),
        hook: hook(hook_name),
        module_path: PathBuf::from(module),
        cwd: PathBuf::from("/repo"),
        timeout_ms: 2500,
        custom: serde_json::Map::new(),
        active: true,
        registered_by: AgentName::from_validated("arch-ctm"),
        registered_at: at,
        disabled_at: None,
    }
}

fn cron_spec(name: &str, stop: StopCondition, created_at: IsoTimestamp) -> NewCronSchedule {
    NewCronSchedule {
        team: team(),
        phase: phase("bk"),
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
        created_by: AgentName::from_validated("arch-ctm"),
        created_at,
    }
}

fn phases(rows: &[HookRegistration]) -> Vec<String> {
    rows.iter()
        .map(|row| row.phase.as_str().to_owned())
        .collect()
}

#[test]
fn disable_keeps_rows_inactive() {
    let store = InMemoryHookRegistrationStore::new();
    store
        .register(registration(
            "atm-dev",
            "bj",
            "task.close",
            "/h.py",
            stamp(1),
        ))
        .expect("register");
    let changed = store
        .disable(&team(), &phase("bj"), None, stamp(2))
        .expect("disable");
    assert_eq!(changed.len(), 1);
    let rows = store.list(&team(), None).expect("list");
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].active);
    assert_eq!(rows[0].disabled_at, Some(stamp(2)));
    assert!(
        store
            .active(&team(), &hook("task.close"))
            .expect("active")
            .is_empty()
    );
}

#[test]
fn register_second_phase_same_hook_succeeds() {
    let store = InMemoryHookRegistrationStore::new();
    store
        .register(registration(
            "atm-dev",
            "bj",
            "task.close",
            "/bj.py",
            stamp(1),
        ))
        .expect("bj");
    store
        .register(registration(
            "atm-dev",
            "bk",
            "task.close",
            "/bk.py",
            stamp(2),
        ))
        .expect("bk");
    let active = store.active(&team(), &hook("task.close")).expect("active");
    assert_eq!(phases(&active), ["bj", "bk"]);
    store
        .disable(&team(), &phase("bj"), None, stamp(3))
        .expect("disable bj");
    let active = store.active(&team(), &hook("task.close")).expect("active");
    assert_eq!(phases(&active), ["bk"]);
}

#[test]
fn reregister_active_key_keeps_chain_position() {
    let store = InMemoryHookRegistrationStore::new();
    store
        .register(registration(
            "atm-dev",
            "bj",
            "task.close",
            "/bj.py",
            stamp(1),
        ))
        .expect("bj");
    store
        .register(registration(
            "atm-dev",
            "bk",
            "task.close",
            "/bk.py",
            stamp(2),
        ))
        .expect("bk");
    let again = store
        .register(registration(
            "atm-dev",
            "bj",
            "task.close",
            "/bj-new.py",
            stamp(3),
        ))
        .expect("re-register");
    assert_eq!(again.registered_at, stamp(1));
    assert_eq!(again.module_path, PathBuf::from("/bj-new.py"));
}

#[test]
fn reregister_disabled_key_lists_last() {
    let store = InMemoryHookRegistrationStore::new();
    store
        .register(registration(
            "atm-dev",
            "bj",
            "task.close",
            "/bj.py",
            stamp(1),
        ))
        .expect("bj");
    store
        .register(registration(
            "atm-dev",
            "bk",
            "task.close",
            "/bk.py",
            stamp(2),
        ))
        .expect("bk");
    store
        .disable(&team(), &phase("bj"), Some(&hook("task.close")), stamp(5))
        .expect("disable");
    let again = store
        .register(registration(
            "atm-dev",
            "bj",
            "task.close",
            "/bj.py",
            stamp(4),
        ))
        .expect("re-register");
    assert_eq!(again.registered_at, stamp(4));
    assert_eq!(again.disabled_at, None);
    assert!(again.active);
    let rows = store.list(&team(), None).expect("list");
    assert_eq!(phases(&rows), ["bk", "bj"]);
}

#[test]
fn disable_single_hook_leaves_other_hooks_of_phase_active() {
    let store = InMemoryHookRegistrationStore::new();
    store
        .register(registration(
            "atm-dev",
            "bj",
            "task.close",
            "/a.py",
            stamp(1),
        ))
        .expect("close");
    store
        .register(registration(
            "atm-dev",
            "bj",
            "task.start",
            "/b.py",
            stamp(1),
        ))
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
fn list_orders_active_first_then_registered_at_then_phase() {
    let store = InMemoryHookRegistrationStore::new();
    store
        .register(registration(
            "atm-dev",
            "bj",
            "task.close",
            "/a.py",
            stamp(1),
        ))
        .expect("bj");
    store
        .register(registration(
            "atm-dev",
            "bl",
            "task.close",
            "/a.py",
            stamp(2),
        ))
        .expect("bl");
    store
        .register(registration(
            "atm-dev",
            "bk",
            "task.close",
            "/a.py",
            stamp(3),
        ))
        .expect("bk");
    store
        .disable(&team(), &phase("bj"), None, stamp(9))
        .expect("disable bj");
    let rows = store.list(&team(), None).expect("list");
    assert_eq!(phases(&rows), ["bl", "bk", "bj"]);
    store
        .register(registration(
            "atm-dev",
            "bl",
            "task.start",
            "/c.py",
            stamp(4),
        ))
        .expect("bl start");
    store
        .register(registration(
            "atm-dev",
            "bk",
            "task.start",
            "/c.py",
            stamp(4),
        ))
        .expect("bk start");
    let tied = store.active(&team(), &hook("task.start")).expect("active");
    assert_eq!(phases(&tied), ["bk", "bl"]);
}

#[test]
fn disable_twice_keeps_first_disabled_at() {
    let store = InMemoryHookRegistrationStore::new();
    store
        .register(registration(
            "atm-dev",
            "bj",
            "task.close",
            "/a.py",
            stamp(1),
        ))
        .expect("register");
    store
        .disable(&team(), &phase("bj"), None, stamp(2))
        .expect("first disable");
    let second = store
        .disable(&team(), &phase("bj"), None, stamp(3))
        .expect("second disable");
    assert!(second.is_empty());
    let rows = store.list(&team(), None).expect("list");
    assert_eq!(rows[0].disabled_at, Some(stamp(2)));
}

#[test]
fn disable_phase_with_no_rows_returns_empty() {
    let store = InMemoryHookRegistrationStore::new();
    let changed = store
        .disable(&team(), &phase("bz"), None, stamp(2))
        .expect("disable");
    assert!(changed.is_empty());
}

#[test]
fn hook_name_parse_sets() {
    for name in HookName::TASK_HOOKS {
        assert!(HookName::parse_task_hook(name).is_ok(), "{name}");
    }
    for name in ["cron.tick", "cron.stop", "task.foo"] {
        let error = HookName::parse_task_hook(name).expect_err("not a task hook");
        assert_eq!(error.code().as_str(), "ATM_HOOK_MODULE_INVALID", "{name}");
    }
    for name in HookName::TASK_HOOKS
        .iter()
        .chain(["cron.tick", "cron.stop"].iter())
    {
        assert!(HookName::parse(name).is_ok(), "{name}");
    }
    let error = HookName::parse("task.foo").expect_err("unknown hook");
    assert_eq!(error.code().as_str(), "ATM_HOOK_MALFORMED");
}

#[test]
fn hook_name_deserialize_rejects_unknown() {
    assert!(serde_json::from_str::<HookName>("\"task.foo\"").is_err());
    assert!(serde_json::from_str::<HookName>("\"task.close\"").is_ok());
}

#[test]
fn phase_id_rejects_empty_uppercase_space_and_newline() {
    for good in ["bk", "bk-2", &format!("a{}", "b".repeat(31))] {
        assert!(PhaseId::parse(good).is_ok(), "{good}");
    }
    for bad in [
        "",
        "Bk",
        "b k",
        "bk\n",
        "2bk",
        &format!("a{}", "b".repeat(32)),
    ] {
        let error = PhaseId::parse(bad).expect_err("refused");
        assert_eq!(error.code().as_str(), "ATM_MESSAGE_VALIDATION_FAILED");
        assert!(
            error.message().contains("^[a-z][a-z0-9-]{0,31}$"),
            "{bad:?}"
        );
        assert!(
            error
                .message()
                .contains("Recovery: use a lowercase phase id such as bk or bk-2"),
            "{bad:?}"
        );
    }
}

#[test]
fn active_all_spans_teams_and_counts_calls() {
    let store = InMemoryHookRegistrationStore::new();
    store
        .register(registration(
            "atm-dev",
            "bj",
            "task.close",
            "/a.py",
            stamp(1),
        ))
        .expect("a");
    store
        .register(registration("other", "bj", "task.close", "/b.py", stamp(1)))
        .expect("b");
    store
        .register(registration("other", "bk", "task.start", "/c.py", stamp(2)))
        .expect("c");
    store
        .disable(
            &TeamName::from_validated("other"),
            &phase("bk"),
            None,
            stamp(3),
        )
        .expect("disable");
    let before = store.calls.load(std::sync::atomic::Ordering::SeqCst);
    let rows = store.active_all().expect("active_all");
    assert_eq!(
        store.calls.load(std::sync::atomic::Ordering::SeqCst),
        before + 1
    );
    let keys: Vec<(String, String)> = rows
        .iter()
        .map(|row| (row.team.as_str().to_owned(), row.hook.as_str().to_owned()))
        .collect();
    assert_eq!(
        keys,
        [
            ("atm-dev".to_owned(), "task.close".to_owned()),
            ("other".to_owned(), "task.close".to_owned())
        ]
    );
}

#[test]
fn active_all_order_equals_active_order_for_every_key() {
    let store = InMemoryHookRegistrationStore::new();
    store
        .register(registration(
            "atm-dev",
            "bk",
            "task.close",
            "/a.py",
            stamp(2),
        ))
        .expect("bk");
    store
        .register(registration(
            "atm-dev",
            "bj",
            "task.close",
            "/a.py",
            stamp(1),
        ))
        .expect("bj");
    store
        .register(registration(
            "atm-dev",
            "bl",
            "task.close",
            "/a.py",
            stamp(2),
        ))
        .expect("bl");
    let all = store.active_all().expect("all");
    let per_key = store.active(&team(), &hook("task.close")).expect("per key");
    let filtered: Vec<HookRegistration> = all
        .into_iter()
        .filter(|row| row.hook.as_str() == "task.close")
        .collect();
    assert_eq!(filtered, per_key);
}

#[test]
fn active_all_returns_started_rows_with_cron_expr_byte_identical() {
    let store = InMemoryCronScheduleStore::new();
    let row = store
        .start(cron_spec("nightly", StopCondition::Hook, stamp(3)))
        .expect("start");
    let active = store.active_all().expect("active_all");
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].cron_expr.as_bytes(), "*/5 * * * *".as_bytes());
    assert_eq!(active[0], row);
}

#[test]
fn jitter_custom_and_created_at_round_trip() {
    let store = InMemoryCronScheduleStore::new();
    let row = store
        .start(cron_spec("nightly", StopCondition::Hook, stamp(3)))
        .expect("start");
    assert_eq!(row.jitter_s, 1799);
    assert_eq!(
        row.custom,
        serde_json::json!({"goal": "x", "n": [1, {"y": null}]})
            .as_object()
            .expect("object")
            .clone()
    );
    assert_eq!(row.created_at, stamp(3));
}

#[test]
fn new_cron_schedule_requires_a_stop_condition() {
    let until = StopCondition::Until {
        at: stamp(9).into_inner(),
    };
    let value = serde_json::to_value(&until).expect("serialize until");
    assert_eq!(value["kind"], "until");
    assert!(value["at"].is_string());
    let hook_value = serde_json::to_value(StopCondition::Hook).expect("serialize hook");
    assert_eq!(hook_value, serde_json::json!({"kind": "hook"}));
    let spec = cron_spec("nightly", StopCondition::Hook, stamp(3));
    assert_eq!(spec.stop, StopCondition::Hook);
}

#[test]
fn record_stop_ends_the_row_once() {
    let store = InMemoryCronScheduleStore::new();
    let row = store
        .start(cron_spec("nightly", StopCondition::Hook, stamp(3)))
        .expect("start");
    assert!(
        store
            .record_stop(row.id, 1, stamp(4), "phase bead closed")
            .expect("stop")
    );
    let ended = store.list(None, None).expect("list");
    assert!(!ended[0].active);
    assert_eq!(ended[0].ended_at, Some(stamp(4)));
    assert_eq!(
        ended[0].last_outcome.as_deref(),
        Some("stopped: phase bead closed")
    );
    assert!(store.active_all().expect("active").is_empty());
    assert!(
        !store
            .record_stop(row.id, 1, stamp(5), "again")
            .expect("second stop")
    );
}

#[test]
fn start_after_end_updates_in_place_and_bumps_generation() {
    let store = InMemoryCronScheduleStore::new();
    let first = store
        .start(cron_spec("nightly", StopCondition::Hook, stamp(3)))
        .expect("start");
    store
        .end(&team(), &phase("bk"), Some("nightly"), stamp(4))
        .expect("end");
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
    let store = InMemoryCronScheduleStore::new();
    store
        .start(cron_spec("nightly", StopCondition::Hook, stamp(3)))
        .expect("start");
    let calls_before = store.calls.load(std::sync::atomic::Ordering::SeqCst);
    let writes_before = store.writes.load(std::sync::atomic::Ordering::SeqCst);
    let error = store
        .start(cron_spec("nightly", StopCondition::Hook, stamp(4)))
        .expect_err("duplicate");
    assert_eq!(error.code().as_str(), "ATM_MESSAGE_VALIDATION_FAILED");
    assert!(
        error
            .message()
            .contains("cron schedule nightly already active for phase bk")
    );
    assert!(
        error
            .message()
            .contains("run atm cron end --phase bk nightly first")
    );
    assert_eq!(
        store.calls.load(std::sync::atomic::Ordering::SeqCst),
        calls_before + 1
    );
    assert_eq!(
        store.writes.load(std::sync::atomic::Ordering::SeqCst),
        writes_before
    );
}

#[test]
fn record_with_stale_generation_changes_nothing() {
    let store = InMemoryCronScheduleStore::new();
    let first = store
        .start(cron_spec("nightly", StopCondition::Hook, stamp(3)))
        .expect("start");
    store
        .end(&team(), &phase("bk"), Some("nightly"), stamp(4))
        .expect("end");
    store
        .start(cron_spec("nightly", StopCondition::Hook, stamp(6)))
        .expect("restart");
    let before = store.list(None, None).expect("list");
    assert!(
        !store
            .record_run(first.id, 1, stamp(7), "ok")
            .expect("stale run")
    );
    assert!(
        !store
            .record_stop(first.id, 1, stamp(7), "stale")
            .expect("stale stop")
    );
    assert_eq!(store.list(None, None).expect("list"), before);
}

#[test]
fn record_run_on_ended_row_changes_nothing() {
    let store = InMemoryCronScheduleStore::new();
    let row = store
        .start(cron_spec("nightly", StopCondition::Hook, stamp(3)))
        .expect("start");
    store
        .end(&team(), &phase("bk"), Some("nightly"), stamp(4))
        .expect("end");
    let before: CronSchedule = store.list(None, None).expect("list").remove(0);
    assert!(
        !store
            .record_run(row.id, 1, stamp(5), "ok")
            .expect("run on ended")
    );
    assert_eq!(store.list(None, None).expect("list")[0], before);
}

#[test]
fn list_schedules_orders_active_first_then_created_at_then_phase() {
    let store = InMemoryCronScheduleStore::new();
    store
        .start(cron_spec("late", StopCondition::Hook, stamp(5)))
        .expect("late");
    store
        .start(cron_spec("early", StopCondition::Hook, stamp(2)))
        .expect("early");
    store
        .start(cron_spec("ended", StopCondition::Hook, stamp(1)))
        .expect("ended");
    store
        .end(&team(), &phase("bk"), Some("ended"), stamp(6))
        .expect("end");
    let names: Vec<String> = store
        .list(None, None)
        .expect("list")
        .into_iter()
        .map(|row| row.name)
        .collect();
    assert_eq!(names, ["early", "late", "ended"]);
}

#[test]
fn request_budget_constants_hold_their_values() {
    assert_eq!(HOOK_TIMEOUT_MAX_MS, 2500);
    assert_eq!(HOOK_ACTION_RESERVE_MS, 500);
    assert_eq!(CRON_TIMEOUT_MAX_MS, 30000);
}
