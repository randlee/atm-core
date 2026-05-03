use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
mod helpers;

use atm_core::inbox_export::default_inbox_export;
use atm_core::inbox_ingress::{InboxIngestOutcome, InboxIngestStore, InboxIngress};
use atm_core::mail_store::MailStore;
use atm_core::observability::NullObservability;
use atm_core::roster_store::RosterStore;
use atm_core::schema::{AgentMember, MessageEnvelope, TeamConfig};
use atm_core::send::{SendMessageSource, SendRequest, resolve_store_team, send_mail_via_store};
use atm_core::task_store::TaskStore;
use atm_core::types::{AgentName, TeamName};
use atm_core::{read_messages, write_messages};
use atm_rusqlite::RusqliteStore;
use helpers::{
    ROLE_TEAM_LEAD, TEST_LEAD, TEST_RECIPIENT, TEST_RECIPIENT_ADDRESS, TEST_SENDER,
    TEST_SENDER_ADDRESS, TEST_TEAM, configure_atm_command,
};
use serde_json::Value;
use serial_test::serial;

#[test]
fn test_send_creates_inbox_file() {
    let fixture = Fixture::new(TEST_RECIPIENT);

    let output = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "hello from test"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    assert!(
        fixture
            .stdout(&output)
            .contains(&format!("Sent to {TEST_RECIPIENT_ADDRESS} [message_id:")),
        "stdout: {}",
        fixture.stdout(&output)
    );

    let inbox = fixture.inbox_contents(TEST_RECIPIENT);
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].text, "hello from test");
    assert_eq!(inbox[0].from, TEST_SENDER);
    assert!(inbox[0].message_id.is_some());
    let raw = fixture.inbox_json_lines(TEST_RECIPIENT);
    assert_eq!(raw.len(), 1);
    assert!(
        raw[0]["metadata"]["atm"]["messageId"]
            .as_str()
            .map(|value| value.len() == 26)
            .unwrap_or(false)
    );
    assert_eq!(raw[0]["metadata"]["atm"]["sourceTeam"], TEST_TEAM);
    assert_eq!(raw[0]["from"], TEST_SENDER);
    assert_eq!(raw[0]["text"], "hello from test");
    assert!(raw[0]["timestamp"].as_str().is_some());
    assert_eq!(raw[0]["read"], false);
    assert!(raw[0].get("message_id").is_none());
    assert!(raw[0].get("source_team").is_none());
}

#[test]
fn test_send_dry_run_no_file() {
    let fixture = Fixture::new("recipient");

    let output = fixture.run(&[
        "send",
        TEST_RECIPIENT_ADDRESS,
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
    assert_eq!(parsed["team"], TEST_TEAM);
    assert_eq!(parsed["agent"], TEST_RECIPIENT);
    assert_eq!(parsed["message"], "hello from test");
    assert_eq!(parsed["dry_run"], true);
    assert_eq!(parsed["requires_ack"], false);

    assert!(!fixture.inbox_path("recipient").exists());
}

#[test]
fn test_send_json_output() {
    let fixture = Fixture::new(TEST_RECIPIENT);

    let output = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "hello json", "--json"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("valid send json");
    assert_eq!(parsed["action"], "send");
    assert_eq!(parsed["team"], TEST_TEAM);
    assert_eq!(parsed["agent"], TEST_RECIPIENT);
    assert_eq!(parsed["outcome"], "sent");
    assert_eq!(parsed["requires_ack"], false);
    assert!(parsed["message_id"].as_str().is_some());
    assert!(parsed["atm_message_id"].as_str().is_some());
}

#[test]
fn test_send_emits_retained_log_record() {
    let fixture = Fixture::new(TEST_RECIPIENT);

    let send = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "hello emit", "--json"]);
    assert!(send.status.success(), "stderr: {}", fixture.stderr(&send));

    let output = fixture.run(&["log", "filter", "--match", "command=send", "--json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid log json");
    let records = parsed["records"].as_array().expect("records array");
    assert!(
        records.iter().any(|record| {
            record["fields"]["command"] == "send"
                && record["fields"]["agent"] == TEST_RECIPIENT
                && record["fields"]["team"] == TEST_TEAM
        }),
        "stdout: {}",
        fixture.stdout(&output)
    );
}

#[test]
fn test_send_export_failure_emits_retained_error_record() {
    let fixture = Fixture::new(TEST_RECIPIENT);
    fs::remove_file(fixture.inbox_path(TEST_RECIPIENT)).ok();
    fs::create_dir_all(fixture.inbox_path(TEST_RECIPIENT)).expect("block recipient inbox path");

    let send = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "hello export failure"]);
    assert!(!send.status.success(), "stdout: {}", fixture.stdout(&send));

    let output = fixture.run(&["log", "filter", "--match", "command=send", "--json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid log json");
    let records = parsed["records"].as_array().expect("records array");
    assert!(
        records.iter().any(|record| {
            record["fields"]["command"] == "send"
                && record["action"] == "export"
                && record["severity"] == "error"
                && record["fields"]["agent"] == TEST_RECIPIENT
                && record["fields"]["team"] == TEST_TEAM
                && record["fields"]["error_code"] == "ATM_MAILBOX_READ_FAILED"
        }),
        "stdout: {}",
        fixture.stdout(&output)
    );
}

#[derive(Default)]
struct RecordingIngress {
    calls: AtomicUsize,
    // INVARIANT: Mutex preserves the last observed target so concurrent test
    // setup can assert ingress routing without racing the recorder state.
    last_target: Mutex<Option<(TeamName, AgentName)>>,
}

impl InboxIngress for RecordingIngress {
    fn ingest_mailbox_state(
        &self,
        _home_dir: &std::path::Path,
        team: &TeamName,
        agent: &AgentName,
        _store: &dyn InboxIngestStore,
        _observability: &dyn atm_core::observability::ObservabilityPort,
    ) -> Result<InboxIngestOutcome, atm_core::error::AtmError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.last_target.lock().expect("target lock") = Some((team.clone(), agent.clone()));
        Ok(InboxIngestOutcome::default())
    }
}

struct FailingIngress;

impl InboxIngress for FailingIngress {
    fn ingest_mailbox_state(
        &self,
        _home_dir: &std::path::Path,
        _team: &TeamName,
        _agent: &AgentName,
        _store: &dyn InboxIngestStore,
        _observability: &dyn atm_core::observability::ObservabilityPort,
    ) -> Result<InboxIngestOutcome, atm_core::error::AtmError> {
        Err(
            atm_core::error::AtmError::mailbox_read("synthetic ingress failure")
                .with_recovery("repair synthetic ingress fixture"),
        )
    }
}

#[test]
fn test_send_mail_via_store_calls_injected_inbox_ingress() {
    let fixture = Fixture::new(TEST_RECIPIENT);
    fixture.write_inbox(TEST_RECIPIENT, &[]);
    let request = SendRequest::new(
        fixture.tempdir.path().to_path_buf(),
        fixture.tempdir.path().to_path_buf(),
        Some(TEST_SENDER),
        TEST_RECIPIENT_ADDRESS,
        Some(TEST_TEAM),
        SendMessageSource::Inline("hello ingress".to_string()),
        None,
        false,
        None,
        false,
    )
    .expect("request");
    let team = resolve_store_team(&request).expect("team");
    let store = RusqliteStore::open_for_team_home(fixture.tempdir.path(), &team).expect("store");
    let ingress = RecordingIngress::default();
    let exporter = default_inbox_export();
    let observability = NullObservability;

    let outcome = send_mail_via_store(request, &store, &ingress, &exporter, &observability)
        .expect("send via store");

    assert_eq!(outcome.outcome, "sent");
    assert_eq!(ingress.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        *ingress.last_target.lock().expect("target lock"),
        Some((
            TEST_TEAM.parse().expect("team"),
            TEST_RECIPIENT.parse().expect("agent"),
        ))
    );
}

#[test]
fn test_send_mail_via_store_propagates_inbox_ingress_failure() {
    let fixture = Fixture::new(TEST_RECIPIENT);
    fixture.write_inbox(TEST_RECIPIENT, &[]);
    let request = SendRequest::new(
        fixture.tempdir.path().to_path_buf(),
        fixture.tempdir.path().to_path_buf(),
        Some(TEST_SENDER),
        TEST_RECIPIENT_ADDRESS,
        Some(TEST_TEAM),
        SendMessageSource::Inline("hello ingress failure".to_string()),
        None,
        false,
        None,
        false,
    )
    .expect("request");
    let team = resolve_store_team(&request).expect("team");
    let store = RusqliteStore::open_for_team_home(fixture.tempdir.path(), &team).expect("store");
    let exporter = default_inbox_export();
    let observability = NullObservability;

    let error = send_mail_via_store(request, &store, &FailingIngress, &exporter, &observability)
        .expect_err("ingress failure should propagate");

    assert_eq!(error.code, atm_core::error::AtmErrorCode::MailboxReadFailed);
}

#[test]
fn test_send_requires_ack() {
    let fixture = Fixture::new(TEST_RECIPIENT);

    let output = fixture.run(&[
        "send",
        TEST_RECIPIENT_ADDRESS,
        "please ack",
        "--requires-ack",
    ]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
    assert!(inbox[0].pending_ack_at.is_some());
    let raw = fixture.inbox_json_lines("recipient");
    assert!(raw[0]["metadata"]["atm"]["pendingAckAt"].as_str().is_some());
    assert!(raw[0].get("pendingAckAt").is_none());
    let atm_message_id = inbox[0].extra["metadata"]["atm"]["messageId"]
        .as_str()
        .expect("atm message id");
    let workflow = fixture.workflow_state_contents(TEST_TEAM, TEST_RECIPIENT);
    assert!(
        workflow["messages"][format!("atm:{atm_message_id}")]["read"].is_null()
            || workflow["messages"][format!("atm:{atm_message_id}")]["read"] == false
    );
    assert!(
        workflow["messages"][format!("atm:{atm_message_id}")]["pendingAckAt"]
            .as_str()
            .is_some()
    );
}

#[test]
fn test_send_persists_task_id() {
    let fixture = Fixture::new("recipient");

    let output = fixture.run(&[
        "send",
        TEST_RECIPIENT_ADDRESS,
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
    let raw = fixture.inbox_json_lines("recipient");
    assert_eq!(raw[0]["metadata"]["atm"]["taskId"], "TASK-123");
    assert!(raw[0].get("taskId").is_none());
}

#[test]
fn test_send_supports_positional_message_with_file() {
    let fixture = Fixture::new("recipient");
    let attachment = fixture.tempdir.path().join("notes.txt");
    fs::write(&attachment, "attachment body").expect("attachment");

    let output = fixture.run(&[
        "send",
        TEST_RECIPIENT_ADDRESS,
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

    let output = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "hello despite bad siblings"]);

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

    let output = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "hello legacy"]);

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

    let output = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "hello"]);

    assert!(!output.status.success());
    let stderr = fixture.stderr(&output);
    assert!(stderr.contains("failed to parse team config"));
    assert!(stderr.contains("config.json"));
    assert!(stderr.contains("Repair the JSON syntax in config.json"));
}

#[test]
fn test_send_missing_config_uses_existing_inbox_fallback_and_warns_sender() {
    let fixture = Fixture::new(TEST_RECIPIENT);
    fs::remove_file(fixture.team_dir().join("config.json")).expect("remove config");
    fixture.write_inbox(TEST_RECIPIENT, &[]);
    fixture.write_inbox(ROLE_TEAM_LEAD, &[]);

    let output = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "hello fallback"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let stdout = fixture.stdout(&output);
    let stderr = fixture.stderr(&output);
    assert!(stdout.contains(&format!("Sent to {TEST_RECIPIENT_ADDRESS}")));
    assert!(stderr.contains("warning: team config is missing"));

    let inbox = fixture.inbox_contents(TEST_RECIPIENT);
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].text, "hello fallback");
    let atm_message_id = inbox[0].atm_message_id().expect("atm message id");
    let store = RusqliteStore::open_for_team_home(
        fixture.tempdir.path(),
        &TEST_TEAM.parse().expect("team"),
    )
    .expect("open store");
    let stored = store
        .load_message_by_atm_id(&atm_message_id)
        .expect("load stored message by atm id")
        .expect("stored row");
    assert_eq!(stored.body, "hello fallback");
    assert_eq!(stored.recipient_agent.as_str(), TEST_RECIPIENT);

    let notices = fixture.inbox_contents(ROLE_TEAM_LEAD);
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].from, "atm-identity-missing");
    assert_eq!(notices[0].source_team.as_deref(), Some(TEST_TEAM));
    assert!(
        notices[0]
            .text
            .contains("ATM warning: send used existing inbox fallback")
    );
}

#[test]
fn test_send_does_not_fall_back_to_obsolete_config_identity() {
    let fixture = Fixture::new("recipient");
    fixture.write_atm_config("[atm]\nidentity = \"config-agent\"\n");

    let output = fixture.run_without_identity(&["send", TEST_RECIPIENT_ADDRESS, "hello"]);

    assert!(!output.status.success());
    let stderr = fixture.stderr(&output);
    assert!(
        stderr.contains("identity is not configured"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("Set ATM_IDENTITY"), "stderr: {stderr}");
}

#[test]
fn test_send_missing_config_deduplicates_team_lead_notice() {
    let fixture = Fixture::new(TEST_RECIPIENT);
    fs::remove_file(fixture.team_dir().join("config.json")).expect("remove config");
    fixture.write_inbox(TEST_RECIPIENT, &[]);
    fixture.write_inbox(ROLE_TEAM_LEAD, &[]);

    let first = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "first"]);
    assert!(first.status.success(), "stderr: {}", fixture.stderr(&first));

    let second = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "second"]);
    assert!(
        second.status.success(),
        "stderr: {}",
        fixture.stderr(&second)
    );

    let notices = fixture.inbox_contents(ROLE_TEAM_LEAD);
    assert_eq!(notices.len(), 1);
}

#[test]
#[serial]
fn test_send_missing_config_deduplicates_team_lead_notice_under_concurrency() {
    // This intentionally races two real send subprocesses. A failure here
    // points to production dedup/locking behavior, not just test harness drift.
    let fixture = Fixture::new("recipient");
    fs::remove_file(fixture.team_dir().join("config.json")).expect("remove config");
    fixture.write_inbox(TEST_RECIPIENT, &[]);
    fixture.write_inbox(ROLE_TEAM_LEAD, &[]);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let fixture = &fixture;

    let (first, second) = std::thread::scope(|scope| {
        let first_barrier = barrier.clone();
        let first = scope.spawn(move || {
            first_barrier.wait();
            fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "first"])
        });
        let second_barrier = barrier.clone();
        let second = scope.spawn(move || {
            second_barrier.wait();
            fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "second"])
        });
        barrier.wait();
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
    let notices = fixture.inbox_contents(ROLE_TEAM_LEAD);
    assert_eq!(notices.len(), 1, "notices: {notices:?}");
    assert_eq!(notices[0].from, "atm-identity-missing");
}

#[test]
fn test_send_missing_config_notice_resets_after_config_is_restored() {
    let fixture = Fixture::new(TEST_RECIPIENT);
    fs::remove_file(fixture.team_dir().join("config.json")).expect("remove config");
    fixture.write_inbox(TEST_RECIPIENT, &[]);
    fixture.write_inbox(ROLE_TEAM_LEAD, &[]);

    let first = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "first"]);
    assert!(first.status.success(), "stderr: {}", fixture.stderr(&first));
    assert_eq!(fixture.inbox_contents(ROLE_TEAM_LEAD).len(), 1);

    fixture.write_team_config(TEST_RECIPIENT);
    let second = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "with config restored"]);
    assert!(
        second.status.success(),
        "stderr: {}",
        fixture.stderr(&second)
    );
    assert_eq!(fixture.inbox_contents(ROLE_TEAM_LEAD).len(), 1);

    fs::remove_file(fixture.team_dir().join("config.json")).expect("remove config again");
    let third = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "broken again"]);
    assert!(third.status.success(), "stderr: {}", fixture.stderr(&third));
    assert_eq!(fixture.inbox_contents(ROLE_TEAM_LEAD).len(), 2);
}

#[test]
fn test_send_missing_config_fails_when_recipient_inbox_does_not_exist() {
    let fixture = Fixture::new("recipient");
    fs::remove_file(fixture.team_dir().join("config.json")).expect("remove config");

    let output = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "hello"]);

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

    let output = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "hello fallback"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
}

#[test]
fn test_send_resolves_recipient_alias_before_membership_validation() {
    let fixture = Fixture::new(ROLE_TEAM_LEAD);
    fixture.write_atm_config(&format!(
        "[atm]\n[atm.aliases]\ntl = \"{ROLE_TEAM_LEAD}\"\n"
    ));

    let output = fixture.run(&["send", "tl@test-team", "hello alias"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let inbox = fixture.inbox_contents(ROLE_TEAM_LEAD);
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].text, "hello alias");
}

#[test]
fn test_send_cross_team_projects_alias_and_persists_canonical_from_identity() {
    let fixture = Fixture::new("recipient");
    fixture.write_team_config_for_team("other-team", "recipient");
    fixture.write_atm_config(&format!("[atm]\n[atm.aliases]\nlead = \"{TEST_SENDER}\"\n"));

    let output = fixture.run_with_env(
        &["send", "recipient@other-team", "hello cross-team"],
        &[("ATM_TEAM", TEST_TEAM)],
    );

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let inbox = fixture.inbox_contents_in_team("other-team", "recipient");
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].from, "lead");
    assert_eq!(
        inbox[0].extra["metadata"]["atm"]["fromIdentity"],
        TEST_SENDER
    );
}

#[test]
fn test_send_json_reports_canonical_sender_identity() {
    let fixture = Fixture::new("recipient");
    fixture.write_team_config_for_team("other-team", "recipient");
    fixture.write_atm_config(&format!("[atm]\n[atm.aliases]\nlead = \"{TEST_SENDER}\"\n"));

    let output = fixture.run_with_env(
        &["send", "recipient@other-team", "hello cross-team", "--json"],
        &[("ATM_TEAM", TEST_TEAM)],
    );

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["sender"], TEST_SENDER);
    assert!(parsed["atm_message_id"].as_str().is_some());
}

#[test]
fn test_send_runs_post_send_hook_with_expected_payload() {
    let fixture = Fixture::new("recipient");
    let (hook_path, payload_path) = fixture.install_hook_fixture("capture");
    fixture.write_atm_config(&format!(
        "[[atm.post_send_hooks]]\nrecipient = 'recipient'\ncommand = ['{}', 'capture', '{}']\n",
        hook_path.display(),
        payload_path.display()
    ));

    let output = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "hello hook"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let payload: serde_json::Value = read_json_file_with_retry(&payload_path, "hook payload");
    assert_eq!(payload["from"], TEST_SENDER_ADDRESS);
    assert_eq!(payload["to"], TEST_RECIPIENT_ADDRESS);
    assert_eq!(payload["requires_ack"], false);
    assert_eq!(payload["is_ack"], false);
    assert!(payload["message_id"].as_str().is_some());
    assert!(payload.get("task_id").is_none());
    assert_eq!(payload["sender"], TEST_SENDER);
    assert_eq!(payload["recipient"], "recipient");
}

#[test]
fn test_send_post_send_hook_failure_does_not_roll_back_send() {
    let fixture = Fixture::new("recipient");
    let (hook_path, payload_path) = fixture.install_hook_fixture("fail");
    fixture.write_atm_config(&format!(
        "[[atm.post_send_hooks]]\nrecipient = 'recipient'\ncommand = ['{}', 'fail', '{}']\n",
        hook_path.display(),
        payload_path.display()
    ));

    let output = fixture.run(&[
        "send",
        TEST_RECIPIENT_ADDRESS,
        "hello failed hook",
        "--json",
    ]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("valid send json");
    let warnings = parsed["warnings"].as_array().expect("warnings array");
    assert!(
        warnings.iter().any(|warning| warning
            .as_str()
            .is_some_and(|warning| warning.contains("post-send hook exited unsuccessfully"))),
        "stdout: {}",
        fixture.stdout(&output)
    );
    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
}

#[test]
fn test_send_post_send_hook_non_match_is_silent() {
    let fixture = Fixture::new("recipient");
    let (hook_path, payload_path) = fixture.install_hook_fixture("capture");
    fixture.write_atm_config(&format!(
        "[[atm.post_send_hooks]]\nrecipient = 'quality-mgr'\ncommand = ['{}', 'capture', '{}']\n",
        hook_path.display(),
        payload_path.display()
    ));

    let output = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "hello unmatched hook"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    assert!(!payload_path.exists(), "hook payload unexpectedly created");
    assert_eq!(fixture.stderr(&output), "");
    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
}

#[test]
fn test_send_runs_post_send_hook_for_wildcard_recipient() {
    let fixture = Fixture::new("recipient");
    let (hook_path, payload_path) = fixture.install_hook_fixture("capture");
    fixture.write_atm_config(&format!(
        "[[atm.post_send_hooks]]\nrecipient = '*'\ncommand = ['{}', 'capture', '{}']\n",
        hook_path.display(),
        payload_path.display()
    ));

    let output = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "hello wildcard hook"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let payload: serde_json::Value = read_json_file_with_retry(&payload_path, "hook payload");
    assert_eq!(payload["recipient"], "recipient");
}

#[test]
fn test_send_runs_multiple_matching_post_send_hooks_in_config_order() {
    let fixture = Fixture::new("recipient");
    let order_path = fixture.tempdir.path().join("hook-order.log");
    fixture.install_executable_script(
        "scripts/append-order.py",
        &format!(
            "#!/usr/bin/env python3\nimport sys\nfrom pathlib import Path\nPath(r\"{}\").open(\"a\", encoding=\"utf-8\").write(sys.argv[1] + \"\\n\")\n",
            order_path.display()
        ),
    );
    fixture.write_atm_config(
        "[[atm.post_send_hooks]]\nrecipient = 'recipient'\ncommand = ['python3', 'scripts/append-order.py', 'recipient']\n\n[[atm.post_send_hooks]]\nrecipient = '*'\ncommand = ['python3', 'scripts/append-order.py', 'wildcard']\n",
    );

    let output = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "hello multiple hooks"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let hook_order = fs::read_to_string(order_path)
        .expect("hook order log")
        .replace("\r\n", "\n");
    assert_eq!(hook_order, "recipient\nwildcard\n");
}

#[test]
fn test_send_runs_post_send_hook_when_recipient_matches_rule() {
    let fixture = Fixture::new("recipient");
    let (hook_path, payload_path) = fixture.install_hook_fixture("capture");
    fixture.write_atm_config(&format!(
        "[[atm.post_send_hooks]]\nrecipient = 'recipient'\ncommand = ['{}', 'capture', '{}']\n",
        hook_path.display(),
        payload_path.display()
    ));

    let output = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "hello recipient hook"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let payload: serde_json::Value = read_json_file_with_retry(&payload_path, "hook payload");
    assert_eq!(payload["recipient"], "recipient");
}

#[test]
fn test_send_runs_post_send_hook_for_multiline_message_when_rule_matches() {
    let fixture = Fixture::new("recipient");
    let (hook_path, payload_path) = fixture.install_hook_fixture("capture");
    fixture.write_atm_config(&format!(
        "[[atm.post_send_hooks]]\nrecipient = 'recipient'\ncommand = ['{}', 'capture', '{}']\n",
        hook_path.display(),
        payload_path.display()
    ));

    let output = fixture.run(&[
        "send",
        TEST_RECIPIENT_ADDRESS,
        "<atm-task id=\"task-1\">\n  <description>Review the Phase 2 plan.</description>\n</atm-task>",
    ]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let payload: serde_json::Value = read_json_file_with_retry(&payload_path, "hook payload");
    assert_eq!(payload["from"], TEST_SENDER_ADDRESS);
    assert_eq!(payload["to"], TEST_RECIPIENT_ADDRESS);
    assert!(payload["message_id"].as_str().is_some());
}

#[test]
fn test_send_ignores_post_send_hook_configured_only_in_core_section() {
    let fixture = Fixture::new("recipient");
    let (hook_path, payload_path) = fixture.install_hook_fixture("capture");
    fixture.write_atm_config(&format!(
        "[core]\ndefault_team = '{}'\nidentity = '{}'\npost_send_hook = ['{}', 'capture', '{}']\n",
        TEST_TEAM,
        TEST_LEAD,
        hook_path.display(),
        payload_path.display()
    ));

    let output = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "hello core section"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    assert!(!payload_path.exists(), "hook payload unexpectedly created");
    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
}

#[test]
fn test_send_post_send_hook_receives_only_configured_positional_args() {
    let fixture = Fixture::new("recipient");
    let (hook_path, payload_path) = fixture.install_hook_fixture("capture-meta");
    fixture.write_atm_config(&format!(
        "[[atm.post_send_hooks]]\nrecipient = 'recipient'\ncommand = ['{}', 'capture-meta', '{}']\n",
        hook_path.display(),
        payload_path.display()
    ));

    let output = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "hello args"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let captured: serde_json::Value = read_json_file_with_retry(&payload_path, "hook meta");
    assert_eq!(captured["args"], serde_json::json!([]));
    assert_eq!(captured["payload"]["to"], TEST_RECIPIENT_ADDRESS);
}

#[cfg(unix)]
#[test]
fn test_send_runs_post_send_hook_with_relative_script_command() {
    let fixture = Fixture::new("recipient");
    let payload_path = fixture.tempdir.path().join("relative-hook.json");
    fixture.install_executable_script(
        "scripts/record-hook.sh",
        &format!(
            "#!/usr/bin/env bash\nprintf '%s\\n' \"$ATM_POST_SEND\" > '{}'\n",
            payload_path.display()
        ),
    );
    fixture.write_atm_config(
        "[[atm.post_send_hooks]]\nrecipient = 'recipient'\ncommand = ['scripts/record-hook.sh']\n",
    );

    let output = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "hello relative script"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let payload: serde_json::Value = read_json_file_with_retry(&payload_path, "hook payload");
    assert_eq!(payload["recipient"], "recipient");
}

#[cfg(unix)]
#[test]
fn test_send_runs_post_send_hook_with_bare_bash_command() {
    let fixture = Fixture::new("recipient");
    let payload_path = fixture.tempdir.path().join("bash-hook.json");
    fixture.install_executable_script(
        "scripts/record-hook.sh",
        &format!(
            "#!/usr/bin/env bash\nprintf '%s\\n' \"$ATM_POST_SEND\" > '{}'\n",
            payload_path.display()
        ),
    );
    fixture.write_atm_config(
        "[[atm.post_send_hooks]]\nrecipient = 'recipient'\ncommand = ['bash', 'scripts/record-hook.sh']\n",
    );

    let output = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "hello bare bash"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let payload: serde_json::Value = read_json_file_with_retry(&payload_path, "hook payload");
    assert_eq!(payload["recipient"], "recipient");
}

#[test]
fn test_send_runs_post_send_hook_with_python_command() {
    let fixture = Fixture::new("recipient");
    let payload_path = fixture.tempdir.path().join("python-hook.json");
    fixture.install_executable_script(
        "scripts/record_hook.py",
        &format!(
            "#!/usr/bin/env python3\nimport os\nfrom pathlib import Path\nPath(r\"{}\").write_text(os.environ['ATM_POST_SEND'])\n",
            payload_path.display()
        ),
    );
    fixture.write_atm_config(
        "[[atm.post_send_hooks]]\nrecipient = 'recipient'\ncommand = ['python3', 'scripts/record_hook.py']\n",
    );

    let output = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "hello python hook"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let payload: serde_json::Value = read_json_file_with_retry(&payload_path, "hook payload");
    assert_eq!(payload["recipient"], "recipient");
}

#[test]
#[cfg(unix)]
fn test_send_persists_store_rows_and_threads_roster_pane_into_hook_payload() {
    let fixture = Fixture::new("recipient");
    let mut recipient = AgentMember::with_name("recipient".parse().expect("agent"));
    recipient.tmux_pane_id = Some("%7".to_string());
    fixture.write_team_config_members(vec![recipient]);

    let payload_path = fixture.tempdir.path().join("roster-pane-hook.json");
    fixture.install_executable_script(
        "scripts/record_hook.py",
        &format!(
            "#!/usr/bin/env python3\nimport os\nfrom pathlib import Path\nPath(r\"{}\").write_text(os.environ['ATM_POST_SEND'])\n",
            payload_path.display()
        ),
    );
    fixture.write_atm_config(
        "[[atm.post_send_hooks]]\nrecipient = 'recipient'\ncommand = ['python3', 'scripts/record_hook.py']\n",
    );

    let output = fixture.run(&[
        "send",
        TEST_RECIPIENT_ADDRESS,
        "task assignment",
        "--requires-ack",
        "--task-id",
        "TASK-321",
    ]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );

    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
    let atm_message_id = inbox[0].atm_message_id().expect("atm message id");

    let store = RusqliteStore::open_for_team_home(
        fixture.tempdir.path(),
        &TEST_TEAM.parse().expect("team"),
    )
    .expect("open store");
    let stored = store
        .load_message_by_atm_id(&atm_message_id)
        .expect("load message by atm id")
        .expect("stored message");
    assert_eq!(stored.body, "task assignment");
    assert_eq!(stored.atm_message_id, Some(atm_message_id));
    assert!(stored.legacy_message_id.is_some());
    assert_eq!(stored.recipient_agent.as_str(), "recipient");

    let ack_state = store
        .load_ack_state(&stored.message_key)
        .expect("load ack state")
        .expect("ack row");
    assert!(ack_state.pending_ack_at.is_some());
    assert!(ack_state.acknowledged_at.is_none());

    let task = store
        .load_task(&"TASK-321".parse().expect("task id"))
        .expect("load task")
        .expect("task row");
    assert_eq!(task.message_key, stored.message_key);
    assert_eq!(task.status, atm_core::task_store::TaskStatus::PendingAck);

    let roster = store
        .load_roster(&TEST_TEAM.parse().expect("team"))
        .expect("load roster");
    let recipient = roster
        .iter()
        .find(|member| member.agent_name.as_str() == "recipient")
        .expect("recipient roster row");
    assert_eq!(
        recipient
            .recipient_pane_id
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("%7")
    );

    let payload: serde_json::Value = read_json_file_with_retry(&payload_path, "hook payload");
    assert_eq!(payload["recipient_pane_id"], "%7");
    assert_eq!(payload["task_id"], "TASK-321");
    assert_eq!(payload["is_ack"], false);
}

#[test]
#[cfg(unix)]
fn test_send_hook_payload_omits_recipient_pane_id_when_roster_entry_is_absent() {
    let fixture = Fixture::new("recipient");
    fixture.write_inbox("recipient", &[]);
    fs::remove_file(fixture.team_dir().join("config.json")).expect("remove config");

    let payload_path = fixture.tempdir.path().join("missing-config-hook.json");
    fixture.install_executable_script(
        "scripts/record_hook.py",
        &format!(
            "#!/usr/bin/env python3\nimport os\nfrom pathlib import Path\nPath(r\"{}\").write_text(os.environ['ATM_POST_SEND'])\n",
            payload_path.display()
        ),
    );
    fixture.write_atm_config(
        "[[atm.post_send_hooks]]\nrecipient = 'recipient'\ncommand = ['python3', 'scripts/record_hook.py']\n",
    );

    let output = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "missing config fallback"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let payload: serde_json::Value = read_json_file_with_retry(&payload_path, "hook payload");
    assert!(payload.get("recipient_pane_id").is_none(), "{payload}");
}

#[test]
fn test_send_rejects_retired_post_send_hook_members_config() {
    let fixture = Fixture::new(TEST_RECIPIENT);
    fixture.write_atm_config(&format!(
        "[atm]\npost_send_hook_members = ['{ROLE_TEAM_LEAD}']\n"
    ));

    let output = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "hello retired"]);

    assert!(!output.status.success());
    let stderr = fixture.stderr(&output);
    assert!(stderr.contains("post_send_hook_members"));
    assert!(stderr.contains(".atm.toml"));
    assert!(stderr.contains("[[atm.post_send_hooks]]"));
}

#[test]
fn test_send_rejects_legacy_post_send_filter_shape() {
    let fixture = Fixture::new("recipient");
    fixture.write_atm_config(
        "[atm]\npost_send_hook = ['bin/hook']\npost_send_hook_recipients = ['recipient']\n",
    );

    let output = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "hello retired"]);

    assert!(!output.status.success());
    let stderr = fixture.stderr(&output);
    assert!(stderr.contains("retired post-send hook keys"));
    assert!(stderr.contains("[[atm.post_send_hooks]]"));
}

#[test]
fn test_send_rejects_post_send_hook_with_empty_recipient() {
    let fixture = Fixture::new("recipient");
    fixture.write_atm_config("[[atm.post_send_hooks]]\nrecipient = '   '\ncommand = ['bash']\n");

    let output = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "hello invalid hook"]);

    assert!(!output.status.success());
    let stderr = fixture.stderr(&output);
    assert!(stderr.contains("recipient must not be empty"));
}

#[test]
fn test_send_rejects_post_send_hook_with_empty_command() {
    let fixture = Fixture::new("recipient");
    fixture.write_atm_config("[[atm.post_send_hooks]]\nrecipient = 'recipient'\ncommand = []\n");

    let output = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "hello invalid hook"]);

    assert!(!output.status.success());
    let stderr = fixture.stderr(&output);
    assert!(stderr.contains("command must not be empty"));
}

#[test]
fn test_send_ignores_invalid_hook_result_stdout() {
    let fixture = Fixture::new("recipient");
    let (hook_path, payload_path) = fixture.install_hook_fixture("result-invalid");
    fixture.write_atm_config(&format!(
        "[[atm.post_send_hooks]]\nrecipient = 'recipient'\ncommand = ['{}', 'result-invalid', '{}']\n",
        hook_path.display(),
        payload_path.display()
    ));

    let output = fixture.run(&["send", TEST_RECIPIENT_ADDRESS, "hello invalid hook result"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    assert_eq!(fixture.stderr(&output), "");
    let inbox = fixture.inbox_contents("recipient");
    assert_eq!(inbox.len(), 1);
}

#[test]
fn test_send_logs_structured_hook_result_stdout() {
    let fixture = Fixture::new("recipient");
    let (hook_path, payload_path) = fixture.install_hook_fixture("result-debug");
    fixture.write_atm_config(&format!(
        "[[atm.post_send_hooks]]\nrecipient = 'recipient'\ncommand = ['{}', 'result-debug', '{}']\n",
        hook_path.display(),
        payload_path.display()
    ));

    let output = fixture.run_with_env(
        &[
            "--stderr-logs",
            "send",
            TEST_RECIPIENT_ADDRESS,
            "hello hook result",
            "--json",
        ],
        &[("ATM_LOG", "debug")],
    );

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["agent"], "recipient");
    assert_eq!(parsed["team"], TEST_TEAM);
    let stderr = fixture.stderr(&output);
    assert!(
        stderr.contains("hook fixture captured payload"),
        "stderr: {stderr}"
    );
    assert!(
        stderr.contains("atm_post_send_hook_fixture"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("hook_result_fields"), "stderr: {stderr}");
}

#[test]
fn test_send_help_mentions_post_send_hook_config() {
    let output = Command::new(env!("CARGO_BIN_EXE_atm"))
        .args(["send", "--help"])
        .output()
        .expect("run atm send --help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    assert!(stdout.contains("[[atm.post_send_hooks]]"));
    assert!(stdout.contains("recipient = \"name-or-*\""));
    assert!(stdout.contains("command = [\"argv\", ...]"));
    assert!(stdout.contains("ATM_LOG=debug"));
    assert!(stdout.contains(".atm.toml"));
}

fn read_json_file_with_retry(path: &std::path::Path, label: &str) -> serde_json::Value {
    let start = std::time::Instant::now();
    let deadline = Duration::from_secs(5);
    while start.elapsed() < deadline {
        match fs::read(path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(value) => return value,
                Err(_) => std::thread::sleep(Duration::from_millis(25)),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => panic!("{label}: {error}"),
        }
    }
    panic!(
        "{label}: file missing or never reached a valid JSON payload before deadline {:?} (elapsed {:?}): {}",
        deadline,
        start.elapsed(),
        path.display(),
    );
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

    fn run_without_identity(&self, args: &[&str]) -> std::process::Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_atm"));
        configure_atm_command(&mut command, self.tempdir.path(), None)
            .args(args)
            .current_dir(self.tempdir.path());
        command.output().expect("run atm without identity")
    }

    fn run_with_env(&self, args: &[&str], extra_env: &[(&str, &str)]) -> std::process::Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_atm"));
        configure_atm_command(&mut command, self.tempdir.path(), Some(TEST_SENDER))
            .args(args)
            .current_dir(self.tempdir.path());
        for (key, value) in extra_env {
            command.env(key, value);
        }
        command.output().expect("run atm")
    }

    fn write_team_config(&self, recipient: &str) {
        self.write_team_config_for_team(TEST_TEAM, recipient);
    }

    fn write_team_config_for_team(&self, team: &str, recipient: &str) {
        self.write_team_config_members_for_team(
            team,
            vec![AgentMember::with_name(recipient.parse().expect("agent"))],
        );
    }

    fn write_team_config_members(&self, members: Vec<AgentMember>) {
        self.write_team_config_members_for_team(TEST_TEAM, members);
    }

    fn write_team_config_members_for_team(&self, team: &str, members: Vec<AgentMember>) {
        let team_dir = self.tempdir.path().join(".claude").join("teams").join(team);
        fs::create_dir_all(&team_dir).expect("team dir");
        let config = TeamConfig {
            members,
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
            .join(TEST_TEAM);
        fs::create_dir_all(&team_dir).expect("team dir");
        fs::write(team_dir.join("config.json"), raw).expect("write raw team config");
    }

    fn write_atm_config(&self, raw: &str) {
        fs::write(self.tempdir.path().join(".atm.toml"), raw).expect("write .atm.toml");
    }

    fn inbox_path(&self, recipient: &str) -> std::path::PathBuf {
        self.inbox_path_in_team(TEST_TEAM, recipient)
    }

    fn inbox_path_in_team(&self, team: &str, recipient: &str) -> std::path::PathBuf {
        self.tempdir
            .path()
            .join(".claude")
            .join("teams")
            .join(team)
            .join("inboxes")
            .join(format!("{recipient}.json"))
    }

    fn write_inbox(&self, recipient: &str, messages: &[MessageEnvelope]) {
        let inbox_path = self.inbox_path(recipient);
        if let Some(parent) = inbox_path.parent() {
            fs::create_dir_all(parent).expect("inbox dir");
        }
        write_messages(&inbox_path, messages).expect("write inbox");
    }

    fn inbox_contents(&self, recipient: &str) -> Vec<MessageEnvelope> {
        self.inbox_contents_in_team(TEST_TEAM, recipient)
    }

    fn inbox_json_lines(&self, recipient: &str) -> Vec<Value> {
        self.inbox_json_lines_in_team(TEST_TEAM, recipient)
    }

    fn inbox_contents_in_team(&self, team: &str, recipient: &str) -> Vec<MessageEnvelope> {
        let inbox_path = self.inbox_path_in_team(team, recipient);
        read_messages(&inbox_path).expect("inbox contents")
    }

    fn inbox_json_lines_in_team(&self, team: &str, recipient: &str) -> Vec<Value> {
        let inbox_path = self.inbox_path_in_team(team, recipient);
        let raw = fs::read_to_string(&inbox_path).expect("inbox contents");
        helpers::parse_inbox_values(&raw)
    }

    fn team_dir(&self) -> std::path::PathBuf {
        self.tempdir
            .path()
            .join(".claude")
            .join("teams")
            .join(TEST_TEAM)
    }

    fn workflow_state_contents(&self, team: &str, agent: &str) -> Value {
        let raw = fs::read_to_string(
            self.tempdir
                .path()
                .join(".claude")
                .join("teams")
                .join(team)
                .join(".atm-state")
                .join("workflow")
                .join(format!("{agent}.json")),
        )
        .expect("workflow state contents");
        serde_json::from_str(&raw).expect("workflow json")
    }

    fn install_hook_fixture(&self, mode: &str) -> (PathBuf, PathBuf) {
        let fixture_binary = PathBuf::from(env!("CARGO_BIN_EXE_atm_post_send_hook_fixture"));
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
            PathBuf::from("bin").join(hook_path.file_name().expect("copied hook binary filename")),
            payload_path,
        )
    }

    fn install_executable_script(&self, relative_path: &str, body: &str) -> PathBuf {
        let path = self.tempdir.path().join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("script dir");
        }
        fs::write(&path, body).expect("write script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = fs::metadata(&path).expect("script metadata").permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&path, permissions).expect("script permissions");
        }
        path
    }

    fn stdout(&self, output: &std::process::Output) -> String {
        String::from_utf8(output.stdout.clone()).expect("stdout utf8")
    }

    fn stdout_json(&self, output: &std::process::Output) -> serde_json::Value {
        serde_json::from_slice(&output.stdout).expect("valid json")
    }

    fn stderr(&self, output: &std::process::Output) -> String {
        String::from_utf8(output.stderr.clone()).expect("stderr utf8")
    }
}
