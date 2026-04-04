use std::fs;
use std::process::Command;

use atm_core::schema::{AgentMember, MessageEnvelope, TeamConfig};
use atm_core::types::IsoTimestamp;
use chrono::{Duration, Utc};
use serde_json::Value;
use uuid::Uuid;

#[test]
fn test_clear_default_removes_only_read_and_acknowledged() {
    let fixture = Fixture::new(&["arch-ctm"]);
    fixture.write_inbox(
        "arch-ctm",
        &[
            fixture.message(
                "team-lead",
                "unread",
                false,
                None,
                None,
                Utc::now() - Duration::days(10),
            ),
            fixture.message(
                "team-lead",
                "pending",
                true,
                Some(Utc::now() - Duration::days(9)),
                None,
                Utc::now() - Duration::days(9),
            ),
            fixture.message(
                "team-lead",
                "read",
                true,
                None,
                None,
                Utc::now() - Duration::days(8),
            ),
            fixture.message(
                "team-lead",
                "acknowledged",
                true,
                None,
                Some(Utc::now() - Duration::days(7)),
                Utc::now() - Duration::days(7),
            ),
        ],
    );

    let output = fixture.run(&["clear", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["action"], "clear");
    assert_eq!(parsed["removed_total"], 2);
    assert_eq!(parsed["remaining_total"], 2);
    assert_eq!(parsed["removed_by_class"]["read"], 1);
    assert_eq!(parsed["removed_by_class"]["acknowledged"], 1);
    assert!(parsed["removed_by_class"]["unread"].is_null());
    assert!(parsed["removed_by_class"]["pending_ack"].is_null());

    let inbox = fixture.inbox_contents("arch-ctm");
    assert_eq!(inbox.len(), 2);
    assert_eq!(inbox[0].text, "unread");
    assert_eq!(inbox[1].text, "pending");
}

#[test]
fn test_clear_dry_run_does_not_mutate() {
    let fixture = Fixture::new(&["arch-ctm"]);
    fixture.write_inbox(
        "arch-ctm",
        &[fixture.message(
            "team-lead",
            "read",
            true,
            None,
            None,
            Utc::now() - Duration::days(3),
        )],
    );

    let output = fixture.run(&["clear", "--dry-run", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["removed_total"], 1);

    let inbox = fixture.inbox_contents("arch-ctm");
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].text, "read");
}

#[test]
fn test_clear_never_removes_pending_ack() {
    let fixture = Fixture::new(&["arch-ctm"]);
    fixture.write_inbox(
        "arch-ctm",
        &[fixture.message(
            "team-lead",
            "pending",
            true,
            Some(Utc::now() - Duration::days(2)),
            None,
            Utc::now() - Duration::days(2),
        )],
    );

    let output = fixture.run(&["clear", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["removed_total"], 0);
    assert_eq!(fixture.inbox_contents("arch-ctm").len(), 1);
    assert!(fixture.inbox_contents("arch-ctm")[0]
        .pending_ack_at
        .is_some());
}

#[test]
fn test_clear_idle_only_removes_only_idle_notifications() {
    let fixture = Fixture::new(&["arch-ctm"]);
    fixture.write_inbox(
        "arch-ctm",
        &[
            fixture.message(
                "team-lead",
                &idle_notification_text("team-lead"),
                true,
                None,
                None,
                Utc::now() - Duration::days(4),
            ),
            fixture.message(
                "team-lead",
                "normal read",
                true,
                None,
                None,
                Utc::now() - Duration::days(4),
            ),
        ],
    );

    let output = fixture.run(&["clear", "--idle-only", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["removed_total"], 1);
    assert_eq!(parsed["removed_by_class"]["read"], 1);

    let inbox = fixture.inbox_contents("arch-ctm");
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].text, "normal read");
}

#[test]
fn test_clear_preserves_unknown_fields_on_retained_messages() {
    let fixture = Fixture::new(&["arch-ctm"]);
    let mut retained = fixture.message(
        "team-lead",
        "pending",
        true,
        Some(Utc::now() - Duration::days(2)),
        None,
        Utc::now() - Duration::days(2),
    );
    retained
        .extra
        .insert("futureField".into(), serde_json::json!({"nested": true}));

    fixture.write_inbox(
        "arch-ctm",
        &[
            fixture.message(
                "team-lead",
                "clearable",
                true,
                None,
                None,
                Utc::now() - Duration::days(3),
            ),
            retained,
        ],
    );

    let output = fixture.run(&["clear", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let inbox = fixture.inbox_contents("arch-ctm");
    assert_eq!(inbox.len(), 1);
    assert_eq!(
        inbox[0].extra["futureField"],
        serde_json::json!({"nested": true})
    );
}

#[test]
fn test_clear_older_than_filters_candidates() {
    let fixture = Fixture::new(&["arch-ctm"]);
    fixture.write_inbox(
        "arch-ctm",
        &[
            fixture.message(
                "team-lead",
                "older",
                true,
                None,
                None,
                Utc::now() - Duration::days(10),
            ),
            fixture.message(
                "team-lead",
                "newer",
                true,
                None,
                None,
                Utc::now() - Duration::hours(6),
            ),
        ],
    );

    let output = fixture.run(&["clear", "--older-than", "7d", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["removed_total"], 1);

    let inbox = fixture.inbox_contents("arch-ctm");
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].text, "newer");
}

#[test]
fn test_clear_explicit_target() {
    let fixture = Fixture::new(&["arch-ctm", "agent-b"]);
    fixture.write_inbox(
        "arch-ctm",
        &[fixture.message(
            "team-lead",
            "keep mine",
            true,
            None,
            None,
            Utc::now() - Duration::days(10),
        )],
    );
    fixture.write_inbox(
        "agent-b",
        &[fixture.message(
            "team-lead",
            "clear agent b",
            true,
            None,
            None,
            Utc::now() - Duration::days(10),
        )],
    );

    let output = fixture.run(&["clear", "agent-b", "--as", "arch-ctm", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["agent"], "agent-b");
    assert_eq!(parsed["removed_total"], 1);
    assert_eq!(fixture.inbox_contents("agent-b").len(), 0);
    assert_eq!(fixture.inbox_contents("arch-ctm").len(), 1);
}

#[test]
fn test_clear_removes_from_origin_inbox_file() {
    let fixture = Fixture::new(&["arch-ctm"]);
    fixture.write_origin_inbox(
        "arch-ctm",
        "host-a",
        &[fixture.message(
            "team-lead",
            "origin read",
            true,
            None,
            None,
            Utc::now() - Duration::days(8),
        )],
    );

    let output = fixture.run(&["clear", "--json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    assert_eq!(fixture.origin_inbox_contents("arch-ctm", "host-a").len(), 0);
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
        serde_json::from_slice(&output.stdout).expect("valid clear json")
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
        pending_ack_at: Option<chrono::DateTime<Utc>>,
        acknowledged_at: Option<chrono::DateTime<Utc>>,
        timestamp: chrono::DateTime<Utc>,
    ) -> MessageEnvelope {
        MessageEnvelope {
            from: from.to_string(),
            text: text.to_string(),
            timestamp: timestamp.into(),
            read,
            source_team: Some("atm-dev".into()),
            summary: None,
            message_id: Some(Uuid::new_v4()),
            pending_ack_at: pending_ack_at.map(Into::into),
            acknowledged_at: acknowledged_at.map(Into::into),
            acknowledges_message_id: None,
            task_id: None,
            extra: serde_json::Map::new(),
        }
    }
}

fn idle_notification_text(from: &str) -> String {
    serde_json::json!({
        "type": "idle_notification",
        "from": from,
        "timestamp": IsoTimestamp::now().into_inner().to_rfc3339(),
        "idleReason": "available"
    })
    .to_string()
}
