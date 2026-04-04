use std::fs;
use std::process::Command;

use atm_core::schema::{AgentMember, MessageEnvelope, TeamConfig};
use atm_core::types::IsoTimestamp;
use chrono::{Duration, Utc};
use serde_json::Value;
use uuid::Uuid;

#[test]
fn test_ack_transitions_pending_ack_and_appends_reply() {
    let fixture = Fixture::new(&["arch-ctm", "team-lead"]);
    let message_id = Uuid::new_v4();
    let mut message = fixture.message(
        "team-lead",
        "please ack",
        true,
        Some(Duration::minutes(5)),
        None,
        message_id,
    );
    message.task_id = Some("TASK-123".into());
    fixture.write_inbox("arch-ctm", &[message]);

    let output = fixture.run(&[
        "ack",
        &message_id.to_string(),
        "received and starting",
        "--json",
    ]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["action"], "ack");
    assert_eq!(parsed["team"], "atm-dev");
    assert_eq!(parsed["agent"], "arch-ctm");
    assert_eq!(parsed["message_id"], message_id.to_string());
    assert_eq!(parsed["task_id"], "TASK-123");
    assert_eq!(parsed["reply_target"], "team-lead@atm-dev");
    assert_eq!(parsed["reply_text"], "received and starting");
    assert!(parsed["reply_message_id"].as_str().is_some());

    let inbox = fixture.inbox_contents("arch-ctm");
    assert_eq!(inbox.len(), 1);
    assert!(inbox[0].read);
    assert!(inbox[0].pending_ack_at.is_none());
    assert!(inbox[0].acknowledged_at.is_some());

    let replies = fixture.inbox_contents("team-lead");
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].text, "received and starting");
    assert_eq!(replies[0].from, "arch-ctm");
    assert_eq!(replies[0].acknowledges_message_id, Some(message_id));
}

#[test]
fn test_ack_updates_origin_inbox_file() {
    let fixture = Fixture::new(&["arch-ctm", "team-lead"]);
    let message_id = Uuid::new_v4();
    fixture.write_origin_inbox(
        "arch-ctm",
        "host-a",
        &[fixture.message(
            "team-lead",
            "origin pending",
            true,
            Some(Duration::minutes(5)),
            None,
            message_id,
        )],
    );

    let output = fixture.run(&["ack", &message_id.to_string(), "got it", "--json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let origin = fixture.origin_inbox_contents("arch-ctm", "host-a");
    assert_eq!(origin.len(), 1);
    assert!(origin[0].pending_ack_at.is_none());
    assert!(origin[0].acknowledged_at.is_some());
}

#[test]
fn test_ack_rejects_already_acknowledged_message() {
    let fixture = Fixture::new(&["arch-ctm", "team-lead"]);
    let message_id = Uuid::new_v4();
    fixture.write_inbox(
        "arch-ctm",
        &[fixture.message(
            "team-lead",
            "already acked",
            true,
            None,
            Some(Duration::minutes(1)),
            message_id,
        )],
    );

    let output = fixture.run(&["ack", &message_id.to_string(), "duplicate"]);

    assert!(!output.status.success());
    assert!(
        fixture.stderr(&output).contains("already acknowledged"),
        "stderr: {}",
        fixture.stderr(&output)
    );
}

#[test]
fn test_ack_rejects_message_that_is_not_pending() {
    let fixture = Fixture::new(&["arch-ctm", "team-lead"]);
    let message_id = Uuid::new_v4();
    fixture.write_inbox(
        "arch-ctm",
        &[fixture.message("team-lead", "plain read", true, None, None, message_id)],
    );

    let output = fixture.run(&["ack", &message_id.to_string(), "nope"]);

    assert!(!output.status.success());
    assert!(
        fixture
            .stderr(&output)
            .contains("is not in the (read, pending_ack) state"),
        "stderr: {}",
        fixture.stderr(&output)
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
            .env("ATM_IDENTITY", "arch-ctm")
            .env("ATM_TEAM", "atm-dev")
            .current_dir(self.tempdir.path())
            .output()
            .expect("run atm")
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

    fn write_inbox(&self, agent: &str, messages: &[MessageEnvelope]) {
        let inbox_path = self.inbox_path(agent);
        if let Some(parent) = inbox_path.parent() {
            fs::create_dir_all(parent).expect("inbox dir");
        }
        let raw = messages
            .iter()
            .map(|message| serde_json::to_string(message).expect("json line"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(inbox_path, format!("{raw}\n")).expect("write inbox");
    }

    fn inbox_path(&self, agent: &str) -> std::path::PathBuf {
        self.team_dir()
            .join("inboxes")
            .join(format!("{agent}.json"))
    }

    fn inbox_contents(&self, agent: &str) -> Vec<MessageEnvelope> {
        let raw = fs::read_to_string(self.inbox_path(agent)).expect("inbox contents");
        raw.lines()
            .map(|line| serde_json::from_str(line).expect("json line"))
            .collect()
    }

    fn write_origin_inbox(&self, agent: &str, origin: &str, messages: &[MessageEnvelope]) {
        let inbox_path = self.origin_inbox_path(agent, origin);
        if let Some(parent) = inbox_path.parent() {
            fs::create_dir_all(parent).expect("origin inbox dir");
        }
        let raw = messages
            .iter()
            .map(|message| serde_json::to_string(message).expect("json line"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(inbox_path, format!("{raw}\n")).expect("write origin inbox");
    }

    fn origin_inbox_path(&self, agent: &str, origin: &str) -> std::path::PathBuf {
        self.team_dir()
            .join("inboxes")
            .join(format!("{agent}.{origin}.json"))
    }

    fn origin_inbox_contents(&self, agent: &str, origin: &str) -> Vec<MessageEnvelope> {
        let raw = fs::read_to_string(self.origin_inbox_path(agent, origin))
            .expect("origin inbox contents");
        raw.lines()
            .map(|line| serde_json::from_str(line).expect("json line"))
            .collect()
    }

    fn stdout_json(&self, output: &std::process::Output) -> Value {
        serde_json::from_slice(&output.stdout).expect("valid ack json")
    }

    fn stderr(&self, output: &std::process::Output) -> String {
        String::from_utf8(output.stderr.clone()).expect("stderr utf8")
    }

    fn team_dir(&self) -> std::path::PathBuf {
        self.tempdir
            .path()
            .join(".claude")
            .join("teams")
            .join("atm-dev")
    }

    fn message(
        &self,
        from: &str,
        text: &str,
        read: bool,
        pending_offset: Option<Duration>,
        acknowledged_offset: Option<Duration>,
        message_id: Uuid,
    ) -> MessageEnvelope {
        let timestamp = Utc::now() - Duration::minutes(30);
        MessageEnvelope {
            from: from.to_string(),
            text: text.to_string(),
            timestamp: timestamp.into(),
            read,
            source_team: Some("atm-dev".into()),
            summary: None,
            message_id: Some(message_id),
            pending_ack_at: pending_offset
                .map(|offset| IsoTimestamp::from_datetime(timestamp + offset)),
            acknowledged_at: acknowledged_offset
                .map(|offset| IsoTimestamp::from_datetime(timestamp + offset)),
            acknowledges_message_id: None,
            task_id: None,
            extra: serde_json::Map::new(),
        }
    }
}
