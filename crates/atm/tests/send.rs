use std::fs;
use std::process::Command;
use std::time::{Duration, Instant};

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
    assert!(inbox[0]
        .text
        .starts_with("context first\n\nFile reference:"));
}

#[test]
fn test_send_invokes_post_send_hook_after_successful_write() {
    let fixture = Fixture::new("recipient");
    let hook_output = fixture.tempdir.path().join("hook-output.txt");
    fixture.write_repo_config(&format!(
        "identity = \"arch-ctm\"\ndefault_team = \"atm-dev\"\n\n[agents.recipient]\npost_send = \"printf '%s\\n%s\\n%s\\n%s\\n' \\\"$ATM_SENDER\\\" \\\"$ATM_RECIPIENT\\\" \\\"$ATM_MESSAGE_BODY\\\" \\\"$ATM_MESSAGE_ID\\\" > '{}'\"\n",
        shell_escape_path(&hook_output),
    ));

    let output = fixture.run(&["send", "recipient@atm-dev", "hello hook"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let lines = wait_for_lines(&hook_output, 4);
    assert_eq!(lines[0], "arch-ctm");
    assert_eq!(lines[1], "recipient");
    assert_eq!(lines[2], "hello hook");
    assert!(!lines[3].is_empty());

    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
    assert_eq!(
        inbox[0].message_id.map(|id| id.to_string()),
        Some(lines[3].clone())
    );
}

#[test]
fn test_send_dry_run_skips_post_send_hook() {
    let fixture = Fixture::new("recipient");
    let hook_output = fixture.tempdir.path().join("hook-dry-run.txt");
    fixture.write_repo_config(&format!(
        "identity = \"arch-ctm\"\ndefault_team = \"atm-dev\"\n\n[agents.recipient]\npost_send = \"printf 'hook-ran' > '{}'\"\n",
        shell_escape_path(&hook_output),
    ));

    let output = fixture.run(&["send", "recipient@atm-dev", "hello hook", "--dry-run"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    assert!(!hook_output.exists(), "hook output should not exist");
    assert!(!fixture.inbox_path("recipient").exists());
}

#[test]
fn test_send_ignores_post_send_spawn_errors() {
    let fixture = Fixture::new("recipient");
    fixture.write_repo_config(
        "identity = \"arch-ctm\"\ndefault_team = \"atm-dev\"\n\n[agents.recipient]\npost_send = \"echo should-not-run\"\n",
    );

    let output = fixture.run_with_env(
        &["send", "recipient@atm-dev", "hello hook"],
        &[("PATH", "")],
    );

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].text, "hello hook");
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
        self.run_with_env(args, &[])
    }

    fn run_with_env(&self, args: &[&str], envs: &[(&str, &str)]) -> std::process::Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_atm"));
        command
            .args(args)
            .env("ATM_HOME", self.tempdir.path())
            .env("ATM_IDENTITY", "arch-ctm")
            .env("ATM_TEAM", "atm-dev")
            .current_dir(self.tempdir.path());

        for (key, value) in envs {
            command.env(key, value);
        }

        command.output().expect("run atm")
    }

    fn write_repo_config(&self, contents: &str) {
        fs::write(self.tempdir.path().join(".atm.toml"), contents).expect("repo config");
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
            .join(".claude")
            .join("teams")
            .join("atm-dev")
            .join("inboxes")
            .join(format!("{recipient}.json"))
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

fn wait_for_lines(path: &std::path::Path, expected_lines: usize) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if let Ok(contents) = fs::read_to_string(path) {
            let lines: Vec<String> = contents.lines().map(ToOwned::to_owned).collect();
            if lines.len() >= expected_lines {
                return lines;
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }

    panic!("timed out waiting for hook output at {}", path.display());
}

fn shell_escape_path(path: &std::path::Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .replace('\'', "'\"'\"'")
}
