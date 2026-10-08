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
fn pinned_1_10_doctor_json_renders_without_an_export_object() {
    let report: DoctorReport = serde_json::from_value(doctor_1_10_fixture()).expect("1.10 fixture");

    let rendered = serde_json::to_value(report).expect("doctor JSON");
    assert!(
        rendered.pointer("/observability/export").is_none(),
        "the compatibility fixture must not invent an export object: {rendered}"
    );
}

#[test]
fn current_doctor_json_preserves_every_export_field_without_credentials() {
    let mut fixture = doctor_1_10_fixture();
    fixture["observability"]["export"] = export_fixture();
    let report: DoctorReport = serde_json::from_value(fixture).expect("current doctor fixture");

    let rendered = serde_json::to_value(report).expect("doctor JSON");
    assert_eq!(rendered["observability"]["export"], export_fixture());
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
