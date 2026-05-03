use std::fs;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use atm_core::schema::{AgentMember, TeamConfig};
use chrono::{Duration as ChronoDuration, Utc};

#[test]
fn test_log_snapshot_json_returns_recent_records() {
    let fixture = Fixture::new(&["arch-ctm", "recipient"]);
    fixture.send("recipient@atm-dev", "hello snapshot");
    assert!(
        fixture.active_log_path().is_file(),
        "expected retained log file at {}",
        fixture.active_log_path().display()
    );

    let output = fixture.run(&[
        "log",
        "snapshot",
        "--match",
        "command=send",
        "--since",
        "5m",
        "--limit",
        "10",
        "--json",
    ]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid json");
    let records = parsed["records"].as_array().expect("records array");
    assert!(!records.is_empty(), "stdout: {}", fixture.stdout(&output));
    assert_eq!(records[0]["fields"]["command"], "send");
    assert_eq!(records[0]["service"], "atm");
    assert_eq!(parsed["truncated"], false);
}

#[test]
fn test_log_filter_matches_structured_fields() {
    let fixture = Fixture::new(&["arch-ctm", "recipient"]);
    fixture.send("recipient@atm-dev", "hello filter");
    let _ = fixture.run(&["read", "--json"]);

    let output = fixture.run(&["log", "filter", "--match", "command=send", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid json");
    let records = parsed["records"].as_array().expect("records array");
    assert!(!records.is_empty(), "stdout: {}", fixture.stdout(&output));
    assert!(
        records
            .iter()
            .all(|record| record["fields"]["command"] == "send")
    );
}

#[test]
fn test_log_snapshot_filters_by_level() {
    let fixture = Fixture::new(&["arch-ctm", "recipient"]);
    fixture.send("recipient@atm-dev", "hello level");
    let _ = fixture.run(&["read", "--json"]);

    let output = fixture.run(&["log", "snapshot", "--level", "info", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    let records = parsed["records"].as_array().expect("records array");
    assert!(!records.is_empty(), "stdout: {}", fixture.stdout(&output));
    assert!(records.iter().all(|record| record["severity"] == "info"));
}

#[test]
fn test_log_snapshot_filters_by_since() {
    let fixture = Fixture::new(&["arch-ctm", "recipient"]);
    fixture.send("recipient@atm-dev", "hello since");
    let future = (Utc::now() + ChronoDuration::minutes(1)).to_rfc3339();

    let output = fixture.run(&["log", "snapshot", "--since", &future, "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    let records = parsed["records"].as_array().expect("records array");
    assert!(records.is_empty(), "stdout: {}", fixture.stdout(&output));
}

#[test]
fn test_log_filter_combines_level_and_match() {
    let fixture = Fixture::new(&["arch-ctm", "recipient"]);
    fixture.send("recipient@atm-dev", "hello combined");
    let _ = fixture.run(&["read", "--json"]);

    let output = fixture.run(&[
        "log",
        "filter",
        "--level",
        "info",
        "--match",
        "command=send",
        "--json",
    ]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    let records = parsed["records"].as_array().expect("records array");
    assert!(!records.is_empty(), "stdout: {}", fixture.stdout(&output));
    assert!(
        records
            .iter()
            .all(|record| record["severity"] == "info" && record["fields"]["command"] == "send")
    );
}

#[test]
fn test_log_tail_streams_new_records() {
    let fixture = Fixture::new(&["arch-ctm", "recipient"]);
    let mut tail = fixture.spawn_tail(&[
        "log",
        "tail",
        "--match",
        "command=send",
        "--json",
        "--poll-interval-ms",
        "25",
    ]);
    fixture.wait_for_tail_ready(&mut tail, "recipient@atm-dev");

    fixture.send("recipient@atm-dev", "hello tail");
    let record = tail.read_record();
    assert_eq!(record["fields"]["command"], "send");
    tail.finish();
}

#[test]
fn test_log_help_lists_subcommands() {
    let fixture = Fixture::new(&["arch-ctm"]);
    let output = fixture.run(&["log", "--help"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let stdout = fixture.stdout(&output);
    assert!(stdout.contains("snapshot"));
    assert!(stdout.contains("tail"));
    assert!(stdout.contains("filter"));
}

#[test]
fn test_invalid_send_logs_error_code_and_exits_nonzero() {
    let fixture = Fixture::new(&["arch-ctm", "recipient"]);

    let failed = fixture.run(&["send", "recipient@atm-dev", "oops", "--stdin"]);
    assert!(!failed.status.success());

    let output = fixture.run(&[
        "log",
        "filter",
        "--level",
        "error",
        "--match",
        "error_code=ATM_MESSAGE_VALIDATION_FAILED",
        "--json",
    ]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    let records = parsed["records"].as_array().expect("records array");
    assert!(!records.is_empty(), "stdout: {}", fixture.stdout(&output));
    assert!(
        records.iter().any(|record| {
            record["severity"] == "error"
                && record["fields"]["error_code"] == "ATM_MESSAGE_VALIDATION_FAILED"
                && record["fields"]["command"] == "atm"
        }),
        "stdout: {}",
        fixture.stdout(&output)
    );
}

#[test]
fn test_send_stdout_remains_clean_without_stderr_logs() {
    let fixture = Fixture::new(&["arch-ctm", "recipient"]);

    let output = fixture.run(&["send", "recipient@atm-dev", "hello stdout", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["agent"], "recipient");
    assert_eq!(parsed["team"], "atm-dev");
    assert!(
        fixture.stderr(&output).trim().is_empty(),
        "stderr: {}",
        fixture.stderr(&output)
    );
}

#[test]
fn test_send_routes_retained_console_logs_to_stderr_when_requested() {
    let fixture = Fixture::new(&["arch-ctm", "recipient"]);

    let output = fixture.run(&[
        "--stderr-logs",
        "send",
        "recipient@atm-dev",
        "hello stderr",
        "--json",
    ]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["agent"], "recipient");
    assert_eq!(parsed["team"], "atm-dev");

    let stderr = fixture.stderr(&output);
    assert!(
        stderr.contains("atm.command send ATM command send completed with outcome sent"),
        "stderr: {stderr}"
    );
}

struct Fixture {
    tempdir: tempfile::TempDir,
}

impl Fixture {
    fn new(members: &[&str]) -> Self {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let fixture = Self { tempdir };
        fixture.write_team_config(members);
        fixture
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_atm"))
            .args(args)
            .env("ATM_HOME", self.tempdir.path())
            .env("ATM_CONFIG_HOME", self.tempdir.path())
            .env("ATM_IDENTITY", "arch-ctm")
            .env("ATM_TEAM", "atm-dev")
            .current_dir(self.tempdir.path())
            .output()
            .expect("run atm")
    }

    fn spawn_tail(&self, args: &[&str]) -> TailReader {
        let mut child = Command::new(env!("CARGO_BIN_EXE_atm"))
            .args(args)
            .env("ATM_HOME", self.tempdir.path())
            .env("ATM_CONFIG_HOME", self.tempdir.path())
            .env("ATM_IDENTITY", "arch-ctm")
            .env("ATM_TEAM", "atm-dev")
            .current_dir(self.tempdir.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn atm");
        let stdout = child.stdout.take().expect("tail stdout");
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        if tx.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        TailReader { child, rx }
    }

    fn wait_for_tail_ready(&self, tail: &mut TailReader, target: &str) {
        for attempt in 0..20 {
            self.send(target, &format!("tail readiness barrier {attempt}"));
            if let Some(record) = tail.try_read_record(Duration::from_millis(250)) {
                assert_eq!(record["fields"]["command"], "send");
                return;
            }
        }

        panic!("tail never produced a readiness record after repeated barrier sends");
    }

    fn send(&self, target: &str, body: &str) {
        let output = self.run(&["send", target, body, "--json"]);
        assert!(output.status.success(), "stderr: {}", self.stderr(&output));
    }

    fn write_team_config(&self, members: &[&str]) {
        let team_dir = self.team_dir();
        fs::create_dir_all(&team_dir).expect("team dir");
        let config = TeamConfig {
            members: members
                .iter()
                .map(|member| AgentMember {
                    name: (*member).parse().expect("agent"),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        fs::write(
            team_dir.join("config.json"),
            serde_json::to_vec(&config).expect("team config"),
        )
        .expect("write team config");
    }

    fn team_dir(&self) -> std::path::PathBuf {
        self.tempdir
            .path()
            .join(".claude")
            .join("teams")
            .join("atm-dev")
    }

    fn active_log_path(&self) -> std::path::PathBuf {
        self.tempdir
            .path()
            .join(".local")
            .join("share")
            .join("logs")
            .join("atm.log.jsonl")
    }

    fn stdout(&self, output: &std::process::Output) -> String {
        String::from_utf8(output.stdout.clone()).expect("stdout utf8")
    }

    fn stdout_json(&self, output: &std::process::Output) -> serde_json::Value {
        serde_json::from_slice(&output.stdout).expect("valid json")
    }

    fn stderr(&self, output: &std::process::Output) -> String {
        String::from_utf8(output.stderr.clone()).expect("stderr utf8")
    }
}

struct TailReader {
    child: std::process::Child,
    rx: Receiver<String>,
}

impl TailReader {
    fn try_read_record(&mut self, timeout: Duration) -> Option<serde_json::Value> {
        loop {
            let line = match self.rx.recv_timeout(timeout) {
                Ok(line) => line,
                Err(mpsc::RecvTimeoutError::Timeout) => return None,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let _ = self.child.kill();
                    panic!("tail exited before producing enough output");
                }
            };
            if line.trim().is_empty() {
                continue;
            }
            return Some(serde_json::from_str(line.trim()).expect("json line"));
        }
    }

    fn read_record(&mut self) -> serde_json::Value {
        self.try_read_record(Duration::from_secs(5))
            .unwrap_or_else(|| {
                let _ = self.child.kill();
                panic!("tail timed out before producing enough output");
            })
    }

    fn finish(mut self) {
        self.child.kill().expect("kill tail");
        let _ = self.child.wait_with_output().expect("tail output");
    }
}
