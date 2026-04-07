use std::fs;
use std::process::Command;

use atm_core::schema::{AgentMember, MessageEnvelope, TeamConfig};

#[test]
fn test_send_creates_inbox_file() {
    let fixture = Fixture::new("recipient");

    let output = fixture.run(&["send", "recipient@atm-dev", "hello from test"]);

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
    assert_eq!(inbox[0].text, "hello from test");
    assert_eq!(inbox[0].from, "arch-ctm");
    assert!(inbox[0].message_id.is_some());
}

#[test]
fn test_send_dry_run_no_file() {
    let fixture = Fixture::new("recipient");

    let output = fixture.run(&[
        "send",
        "recipient@atm-dev",
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

    let output = fixture.run(&["send", "recipient@atm-dev", "hello json", "--json"]);

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

    let output = fixture.run(&["send", "recipient@atm-dev", "please ack", "--requires-ack"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
    assert!(inbox[0].pending_ack_at.is_some());
}

#[test]
fn test_send_persists_task_id() {
    let fixture = Fixture::new("recipient");

    let output = fixture.run(&[
        "send",
        "recipient@atm-dev",
        "task assignment",
        "--task-id",
        "TASK-123",
    ]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].task_id.as_deref(), Some("TASK-123"));
}

#[test]
fn test_send_supports_positional_message_with_file() {
    let fixture = Fixture::new("recipient");
    let attachment = fixture.tempdir.path().join("notes.txt");
    fs::write(&attachment, "attachment body").expect("attachment");

    let output = fixture.run(&[
        "send",
        "recipient@atm-dev",
        "context first",
        "--file",
        attachment.to_str().expect("attachment path"),
    ]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
    assert!(
        inbox[0]
            .text
            .starts_with("context first\n\nFile reference:")
    );
}

#[test]
fn test_send_tolerates_invalid_team_members_when_recipient_is_valid() {
    let fixture = Fixture::new("recipient");
    fixture.write_raw_team_config(
        r#"{
            "members": [
                {"name":"recipient"},
                {"broken": true},
                17
            ]
        }"#,
    );

    let output = fixture.run(&["send", "recipient@atm-dev", "hello despite bad siblings"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].text, "hello despite bad siblings");
}

#[test]
fn test_send_accepts_string_member_compatibility_form() {
    let fixture = Fixture::new("recipient");
    fixture.write_raw_team_config(r#"{"members":["recipient"]}"#);

    let output = fixture.run(&["send", "recipient@atm-dev", "hello legacy"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].text, "hello legacy");
}

#[test]
fn test_send_reports_actionable_error_for_malformed_team_config() {
    let fixture = Fixture::new("recipient");
    fixture.write_raw_team_config(r#"{"members":[{"name":"recipient"}"#);

    let output = fixture.run(&["send", "recipient@atm-dev", "hello"]);

    assert!(!output.status.success());
    let stderr = fixture.stderr(&output);
    assert!(stderr.contains("failed to parse team config"));
    assert!(stderr.contains("config.json"));
    assert!(stderr.contains("Repair the JSON syntax in config.json"));
}

#[test]
fn test_send_missing_config_uses_existing_inbox_fallback_and_warns_sender() {
    let fixture = Fixture::new("recipient");
    fs::remove_file(fixture.team_dir().join("config.json")).expect("remove config");
    fixture.write_inbox("recipient", &[]);
    fixture.write_inbox("team-lead", &[]);

    let output = fixture.run(&["send", "recipient@atm-dev", "hello fallback"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let stdout = fixture.stdout(&output);
    assert!(stdout.contains("Sent to recipient@atm-dev"));
    assert!(stdout.contains("warning: team config is missing"));

    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].text, "hello fallback");

    let notices = fixture.inbox_contents("team-lead");
    assert_eq!(notices.len(), 1);
    assert!(
        notices[0]
            .text
            .contains("ATM warning: send used existing inbox fallback")
    );
}

#[test]
fn test_send_missing_config_deduplicates_team_lead_notice() {
    let fixture = Fixture::new("recipient");
    fs::remove_file(fixture.team_dir().join("config.json")).expect("remove config");
    fixture.write_inbox("recipient", &[]);
    fixture.write_inbox("team-lead", &[]);

    let first = fixture.run(&["send", "recipient@atm-dev", "first"]);
    assert!(first.status.success(), "stderr: {}", fixture.stderr(&first));

    let second = fixture.run(&["send", "recipient@atm-dev", "second"]);
    assert!(
        second.status.success(),
        "stderr: {}",
        fixture.stderr(&second)
    );

    let notices = fixture.inbox_contents("team-lead");
    assert_eq!(notices.len(), 1);
}

#[test]
fn test_send_missing_config_deduplicates_team_lead_notice_under_concurrency() {
    let fixture = Fixture::new("recipient");
    fs::remove_file(fixture.team_dir().join("config.json")).expect("remove config");
    fixture.write_inbox("recipient", &[]);
    fixture.write_inbox("team-lead", &[]);

    let (first, second) = std::thread::scope(|scope| {
        let first = scope.spawn(|| fixture.run(&["send", "recipient@atm-dev", "first"]));
        let second = scope.spawn(|| fixture.run(&["send", "recipient@atm-dev", "second"]));
        (
            first.join().expect("first send"),
            second.join().expect("second send"),
        )
    });

    assert!(first.status.success(), "stderr: {}", fixture.stderr(&first));
    assert!(
        second.status.success(),
        "stderr: {}",
        fixture.stderr(&second)
    );
    let notices = fixture.inbox_contents("team-lead");
    assert_eq!(notices.len(), 1);
}

#[test]
fn test_send_missing_config_notice_resets_after_config_is_restored() {
    let fixture = Fixture::new("recipient");
    fs::remove_file(fixture.team_dir().join("config.json")).expect("remove config");
    fixture.write_inbox("recipient", &[]);
    fixture.write_inbox("team-lead", &[]);

    let first = fixture.run(&["send", "recipient@atm-dev", "first"]);
    assert!(first.status.success(), "stderr: {}", fixture.stderr(&first));
    assert_eq!(fixture.inbox_contents("team-lead").len(), 1);

    fixture.write_team_config("recipient");
    let second = fixture.run(&["send", "recipient@atm-dev", "with config restored"]);
    assert!(
        second.status.success(),
        "stderr: {}",
        fixture.stderr(&second)
    );
    assert_eq!(fixture.inbox_contents("team-lead").len(), 1);

    fs::remove_file(fixture.team_dir().join("config.json")).expect("remove config again");
    let third = fixture.run(&["send", "recipient@atm-dev", "broken again"]);
    assert!(third.status.success(), "stderr: {}", fixture.stderr(&third));
    assert_eq!(fixture.inbox_contents("team-lead").len(), 2);
}

#[test]
fn test_send_missing_config_fails_when_recipient_inbox_does_not_exist() {
    let fixture = Fixture::new("recipient");
    fs::remove_file(fixture.team_dir().join("config.json")).expect("remove config");

    let output = fixture.run(&["send", "recipient@atm-dev", "hello"]);

    assert!(!output.status.success());
    let stderr = fixture.stderr(&output);
    assert!(stderr.contains("team config is missing"));
    assert!(stderr.contains("cannot safely proceed"));
    assert!(stderr.contains("Restore config.json"));
}

#[test]
fn test_send_missing_config_does_not_block_when_team_lead_inbox_is_absent() {
    let fixture = Fixture::new("recipient");
    fs::remove_file(fixture.team_dir().join("config.json")).expect("remove config");
    fixture.write_inbox("recipient", &[]);

    let output = fixture.run(&["send", "recipient@atm-dev", "hello fallback"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
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
            .env("ATM_TEAM", "atm-dev")
            .current_dir(self.tempdir.path())
            .output()
            .expect("run atm")
    }

    fn write_team_config(&self, recipient: &str) {
        let team_dir = self
            .tempdir
            .path()
            .join(".claude")
            .join("teams")
            .join("atm-dev");
        fs::create_dir_all(&team_dir).expect("team dir");
        let config = TeamConfig {
            members: vec![AgentMember {
                name: recipient.to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };
        fs::write(
            team_dir.join("config.json"),
            serde_json::to_vec(&config).expect("team config"),
        )
        .expect("write team config");
    }

    fn write_raw_team_config(&self, raw: &str) {
        let team_dir = self
            .tempdir
            .path()
            .join(".claude")
            .join("teams")
            .join("atm-dev");
        fs::create_dir_all(&team_dir).expect("team dir");
        fs::write(team_dir.join("config.json"), raw).expect("write raw team config");
    }

    fn inbox_path(&self, recipient: &str) -> std::path::PathBuf {
        self.tempdir
            .path()
            .join(".claude")
            .join("teams")
            .join("atm-dev")
            .join("inboxes")
            .join(format!("{recipient}.json"))
    }

    fn write_inbox(&self, recipient: &str, messages: &[MessageEnvelope]) {
        let inbox_path = self.inbox_path(recipient);
        if let Some(parent) = inbox_path.parent() {
            fs::create_dir_all(parent).expect("inbox dir");
        }
        let raw = if messages.is_empty() {
            String::new()
        } else {
            format!(
                "{}\n",
                messages
                    .iter()
                    .map(|message| serde_json::to_string(message).expect("json line"))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        };
        fs::write(inbox_path, raw).expect("write inbox");
    }

    fn inbox_contents(&self, recipient: &str) -> Vec<MessageEnvelope> {
        let inbox_path = self.inbox_path(recipient);
        let raw = fs::read_to_string(&inbox_path).expect("inbox contents");
        if raw.trim().is_empty() {
            return Vec::new();
        }
        raw.lines()
            .map(|line| serde_json::from_str(line).expect("json line"))
            .collect()
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
