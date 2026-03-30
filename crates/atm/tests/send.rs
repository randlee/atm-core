use std::fs;
use std::process::Command;

use atm_core::schema::{AgentMember, MessageEnvelope, TeamConfig};

#[test]
fn test_send_creates_inbox_file() {
    let fixture = Fixture::new("recipient");

    let output = fixture.run(&[
        "send",
        "--to",
        "recipient@atm-dev",
        "--message",
        "hello from test",
    ]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    assert!(
        fixture
            .stdout(&output)
            .contains("Sent to recipient@atm-dev [message_id:"),
        "stdout: {}",
        fixture.stdout(&output)
    );

    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].body, "hello from test");
    assert_eq!(inbox[0].from, "arch-ctm");
}

#[test]
fn test_send_dry_run_no_file() {
    let fixture = Fixture::new("recipient");

    let output = fixture.run(&[
        "send",
        "--to",
        "recipient@atm-dev",
        "--message",
        "hello from test",
        "--dry-run",
        "--json",
    ]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("valid dry-run json");
    assert_eq!(parsed["action"], "send");
    assert_eq!(parsed["team"], "atm-dev");
    assert_eq!(parsed["agent"], "recipient");
    assert_eq!(parsed["message"], "hello from test");
    assert_eq!(parsed["dry_run"], true);
    assert_eq!(parsed["requires_ack"], false);

    assert!(!fixture.inbox_path("recipient").exists());
}

#[test]
fn test_send_json_output() {
    let fixture = Fixture::new("recipient");

    let output = fixture.run(&[
        "send",
        "--to",
        "recipient@atm-dev",
        "--message",
        "hello json",
        "--json",
    ]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("valid send json");
    assert_eq!(parsed["action"], "send");
    assert_eq!(parsed["team"], "atm-dev");
    assert_eq!(parsed["agent"], "recipient");
    assert_eq!(parsed["outcome"], "sent");
    assert_eq!(parsed["requires_ack"], false);
    assert!(parsed["message_id"].as_str().is_some());
}

#[test]
fn test_send_requires_ack() {
    let fixture = Fixture::new("recipient");

    let output = fixture.run(&[
        "send",
        "--to",
        "recipient@atm-dev",
        "--message",
        "please ack",
        "--ack",
    ]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
    assert!(inbox[0].requires_ack);
}

struct Fixture {
    tempdir: tempfile::TempDir,
}

impl Fixture {
    fn new(recipient: &str) -> Self {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let fixture = Self { tempdir };
        fixture.write_team_config(recipient);
        fixture
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_atm"))
            .args(args)
            .env("ATM_HOME", self.tempdir.path())
            .env("ATM_IDENTITY", "arch-ctm")
            .current_dir(self.tempdir.path())
            .output()
            .expect("run atm")
    }

    fn write_team_config(&self, recipient: &str) {
        let team_dir = self.tempdir.path().join("teams").join("atm-dev");
        fs::create_dir_all(&team_dir).expect("team dir");
        let config = TeamConfig {
            members: vec![AgentMember {
                name: recipient.to_string(),
            }],
        };
        fs::write(
            team_dir.join("config.json"),
            serde_json::to_vec(&config).expect("team config"),
        )
        .expect("write team config");
    }

    fn inbox_path(&self, recipient: &str) -> std::path::PathBuf {
        self.tempdir
            .path()
            .join("teams")
            .join("atm-dev")
            .join("inbox")
            .join(format!("{recipient}.jsonl"))
    }

    fn inbox_contents(&self, recipient: &str) -> Vec<MessageEnvelope> {
        let inbox_path = self.inbox_path(recipient);
        let raw = fs::read_to_string(&inbox_path).expect("inbox contents");
        raw.lines()
            .map(|line| serde_json::from_str(line).expect("json line"))
            .collect()
    }

    fn stdout(&self, output: &std::process::Output) -> String {
        String::from_utf8(output.stdout.clone()).expect("stdout utf8")
    }

    fn stderr(&self, output: &std::process::Output) -> String {
        String::from_utf8(output.stderr.clone()).expect("stderr utf8")
    }
}
