//! Public-contract fixtures for doctor telemetry diagnostics.

use atm_core::doctor::DoctorReport;
use serde_json::{Value, json};

const CREDENTIAL_SENTINEL: &str = "Bearer doctor-fixture-secret";
const MAX_DOCTOR_GUIDE_LINES: usize = 160;

fn doctor_1_10_fixture() -> Value {
    json!({
        "summary": {
            "status": "healthy",
            "message": "pinned 1.10 doctor fixture",
            "info_count": 0,
            "warning_count": 0,
            "error_count": 0
        },
        "findings": [],
        "recommendations": [],
        "environment": {
            "atm_home": null,
            "atm_team": null,
            "atm_identity": null,
            "team_override": null
        },
        "member_roster": null,
        "observability": {
            "active_log_path": null,
            "logging_state": "healthy",
            "query_state": "healthy",
            "maintenance": null,
            "diagnostic": null,
            "jsonl": {
                "forwarded_total": 0,
                "dropped_queue_full_total": 0,
                "dropped_reentrant_total": 0
            },
            "timeline": {
                "written_total": 0,
                "dropped_queue_full_total": 0,
                "dropped_persist_error_total": 0
            },
            "degraded": [],
            "detail": null
        },
        "config": { "findings": [] },
        "mail_store": { "findings": [] },
        "roster_store": { "findings": [] }
    })
}

fn export_fixture() -> Value {
    json!({
        "state": "unavailable",
        "endpoint": "https://collector.example:4317",
        "protocol": "grpc",
        "emitted": 13,
        "dropped_full": 2,
        "dropped_timeout": 3,
        "dropped_failure": 5,
        "dropped_shutdown": 7,
        "last_failure": "timed_out"
    })
}

#[test]
fn doctor_json_1_10_0_without_export_renders() {
    let fixture = doctor_1_10_fixture();
    let report: DoctorReport = serde_json::from_value(fixture.clone()).expect("1.10.0 fixture");

    let rendered = serde_json::to_value(report).expect("doctor JSON");
    assert!(
        rendered.pointer("/observability/export").is_none(),
        "the compatibility fixture must not invent an export object: {rendered}"
    );
    assert_eq!(rendered["observability"], fixture["observability"]);
    assert_eq!(rendered["summary"], fixture["summary"]);
}

#[test]
fn doctor_json_with_export_round_trips() {
    let mut export = export_fixture();
    export["authorization"] = json!(CREDENTIAL_SENTINEL);
    let mut fixture = doctor_1_10_fixture();
    fixture["observability"]["export"] = export;
    let report: DoctorReport = serde_json::from_value(fixture).expect("current doctor fixture");

    let rendered = serde_json::to_value(report).expect("doctor JSON");
    assert_eq!(rendered["observability"]["export"], export_fixture());
    let fields: Vec<&str> = rendered["observability"]["export"]
        .as_object()
        .expect("export object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        fields,
        [
            "dropped_failure",
            "dropped_full",
            "dropped_shutdown",
            "dropped_timeout",
            "emitted",
            "endpoint",
            "last_failure",
            "protocol",
            "state",
        ]
    );
    assert!(
        !rendered.to_string().contains(CREDENTIAL_SENTINEL),
        "doctor JSON must never expose telemetry credentials"
    );
}

#[test]
fn doctor_user_guide_stays_within_the_operator_readability_budget() {
    let guide = include_str!("../../../docs/user-documents/doctor-and-log.md");
    assert!(
        guide.lines().count() <= MAX_DOCTOR_GUIDE_LINES,
        "doctor guide exceeded its {MAX_DOCTOR_GUIDE_LINES}-line budget"
    );
}
