use std::fs;
use std::process::Command;

use serde_json::json;
use tempfile::TempDir;

#[test]
fn test_log_json_output() {
    let fixture = Fixture::new(json!({
        "records": [
            record("2026-03-31T06:00:00Z", "info", "atm", "command.started", "started", json!({
                "command": "read",
                "team": "atm-dev"
            })),
            record("2026-03-31T06:00:05Z", "warn", "atm", "command.failed", "failed", json!({
                "command": "send",
                "team": "atm-dev"
            }))
        ]
    }));

    let output = fixture.run(&["log", "--json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid log json");
    assert_eq!(parsed["action"], "log");
    assert_eq!(parsed["follow"], false);
    assert_eq!(parsed["records"].as_array().map(Vec::len), Some(2));
    assert_eq!(parsed["records"][0]["level"], "warn");
    assert_eq!(parsed["records"][0]["fields"]["command"], "send");
}

#[test]
fn test_log_level_filter() {
    let fixture = Fixture::new(json!({
        "records": [
            record("2026-03-31T06:00:00Z", "info", "atm", "command.started", "started", json!({
                "command": "read"
            })),
            record("2026-03-31T06:00:05Z", "error", "atm", "command.failed", "failed", json!({
                "command": "send"
            }))
        ]
    }));

    let output = fixture.run(&["log", "--level", "error", "--json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid log json");
    assert_eq!(parsed["records"].as_array().map(Vec::len), Some(1));
    assert_eq!(parsed["records"][0]["level"], "error");
}

#[test]
fn test_log_field_filter() {
    let fixture = Fixture::new(json!({
        "records": [
            record("2026-03-31T06:00:00Z", "info", "atm", "command.started", "started", json!({
                "command": "read",
                "team": "atm-dev"
            })),
            record("2026-03-31T06:00:05Z", "info", "atm", "command.started", "started", json!({
                "command": "send",
                "team": "atm-dev"
            }))
        ]
    }));

    let output = fixture.run(&["log", "--filter", "command=read", "--json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid log json");
    assert_eq!(parsed["records"].as_array().map(Vec::len), Some(1));
    assert_eq!(parsed["records"][0]["fields"]["command"], "read");
}

#[test]
fn test_log_follow_mode_exits_when_fixture_stream_ends() {
    let fixture = Fixture::new(json!({
        "follow_records": [
            record("2026-03-31T06:00:10Z", "info", "atm", "command.started", "started", json!({
                "command": "read"
            })),
            record("2026-03-31T06:00:11Z", "warn", "atm", "command.failed", "failed", json!({
                "command": "read"
            }))
        ]
    }));

    let output = fixture.run(&["log", "--follow", "--json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let stdout = fixture.stdout(&output);
    let lines = stdout.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);

    let first: serde_json::Value = serde_json::from_str(lines[0]).expect("first json line");
    let second: serde_json::Value = serde_json::from_str(lines[1]).expect("second json line");
    assert_eq!(first["event"], "command.started");
    assert_eq!(second["level"], "warn");
}

fn record(
    timestamp: &str,
    level: &str,
    service: &str,
    event: &str,
    message: &str,
    fields: serde_json::Value,
) -> serde_json::Value {
    json!({
        "timestamp": timestamp,
        "level": level,
        "service": service,
        "event": event,
        "message": message,
        "fields": fields
    })
}

struct Fixture {
    tempdir: TempDir,
    log_fixture_path: std::path::PathBuf,
}

impl Fixture {
    fn new(log_fixture: serde_json::Value) -> Self {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let log_fixture_path = tempdir.path().join("log-fixture.json");
        fs::write(
            &log_fixture_path,
            serde_json::to_vec(&log_fixture).expect("log fixture"),
        )
        .expect("write log fixture");

        Self {
            tempdir,
            log_fixture_path,
        }
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_atm"))
            .args(args)
            .env("ATM_HOME", self.tempdir.path())
            .env("ATM_TEST_LOG_FIXTURE", &self.log_fixture_path)
            .current_dir(self.tempdir.path())
            .output()
            .expect("run atm")
    }

    fn stdout(&self, output: &std::process::Output) -> String {
        String::from_utf8(output.stdout.clone()).expect("stdout utf8")
    }

    fn stderr(&self, output: &std::process::Output) -> String {
        String::from_utf8(output.stderr.clone()).expect("stderr utf8")
    }
}
