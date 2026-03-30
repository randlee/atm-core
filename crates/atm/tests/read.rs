use std::fs;
use std::process::Command;

use atm_core::schema::{AgentMember, MessageEnvelope, TeamConfig};
use chrono::{TimeZone, Utc};
use serde_json::Value;
use uuid::Uuid;

#[test]
fn test_read_own_inbox_default() {
    let fixture = Fixture::new(&["arch-ctm", "recipient"]);
    fixture.write_inbox(
        "arch-ctm",
        &[fixture.message("team-lead", "hello", false, None, None, 0)],
    );

    let output = fixture.run(&["read", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["action"], "read");
    assert_eq!(parsed["agent"], "arch-ctm");
    assert_eq!(parsed["count"], 1);
    assert_eq!(parsed["bucket_counts"]["unread"], 1);
    assert_eq!(parsed["messages"][0]["bucket"], "unread");
}

#[test]
fn test_read_marks_read() {
    let fixture = Fixture::new(&["arch-ctm"]);
    fixture.write_inbox(
        "arch-ctm",
        &[fixture.message("team-lead", "hello", false, None, None, 0)],
    );

    let output = fixture.run(&["read", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let inbox = fixture.inbox_contents("arch-ctm");
    assert!(inbox[0].read);
}

#[test]
fn test_read_ack_activation() {
    let fixture = Fixture::new(&["arch-ctm"]);
    fixture.write_inbox(
        "arch-ctm",
        &[fixture.message("team-lead", "hello", false, None, None, 0)],
    );

    let output = fixture.run(&["read", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let inbox = fixture.inbox_contents("arch-ctm");
    assert!(inbox[0].pending_ack_at.is_some());
}

#[test]
fn test_read_no_mark() {
    let fixture = Fixture::new(&["arch-ctm"]);
    fixture.write_inbox(
        "arch-ctm",
        &[fixture.message("team-lead", "hello", false, None, None, 0)],
    );

    let output = fixture.run(&["read", "--no-mark", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let inbox = fixture.inbox_contents("arch-ctm");
    assert!(inbox[0].read);
    assert!(inbox[0].pending_ack_at.is_none());
}

#[test]
fn test_read_unread_only() {
    let fixture = Fixture::new(&["arch-ctm"]);
    fixture.write_inbox(
        "arch-ctm",
        &[
            fixture.message("team-lead", "unread", false, None, None, 2),
            fixture.message("team-lead", "pending", true, Some(1), None, 1),
            fixture.message("team-lead", "history", true, None, None, 0),
        ],
    );

    let output = fixture.run(&["read", "--unread-only", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["count"], 1);
    assert_eq!(parsed["messages"][0]["text"], "unread");
    assert_eq!(parsed["bucket_counts"]["unread"], 1);
    assert_eq!(parsed["bucket_counts"]["pending_ack"], 1);
    assert_eq!(parsed["bucket_counts"]["history"], 1);
}

#[test]
fn test_read_json_output() {
    let fixture = Fixture::new(&["arch-ctm"]);
    fixture.write_inbox(
        "arch-ctm",
        &[fixture.message("team-lead", "hello", false, None, None, 0)],
    );

    let output = fixture.run(&["read", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["action"], "read");
    assert_eq!(parsed["team"], "atm-dev");
    assert_eq!(parsed["history_collapsed"], false);
    assert_eq!(parsed["messages"][0]["from"], "team-lead");
}

#[test]
fn test_read_seen_state() {
    let fixture = Fixture::new(&["arch-ctm"]);
    fixture.write_inbox(
        "arch-ctm",
        &[
            fixture.message("team-lead", "history", true, None, None, 0),
            fixture.message("team-lead", "new unread", false, None, None, 10),
        ],
    );
    fixture.write_seen_state("arch-ctm", fixture.timestamp(5));

    let output = fixture.run(&["read", "--history", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["count"], 1);
    assert_eq!(parsed["messages"][0]["text"], "new unread");
    assert_eq!(parsed["bucket_counts"]["history"], 0);
}

#[test]
fn test_read_limit() {
    let fixture = Fixture::new(&["arch-ctm"]);
    fixture.write_inbox(
        "arch-ctm",
        &[
            fixture.message("team-lead", "first", false, None, None, 0),
            fixture.message("team-lead", "second", false, None, None, 1),
        ],
    );

    let output = fixture.run(&["read", "--limit", "1", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["count"], 1);
    assert_eq!(parsed["messages"][0]["text"], "second");
}

#[test]
fn test_read_timeout_with_existing_pending_ack_returns_immediately() {
    let fixture = Fixture::new(&["arch-ctm"]);
    fixture.write_inbox(
        "arch-ctm",
        &[fixture.message("team-lead", "pending", true, Some(0), None, 0)],
    );

    let start = std::time::Instant::now();
    let output = fixture.run(&["read", "--timeout", "5", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["count"], 1);
    assert_eq!(parsed["messages"][0]["bucket"], "pending_ack");
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

    fn write_seen_state(&self, agent: &str, timestamp: chrono::DateTime<Utc>) {
        let path = self.team_dir().join(".seen").join(agent);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("seen dir");
        }
        fs::write(path, timestamp.to_rfc3339()).expect("write seen state");
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

    fn stdout_json(&self, output: &std::process::Output) -> Value {
        serde_json::from_slice(&output.stdout).expect("valid read json")
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

    fn timestamp(&self, seconds: i64) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
            .single()
            .expect("timestamp")
            + chrono::Duration::seconds(seconds)
    }

    fn message(
        &self,
        from: &str,
        text: &str,
        read: bool,
        pending_ack_offset: Option<i64>,
        acknowledged_offset: Option<i64>,
        timestamp_offset: i64,
    ) -> MessageEnvelope {
        MessageEnvelope {
            from: from.to_string(),
            text: text.to_string(),
            timestamp: self.timestamp(timestamp_offset),
            read,
            source_team: Some("atm-dev".into()),
            summary: None,
            message_id: Some(Uuid::new_v4()),
            pending_ack_at: pending_ack_offset.map(|offset| self.timestamp(offset)),
            acknowledged_at: acknowledged_offset.map(|offset| self.timestamp(offset)),
            acknowledges_message_id: None,
            extra: serde_json::Map::new(),
        }
    }
}
