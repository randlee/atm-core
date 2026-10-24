//! Deterministic integration coverage for the documented cron admission and
//! durable scheduling boundary.
//!
//! This target deliberately uses a test-owned SQLite path and fixed timestamps;
//! it never starts a worker or waits for a clock tick.

use std::{fs, path::PathBuf};

use atm_core::{
    boundary::{PhaseId, StopCondition},
    hook::CronStartRequest,
    types::IsoTimestamp,
};
use atm_runtime_test_support::open_isolated_sqlite_boundary;
use atm_storage::NewCronSchedule;
use serde_json::{Value, json};

const FIXTURE_ID: &str = "start-until-hook";
const REFUSAL_FIXTURE_ID: &str = "reject-invalid-custom";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn fixture(id: &str) -> Value {
    let manifest = repo_root().join("docs/user-documents/examples/cron/manifest.json");
    let document: Value = serde_json::from_slice(&fs::read(manifest).expect("read cron manifest"))
        .expect("parse cron manifest");
    document["commands"]
        .as_array()
        .expect("commands array")
        .iter()
        .find(|entry| entry["id"] == id)
        .unwrap_or_else(|| panic!("missing fixture {id}"))
        .clone()
}

fn documented_start_request(custom: Value) -> CronStartRequest {
    CronStartRequest {
        caller_identity: "creator".parse().expect("creator"),
        caller_team: "atm-dev".parse().expect("team"),
        phase: PhaseId::parse("bk").expect("phase"),
        name: "nightly".to_owned(),
        cron_expr: "0 */5 * * * *".to_owned(),
        module_path: "./cron.py".into(),
        cwd: ".".into(),
        args: vec!["status".to_owned(), "--json".to_owned()],
        timeout_ms: 30_000,
        stop: StopCondition::Hook,
        custom,
    }
}

fn new_schedule(spec: atm_core::boundary::CronStartSpec) -> NewCronSchedule {
    NewCronSchedule {
        team: spec.team,
        phase: spec.phase,
        name: spec.name.into(),
        cron_expr: spec.cron_expr,
        module_path: spec.module_path,
        cwd: spec.cwd,
        args: spec.args,
        timeout_ms: spec.timeout_ms,
        jitter_s: 0,
        custom: spec.custom,
        stop: spec.stop,
        created_by: spec.created_by,
        created_at: "2026-10-23T00:00:00Z"
            .parse::<IsoTimestamp>()
            .expect("timestamp"),
    }
}

#[test]
fn documented_cron_fixture_admits_and_restarts_on_an_isolated_sqlite_runtime() {
    let documented = fixture(FIXTURE_ID);
    assert_eq!(documented["valid"], true, "fixture remains a success case");
    assert_eq!(
        documented["argv"],
        json!([
            "cron",
            "start",
            "nightly",
            "--cron",
            "0 */5 * * * *",
            "--module",
            "./cron.py",
            "--phase",
            "bk",
            "--cwd",
            ".",
            "--until-hook",
            "--arg",
            "status",
            "--arg",
            "--json",
            "--custom",
            "{\"goal\":\"ship\"}"
        ]),
        "the test is bound to the BK11 documented fixture",
    );

    let spec = documented_start_request(json!({"goal": "ship"}))
        .into_spec()
        .expect("documented start request is admitted");
    let root = tempfile::tempdir().expect("test root");
    let runtime = open_isolated_sqlite_boundary(root.path()).expect("isolated SQLite runtime");
    let store = runtime
        .service_runtime
        .cron_schedule_store()
        .expect("runtime has cron schedule storage");

    let first = store
        .start(new_schedule(spec.clone()))
        .expect("start schedule");
    assert_eq!(first.generation.get(), 1);
    assert_eq!(
        first.custom,
        json!({"goal": "ship"}).as_object().unwrap().clone()
    );
    assert!(
        store.start(new_schedule(spec.clone())).is_err(),
        "duplicate active schedule is refused"
    );

    let ended = store
        .end(
            &spec.team,
            &spec.phase,
            Some(spec.name.as_str()),
            "2026-10-23T00:05:00Z".parse().expect("end timestamp"),
        )
        .expect("manual stop");
    assert_eq!(ended.len(), 1);
    assert!(!ended[0].active);

    let restarted = store
        .start(new_schedule(spec))
        .expect("restart ended schedule");
    assert_eq!(
        restarted.id, first.id,
        "restart preserves durable schedule identity"
    );
    assert_eq!(
        restarted.generation.get(),
        2,
        "restart advances the generation fence"
    );
}

#[test]
fn documented_non_object_custom_fixture_is_refused_before_durable_write() {
    let refusal = fixture(REFUSAL_FIXTURE_ID);
    assert_eq!(refusal["valid"], false, "fixture remains a refusal case");
    assert_eq!(refusal["checks"], json!(["custom-must-be-object"]));

    let error = documented_start_request(json!([1]))
        .into_spec()
        .expect_err("array custom data must not cross the cron boundary");
    assert!(error.detail().contains("custom"));
}
