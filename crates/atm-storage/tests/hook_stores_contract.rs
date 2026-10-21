//! Contract tests for the hook registration and cron schedule stores.
//!
//! Run against the in-memory doubles with fixed timestamps; bk-6 runs the
//! same cases against SQLite.

#![cfg(feature = "test-utils")]

use std::path::PathBuf;

use atm_storage::testing::{InMemoryCronScheduleStore, InMemoryHookRegistrationStore};
use atm_storage::types::{AgentName, IsoTimestamp, TeamName};
use atm_storage::{
    AtmErrorCode, CronGeneration, CronSchedule, CronScheduleStore, HookName, HookRegistration,
    HookRegistrationStore, NewCronSchedule, PhaseId, StopCondition,
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
    TeamName::from_validated("test-team")
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
        registered_by: AgentName::from_validated("test-agent"),
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
        created_by: AgentName::from_validated("test-agent"),
        created_at,
    }
}

fn phases(rows: &[HookRegistration]) -> Vec<String> {
    rows.iter()
        .map(|row| row.phase.as_str().to_owned())
        .collect()
}

fn generation(value: u32) -> CronGeneration {
    CronGeneration::new(value).expect("generation")
}

#[test]
fn disable_keeps_rows_inactive() {
    let store = InMemoryHookRegistrationStore::new();
    store
        .register(registration(
            "test-team",
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
            "test-team",
            "bj",
            "task.close",
            "/bj.py",
            stamp(1),
        ))
        .expect("bj");
    store
        .register(registration(
            "test-team",
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
            "test-team",
            "bj",
            "task.close",
            "/bj.py",
            stamp(1),
        ))
        .expect("bj");
    store
        .register(registration(
            "test-team",
            "bk",
            "task.close",
            "/bk.py",
            stamp(2),
        ))
        .expect("bk");
    let again = store
        .register(registration(
            "test-team",
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
            "test-team",
            "bj",
            "task.close",
            "/bj.py",
            stamp(1),
        ))
        .expect("bj");
    store
        .register(registration(
            "test-team",
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
            "test-team",
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
            "test-team",
            "bj",
            "task.close",
            "/a.py",
            stamp(1),
        ))
        .expect("close");
    store
        .register(registration(
            "test-team",
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
            "test-team",
            "bj",
            "task.close",
            "/a.py",
            stamp(1),
        ))
        .expect("bj");
    store
        .register(registration(
            "test-team",
            "bl",
            "task.close",
            "/a.py",
            stamp(2),
        ))
        .expect("bl");
    store
        .register(registration(
            "test-team",
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
            "test-team",
            "bl",
            "task.start",
            "/c.py",
            stamp(4),
        ))
        .expect("bl start");
    store
        .register(registration(
            "test-team",
            "bk",
            "task.start",
            "/c.py",
            stamp(4),
        ))
        .expect("bk start");
    let tied = store.active(&team(), &hook("task.start")).expect("active");
    assert_eq!(phases(&tied), ["bk", "bl"]);
    let listed = store.list(&team(), None).expect("list with tied times");
    assert_eq!(phases(&listed), ["bl", "bk", "bk", "bl", "bj"]);
}

#[test]
fn disable_twice_keeps_first_disabled_at() {
    let store = InMemoryHookRegistrationStore::new();
    store
        .register(registration(
            "test-team",
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
    let task_names = [
        "task.assign",
        "task.start",
        "task.close",
        "task.move",
        "task.list",
        "task.events",
    ];
    for name in task_names {
        assert!(HookName::parse_task_hook(name).is_ok(), "{name}");
        assert!(HookName::parse(name).is_ok(), "{name}");
    }
    for name in ["cron.tick", "cron.stop"] {
        assert!(HookName::parse(name).is_ok(), "{name}");
        let error = HookName::parse_task_hook(name).expect_err("not a task hook");
        assert_eq!(error.code().as_str(), "ATM_HOOK_MODULE_INVALID", "{name}");
        assert!(error.is_validation(), "{name}");
        assert!(error.message().contains(&task_names.join(", ")), "{name}");
    }
    for name in ["task.foo", "", "TASK.CLOSE"] {
        let error = HookName::parse(name).expect_err("unknown hook");
        let task_error = HookName::parse_task_hook(name).expect_err("unknown task hook");
        assert_eq!(error.code().as_str(), "ATM_HOOK_MODULE_INVALID", "{name}");
        assert!(error.is_validation(), "{name}");
        assert_eq!(task_error.code(), error.code(), "{name}");
        assert_eq!(task_error.message(), error.message(), "{name}");
        assert!(
            error.message().contains(
                "task.assign, task.start, task.close, task.move, task.list, task.events, cron.tick, cron.stop"
            ),
            "{name}: {}",
            error.message()
        );
    }
}

#[test]
fn cron_hook_names_have_infallible_constructors() {
    assert_eq!(HookName::cron_tick().as_str(), HookName::CRON_TICK);
    assert_eq!(HookName::cron_stop().as_str(), HookName::CRON_STOP);
}

#[test]
fn hook_name_deserialize_rejects_unknown() {
    let error = serde_json::from_str::<HookName>("\"task.foo\"").expect_err("unknown hook");
    assert!(error.to_string().contains("cron.tick, cron.stop"));
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
            "test-team",
            "bj",
            "task.close",
            "/a.py",
            stamp(1),
        ))
        .expect("a");
    store
        .register(registration(
            "test-team-z",
            "bj",
            "task.close",
            "/b.py",
            stamp(1),
        ))
        .expect("b");
    store
        .register(registration(
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
            ("test-team".to_owned(), "task.close".to_owned()),
            ("test-team-z".to_owned(), "task.close".to_owned())
        ]
    );
}

#[test]
fn active_all_order_equals_active_order_for_every_key() {
    let store = InMemoryHookRegistrationStore::new();
    store
        .register(registration(
            "test-team",
            "bk",
            "task.close",
            "/a.py",
            stamp(2),
        ))
        .expect("bk");
    store
        .register(registration(
            "test-team",
            "bj",
            "task.close",
            "/a.py",
            stamp(1),
        ))
        .expect("bj");
    store
        .register(registration(
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
                .register(registration(
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
            assert_eq!(phases(&per_key), ["bj", "bk", "bl"]);
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
fn record_stop_ends_the_row_once() {
    let store = InMemoryCronScheduleStore::new();
    let row = store
        .start(cron_spec("nightly", StopCondition::Hook, stamp(3)))
        .expect("start");
    assert!(
        store
            .record_stop(row.id, generation(1), stamp(4), "phase bead closed")
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
            .record_stop(row.id, generation(1), stamp(5), "again")
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
    assert_eq!(restarted.generation.get(), 2);
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
            .record_run(first.id, generation(1), stamp(7), "ok")
            .expect("stale run")
    );
    assert!(
        !store
            .record_stop(first.id, generation(1), stamp(7), "stale")
            .expect("stale stop")
    );
    assert_eq!(store.list(None, None).expect("list"), before);
}

#[test]
fn cron_write_counter_excludes_fenced_and_noop_calls() {
    use std::sync::atomic::Ordering;

    let store = InMemoryCronScheduleStore::new();
    let first = store
        .start(cron_spec("nightly", StopCondition::Hook, stamp(3)))
        .expect("start");
    assert_eq!(store.writes.load(Ordering::SeqCst), 1);
    assert!(
        store
            .record_run(first.id, generation(1), stamp(4), "ok")
            .unwrap()
    );
    assert_eq!(store.writes.load(Ordering::SeqCst), 2);
    assert_eq!(
        store
            .end(&team(), &phase("bk"), None, stamp(5))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(store.writes.load(Ordering::SeqCst), 3);
    assert!(
        store
            .end(&team(), &phase("bk"), None, stamp(6))
            .unwrap()
            .is_empty()
    );
    assert!(
        !store
            .record_run(first.id, generation(1), stamp(6), "ended")
            .unwrap()
    );
    assert!(
        !store
            .record_stop(first.id, generation(1), stamp(6), "ended")
            .unwrap()
    );
    assert_eq!(store.writes.load(Ordering::SeqCst), 3);

    let restarted = store
        .start(cron_spec("nightly", StopCondition::Hook, stamp(7)))
        .expect("restart");
    assert_eq!(restarted.id, first.id);
    assert_eq!(restarted.generation.get(), 2);
    assert_eq!(store.writes.load(Ordering::SeqCst), 4);
    assert!(
        !store
            .record_run(first.id, generation(1), stamp(8), "stale")
            .unwrap()
    );
    assert_eq!(store.writes.load(Ordering::SeqCst), 4);
    assert!(
        !store
            .record_stop(first.id, generation(1), stamp(8), "stale")
            .unwrap()
    );
    assert_eq!(store.writes.load(Ordering::SeqCst), 4);
    let missing = atm_storage::CronScheduleId::new(first.id.get() + 1);
    assert!(
        !store
            .record_run(missing, generation(2), stamp(8), "missing")
            .unwrap()
    );
    assert!(
        !store
            .record_stop(missing, generation(2), stamp(8), "missing")
            .unwrap()
    );
    assert_eq!(store.writes.load(Ordering::SeqCst), 4);
    assert!(
        store
            .record_stop(first.id, generation(2), stamp(9), "done")
            .unwrap()
    );
    assert_eq!(store.writes.load(Ordering::SeqCst), 5);
    assert_eq!(store.calls.load(Ordering::SeqCst), 12);
}

#[test]
fn active_all_failure_preserves_rows_and_write_count() {
    use std::sync::atomic::Ordering;

    let store = InMemoryCronScheduleStore::new();
    store
        .start(cron_spec("nightly", StopCondition::Hook, stamp(3)))
        .expect("start");
    let rows_before = store.list(None, None).expect("list before failure");
    let calls_before = store.calls.load(Ordering::SeqCst);
    let writes_before = store.writes.load(Ordering::SeqCst);
    store.fail_active_all.store(true, Ordering::SeqCst);

    let error = store.active_all().expect_err("injected active_all failure");

    assert_eq!(error.code(), AtmErrorCode::InternalError);
    assert_eq!(store.calls.load(Ordering::SeqCst), calls_before + 1);
    assert_eq!(store.writes.load(Ordering::SeqCst), writes_before);
    assert_eq!(
        store.list(None, None).expect("list after failure"),
        rows_before
    );
    assert_eq!(store.writes.load(Ordering::SeqCst), writes_before);
}

#[test]
fn write_failure_preserves_rows_and_write_count() {
    use std::sync::atomic::Ordering;

    let store = InMemoryCronScheduleStore::new();
    store
        .start(cron_spec("nightly", StopCondition::Hook, stamp(3)))
        .expect("start");
    let rows_before = store.list(None, None).expect("list before failure");
    let calls_before = store.calls.load(Ordering::SeqCst);
    let writes_before = store.writes.load(Ordering::SeqCst);
    store.fail_writes.store(true, Ordering::SeqCst);

    let error = store
        .end(&team(), &phase("bk"), Some("nightly"), stamp(4))
        .expect_err("injected write failure");

    assert_eq!(error.code(), AtmErrorCode::InternalError);
    assert_eq!(store.calls.load(Ordering::SeqCst), calls_before + 1);
    assert_eq!(store.writes.load(Ordering::SeqCst), writes_before);
    assert_eq!(
        store.list(None, None).expect("list after failure"),
        rows_before
    );
    assert_eq!(store.writes.load(Ordering::SeqCst), writes_before);
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
            .record_run(row.id, generation(1), stamp(5), "ok")
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
    let mut other_phase = cron_spec("zulu", StopCondition::Hook, stamp(2));
    other_phase.phase = phase("bj");
    store.start(other_phase).expect("earlier phase");
    store
        .start(cron_spec("alpha", StopCondition::Hook, stamp(2)))
        .expect("earlier name");
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
    assert_eq!(names, ["zulu", "alpha", "early", "late", "ended"]);
}
