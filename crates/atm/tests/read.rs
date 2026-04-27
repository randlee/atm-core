use std::fs;
use std::process::Command;

use atm_core::schema::{
    AgentMember, AtmMessageId, AtmMetadataFields, ForwardMetadataEnvelope, LegacyMessageId,
    MessageEnvelope, MessageMetadata, TeamConfig,
};
use atm_core::types::IsoTimestamp;
use chrono::{TimeZone, Utc};
use serde_json::Value;

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
    let message = fixture.message("team-lead", "hello", false, None, None, 0);
    let workflow_key = format!("legacy:{}", message.message_id.expect("message id"));
    fixture.write_inbox("arch-ctm", &[message]);

    let output = fixture.run(&["read", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let inbox = fixture.inbox_contents("arch-ctm");
    assert!(!inbox[0].read);
    let workflow = fixture
        .workflow_state_contents("arch-ctm")
        .expect("workflow state");
    assert_eq!(workflow["messages"][&workflow_key]["read"], true);
}

#[test]
fn test_read_ack_activation() {
    let fixture = Fixture::new(&["arch-ctm"]);
    let message = fixture.message("team-lead", "hello", false, None, None, 0);
    let workflow_key = format!("legacy:{}", message.message_id.expect("message id"));
    fixture.write_inbox("arch-ctm", &[message]);

    let output = fixture.run(&["read", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let inbox = fixture.inbox_contents("arch-ctm");
    assert!(inbox[0].pending_ack_at.is_none());
    let workflow = fixture
        .workflow_state_contents("arch-ctm")
        .expect("workflow state");
    assert!(
        workflow["messages"][&workflow_key]["pendingAckAt"]
            .as_str()
            .is_some()
    );
}

#[test]
fn test_read_no_mark() {
    let fixture = Fixture::new(&["arch-ctm"]);
    let message = fixture.message("team-lead", "hello", false, None, None, 0);
    let workflow_key = format!("legacy:{}", message.message_id.expect("message id"));
    fixture.write_inbox("arch-ctm", &[message]);

    let output = fixture.run(&["read", "--no-mark", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let inbox = fixture.inbox_contents("arch-ctm");
    assert!(!inbox[0].read);
    assert!(inbox[0].pending_ack_at.is_none());
    let workflow = fixture
        .workflow_state_contents("arch-ctm")
        .expect("workflow state");
    assert_eq!(workflow["messages"][&workflow_key]["read"], true);
    assert!(workflow["messages"][&workflow_key]["pendingAckAt"].is_null());
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
    assert_eq!(parsed["count"].as_u64(), Some(1));
    assert!(parsed["bucket_counts"]["unread"].as_u64().is_some());
    assert!(parsed["bucket_counts"]["pending_ack"].as_u64().is_some());
    assert!(parsed["bucket_counts"]["history"].as_u64().is_some());
    assert_eq!(parsed["messages"][0]["from"], "team-lead");
}

#[test]
fn test_read_emits_retained_log_record() {
    let fixture = Fixture::new(&["arch-ctm"]);
    fixture.write_inbox(
        "arch-ctm",
        &[fixture.message("team-lead", "hello", false, None, None, 0)],
    );

    let read = fixture.run(&["read", "--json"]);
    assert!(read.status.success(), "stderr: {}", fixture.stderr(&read));

    let output = fixture.run(&["log", "filter", "--match", "command=read", "--json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    let records = parsed["records"].as_array().expect("records array");
    assert!(
        records.iter().any(|record| {
            record["fields"]["command"] == "read"
                && record["fields"]["agent"] == "arch-ctm"
                && record["fields"]["team"] == "atm-dev"
        }),
        "stdout: {}",
        String::from_utf8(output.stdout.clone()).expect("stdout utf8")
    );
}

#[test]
fn test_read_missing_team_config_fails_with_actionable_error() {
    let fixture = Fixture::new(&["arch-ctm"]);
    fs::remove_file(fixture.team_dir().join("config.json")).expect("remove config");

    let output = fixture.run(&["read", "--json"]);

    assert!(!output.status.success());
    let stderr = fixture.stderr(&output);
    assert!(stderr.contains("team config is missing"));
    assert!(stderr.contains("Restore config.json"));
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
    assert_eq!(parsed["bucket_counts"]["history"], 1);
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

#[test]
fn test_read_pending_ack_only() {
    let fixture = Fixture::new(&["arch-ctm"]);
    fixture.write_inbox(
        "arch-ctm",
        &[fixture.message("team-lead", "pending", true, Some(0), None, 0)],
    );

    let output = fixture.run(&["read", "--pending-ack-only", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["count"], 1);
    assert_eq!(parsed["messages"][0]["bucket"], "pending_ack");
}

#[test]
fn test_read_all_flag() {
    let fixture = Fixture::new(&["arch-ctm"]);
    fixture.write_inbox(
        "arch-ctm",
        &[
            fixture.message("sender-a", "unread", false, None, None, 0),
            fixture.message("sender-b", "pending", true, Some(1), None, 1),
            fixture.message("sender-c", "history", true, None, None, 2),
        ],
    );

    let output = fixture.run(&["read", "--all", "--no-mark", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["count"], 3);
}

#[test]
fn test_read_no_update_seen() {
    let fixture = Fixture::new(&["arch-ctm"]);
    fixture.write_inbox(
        "arch-ctm",
        &[fixture.message("team-lead", "history", true, None, None, 10)],
    );
    let initial = fixture.timestamp(0);
    fixture.write_seen_state("arch-ctm", initial);

    let output = fixture.run(&["read", "--history", "--no-update-seen", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    assert_eq!(fixture.read_seen_state("arch-ctm"), Some(initial));
}

#[test]
fn test_read_from_filter() {
    let fixture = Fixture::new(&["arch-ctm"]);
    fixture.write_inbox(
        "arch-ctm",
        &[
            fixture.message("sender-a", "alpha", false, None, None, 0),
            fixture.message("sender-b", "beta", false, None, None, 1),
        ],
    );

    let output = fixture.run(&["read", "--from", "sender-a", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["count"], 1);
    assert_eq!(parsed["messages"][0]["from"], "sender-a");
    assert_eq!(parsed["bucket_counts"]["unread"], 2);
}

#[test]
fn test_read_deduplicates_unread_idle_notifications_per_sender() {
    let fixture = Fixture::new(&["arch-ctm"]);
    fixture.write_inbox(
        "arch-ctm",
        &[
            fixture.message(
                "daemon",
                &idle_notification_text("team-lead", "available"),
                false,
                None,
                None,
                0,
            ),
            fixture.message(
                "daemon",
                &idle_notification_text("team-lead", "available"),
                false,
                None,
                None,
                1,
            ),
            fixture.message("team-lead", "normal unread", false, None, None, 2),
        ],
    );

    let output = fixture.run(&["read", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["count"], 2);
    assert_eq!(parsed["bucket_counts"]["unread"], 2);
    let messages = parsed["messages"].as_array().expect("messages array");
    assert_eq!(messages[0]["text"], "normal unread");
    assert!(
        messages
            .iter()
            .filter(|message| message["text"] == idle_notification_text("team-lead", "available"))
            .count()
            == 1
    );
}

#[test]
fn test_read_deduplicates_idle_notifications_per_sender_only() {
    let fixture = Fixture::new(&["arch-ctm"]);
    fixture.write_inbox(
        "arch-ctm",
        &[
            fixture.message(
                "daemon",
                &idle_notification_text("sender-a", "available"),
                false,
                None,
                None,
                0,
            ),
            fixture.message(
                "daemon",
                &idle_notification_text("sender-a", "available"),
                false,
                None,
                None,
                1,
            ),
            fixture.message(
                "daemon",
                &idle_notification_text("sender-b", "available"),
                false,
                None,
                None,
                2,
            ),
            fixture.message(
                "daemon",
                &idle_notification_text("sender-b", "available"),
                false,
                None,
                None,
                3,
            ),
        ],
    );

    let output = fixture.run(&["read", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    let messages = parsed["messages"].as_array().expect("messages array");
    assert_eq!(messages.len(), 2);
    assert_eq!(parsed["bucket_counts"]["unread"], 2);
    assert_eq!(
        messages
            .iter()
            .filter(|message| message["text"] == idle_notification_text("sender-a", "available"))
            .count(),
        1
    );
    assert_eq!(
        messages
            .iter()
            .filter(|message| message["text"] == idle_notification_text("sender-b", "available"))
            .count(),
        1
    );
}

#[test]
fn test_read_keeps_read_idle_notifications_visible() {
    let fixture = Fixture::new(&["arch-ctm"]);
    fixture.write_inbox(
        "arch-ctm",
        &[
            fixture.message(
                "daemon",
                &idle_notification_text("team-lead", "available"),
                true,
                None,
                None,
                0,
            ),
            fixture.message(
                "daemon",
                &idle_notification_text("team-lead", "available"),
                true,
                None,
                None,
                1,
            ),
            fixture.message(
                "daemon",
                &idle_notification_text("team-lead", "available"),
                false,
                None,
                None,
                2,
            ),
        ],
    );

    let output = fixture.run(&["read", "--all", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    let messages = parsed["messages"].as_array().expect("messages array");
    assert_eq!(parsed["count"], 3);
    assert_eq!(parsed["bucket_counts"]["unread"], 1);
    assert_eq!(parsed["bucket_counts"]["history"], 2);
    assert_eq!(
        messages
            .iter()
            .filter(|message| message["text"] == idle_notification_text("team-lead", "available"))
            .count(),
        3
    );
}

#[test]
fn test_read_preserves_none_message_id_records_in_output() {
    let fixture = Fixture::new(&["arch-ctm"]);
    let mut without_id = fixture.message("team-lead", "no id", false, None, None, 0);
    without_id.message_id = None;
    let duplicate_id = LegacyMessageId::new();
    let mut older = fixture.message("team-lead", "older dup", false, None, None, 1);
    older.message_id = Some(duplicate_id);
    let mut newer = fixture.message("team-lead", "newer dup", false, None, None, 2);
    newer.message_id = Some(duplicate_id);
    fixture.write_inbox("arch-ctm", &[without_id, older, newer]);

    let output = fixture.run(&["read", "--all", "--no-mark", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    let messages = parsed["messages"].as_array().expect("messages array");
    assert_eq!(messages.len(), 2);
    assert!(messages.iter().any(|message| message["text"] == "no id"));
    assert!(
        messages
            .iter()
            .any(|message| message["text"] == "newer dup")
    );
}

#[test]
fn test_read_accepts_json_array_inbox_without_message_id() {
    let fixture = Fixture::new(&["arch-ctm"]);
    fixture.write_raw_inbox(
        "arch-ctm",
        &serde_json::json!([
            {
                "from": "team-lead",
                "text": "array without id",
                "timestamp": "2026-03-30T00:00:00Z",
                "read": false
            }
        ])
        .to_string(),
    );

    let output = fixture.run(&["read", "--all", "--no-mark", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["count"], 1);
    assert_eq!(parsed["messages"][0]["text"], "array without id");
    assert!(parsed["messages"][0]["message_id"].is_null());
}

#[test]
fn test_read_accepts_json_array_inbox_with_message_id() {
    let fixture = Fixture::new(&["arch-ctm"]);
    let message = fixture.message("team-lead", "array with id", false, None, None, 0);
    fixture.write_raw_inbox(
        "arch-ctm",
        &serde_json::to_string(&vec![message.clone()]).expect("json"),
    );

    let output = fixture.run(&["read", "--all", "--no-mark", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["count"], 1);
    assert_eq!(parsed["messages"][0]["text"], "array with id");
    assert_eq!(
        parsed["messages"][0]["message_id"],
        message.message_id.unwrap().to_string()
    );
}

#[test]
fn test_read_keeps_read_and_unread_idle_notifications_from_different_files() {
    let fixture = Fixture::new(&["arch-ctm"]);
    fixture.write_inbox(
        "arch-ctm",
        &[fixture.message(
            "daemon",
            &idle_notification_text("team-lead", "available"),
            false,
            None,
            None,
            1,
        )],
    );
    fixture.write_origin_inbox(
        "arch-ctm",
        "host-a",
        &[fixture.message(
            "daemon",
            &idle_notification_text("team-lead", "available"),
            true,
            None,
            None,
            0,
        )],
    );

    let output = fixture.run(&["read", "--all", "--no-mark", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    let messages = parsed["messages"].as_array().expect("messages array");
    assert_eq!(
        messages
            .iter()
            .filter(|message| message["text"] == idle_notification_text("team-lead", "available"))
            .count(),
        2
    );
    assert_eq!(parsed["bucket_counts"]["unread"], 1);
    assert_eq!(parsed["bucket_counts"]["history"], 1);
}

#[test]
fn test_read_logs_malformed_idle_notification_json_without_dropping_valid_records() {
    let fixture = Fixture::new(&["arch-ctm"]);
    fixture.write_inbox(
        "arch-ctm",
        &[
            fixture.message(
                "daemon",
                r#"{"type":"idle_notification","from":"team-lead""#,
                false,
                None,
                None,
                0,
            ),
            fixture.message("team-lead", "normal unread", false, None, None, 1),
        ],
    );

    let output = fixture.run_with_env(
        &["--stderr-logs", "read", "--all", "--no-mark", "--json"],
        &[("ATM_LOG", "debug")],
    );

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["count"], 2);
    let messages = parsed["messages"].as_array().expect("messages array");
    assert!(
        messages
            .iter()
            .any(|message| message["text"] == "normal unread")
    );
    assert!(
        messages.iter().any(|message| {
            message["text"] == r#"{"type":"idle_notification","from":"team-lead""#
        })
    );
    let stderr = fixture.stderr(&output);
    assert!(
        stderr.contains("ignoring malformed idle-notification JSON while classifying read surface"),
        "stderr: {stderr}"
    );
}

#[test]
fn test_read_logs_idle_notification_missing_sender_without_changing_mailbox_state() {
    let fixture = Fixture::new(&["arch-ctm"]);
    let malformed = serde_json::json!({
        "type": "idle_notification",
        "timestamp": "2026-03-30T00:00:00Z",
        "idleReason": "available"
    })
    .to_string();
    fixture.write_inbox(
        "arch-ctm",
        &[
            fixture.message("daemon", &malformed, false, None, None, 0),
            fixture.message("team-lead", "normal unread", false, None, None, 1),
        ],
    );
    let before = fixture.inbox_contents("arch-ctm");

    let output = fixture.run_with_env(
        &["--stderr-logs", "read", "--all", "--no-mark", "--json"],
        &[("ATM_LOG", "debug")],
    );

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    assert_eq!(fixture.inbox_contents("arch-ctm"), before);
    let stderr = fixture.stderr(&output);
    assert!(
        stderr.contains("ignoring malformed idle-notification payload missing string `from`"),
        "stderr: {stderr}"
    );
}

#[test]
fn test_forward_metadata_message_id_timestamp_matches_persisted_timestamp() {
    let (message_id, timestamp) = AtmMessageId::new_with_timestamp();
    let envelope = ForwardMetadataEnvelope {
        timestamp,
        metadata: MessageMetadata {
            atm: Some(AtmMetadataFields {
                message_id: Some(message_id),
                source_team: Some("atm-dev".into()),
                pending_ack_at: None,
                acknowledged_at: None,
                acknowledges_message_id: None,
                alert_kind: None,
                missing_config_path: None,
                extra: serde_json::Map::new(),
            }),
            extra: serde_json::Map::new(),
        },
    };

    assert_eq!(
        envelope
            .metadata
            .atm
            .expect("atm metadata")
            .message_id
            .expect("message id")
            .timestamp(),
        envelope.timestamp
    );
}

#[test]
fn test_read_mutual_exclusion() {
    let fixture = Fixture::new(&["arch-ctm"]);

    let output = fixture.run(&["read", "--all", "--unread-only"]);

    assert!(!output.status.success());
}

#[test]
fn test_read_timeout_expiry() {
    let fixture = Fixture::new(&["arch-ctm"]);

    let output = fixture.run(&["read", "--timeout", "0", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["count"], 0);
}

#[test]
fn test_read_no_since_last_seen_wins() {
    let fixture = Fixture::new(&["arch-ctm"]);
    fixture.write_inbox(
        "arch-ctm",
        &[fixture.message("team-lead", "history", true, None, None, 0)],
    );
    fixture.write_seen_state("arch-ctm", fixture.timestamp(10));

    let output = fixture.run(&[
        "read",
        "--history",
        "--since-last-seen",
        "--no-since-last-seen",
        "--json",
    ]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["count"], 1);
    assert_eq!(parsed["messages"][0]["bucket"], "history");
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
        Command::new(env!("CARGO_BIN_EXE_atm"))
            .args(args)
            .env("ATM_HOME", self.tempdir.path())
            .env("ATM_CONFIG_HOME", self.tempdir.path())
            .env("ATM_IDENTITY", "arch-ctm")
            .env("ATM_TEAM", "atm-dev")
            .envs(extra_env.iter().copied())
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
            ..Default::default()
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

    fn write_raw_inbox(&self, agent: &str, raw: &str) {
        let inbox_path = self.inbox_path(agent);
        if let Some(parent) = inbox_path.parent() {
            fs::create_dir_all(parent).expect("inbox dir");
        }
        fs::write(inbox_path, raw).expect("write raw inbox");
    }

    fn write_seen_state(&self, agent: &str, timestamp: chrono::DateTime<Utc>) {
        let path = self.team_dir().join(".seen").join(agent);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("seen dir");
        }
        fs::write(path, timestamp.to_rfc3339()).expect("write seen state");
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

    fn read_seen_state(&self, agent: &str) -> Option<chrono::DateTime<Utc>> {
        let path = self.team_dir().join(".seen").join(agent);
        let raw = fs::read_to_string(path).ok()?;
        chrono::DateTime::parse_from_rfc3339(raw.trim())
            .ok()
            .map(|timestamp| timestamp.with_timezone(&Utc))
    }

    fn inbox_path(&self, agent: &str) -> std::path::PathBuf {
        self.team_dir()
            .join("inboxes")
            .join(format!("{agent}.json"))
    }

    fn origin_inbox_path(&self, agent: &str, origin: &str) -> std::path::PathBuf {
        self.team_dir()
            .join("inboxes")
            .join(format!("{agent}.{origin}.json"))
    }

    fn inbox_contents(&self, agent: &str) -> Vec<MessageEnvelope> {
        let raw = fs::read_to_string(self.inbox_path(agent)).expect("inbox contents");
        raw.lines()
            .map(|line| serde_json::from_str(line).expect("json line"))
            .collect()
    }

    fn workflow_state_contents(&self, agent: &str) -> Option<Value> {
        let path = self
            .team_dir()
            .join(".atm-state")
            .join("workflow")
            .join(format!("{agent}.json"));
        let raw = fs::read_to_string(path).ok()?;
        Some(serde_json::from_str(&raw).expect("workflow json"))
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
            timestamp: IsoTimestamp::from_datetime(self.timestamp(timestamp_offset)),
            read,
            source_team: Some("atm-dev".into()),
            summary: None,
            message_id: Some(LegacyMessageId::new()),
            pending_ack_at: pending_ack_offset
                .map(|offset| IsoTimestamp::from_datetime(self.timestamp(offset))),
            acknowledged_at: acknowledged_offset
                .map(|offset| IsoTimestamp::from_datetime(self.timestamp(offset))),
            acknowledges_message_id: None,
            task_id: None,
            extra: serde_json::Map::new(),
        }
    }
}

fn idle_notification_text(from: &str, idle_reason: &str) -> String {
    // Claude Code owns the idle-notification payload shape in the text field.
    // Keep this fixture aligned with docs/claude-code-message-schema.md.
    serde_json::json!({
        "type": "idle_notification",
        "from": from,
        "timestamp": "2026-03-30T00:00:00Z",
        "idleReason": idle_reason,
    })
    .to_string()
}
