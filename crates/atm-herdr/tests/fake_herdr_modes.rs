use std::path::Path;
use std::process::Command;

fn fake(mode: &str, arguments: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_fake-herdr"))
        .env("FAKE_HERDR_MODE", mode)
        .args(arguments)
        .output()
        .expect("fake-herdr must execute")
}

#[test]
fn fake_modes_have_portable_structured_output() {
    for mode in ["exit-success", "stdout-json-line", "v0.8.0-blocked-prompt"] {
        let output = fake(mode, &[]);
        assert!(
            output.stdout.ends_with(b"\n") || output.stderr.ends_with(b"\n"),
            "{mode} must use LF-delimited output"
        );
    }
    assert!(fake("exit-success", &[]).status.success());
    assert!(!fake("exit-failure", &[]).status.success());
    for code in [
        "server_not_running",
        "agent_prompt_stalled",
        "timeout",
        "protocol_mismatch",
    ] {
        let output = fake(&format!("stderr-envelope:{code}"), &[]);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains(code));
    }
}

#[test]
fn fake_echoes_argv_and_explicit_session() {
    let output = Command::new(env!("CARGO_BIN_EXE_fake-herdr"))
        .env("FAKE_HERDR_MODE", "echo-argv-and-herdr-session")
        .env("HERDR_SESSION", "session-two")
        .args(["agent", "get", "alice"])
        .output()
        .expect("fake-herdr must execute");
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON output");
    assert_eq!(value["result"]["herdr_session"], "session-two");
    assert_eq!(
        value["result"]["argv"],
        serde_json::json!(["agent", "get", "alice"])
    );
}

#[test]
fn replay_and_status_modes_load_versioned_fixtures() {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/herdr-versions/v0.8.2");
    let output = fake(&format!("replay:{}", fixture.display()), &[]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("future_field"));

    let status = fake("status-server-json:0.8.2,20,[]", &[]);
    assert!(status.status.success());
    assert!(String::from_utf8_lossy(&status.stdout).contains("0.8.2,20,[]"));
}
