//! Regression coverage for retained CLI command-error messages.

use std::process::Command;

use serde_json::Value;

#[test]
fn invalid_message_id_persists_code_and_stderr_message() {
    let fixture = tempfile::tempdir().expect("temporary ATM environment");
    let log_dir = fixture.path().join("logs");
    let output = Command::new(env!("CARGO_BIN_EXE_atm"))
        .args(["ack", "localhost", "received"])
        .env("ATM_HOME", fixture.path())
        .env("ATM_CONFIG_HOME", fixture.path().join("config"))
        .env("ATM_LOG_DIR", &log_dir)
        .env("ATM_TEAMS_DIR", fixture.path().join("teams"))
        .env("ATM_IDENTITY", "sender-a")
        .env("ATM_TEAM", "test-team")
        .output()
        .expect("run atm ack");

    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid message id: localhost"), "{stderr}");

    let log = std::fs::read_to_string(log_dir.join("atm.log.jsonl")).expect("retained log");
    let event = log
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("JSON log event"))
        .find(|event| event["fields"]["code"] == "ATM_MESSAGE_VALIDATION_FAILED")
        .expect("command validation error event");

    assert_eq!(
        event["message"].as_str(),
        Some(stderr.trim_end()),
        "persisted event must retain the same authoritative text as stderr: {event}"
    );
    assert_eq!(event["fields"]["code"], "ATM_MESSAGE_VALIDATION_FAILED");
}
