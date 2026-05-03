use std::fs;
use std::process::Command;
mod helpers;

use atm_core::ack::{
    self, AckMessageId, AckRequest, ScopedReplyAtmMessageIdOverride, ScopedReplyMessageIdOverride,
};
use atm_core::error::AtmErrorCode;
use atm_core::inbox_ingress::{InboxIngress, default_inbox_ingress};
use atm_core::mail_store::{
    AckStateRecord, IngestRecord, MailStore, MailStoreHealth, PendingExportRecord,
    StoredMessageRecord, VisibilityStateRecord,
};
use atm_core::observability::NullObservability;
use atm_core::schema::{AgentMember, AtmMessageId, LegacyMessageId, MessageEnvelope, TeamConfig};
use atm_core::store::{
    InsertOutcome, MessageKey, SourceFingerprint, StoreBootstrapReport, StoreError, StoreHealth,
};
use atm_core::task_store::{TaskRecord, TaskStore};
use atm_core::types::{AgentName, IsoTimestamp, TeamName};
use atm_core::{read_messages, write_messages};
use atm_rusqlite::RusqliteStore;
use chrono::{Duration, TimeZone, Utc};
use helpers::{
    TEST_LEAD, TEST_LEAD_ADDRESS, TEST_ORIGIN, TEST_SENDER, TEST_SENDER_ADDRESS, TEST_TEAM,
    configure_atm_command,
};
use serde_json::Value;
use serial_test::serial;
use uuid::Uuid;

#[test]
#[serial]
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
    let store = fixture.store();
    let stored = store
        .load_message_by_legacy_id(&LegacyMessageId::from(message_id))
        .expect("load stored message")
        .expect("stored row");
    let ack_state = store
        .load_ack_state(&stored.message_key)
        .expect("load ack state")
        .expect("ack row");
    assert!(ack_state.pending_ack_at.is_none());
    assert!(ack_state.acknowledged_at.is_some());
    let task = store
        .load_task(&"TASK-123".parse().expect("task id"))
        .expect("load task")
        .expect("task row");
    assert!(task.acknowledged_at.is_some());
    assert_eq!(task.status, atm_core::task_store::TaskStatus::Acknowledged);

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
#[serial]
fn test_ack_ingests_origin_inbox_and_persists_ack_state_in_sqlite() {
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

    // The origin inbox file keeps the pre-commit compatibility snapshot.
    // SQLite is authoritative for ack state after ingestion/commit.
    let origin = fixture.origin_inbox_contents(TEST_SENDER, TEST_ORIGIN);
    assert_eq!(origin.len(), 1);
    assert!(origin[0].pending_ack_at.is_some());
    assert!(origin[0].acknowledged_at.is_none());
    let store = fixture.store();
    let stored = store
        .load_message_by_legacy_id(&LegacyMessageId::from(message_id))
        .expect("load stored message")
        .expect("stored row");
    let ack_state = store
        .load_ack_state(&stored.message_key)
        .expect("load ack state")
        .expect("ack row");
    assert!(ack_state.pending_ack_at.is_none());
    assert!(ack_state.acknowledged_at.is_some());
}

#[test]
#[serial]
fn test_ack_duplicate_reply_identity_reports_store_constraint_violation() {
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

    let conflicting_reply_id = LegacyMessageId::new();
    let conflicting_reply_atm_message_id = AtmMessageId::new();
    let mut conflicting_reply_extra = serde_json::Map::new();
    conflicting_reply_extra.insert(
        "metadata".to_string(),
        serde_json::json!({
            "atm": {
                "messageId": conflicting_reply_atm_message_id.to_string(),
            }
        }),
    );
    let conflicting_reply = MessageEnvelope {
        from: TEST_SENDER.parse().expect("sender"),
        text: "preexisting conflicting reply".to_string(),
        timestamp: "2026-05-02T19:45:00Z".parse().expect("timestamp"),
        read: false,
        source_team: Some(TEST_TEAM.parse().expect("team")),
        summary: Some("preexisting conflicting reply".to_string()),
        message_id: Some(conflicting_reply_id),
        pending_ack_at: None,
        acknowledged_at: None,
        acknowledges_message_id: None,
        task_id: None,
        extra: conflicting_reply_extra,
    };
    fixture.write_inbox(TEST_LEAD, &[conflicting_reply]);

    let store = fixture.store();
    let observability = NullObservability;
    let team = TEST_TEAM.parse().expect("team");
    let reply_agent = TEST_LEAD.parse().expect("reply agent");
    default_inbox_ingress()
        .ingest_mailbox_state(
            fixture.tempdir.path(),
            &team,
            &reply_agent,
            &store,
            &observability,
        )
        .expect("ingest conflicting reply into SQLite");

    let _reply_id_override = ScopedReplyMessageIdOverride::set(conflicting_reply_id);
    let _reply_atm_id_override =
        ScopedReplyAtmMessageIdOverride::set(conflicting_reply_atm_message_id);

    let error = ack::ack_mail(
        AckRequest {
            home_dir: fixture.tempdir.path().to_path_buf(),
            current_dir: fixture.tempdir.path().to_path_buf(),
            actor_override: Some(TEST_SENDER.parse().expect("actor")),
            team_override: Some(TEST_TEAM.parse().expect("team")),
            message_id: AckMessageId::Legacy(LegacyMessageId::from(message_id)),
            reply_body: "duplicate reply".to_string(),
        },
        &store,
        &observability,
    )
    .expect_err("duplicate reply identity should fail");

    assert_eq!(error.code, AtmErrorCode::StoreConstraintViolation);
}

#[test]
#[serial]
fn test_ack_imports_legacy_origin_message_and_persists_task_state_across_restart() {
    let fixture = Fixture::new(&[TEST_SENDER, TEST_LEAD]);
    let message_id = Uuid::new_v4();
    let mut message = fixture.message(
        TEST_LEAD,
        "origin pending",
        true,
        Some(Duration::minutes(5)),
        None,
        message_id,
    );
    message.task_id = Some("TASK-ORIGIN-123".parse().expect("task id"));
    fixture.write_origin_inbox(TEST_SENDER, TEST_ORIGIN, &[message]);

    let output = fixture.run(&["ack", &message_id.to_string(), "got it", "--json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let restarted_store = fixture.store();
    let stored = restarted_store
        .load_message_by_legacy_id(&LegacyMessageId::from(message_id))
        .expect("load stored message")
        .expect("stored row");
    let ack_state = restarted_store
        .load_ack_state(&stored.message_key)
        .expect("load ack state")
        .expect("ack row");
    assert!(ack_state.pending_ack_at.is_none());
    assert!(ack_state.acknowledged_at.is_some());
    let task = restarted_store
        .load_task(&"TASK-ORIGIN-123".parse().expect("task id"))
        .expect("load task")
        .expect("task row");
    assert_eq!(task.message_key, stored.message_key);
    assert_eq!(task.status, atm_core::task_store::TaskStatus::Acknowledged);
    assert!(task.acknowledged_at.is_some());
}

#[test]
#[serial]
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
#[serial]
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
    let hook_path_toml = toml_single_quoted_path(&hook_path);
    let payload_path_toml = toml_single_quoted_path(&payload_path);
    fixture.write_atm_config(&format!(
        "[[atm.post_send_hooks]]\nrecipient = 'team-lead'\ncommand = [{hook_path_toml}, 'capture', {payload_path_toml}]\n",
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
#[serial]
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
    let hook_path_toml = toml_single_quoted_path(&hook_path);
    let payload_path_toml = toml_single_quoted_path(&payload_path);
    fixture.write_atm_config(&format!(
        "[[atm.post_send_hooks]]\nrecipient = 'team-lead'\ncommand = [{hook_path_toml}, 'fail', {payload_path_toml}]\n",
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
#[serial]
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
#[serial]
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
            .contains("SQLite-authoritative (read, pending_ack) state"),
        "stderr: {}",
        fixture.stderr(&output)
    );
}

#[test]
#[serial]
fn test_ack_preserves_sqlite_state_when_reply_export_fails_after_commit() {
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
    message.task_id = Some("TASK-EXPORT-123".parse().expect("task id"));
    fixture.write_inbox(TEST_SENDER, &[message]);
    fs::create_dir_all(fixture.inbox_path(TEST_LEAD)).expect("block reply export path");

    let output = fixture.run(&["ack", &message_id.to_string(), "received and starting"]);

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        fixture.stderr(&output)
    );
    let stderr = fixture.stderr(&output);
    assert!(
        stderr.contains("acknowledgement reply export failed after SQLite commit"),
        "stderr: {stderr}"
    );

    let restarted_store = fixture.store();
    let stored = restarted_store
        .load_message_by_legacy_id(&LegacyMessageId::from(message_id))
        .expect("load stored message")
        .expect("stored row");
    let ack_state = restarted_store
        .load_ack_state(&stored.message_key)
        .expect("load ack state")
        .expect("ack row");
    assert!(ack_state.pending_ack_at.is_none());
    assert!(ack_state.acknowledged_at.is_some());
    assert!(ack_state.ack_reply_message_key.is_some());
    let task = restarted_store
        .load_task(&"TASK-EXPORT-123".parse().expect("task id"))
        .expect("load task")
        .expect("task row");
    assert_eq!(task.status, atm_core::task_store::TaskStatus::Acknowledged);
    assert!(task.acknowledged_at.is_some());
}

#[test]
#[serial]
fn test_ack_accepts_ulid_message_id_for_message_written_by_atm_send() {
    let fixture = Fixture::new(&[TEST_SENDER, TEST_LEAD]);
    let send = fixture.run_with_env(
        &["send", TEST_SENDER_ADDRESS, "please ack", "--requires-ack"],
        &[("ATM_IDENTITY", TEST_LEAD)],
    );
    assert!(send.status.success(), "stderr: {}", fixture.stderr(&send));

    let inbox = fixture.inbox_contents(TEST_SENDER);
    assert_eq!(inbox.len(), 1);
    let atm_message_id = inbox[0].atm_message_id().expect("atm message id");
    let store = fixture.store();
    let stored = store
        .load_message_by_atm_id(&atm_message_id)
        .expect("load stored message")
        .expect("stored row");
    store
        .upsert_visibility(&VisibilityStateRecord {
            message_key: stored.message_key,
            read_at: Some(inbox[0].timestamp),
            cleared_at: None,
        })
        .expect("mark message read in sqlite");

    let output = fixture.run(&[
        "ack",
        &atm_message_id.to_string(),
        "received and starting",
        "--json",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["message_id"], atm_message_id.to_string());
    assert_eq!(parsed["reply_target"], TEST_LEAD_ADDRESS);

    let replies = fixture.inbox_contents(TEST_LEAD);
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].text, "received and starting");
}

#[test]
#[serial]
fn test_ack_accepts_legacy_uuid_only_message_after_store_ingest() {
    let fixture = Fixture::new(&[TEST_SENDER, TEST_LEAD]);
    let message_id = Uuid::new_v4();
    fixture.write_inbox(
        TEST_SENDER,
        &[fixture.message(
            TEST_LEAD,
            "legacy pending",
            true,
            Some(Duration::minutes(5)),
            None,
            message_id,
        )],
    );

    let store = fixture.store();
    let observability = NullObservability;
    let team = TEST_TEAM.parse().expect("team");
    let actor = TEST_SENDER.parse().expect("agent");
    let ingest = default_inbox_ingress()
        .ingest_mailbox_state(
            fixture.tempdir.path(),
            &team,
            &actor,
            &store,
            &observability,
        )
        .expect("ingest legacy inbox");
    assert_eq!(ingest.imported_messages, 1);

    let output = fixture.run(&["ack", &message_id.to_string(), "legacy ack", "--json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["message_id"], message_id.to_string());

    let replies = fixture.inbox_contents(TEST_LEAD);
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].text, "legacy ack");
}

#[test]
#[serial]
fn test_ack_surfaces_typed_store_error_when_commit_fails() {
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

    let store = fixture.store();
    let failing_store = FailingCommitStore::new(
        store,
        StoreError::transaction("synthetic commit failure for ack test"),
    );
    let observability = NullObservability;
    let error = ack::ack_mail(
        AckRequest {
            home_dir: fixture.tempdir.path().to_path_buf(),
            current_dir: fixture.tempdir.path().to_path_buf(),
            actor_override: Some(TEST_SENDER.parse().expect("actor")),
            team_override: Some(TEST_TEAM.parse().expect("team")),
            message_id: AckMessageId::Legacy(LegacyMessageId::from(message_id)),
            reply_body: "store failure".to_string(),
        },
        &failing_store,
        &observability,
    )
    .expect_err("store commit failure should surface as typed ATM error");

    assert_eq!(error.code, AtmErrorCode::StoreTransactionFailed);
    assert!(error.is_store(), "error should stay in the store kind");
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
        configure_atm_command(&mut command, self.tempdir.path(), Some(TEST_SENDER));
        command
            .args(args)
            .current_dir(self.tempdir.path())
            .envs(extra_env.iter().copied())
            .output()
            .expect("run atm")
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

    fn inbox_json_lines(&self, agent: &str) -> Vec<Value> {
        let raw = fs::read_to_string(self.inbox_path(agent)).expect("inbox contents");
        helpers::parse_inbox_values(&raw)
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

    fn store(&self) -> RusqliteStore {
        RusqliteStore::open_for_team_home(self.tempdir.path(), &TEST_TEAM.parse().expect("team"))
            .expect("open store")
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
        let timestamp = fixture_base_timestamp();
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

struct FailingCommitStore {
    inner: RusqliteStore,
    error_message: String,
}

impl FailingCommitStore {
    fn new(inner: RusqliteStore, error: StoreError) -> Self {
        Self {
            inner,
            error_message: error.message,
        }
    }
}

impl atm_core::ack::sealed::Sealed for FailingCommitStore {}
impl atm_core::mail_store::sealed::Sealed for FailingCommitStore {}
impl atm_core::task_store::sealed::Sealed for FailingCommitStore {}

impl atm_core::store::StoreBoundary for FailingCommitStore {
    fn bootstrap_report(&self) -> Result<StoreBootstrapReport, StoreError> {
        self.inner.bootstrap_report()
    }

    fn health(&self) -> Result<StoreHealth, StoreError> {
        self.inner.health()
    }
}

impl MailStore for FailingCommitStore {
    fn insert_message(
        &self,
        message: &StoredMessageRecord,
    ) -> Result<InsertOutcome<StoredMessageRecord>, StoreError> {
        self.inner.insert_message(message)
    }

    fn insert_message_batch(&self, messages: &[StoredMessageRecord]) -> Result<(), StoreError> {
        self.inner.insert_message_batch(messages)
    }

    fn load_message(
        &self,
        message_key: &MessageKey,
    ) -> Result<Option<StoredMessageRecord>, StoreError> {
        self.inner.load_message(message_key)
    }

    fn load_message_by_legacy_id(
        &self,
        legacy_message_id: &LegacyMessageId,
    ) -> Result<Option<StoredMessageRecord>, StoreError> {
        self.inner.load_message_by_legacy_id(legacy_message_id)
    }

    fn load_message_by_atm_id(
        &self,
        atm_message_id: &AtmMessageId,
    ) -> Result<Option<StoredMessageRecord>, StoreError> {
        self.inner.load_message_by_atm_id(atm_message_id)
    }

    fn list_messages_for_recipient(
        &self,
        team_name: &TeamName,
        recipient_agent: &AgentName,
    ) -> Result<Vec<StoredMessageRecord>, StoreError> {
        self.inner
            .list_messages_for_recipient(team_name, recipient_agent)
    }

    fn upsert_ack_state(&self, ack_state: &AckStateRecord) -> Result<AckStateRecord, StoreError> {
        self.inner.upsert_ack_state(ack_state)
    }

    fn upsert_ack_state_batch(&self, ack_states: &[AckStateRecord]) -> Result<(), StoreError> {
        self.inner.upsert_ack_state_batch(ack_states)
    }

    fn load_ack_state(
        &self,
        message_key: &MessageKey,
    ) -> Result<Option<AckStateRecord>, StoreError> {
        self.inner.load_ack_state(message_key)
    }

    fn upsert_visibility(
        &self,
        visibility: &VisibilityStateRecord,
    ) -> Result<VisibilityStateRecord, StoreError> {
        self.inner.upsert_visibility(visibility)
    }

    fn upsert_visibility_batch(
        &self,
        visibilities: &[VisibilityStateRecord],
    ) -> Result<(), StoreError> {
        self.inner.upsert_visibility_batch(visibilities)
    }

    fn load_visibility(
        &self,
        message_key: &MessageKey,
    ) -> Result<Option<VisibilityStateRecord>, StoreError> {
        self.inner.load_visibility(message_key)
    }

    fn record_ingest(
        &self,
        ingest_record: &IngestRecord,
    ) -> Result<InsertOutcome<IngestRecord>, StoreError> {
        self.inner.record_ingest(ingest_record)
    }

    fn insert_message_with_ingest(
        &self,
        message: &StoredMessageRecord,
        ingest_record: &IngestRecord,
    ) -> Result<InsertOutcome<StoredMessageRecord>, StoreError> {
        self.inner
            .insert_message_with_ingest(message, ingest_record)
    }

    fn insert_message_with_ingest_state(
        &self,
        message: &StoredMessageRecord,
        ingest_record: &IngestRecord,
        state: &atm_core::mail_store::ImportedMessageState,
    ) -> Result<InsertOutcome<StoredMessageRecord>, StoreError> {
        self.inner
            .insert_message_with_ingest_state(message, ingest_record, state)
    }

    fn load_ingest(
        &self,
        team_name: &TeamName,
        recipient_agent: &AgentName,
        source_fingerprint: &SourceFingerprint,
    ) -> Result<Option<IngestRecord>, StoreError> {
        self.inner
            .load_ingest(team_name, recipient_agent, source_fingerprint)
    }

    fn record_pending_export(&self, export: &PendingExportRecord) -> Result<(), StoreError> {
        self.inner.record_pending_export(export)
    }

    fn remove_pending_export(&self, message_key: &MessageKey) -> Result<(), StoreError> {
        self.inner.remove_pending_export(message_key)
    }

    fn load_due_pending_exports(
        &self,
        now: &IsoTimestamp,
        limit: usize,
    ) -> Result<Vec<PendingExportRecord>, StoreError> {
        self.inner.load_due_pending_exports(now, limit)
    }

    fn remove_expired_pending_exports(&self, now: &IsoTimestamp) -> Result<u64, StoreError> {
        self.inner.remove_expired_pending_exports(now)
    }

    fn mail_health(&self) -> Result<MailStoreHealth, StoreError> {
        self.inner.mail_health()
    }
}

impl TaskStore for FailingCommitStore {
    fn upsert_task(&self, task: &TaskRecord) -> Result<TaskRecord, StoreError> {
        self.inner.upsert_task(task)
    }

    fn load_task(
        &self,
        task_id: &atm_core::types::TaskId,
    ) -> Result<Option<TaskRecord>, StoreError> {
        self.inner.load_task(task_id)
    }

    fn load_tasks_for_message(
        &self,
        message_key: &MessageKey,
    ) -> Result<Vec<TaskRecord>, StoreError> {
        self.inner.load_tasks_for_message(message_key)
    }

    fn acknowledge_task(
        &self,
        task_id: &atm_core::types::TaskId,
        acknowledged_at: IsoTimestamp,
    ) -> Result<Option<TaskRecord>, StoreError> {
        self.inner.acknowledge_task(task_id, acknowledged_at)
    }
}

impl atm_core::ack::AckStore for FailingCommitStore {
    fn commit_ack_reply(
        &self,
        _command: &atm_core::ack::AckCommitCommand<'_>,
    ) -> Result<atm_core::ack::AckCommitResult, StoreError> {
        Err(StoreError::transaction(self.error_message.clone()))
    }
}

fn fixture_base_timestamp() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 5, 2, 20, 0, 0)
        .single()
        .expect("fixture timestamp")
}

fn toml_single_quoted_path(path: &std::path::Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "''"))
}
