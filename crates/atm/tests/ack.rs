#![cfg(unix)]

mod support;

use std::fs;
use std::process::Command;

use atm_core::schema::{
    AgentMember, LegacyMessageId, MessageEnvelope, TeamConfig, hydrate_legacy_fields_from_metadata,
};
use atm_core::types::{AgentName, IsoTimestamp, TeamName};
use chrono::{Duration, Utc};
use serde_json::Value;
use support::{
    TEST_LEAD, TEST_LEAD_ADDRESS, TEST_ORIGIN, TEST_SENDER, TEST_SENDER_ADDRESS, TEST_TEAM,
};
use uuid::Uuid;

fn parse_inbox_values(raw: &str) -> Vec<Value> {
    if raw.trim().is_empty() {
        return Vec::new();
    }

    match raw.chars().find(|ch| !ch.is_whitespace()) {
        Some('[') => serde_json::from_str(raw).expect("json array"),
        _ => raw
            .lines()
            .map(|line| serde_json::from_str(line).expect("json line"))
            .collect(),
    }
}

#[test]
fn test_ack_transitions_pending_ack_and_appends_reply() {
    let fixture = Fixture::new(&[TEST_SENDER, TEST_LEAD]);
    let message_id = Uuid::new_v4();
    let mut message = fixture.message(
        TEST_LEAD,
        "please ack",
        true,
        Some(Duration::minutes(5)),
        None,
        message_id,
    );
    message.task_id = Some("TASK-123".parse().expect("task id"));
    fixture.write_inbox(TEST_SENDER, &[message]);

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
    assert_eq!(parsed["team"], TEST_TEAM);
    assert_eq!(parsed["agent"], TEST_SENDER);
    assert_eq!(parsed["message_id"], message_id.to_string());
    assert_eq!(parsed["task_id"], "TASK-123");
    assert_eq!(parsed["reply_target"], TEST_LEAD_ADDRESS);
    assert_eq!(parsed["reply_text"], "received and starting");
    assert!(parsed["reply_message_id"].as_str().is_some());

    let inbox = fixture.inbox_contents(TEST_SENDER);
    assert_eq!(inbox.len(), 1);
    assert!(inbox[0].read);
    assert!(inbox[0].pending_ack_at.is_some());
    assert!(inbox[0].acknowledged_at.is_none());
    let workflow = fixture.workflow_state_contents(TEST_SENDER);
    assert_eq!(
        workflow["messages"][format!("legacy:{message_id}")]["read"],
        true
    );
    assert!(workflow["messages"][format!("legacy:{message_id}")]["pendingAckAt"].is_null());
    assert!(
        workflow["messages"][format!("legacy:{message_id}")]["acknowledgedAt"]
            .as_str()
            .is_some()
    );

    let replies = fixture.inbox_contents(TEST_LEAD);
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].text, "received and starting");
    assert_eq!(replies[0].from, TEST_SENDER);
    assert_eq!(
        replies[0].acknowledges_message_id,
        Some(LegacyMessageId::from(message_id))
    );
    let raw_replies = fixture.inbox_json_lines(TEST_LEAD);
    assert!(
        raw_replies[0]["metadata"]["atm"]["acknowledgesMessageId"]
            .as_str()
            .is_some()
    );
    assert!(raw_replies[0].get("acknowledgesMessageId").is_none());
}

#[test]
fn test_ack_updates_origin_inbox_file() {
    let fixture = Fixture::new(&[TEST_SENDER, TEST_LEAD]);
    let message_id = Uuid::new_v4();
    fixture.write_origin_inbox(
        TEST_SENDER,
        TEST_ORIGIN,
        &[fixture.message(
            TEST_LEAD,
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

    let origin = fixture.origin_inbox_contents(TEST_SENDER, TEST_ORIGIN);
    assert_eq!(origin.len(), 1);
    assert!(origin[0].pending_ack_at.is_some());
    assert!(origin[0].acknowledged_at.is_none());
    let workflow = fixture.workflow_state_contents(TEST_SENDER);
    assert!(
        workflow["messages"][format!("legacy:{message_id}")]["acknowledgedAt"]
            .as_str()
            .is_some()
    );
}

#[test]
fn test_ack_emits_retained_log_record() {
    let fixture = Fixture::new(&[TEST_SENDER, TEST_LEAD]);
    let message_id = Uuid::new_v4();
    fixture.write_inbox(
        TEST_SENDER,
        &[fixture.message(
            TEST_LEAD,
            "please ack",
            true,
            Some(Duration::minutes(5)),
            None,
            message_id,
        )],
    );

    let ack = fixture.run(&["ack", &message_id.to_string(), "got it", "--json"]);
    assert!(ack.status.success(), "stderr: {}", fixture.stderr(&ack));

    let output = fixture.run(&["log", "filter", "--match", "command=ack", "--json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    let records = parsed["records"].as_array().expect("records array");
    assert!(
        records.iter().any(|record| {
            record["fields"]["command"] == "ack"
                && record["fields"]["agent"] == TEST_SENDER
                && record["fields"]["message_id"] == message_id.to_string()
        }),
        "stdout: {}",
        String::from_utf8(output.stdout.clone()).expect("stdout utf8")
    );
}

#[test]
fn test_ack_runs_post_send_hook_with_expected_payload() {
    let fixture = Fixture::new(&[TEST_SENDER, TEST_LEAD]);
    let message_id = Uuid::new_v4();
    let mut message = fixture.message(
        TEST_LEAD,
        "please ack",
        true,
        Some(Duration::minutes(5)),
        None,
        message_id,
    );
    message.task_id = Some("TASK-123".parse().expect("task id"));
    fixture.write_inbox(TEST_SENDER, &[message]);

    let (hook_path, payload_path) = fixture.install_hook_fixture("capture");
    fixture.write_atm_config(&format!(
        "[[atm.post_send_hooks]]\nrecipient = '{}'\ncommand = ['{}', 'capture', '{}']\n",
        TEST_LEAD,
        hook_path.display(),
        payload_path.display()
    ));

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
    let payload: Value =
        serde_json::from_slice(&fs::read(payload_path).expect("hook payload")).expect("json");
    assert_eq!(payload["from"], TEST_SENDER_ADDRESS);
    assert_eq!(payload["to"], TEST_LEAD_ADDRESS);
    assert_eq!(payload["requires_ack"], false);
    assert_eq!(payload["is_ack"], true);
    assert_eq!(payload["task_id"], "TASK-123");
    assert!(payload["message_id"].as_str().is_some());
}

#[test]
fn test_ack_post_send_hook_failure_surfaces_warning() {
    let fixture = Fixture::new(&[TEST_SENDER, TEST_LEAD]);
    let message_id = Uuid::new_v4();
    fixture.write_inbox(
        TEST_SENDER,
        &[fixture.message(
            TEST_LEAD,
            "please ack",
            true,
            Some(Duration::minutes(5)),
            None,
            message_id,
        )],
    );

    let (hook_path, payload_path) = fixture.install_hook_fixture("fail");
    fixture.write_atm_config(&format!(
        "[[atm.post_send_hooks]]\nrecipient = '{}'\ncommand = ['{}', 'fail', '{}']\n",
        TEST_LEAD,
        hook_path.display(),
        payload_path.display()
    ));

    let output = fixture.run(&["ack", &message_id.to_string(), "received and starting"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let stderr = fixture.stderr(&output);
    assert!(
        stderr.contains("post-send hook exited unsuccessfully"),
        "stderr: {stderr}"
    );
}

#[test]
fn test_ack_rejects_already_acknowledged_message() {
    let fixture = Fixture::new(&[TEST_SENDER, TEST_LEAD]);
    let message_id = Uuid::new_v4();
    fixture.write_inbox(
        TEST_SENDER,
        &[fixture.message(
            TEST_LEAD,
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
    let fixture = Fixture::new(&[TEST_SENDER, TEST_LEAD]);
    let message_id = Uuid::new_v4();
    fixture.write_inbox(
        TEST_SENDER,
        &[fixture.message(TEST_LEAD, "plain read", true, None, None, message_id)],
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
        fixture.warm_daemon();
        fixture
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        let mut first = Command::new(env!("CARGO_BIN_EXE_atm"));
        let output =
            support::configure_atm_command(&mut first, self.tempdir.path(), Some(TEST_SENDER))
                .args(args)
                .current_dir(self.tempdir.path())
                .output()
                .expect("run atm");
        if !support::is_daemon_start_transient(&output) {
            return output;
        }

        std::thread::sleep(std::time::Duration::from_millis(50));
        let mut retry = Command::new(env!("CARGO_BIN_EXE_atm"));
        support::configure_atm_command(&mut retry, self.tempdir.path(), Some(TEST_SENDER))
            .args(args)
            .current_dir(self.tempdir.path())
            .output()
            .unwrap_or_else(|error| panic!("retry atm {:?} failed: {error}", args))
    }

    fn warm_daemon(&self) {
        let output = self.run(&["read", "--all", "--no-mark", "--json"]);
        assert!(output.status.success(), "stderr: {}", self.stderr(&output));
    }

    fn write_atm_config(&self, body: &str) {
        fs::write(self.tempdir.path().join(".atm.toml"), body).expect("write .atm.toml");
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

    fn write_inbox(&self, agent: &str, messages: &[MessageEnvelope]) {
        let inbox_path = self.inbox_path(agent);
        if let Some(parent) = inbox_path.parent() {
            fs::create_dir_all(parent).expect("inbox dir");
        }
        let values: Vec<Value> = messages
            .iter()
            .map(|message| serde_json::to_value(message).expect("json value"))
            .collect();
        fs::write(
            inbox_path,
            serde_json::to_string_pretty(&values).expect("json array"),
        )
        .expect("write inbox");
    }

    fn inbox_path(&self, agent: &str) -> std::path::PathBuf {
        self.team_dir()
            .join("inboxes")
            .join(format!("{agent}.json"))
    }

    fn inbox_contents(&self, agent: &str) -> Vec<MessageEnvelope> {
        let raw = fs::read_to_string(self.inbox_path(agent)).expect("inbox contents");
        parse_inbox_values(&raw)
            .into_iter()
            .map(|mut value| {
                hydrate_legacy_fields_from_metadata(&mut value);
                serde_json::from_value(value).expect("message envelope")
            })
            .collect()
    }

    fn inbox_json_lines(&self, agent: &str) -> Vec<Value> {
        let raw = fs::read_to_string(self.inbox_path(agent)).expect("inbox contents");
        parse_inbox_values(&raw)
    }

    fn write_origin_inbox(&self, agent: &str, origin: &str, messages: &[MessageEnvelope]) {
        let inbox_path = self.origin_inbox_path(agent, origin);
        if let Some(parent) = inbox_path.parent() {
            fs::create_dir_all(parent).expect("origin inbox dir");
        }
        let values: Vec<Value> = messages
            .iter()
            .map(|message| serde_json::to_value(message).expect("json value"))
            .collect();
        fs::write(
            inbox_path,
            serde_json::to_string_pretty(&values).expect("json array"),
        )
        .expect("write origin inbox");
    }

    fn origin_inbox_path(&self, agent: &str, origin: &str) -> std::path::PathBuf {
        self.team_dir()
            .join("inboxes")
            .join(format!("{agent}.{origin}.json"))
    }

    fn origin_inbox_contents(&self, agent: &str, origin: &str) -> Vec<MessageEnvelope> {
        let raw = fs::read_to_string(self.origin_inbox_path(agent, origin))
            .expect("origin inbox contents");
        parse_inbox_values(&raw)
            .into_iter()
            .map(|mut value| {
                hydrate_legacy_fields_from_metadata(&mut value);
                serde_json::from_value(value).expect("message envelope")
            })
            .collect()
    }

    fn workflow_state_contents(&self, agent: &str) -> Value {
        let raw = fs::read_to_string(
            self.team_dir()
                .join(".atm-state")
                .join("workflow")
                .join(format!("{agent}.json")),
        )
        .expect("workflow state contents");
        serde_json::from_str(&raw).expect("workflow json")
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
            .join(TEST_TEAM)
    }

    fn install_hook_fixture(&self, mode: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let fixture_binary =
            std::path::PathBuf::from(env!("CARGO_BIN_EXE_atm_post_send_hook_fixture"));
        let hook_dir = self.tempdir.path().join("bin");
        fs::create_dir_all(&hook_dir).expect("hook dir");
        let hook_path = hook_dir.join(fixture_binary.file_name().expect("hook binary filename"));
        fs::copy(&fixture_binary, &hook_path).expect("copy hook fixture");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = fs::metadata(&hook_path)
                .expect("hook metadata")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&hook_path, permissions).expect("hook permissions");
        }
        let payload_path = self.tempdir.path().join(format!("{mode}-payload.json"));
        (
            std::path::PathBuf::from("bin")
                .join(hook_path.file_name().expect("copied hook binary filename")),
            payload_path,
        )
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
            from: from.parse::<AgentName>().expect("agent"),
            text: text.to_string(),
            timestamp: timestamp.into(),
            read,
            source_team: Some(TEST_TEAM.parse::<TeamName>().expect("team")),
            summary: None,
            message_id: Some(LegacyMessageId::from(message_id)),
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
