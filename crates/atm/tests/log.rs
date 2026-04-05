use std::fs;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use atm_core::schema::{AgentMember, TeamConfig};

#[test]
fn test_log_snapshot_json_returns_recent_records() {
    let fixture = Fixture::new(&["arch-ctm", "recipient"]);
    fixture.send("recipient@atm-dev", "hello snapshot");

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
fn test_log_tail_streams_new_records() {
    let fixture = Fixture::new(&["arch-ctm", "recipient"]);
    let child = fixture.spawn(&[
        "log",
        "tail",
        "--match",
        "command=send",
        "--json",
        "--poll-interval-ms",
        "25",
        "--max-polls",
        "20",
    ]);

    thread::sleep(Duration::from_millis(100));
    fixture.send("recipient@atm-dev", "hello tail");

    let output = child.wait_with_output().expect("tail output");
    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let stdout = fixture.stdout(&output);
    let records = stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("json line"))
        .collect::<Vec<_>>();
    assert!(!records.is_empty(), "stdout: {stdout}");
    assert!(
        records
            .iter()
            .any(|record| record["fields"]["command"] == "send")
    );
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
            .env("ATM_IDENTITY", "arch-ctm")
            .env("ATM_TEAM", "atm-dev")
            .current_dir(self.tempdir.path())
            .output()
            .expect("run atm")
    }

    fn spawn(&self, args: &[&str]) -> std::process::Child {
        Command::new(env!("CARGO_BIN_EXE_atm"))
            .args(args)
            .env("ATM_HOME", self.tempdir.path())
            .env("ATM_IDENTITY", "arch-ctm")
            .env("ATM_TEAM", "atm-dev")
            .current_dir(self.tempdir.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn atm")
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
                    name: (*member).to_string(),
                    ..Default::default()
                })
                .collect(),
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

    fn stdout(&self, output: &std::process::Output) -> String {
        String::from_utf8(output.stdout.clone()).expect("stdout utf8")
    }

    fn stderr(&self, output: &std::process::Output) -> String {
        String::from_utf8(output.stderr.clone()).expect("stderr utf8")
    }
}
