#![cfg(all(unix, feature = "daemon-integration-tests"))]

mod support;

use std::fs;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use crate::support::{TEST_RECIPIENT, TEST_SENDER, TEST_TEAM, qualified};
use atm_core::schema::{AgentMember, TeamConfig};
use chrono::{Duration as ChronoDuration, Utc};

#[test]
fn test_log_snapshot_json_returns_recent_records() {
    let fixture = Fixture::new(&[TEST_SENDER, TEST_RECIPIENT]);
    fixture.send(&qualified(TEST_RECIPIENT), "hello snapshot");
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
    let fixture = Fixture::new(&[TEST_SENDER, TEST_RECIPIENT]);
    fixture.send(&qualified(TEST_RECIPIENT), "hello filter");
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
    let fixture = Fixture::new(&[TEST_SENDER, TEST_RECIPIENT]);
    fixture.send(&qualified(TEST_RECIPIENT), "hello level");
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
    let fixture = Fixture::new(&[TEST_SENDER, TEST_RECIPIENT]);
    fixture.send(&qualified(TEST_RECIPIENT), "hello since");
    let future = (Utc::now() + ChronoDuration::minutes(5)).to_rfc3339();

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
    let fixture = Fixture::new(&[TEST_SENDER, TEST_RECIPIENT]);
    fixture.send(&qualified(TEST_RECIPIENT), "hello combined");
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
    let fixture = Fixture::new(&[TEST_SENDER, TEST_RECIPIENT]);
    let mut tail = fixture.spawn_tail(&[
        "log",
        "tail",
        "--match",
        "command=send",
        "--json",
        "--poll-interval-ms",
        "25",
    ]);
    fixture.wait_for_tail_ready(&mut tail, &qualified(TEST_RECIPIENT));

    fixture.send(&qualified(TEST_RECIPIENT), "hello tail");
    let record = tail.read_record();
    assert_eq!(record["fields"]["command"], "send");
    tail.finish();
}

#[test]
fn test_log_help_lists_subcommands() {
    let fixture = Fixture::new(&[TEST_SENDER]);
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
    let fixture = Fixture::new(&[TEST_SENDER, TEST_RECIPIENT]);

    let failed = fixture.run(&["send", &qualified(TEST_RECIPIENT), "oops", "--stdin"]);
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
    let fixture = Fixture::new(&[TEST_SENDER, TEST_RECIPIENT]);

    let output = fixture.run(&["send", &qualified(TEST_RECIPIENT), "hello stdout", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["agent"], TEST_RECIPIENT);
    assert_eq!(parsed["team"], TEST_TEAM);
    assert!(
        fixture.stderr(&output).trim().is_empty(),
        "stderr: {}",
        fixture.stderr(&output)
    );
}

#[test]
fn test_send_routes_retained_console_logs_to_stderr_when_requested() {
    let fixture = Fixture::new(&[TEST_SENDER, TEST_RECIPIENT]);

    let output = fixture.run(&[
        "--stderr-logs",
        "send",
        &qualified(TEST_RECIPIENT),
        "hello stderr",
        "--json",
    ]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["agent"], TEST_RECIPIENT);
    assert_eq!(parsed["team"], TEST_TEAM);

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
        fixture.warm_daemon();
        fixture
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        self.run_with_env(args, &[])
    }

    fn run_with_env(&self, args: &[&str], extra_env: &[(&str, &str)]) -> std::process::Output {
        let mut first = Command::new(env!("CARGO_BIN_EXE_atm"));
        crate::support::configure_atm_command(&mut first, self.tempdir.path(), Some(TEST_SENDER))
            .args(args)
            .envs(extra_env.iter().copied())
            .current_dir(self.tempdir.path());
        let output = first.output().expect("run atm");
        if !crate::support::is_daemon_start_transient(&output) {
            return output;
        }

        std::thread::sleep(Duration::from_millis(50));
        let mut retry = Command::new(env!("CARGO_BIN_EXE_atm"));
        crate::support::configure_atm_command(&mut retry, self.tempdir.path(), Some(TEST_SENDER))
            .args(args)
            .envs(extra_env.iter().copied())
            .current_dir(self.tempdir.path())
            .output()
            .expect("retry atm")
    }

    fn warm_daemon(&self) {
        let output = self.run(&["read", "--all", "--no-mark", "--json"]);
        assert!(output.status.success(), "stderr: {}", self.stderr(&output));
    }

    fn spawn_tail(&self, args: &[&str]) -> TailReader {
        let mut command = Command::new(env!("CARGO_BIN_EXE_atm"));
        let mut child = crate::support::configure_atm_command(
            &mut command,
            self.tempdir.path(),
            Some(TEST_SENDER),
        )
        .args(args)
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
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut attempt = 0usize;
        while std::time::Instant::now() < deadline {
            self.send(target, &format!("tail readiness barrier {attempt}"));
            let probe = Duration::from_millis((50 * (attempt + 1) as u64).min(500));
            if let Some(record) = tail.try_read_record(probe) {
                assert_eq!(record["fields"]["command"], "send");
                return;
            }
            attempt += 1;
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
                .map(|member| AgentMember::with_name((*member).parse().expect("agent")))
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
            .join(TEST_TEAM)
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
        self.try_read_record(Duration::from_secs(30))
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
