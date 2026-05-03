use std::fs;
use std::process::Command;

use atm_core::schema::{AgentMember, LegacyMessageId, MessageEnvelope, TeamConfig};
use atm_core::types::{AgentName, TeamName};
use atm_core::{read_messages, write_messages};
use chrono::{Duration, TimeZone, Utc};
use serde_json::Value;

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
    assert_eq!(inbox.len(), 4);
    let read_back = fixture.run(&["read", "--all", "--no-mark", "--json"]);
    assert!(
        read_back.status.success(),
        "stderr: {}",
        fixture.stderr(&read_back)
    );
    let projected = fixture.stdout_json(&read_back);
    let messages = projected["messages"].as_array().expect("messages array");
    assert_eq!(messages.len(), 2);
    assert!(messages.iter().any(|message| message["text"] == "unread"));
    assert!(messages.iter().any(|message| message["text"] == "pending"));
}

#[test]
fn test_clear_uses_default_team_from_workspace_config_for_sqlite_path() {
    let fixture = Fixture::new(&["arch-ctm"]);
    fixture.write_atm_config("[atm]\ndefault_team = \"atm-dev\"\n");
    fixture.write_inbox(
        "arch-ctm",
        &[fixture.message(
            "team-lead",
            "read",
            true,
            None,
            None,
            Utc::now() - Duration::days(1),
        )],
    );

    let output = fixture.run_with_env(&["clear", "--json"], &[("ATM_TEAM", "")]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["removed_total"], 1);
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
    let read_back = fixture.run(&["read", "--all", "--no-mark", "--json"]);
    assert!(
        read_back.status.success(),
        "stderr: {}",
        fixture.stderr(&read_back)
    );
    let projected = fixture.stdout_json(&read_back);
    assert_eq!(projected["count"], 1);
}

#[test]
fn test_clear_emits_retained_log_record() {
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

    let clear = fixture.run(&["clear", "--json"]);
    assert!(clear.status.success(), "stderr: {}", fixture.stderr(&clear));

    let output = fixture.run(&["log", "filter", "--match", "command=clear", "--json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    let records = parsed["records"].as_array().expect("records array");
    assert!(
        records.iter().any(|record| {
            record["fields"]["command"] == "clear"
                && record["fields"]["agent"] == "arch-ctm"
                && record["fields"]["team"] == "atm-dev"
        }),
        "stdout: {}",
        String::from_utf8(output.stdout.clone()).expect("stdout utf8")
    );
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
    assert!(
        fixture.inbox_contents("arch-ctm")[0]
            .pending_ack_at
            .is_some()
    );
}

#[test]
fn test_clear_already_cleared_message_is_idempotent() {
    let fixture = Fixture::new(&["arch-ctm"]);
    fixture.write_inbox(
        "arch-ctm",
        &[fixture.message(
            "team-lead",
            "read once",
            true,
            None,
            None,
            Utc::now() - Duration::days(2),
        )],
    );

    let first = fixture.run(&["clear", "--json"]);
    assert!(first.status.success(), "stderr: {}", fixture.stderr(&first));
    assert_eq!(fixture.stdout_json(&first)["removed_total"], 1);

    let second = fixture.run(&["clear", "--json"]);
    assert!(
        second.status.success(),
        "stderr: {}",
        fixture.stderr(&second)
    );
    let parsed = fixture.stdout_json(&second);
    assert_eq!(parsed["removed_total"], 0);
    let read_back = fixture.run(&["read", "--all", "--no-mark", "--json"]);
    assert!(
        read_back.status.success(),
        "stderr: {}",
        fixture.stderr(&read_back)
    );
    assert_eq!(fixture.stdout_json(&read_back)["count"], 0);
}

#[test]
fn test_clear_uses_workflow_sidecar_and_removes_cleared_entry() {
    let fixture = Fixture::new(&["arch-ctm"]);
    let message = fixture.message(
        "team-lead",
        "sidecar-managed read",
        false,
        None,
        None,
        Utc::now() - Duration::days(2),
    );
    let message_id = message.message_id.expect("message id");
    fixture.write_inbox("arch-ctm", &[message]);
    let workflow_key = fixture
        .inbox_contents("arch-ctm")
        .first()
        .and_then(|message| {
            message
                .atm_message_id()
                .map(|message_id| format!("atm:{message_id}"))
        })
        .unwrap_or_else(|| format!("legacy:{message_id}"));
    fixture.write_workflow_state(
        "arch-ctm",
        serde_json::json!({
            "messages": {
                workflow_key.clone(): {
                    "read": true
                }
            }
        }),
    );

    let output = fixture.run(&["clear", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    assert_eq!(fixture.inbox_contents("arch-ctm").len(), 1);
    let read_back = fixture.run(&["read", "--all", "--no-mark", "--json"]);
    assert!(
        read_back.status.success(),
        "stderr: {}",
        fixture.stderr(&read_back)
    );
    let projected = fixture.stdout_json(&read_back);
    assert_eq!(projected["count"], 0);
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
    assert_eq!(inbox.len(), 2);
    let read_back = fixture.run(&["read", "--all", "--no-mark", "--json"]);
    assert!(
        read_back.status.success(),
        "stderr: {}",
        fixture.stderr(&read_back)
    );
    let projected = fixture.stdout_json(&read_back);
    let messages = projected["messages"].as_array().expect("messages array");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["text"], "normal read");
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
    assert_eq!(inbox.len(), 2);
    assert_eq!(
        inbox[1].extra["futureField"],
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
    assert_eq!(inbox.len(), 2);
    let read_back = fixture.run(&["read", "--all", "--no-mark", "--json"]);
    assert!(
        read_back.status.success(),
        "stderr: {}",
        fixture.stderr(&read_back)
    );
    let projected = fixture.stdout_json(&read_back);
    assert_eq!(projected["count"], 1);
    assert_eq!(projected["messages"][0]["text"], "newer");
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
    assert_eq!(fixture.inbox_contents("agent-b").len(), 1);
    assert_eq!(fixture.inbox_contents("arch-ctm").len(), 1);
    let agent_b_read = fixture.run(&[
        "read",
        "agent-b",
        "--as",
        "arch-ctm",
        "--all",
        "--no-mark",
        "--json",
    ]);
    assert!(
        agent_b_read.status.success(),
        "stderr: {}",
        fixture.stderr(&agent_b_read)
    );
    assert_eq!(fixture.stdout_json(&agent_b_read)["count"], 0);
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

    assert_eq!(fixture.origin_inbox_contents("arch-ctm", "host-a").len(), 1);
    let read_back = fixture.run(&["read", "--all", "--no-mark", "--json"]);
    assert!(
        read_back.status.success(),
        "stderr: {}",
        fixture.stderr(&read_back)
    );
    assert_eq!(fixture.stdout_json(&read_back)["count"], 0);
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
        self.run_with_env(args, &[])
    }

    fn run_with_env(&self, args: &[&str], extra_env: &[(&str, &str)]) -> std::process::Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_atm"));
        command
            .args(args)
            .env("ATM_HOME", self.tempdir.path())
            .env("ATM_CONFIG_HOME", self.tempdir.path())
            .env("ATM_IDENTITY", "arch-ctm")
            .env("ATM_TEAM", "atm-dev")
            .current_dir(self.tempdir.path());
        for (key, value) in extra_env {
            command.env(key, value);
        }
        command.output().expect("run atm")
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

    fn write_atm_config(&self, raw: &str) {
        fs::write(self.tempdir.path().join(".atm.toml"), raw).expect("write .atm.toml");
    }

    fn write_inbox(&self, agent: &str, messages: &[MessageEnvelope]) {
        let inbox_path = self.inbox_path(agent);
        if let Some(parent) = inbox_path.parent() {
            fs::create_dir_all(parent).expect("inbox dir");
        }
        write_messages(&inbox_path, messages).expect("write inbox");
    }

    fn inbox_path(&self, agent: &str) -> std::path::PathBuf {
        self.team_dir()
            .join("inboxes")
            .join(format!("{agent}.json"))
    }

    fn inbox_contents(&self, agent: &str) -> Vec<MessageEnvelope> {
        read_messages(&self.inbox_path(agent)).expect("inbox contents")
    }

    fn write_workflow_state(&self, agent: &str, value: Value) {
        let path = self
            .team_dir()
            .join(".atm-state")
            .join("workflow")
            .join(format!("{agent}.json"));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("workflow dir");
        }
        fs::write(path, serde_json::to_vec(&value).expect("workflow json"))
            .expect("write workflow");
    }

    fn write_origin_inbox(&self, agent: &str, origin: &str, messages: &[MessageEnvelope]) {
        let inbox_path = self.origin_inbox_path(agent, origin);
        if let Some(parent) = inbox_path.parent() {
            fs::create_dir_all(parent).expect("origin inbox dir");
        }
        write_messages(&inbox_path, messages).expect("write origin inbox");
    }

    fn origin_inbox_path(&self, agent: &str, origin: &str) -> std::path::PathBuf {
        self.team_dir()
            .join("inboxes")
            .join(format!("{agent}.{origin}.json"))
    }

    fn origin_inbox_contents(&self, agent: &str, origin: &str) -> Vec<MessageEnvelope> {
        read_messages(&self.origin_inbox_path(agent, origin)).expect("origin inbox contents")
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
            from: from.parse::<AgentName>().expect("agent"),
            text: text.to_string(),
            timestamp: timestamp.into(),
            read,
            source_team: Some("atm-dev".parse::<TeamName>().expect("team")),
            summary: None,
            message_id: Some(LegacyMessageId::new()),
            pending_ack_at: pending_ack_at.map(Into::into),
            acknowledged_at: acknowledged_at.map(Into::into),
            acknowledges_message_id: None,
            task_id: None,
            extra: serde_json::Map::new(),
        }
    }
}

fn idle_notification_text(from: &str) -> String {
    let timestamp = Utc
        .with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
        .single()
        .expect("idle notification timestamp");
    serde_json::json!({
        "type": "idle_notification",
        "from": from,
        "timestamp": timestamp.to_rfc3339(),
        "idleReason": "available"
    })
    .to_string()
}
