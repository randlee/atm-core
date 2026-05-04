use std::ffi::{OsStr, OsString};
use std::fs;
use std::fs::OpenOptions;
use std::sync::{Arc, Barrier, Mutex, OnceLock, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use atm_core::ack::{AckRequest, ack_mail};
use atm_core::clear::{ClearQuery, clear_mail};
use atm_core::error::AtmErrorCode;
use atm_core::observability::NullObservability;
use atm_core::read::{ReadQuery, read_mail};
use atm_core::roles::ROLE_TEAM_LEAD;
use atm_core::schema::{
    AgentMember, AtmMessageId, LegacyMessageId, MessageEnvelope, TeamConfig,
    hydrate_legacy_fields_from_metadata,
};
use atm_core::send::{SendMessageSource, SendRequest, send_mail};
use atm_core::test_support::{TEST_QA, TEST_RECIPIENT, TEST_SENDER, TEST_TEAM};
use atm_core::types::{AckActivationMode, AgentName, IsoTimestamp, ReadSelection, TeamName};
use chrono::Utc;
use fs2::FileExt;
use serial_test::serial;
use tempfile::TempDir;
use uuid::Uuid;

// Test-side ceiling guard only; production lock timeout defaults to 5s per
// architecture §18.3.
const TEST_LOCK_BUDGET_CEILING: Duration = Duration::from_secs(2);
const PRIMARY_TEAM: &str = TEST_TEAM;
const PRIMARY_AGENT: &str = TEST_SENDER;
const SECONDARY_AGENT: &str = TEST_QA;
const TEAM_LEAD: &str = ROLE_TEAM_LEAD;

fn qualified(agent: &str) -> String {
    format!("{agent}@{PRIMARY_TEAM}")
}

#[test]
#[serial]
fn concurrent_ack_on_overlapping_inbox_sets_completes_without_deadlock() {
    let fixture = Fixture::new();
    let observability = Arc::new(NullObservability);
    let barrier = Arc::new(Barrier::new(3));
    let (tx, rx) = mpsc::channel();

    let arch_request = fixture.ack_request(PRIMARY_AGENT, fixture.arch_message_id, "ack from arch");
    let qa_request = fixture.ack_request(
        SECONDARY_AGENT,
        fixture.qa_message_id,
        &format!("ack from {SECONDARY_AGENT}"),
    );

    for (label, request) in [("arch", arch_request), ("secondary", qa_request)] {
        let barrier = Arc::clone(&barrier);
        let tx = tx.clone();
        let observability = Arc::clone(&observability);
        thread::spawn(move || {
            barrier.wait();
            tx.send((label, ack_mail(request, observability.as_ref())))
                .expect("send result");
        });
    }
    drop(tx);

    barrier.wait();
    let first = rx
        .recv_timeout(Duration::from_secs(4))
        .expect("first ack result");
    let second = rx
        .recv_timeout(Duration::from_secs(4))
        .expect("second ack result");

    assert!(
        first.1.is_ok(),
        "first ack failed for {}: {:?}",
        first.0,
        first.1
    );
    assert!(
        second.1.is_ok(),
        "second ack failed for {}: {:?}; arch inbox: {:?}; secondary inbox: {:?}",
        second.0,
        second.1,
        fixture.inbox_contents(PRIMARY_AGENT),
        fixture.inbox_contents(SECONDARY_AGENT)
    );
    let arch_inbox = fixture.inbox_contents(PRIMARY_AGENT);
    let qa_inbox = fixture.inbox_contents(SECONDARY_AGENT);
    assert!(
        arch_inbox
            .iter()
            .any(|message| message.text == format!("ack from {SECONDARY_AGENT}"))
    );
    assert!(
        qa_inbox
            .iter()
            .any(|message| message.text == "ack from arch")
    );
}

#[test]
#[serial]
fn concurrent_send_with_ack_and_clear_completes_without_deadlock_or_data_loss() {
    let observability = Arc::new(NullObservability);

    let clear_fixture = Fixture::new();
    clear_fixture.write_primary_inbox(
        PRIMARY_AGENT,
        &[read_message(
            SECONDARY_AGENT,
            "clearable history entry",
            LegacyMessageId::from(Uuid::new_v4()),
        )],
    );
    let barrier = Arc::new(Barrier::new(3));
    let (tx, rx) = mpsc::channel();
    let send_request =
        clear_fixture.send_request(TEAM_LEAD, &qualified(PRIMARY_AGENT), "new message");
    let clear_request = clear_fixture.clear_query(PRIMARY_AGENT);
    {
        let barrier = Arc::clone(&barrier);
        let tx = tx.clone();
        let observability = Arc::clone(&observability);
        thread::spawn(move || {
            barrier.wait();
            tx.send((
                "send-clear/send",
                send_mail(send_request, observability.as_ref()).map(|_| ()),
            ))
            .expect("send result");
        });
    }
    {
        let barrier = Arc::clone(&barrier);
        let tx = tx.clone();
        let observability = Arc::clone(&observability);
        thread::spawn(move || {
            barrier.wait();
            tx.send((
                "send-clear/clear",
                clear_mail(clear_request, observability.as_ref()).map(|_| ()),
            ))
            .expect("clear result");
        });
    }
    drop(tx);
    barrier.wait();
    let first = rx
        .recv_timeout(Duration::from_secs(4))
        .expect("first send/clear result");
    let second = rx
        .recv_timeout(Duration::from_secs(4))
        .expect("second send/clear result");
    assert!(first.1.is_ok(), "{} failed: {:?}", first.0, first.1);
    assert!(second.1.is_ok(), "{} failed: {:?}", second.0, second.1);
    let arch_inbox = clear_fixture.inbox_contents(PRIMARY_AGENT);
    assert!(
        arch_inbox
            .iter()
            .any(|message| message.text == "new message"),
        "new send was lost during concurrent clear: {:?}",
        arch_inbox
    );

    let ack_fixture = Fixture::new();
    let pending_message_id = LegacyMessageId::from(Uuid::new_v4());
    ack_fixture.write_primary_inbox(
        PRIMARY_AGENT,
        &[pending_ack_message(
            SECONDARY_AGENT,
            "pending ack",
            pending_message_id,
            PRIMARY_TEAM,
        )],
    );
    let barrier = Arc::new(Barrier::new(3));
    let (tx, rx) = mpsc::channel();
    let send_request =
        ack_fixture.send_request(TEAM_LEAD, &qualified(PRIMARY_AGENT), "new message");
    let ack_request = ack_fixture.ack_request(PRIMARY_AGENT, pending_message_id, "ack reply");
    {
        let barrier = Arc::clone(&barrier);
        let tx = tx.clone();
        let observability = Arc::clone(&observability);
        thread::spawn(move || {
            barrier.wait();
            tx.send((
                "send-ack/send",
                send_mail(send_request, observability.as_ref()).map(|_| ()),
            ))
            .expect("send result");
        });
    }
    {
        let barrier = Arc::clone(&barrier);
        let tx = tx.clone();
        let observability = Arc::clone(&observability);
        thread::spawn(move || {
            barrier.wait();
            tx.send((
                "send-ack/ack",
                ack_mail(ack_request, observability.as_ref()).map(|_| ()),
            ))
            .expect("ack result");
        });
    }
    drop(tx);
    barrier.wait();
    let first = rx
        .recv_timeout(Duration::from_secs(4))
        .expect("first send/ack result");
    let second = rx
        .recv_timeout(Duration::from_secs(4))
        .expect("second send/ack result");
    assert!(first.1.is_ok(), "{} failed: {:?}", first.0, first.1);
    assert!(second.1.is_ok(), "{} failed: {:?}", second.0, second.1);
    let arch_inbox = ack_fixture.inbox_contents(PRIMARY_AGENT);
    assert!(
        arch_inbox
            .iter()
            .any(|message| message.text == "new message"),
        "new send was lost during concurrent ack: {:?}",
        arch_inbox
    );
    assert!(
        arch_inbox.iter().any(|message| {
            message.message_id == Some(pending_message_id) && message.acknowledged_at.is_none()
        }),
        "pending message was not acknowledged: {:?}",
        arch_inbox
    );
    let arch_workflow = ack_fixture.workflow_state_contents(PRIMARY_AGENT);
    assert!(
        arch_workflow["messages"][format!("legacy:{pending_message_id}")]["acknowledgedAt"]
            .as_str()
            .is_some()
            || arch_workflow["messages"]
                [format!("atm:{}", pending_message_id.into_atm_message_id())]["acknowledgedAt"]
                .as_str()
                .is_some(),
        "pending message was not acknowledged in workflow state: {arch_workflow:?}"
    );
    let qa_inbox = ack_fixture.inbox_contents(SECONDARY_AGENT);
    assert!(
        qa_inbox.iter().any(|message| message.text == "ack reply"),
        "ack reply was not persisted: {:?}",
        qa_inbox
    );
}

#[test]
#[serial]
fn concurrent_same_recipient_sends_preserve_mixed_payloads_and_workflow_state() {
    let fixture = Fixture::new();
    let observability = Arc::new(NullObservability);
    let barrier = Arc::new(Barrier::new(3));
    let (tx, rx) = mpsc::channel();

    let plain_request = fixture.send_request(TEAM_LEAD, &qualified(PRIMARY_AGENT), "plain payload");
    let mut task_request =
        fixture.send_request(SECONDARY_AGENT, &qualified(PRIMARY_AGENT), "task payload");
    task_request.requires_ack = true;
    task_request.task_id = Some("TASK-123".parse().expect("task id"));
    task_request.summary_override = Some("manual summary".to_string());

    for (label, request) in [("plain", plain_request), ("task", task_request)] {
        let barrier = Arc::clone(&barrier);
        let tx = tx.clone();
        let observability = Arc::clone(&observability);
        thread::spawn(move || {
            barrier.wait();
            tx.send((label, send_mail(request, observability.as_ref())))
                .expect("send result");
        });
    }
    drop(tx);

    barrier.wait();
    let first = rx
        .recv_timeout(Duration::from_secs(4))
        .expect("first send result");
    let second = rx
        .recv_timeout(Duration::from_secs(4))
        .expect("second send result");
    assert!(first.1.is_ok(), "{} failed: {:?}", first.0, first.1);
    assert!(second.1.is_ok(), "{} failed: {:?}", second.0, second.1);

    let inbox = fixture.inbox_contents(PRIMARY_AGENT);
    let plain_message = inbox
        .iter()
        .find(|message| message.text == "plain payload")
        .expect("plain inbox message");
    let task_message = inbox
        .iter()
        .find(|message| message.text == "task payload")
        .expect("task inbox message");
    assert_eq!(task_message.task_id.as_deref(), Some("TASK-123"));
    assert_eq!(task_message.summary.as_deref(), Some("manual summary"));
    assert!(task_message.pending_ack_at.is_some());
    assert!(plain_message.task_id.is_none());
    assert!(plain_message.pending_ack_at.is_none());

    let plain_atm_id = message_atm_id(plain_message);
    let task_atm_id = message_atm_id(task_message);
    let workflow = fixture.workflow_state_contents(PRIMARY_AGENT);
    assert!(
        workflow["messages"][format!("atm:{plain_atm_id}")]
            .as_object()
            .is_some(),
        "plain workflow entry missing: {workflow:?}"
    );
    assert!(
        workflow["messages"][format!("atm:{plain_atm_id}")]["pendingAckAt"].is_null(),
        "plain workflow state should not require ack: {workflow:?}"
    );
    assert!(
        workflow["messages"][format!("atm:{task_atm_id}")]["pendingAckAt"]
            .as_str()
            .is_some(),
        "task workflow state should preserve pending ack: {workflow:?}"
    );
}

#[test]
#[serial]
fn concurrent_same_recipient_sends_preserve_preseeded_workflow_entries() {
    let fixture = Fixture::new();
    let observability = Arc::new(NullObservability);
    fixture.write_workflow_state(
        PRIMARY_AGENT,
        serde_json::json!({
            "messages": {
                "legacy:existing": {
                    "read": true,
                    "pendingAckAt": null,
                    "acknowledgedAt": null
                }
            }
        }),
    );

    let barrier = Arc::new(Barrier::new(3));
    let (tx, rx) = mpsc::channel();
    let first_request = fixture.send_request(TEAM_LEAD, &qualified(PRIMARY_AGENT), "first payload");
    let second_request =
        fixture.send_request(SECONDARY_AGENT, &qualified(PRIMARY_AGENT), "second payload");

    for (label, request) in [("first", first_request), ("second", second_request)] {
        let barrier = Arc::clone(&barrier);
        let tx = tx.clone();
        let observability = Arc::clone(&observability);
        thread::spawn(move || {
            barrier.wait();
            tx.send((label, send_mail(request, observability.as_ref())))
                .expect("send result");
        });
    }
    drop(tx);

    barrier.wait();
    let first = rx
        .recv_timeout(Duration::from_secs(4))
        .expect("first send result");
    let second = rx
        .recv_timeout(Duration::from_secs(4))
        .expect("second send result");
    assert!(first.1.is_ok(), "{} failed: {:?}", first.0, first.1);
    assert!(second.1.is_ok(), "{} failed: {:?}", second.0, second.1);

    let inbox = fixture.inbox_contents(PRIMARY_AGENT);
    let first_message = inbox
        .iter()
        .find(|message| message.text == "first payload")
        .expect("first inbox message");
    let second_message = inbox
        .iter()
        .find(|message| message.text == "second payload")
        .expect("second inbox message");
    let workflow = fixture.workflow_state_contents(PRIMARY_AGENT);

    assert!(
        workflow["messages"]["legacy:existing"]
            .as_object()
            .is_some(),
        "preseeded workflow entry was dropped: {workflow:?}"
    );
    assert!(
        workflow["messages"][format!("atm:{}", message_atm_id(first_message))]
            .as_object()
            .is_some(),
        "first send workflow entry missing after concurrent update: {workflow:?}"
    );
    assert!(
        workflow["messages"][format!("atm:{}", message_atm_id(second_message))]
            .as_object()
            .is_some(),
        "second send workflow entry missing after concurrent update: {workflow:?}"
    );
}

#[test]
#[serial]
fn missing_config_notice_seeds_team_lead_workflow_state() {
    let fixture = Fixture::new();
    let observability = NullObservability;
    fixture.create_team_without_config("broken-dev");
    fixture.write_primary_inbox_for_team("broken-dev", TEST_RECIPIENT, &[]);
    fixture.write_primary_inbox_for_team("broken-dev", TEAM_LEAD, &[]);

    send_mail(
        fixture.send_request(
            TEAM_LEAD,
            &format!("{TEST_RECIPIENT}@broken-dev"),
            "broken send",
        ),
        &observability,
    )
    .expect("missing-config send");

    let notices = fixture.inbox_contents_for_team("broken-dev", TEAM_LEAD);
    let notice = notices.first().expect("missing-config notice");
    assert_eq!(notice.from, "atm-identity-missing");
    assert_eq!(notice.source_team.as_deref(), Some("broken-dev"));
    let workflow = fixture.workflow_state_contents_for_team("broken-dev", TEAM_LEAD);
    let notice_atm_id = message_atm_id(notice);
    assert!(
        workflow["messages"][format!("atm:{notice_atm_id}")]
            .as_object()
            .is_some(),
        "missing-config workflow entry missing: {workflow:?}"
    );
}

#[test]
#[serial]
fn concurrent_normal_send_and_missing_config_notice_complete_without_data_loss() {
    let fixture = Fixture::new();
    let observability = Arc::new(NullObservability);
    fixture.create_team_without_config("broken-dev");
    fixture.write_primary_inbox_for_team("broken-dev", TEST_RECIPIENT, &[]);
    fixture.write_primary_inbox_for_team("broken-dev", TEAM_LEAD, &[]);

    let barrier = Arc::new(Barrier::new(3));
    let (tx, rx) = mpsc::channel();
    let normal_request = fixture.send_request(TEAM_LEAD, &qualified(PRIMARY_AGENT), "normal send");
    let broken_request = fixture.send_request(
        SECONDARY_AGENT,
        &format!("{TEST_RECIPIENT}@broken-dev"),
        "broken send",
    );

    for (label, request) in [("normal", normal_request), ("broken", broken_request)] {
        let barrier = Arc::clone(&barrier);
        let tx = tx.clone();
        let observability = Arc::clone(&observability);
        thread::spawn(move || {
            barrier.wait();
            tx.send((label, send_mail(request, observability.as_ref())))
                .expect("send result");
        });
    }
    drop(tx);

    barrier.wait();
    let first = rx
        .recv_timeout(Duration::from_secs(4))
        .expect("first send result");
    let second = rx
        .recv_timeout(Duration::from_secs(4))
        .expect("second send result");
    assert!(first.1.is_ok(), "{} failed: {:?}", first.0, first.1);
    assert!(second.1.is_ok(), "{} failed: {:?}", second.0, second.1);

    assert!(
        fixture
            .inbox_contents(PRIMARY_AGENT)
            .iter()
            .any(|message| message.text == "normal send"),
        "normal send missing from primary team inbox"
    );
    assert!(
        fixture
            .inbox_contents_for_team("broken-dev", TEST_RECIPIENT)
            .iter()
            .any(|message| message.text == "broken send"),
        "missing-config recipient send was not persisted"
    );
    let notices = fixture.inbox_contents_for_team("broken-dev", TEAM_LEAD);
    let notice = notices.first().expect("missing-config notice");
    let workflow = fixture.workflow_state_contents_for_team("broken-dev", TEAM_LEAD);
    let notice_atm_id = message_atm_id(notice);
    assert!(
        workflow["messages"][format!("atm:{notice_atm_id}")]["pendingAckAt"].is_null(),
        "missing-config notice workflow state missing after concurrent send: {workflow:?}"
    );
}

#[test]
#[serial]
fn multi_source_read_and_clear_complete_without_deadlock() {
    let fixture = Fixture::new();
    let observability = Arc::new(NullObservability);
    fixture.write_primary_inbox(
        PRIMARY_AGENT,
        &[unread_message(
            TEAM_LEAD,
            "primary unread",
            LegacyMessageId::from(Uuid::new_v4()),
        )],
    );
    fixture.write_origin_inbox(
        PRIMARY_AGENT,
        "host-b",
        &[unread_message(
            SECONDARY_AGENT,
            "origin unread b",
            LegacyMessageId::from(Uuid::new_v4()),
        )],
    );
    fixture.write_origin_inbox(
        PRIMARY_AGENT,
        "host-a",
        &[read_message(
            SECONDARY_AGENT,
            "origin read a",
            LegacyMessageId::from(Uuid::new_v4()),
        )],
    );

    let barrier = Arc::new(Barrier::new(3));
    let (tx, rx) = mpsc::channel();
    let read_request = fixture.read_query(PRIMARY_AGENT);
    let clear_request = fixture.clear_query(PRIMARY_AGENT);
    for (label, op) in [
        (
            "read",
            CommandOp::Read(read_request, Arc::clone(&observability)),
        ),
        (
            "clear",
            CommandOp::Clear(clear_request, Arc::clone(&observability)),
        ),
    ] {
        let barrier = Arc::clone(&barrier);
        let tx = tx.clone();
        thread::spawn(move || {
            barrier.wait();
            let result = match op {
                CommandOp::Read(request, observability) => {
                    read_mail(request, observability.as_ref()).map(|_| ())
                }
                CommandOp::Clear(request, observability) => {
                    clear_mail(request, observability.as_ref()).map(|_| ())
                }
            };
            tx.send((label, result)).expect("command result");
        });
    }
    drop(tx);
    barrier.wait();

    let first = rx
        .recv_timeout(Duration::from_secs(4))
        .expect("first read/clear result");
    let second = rx
        .recv_timeout(Duration::from_secs(4))
        .expect("second read/clear result");
    assert!(first.1.is_ok(), "{} failed: {:?}", first.0, first.1);
    assert!(second.1.is_ok(), "{} failed: {:?}", second.0, second.1);
    let arch_inbox = fixture.inbox_contents(PRIMARY_AGENT);
    let host_a_inbox = fixture.origin_inbox_contents(PRIMARY_AGENT, "host-a");
    let host_b_inbox = fixture.origin_inbox_contents(PRIMARY_AGENT, "host-b");
    let _ = (arch_inbox, host_a_inbox, host_b_inbox);
    assert!(fixture.primary_inbox_path(PRIMARY_AGENT).exists());
    assert!(fixture.origin_inbox_path(PRIMARY_AGENT, "host-a").exists());
    assert!(fixture.origin_inbox_path(PRIMARY_AGENT, "host-b").exists());
}

#[test]
#[serial]
fn send_times_out_under_bounded_lock_contention() {
    let _env_lock = env_lock().lock().expect("env lock");
    let _timeout = EnvGuard::set_raw("ATM_TEST_MAILBOX_LOCK_TIMEOUT_MS", "100");
    let fixture = Fixture::new();
    let observability = NullObservability;
    let lock_path = sentinel_path(&fixture.primary_inbox_path(PRIMARY_AGENT));
    let lock_file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .expect("open lock file");
    lock_file.lock_exclusive().expect("hold mailbox lock");

    let started = Instant::now();
    let error = send_mail(
        fixture.send_request(TEAM_LEAD, &qualified(PRIMARY_AGENT), "blocked send"),
        &observability,
    )
    .expect_err("timeout");

    assert_eq!(error.code, AtmErrorCode::MailboxLockTimeout);
    assert!(
        started.elapsed() < TEST_LOCK_BUDGET_CEILING,
        "retain only a coarse non-blocking budget here; recv_timeout-based tests above already cover deadlock detection"
    );
}

#[test]
#[serial]
fn clear_dry_run_does_not_wait_on_mailbox_lock() {
    let _env_lock = env_lock().lock().expect("env lock");
    let fixture = Fixture::new();
    let observability = NullObservability;
    fixture.write_primary_inbox(
        PRIMARY_AGENT,
        &[unread_message(
            TEAM_LEAD,
            "read without lock",
            LegacyMessageId::from(Uuid::new_v4()),
        )],
    );
    let lock_path = sentinel_path(&fixture.primary_inbox_path(PRIMARY_AGENT));
    let lock_file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .expect("open lock file");
    lock_file.lock_exclusive().expect("hold mailbox lock");

    let started = Instant::now();
    let mut clear_query = fixture.clear_query(PRIMARY_AGENT);
    clear_query.dry_run = true;
    let outcome = clear_mail(clear_query, &observability).expect("dry-run clear");

    assert_eq!(outcome.removed_total, 0);
    assert_eq!(outcome.remaining_total, 1);
    assert!(
        started.elapsed() < TEST_LOCK_BUDGET_CEILING,
        "retain only a coarse non-blocking budget here; recv_timeout-based tests above already cover deadlock detection"
    );
}

#[test]
#[serial]
fn read_possible_write_only_locks_when_display_mutation_is_required() {
    let _env_lock = env_lock().lock().expect("env lock");
    let _timeout = EnvGuard::set_raw("ATM_TEST_MAILBOX_LOCK_TIMEOUT_MS", "100");
    let observability = NullObservability;

    let mutation_fixture = Fixture::new();
    mutation_fixture.write_primary_inbox(
        PRIMARY_AGENT,
        &[unread_message(
            TEAM_LEAD,
            "needs mark-read",
            LegacyMessageId::from(Uuid::new_v4()),
        )],
    );
    let mutation_lock_path = sentinel_path(&mutation_fixture.primary_inbox_path(PRIMARY_AGENT));
    let mutation_lock_file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&mutation_lock_path)
        .expect("open mutation lock file");
    mutation_lock_file
        .lock_exclusive()
        .expect("hold mutation lock");
    let mut mutation_query = mutation_fixture.read_query(PRIMARY_AGENT);
    mutation_query.ack_activation_mode = AckActivationMode::PromoteDisplayedUnread;
    let error = read_mail(mutation_query, &observability).expect_err("lock timeout");
    assert_eq!(error.code, AtmErrorCode::MailboxLockTimeout);

    let no_mutation_fixture = Fixture::new();
    no_mutation_fixture.write_primary_inbox(
        PRIMARY_AGENT,
        &[read_message(
            TEAM_LEAD,
            "already read",
            LegacyMessageId::from(Uuid::new_v4()),
        )],
    );
    let no_mutation_lock_path =
        sentinel_path(&no_mutation_fixture.primary_inbox_path(PRIMARY_AGENT));
    let no_mutation_lock_file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&no_mutation_lock_path)
        .expect("open no-mutation lock file");
    no_mutation_lock_file
        .lock_exclusive()
        .expect("hold no-mutation lock");
    let mut no_mutation_query = no_mutation_fixture.read_query(PRIMARY_AGENT);
    no_mutation_query.ack_activation_mode = AckActivationMode::PromoteDisplayedUnread;
    no_mutation_query.selection_mode = ReadSelection::All;
    let started = Instant::now();
    let outcome = read_mail(no_mutation_query, &observability).expect("read without mutation");
    assert_eq!(outcome.count, 1);
    assert_eq!(outcome.messages[0].envelope.text, "already read");
    assert!(
        started.elapsed() < TEST_LOCK_BUDGET_CEILING,
        "retain only a coarse non-blocking budget here; recv_timeout-based tests above already cover deadlock detection"
    );
}

#[test]
#[serial]
fn read_mail_updates_sidecar_for_ulid_authored_message_without_mutating_inbox() {
    let fixture = Fixture::new();
    let observability = NullObservability;

    // Criterion (a) is verified through the standard send path rather than a
    // direct helper call: send_mail internally assigns metadata.atm.messageId
    // via the private workflow::set_atm_message_id path before read_mail runs.
    send_mail(
        fixture.send_request(TEAM_LEAD, &qualified(PRIMARY_AGENT), "hello sidecar"),
        &observability,
    )
    .expect("send ULID-authored message");

    let inbox_before = fs::read_to_string(fixture.primary_inbox_path(PRIMARY_AGENT))
        .expect("raw inbox before read");
    let physical_before = find_inbox_json_line(&inbox_before, "hello sidecar");
    let atm_message_id = physical_before["metadata"]["atm"]["messageId"]
        .as_str()
        .expect("atm message id")
        .to_string();
    assert_eq!(physical_before["read"], false);

    let mut read_query = fixture.read_query(PRIMARY_AGENT);
    read_query.ack_activation_mode = AckActivationMode::PromoteDisplayedUnread;
    let outcome = read_mail(read_query, &observability).expect("read mail");
    assert!(
        outcome
            .messages
            .iter()
            .any(|message| message.envelope.text == "hello sidecar"),
        "read outcome should include the ULID-authored message"
    );

    let inbox_after = fs::read_to_string(fixture.primary_inbox_path(PRIMARY_AGENT))
        .expect("raw inbox after read");
    assert_eq!(inbox_after, inbox_before);
    let physical_after = find_inbox_json_line(&inbox_after, "hello sidecar");
    assert_eq!(
        physical_after["metadata"]["atm"]["messageId"],
        atm_message_id
    );
    assert_eq!(physical_after["read"], false);
    assert!(
        !sentinel_path(&fixture.primary_inbox_path(PRIMARY_AGENT)).exists(),
        "read-only ULID sidecar path must not leave a lock sentinel behind",
    );

    let workflow = fixture.workflow_state_contents(PRIMARY_AGENT);
    assert_eq!(
        workflow["messages"][format!("atm:{atm_message_id}")]["read"],
        true
    );
}

#[test]
#[serial]
fn clear_fails_closed_on_synthetic_source_discovery_fault() {
    let _env_lock = env_lock().lock().expect("env lock");
    let _fault = EnvGuard::set_raw("ATM_TEST_FORCE_SOURCE_DISCOVERY_FAULT", "1");
    let fixture = Fixture::new();
    let observability = NullObservability;
    fixture.write_origin_inbox(
        PRIMARY_AGENT,
        "host-a",
        &[read_message(
            SECONDARY_AGENT,
            "origin read a",
            LegacyMessageId::from(Uuid::new_v4()),
        )],
    );
    let before_primary = fs::read_to_string(fixture.primary_inbox_path(PRIMARY_AGENT))
        .expect("primary inbox before");
    let before_origin = fs::read_to_string(fixture.origin_inbox_path(PRIMARY_AGENT, "host-a"))
        .expect("origin inbox before");

    let error = clear_mail(fixture.clear_query(PRIMARY_AGENT), &observability).expect_err("fault");

    assert_eq!(error.code, AtmErrorCode::MailboxReadFailed);
    assert_eq!(
        fs::read_to_string(fixture.primary_inbox_path(PRIMARY_AGENT)).expect("primary inbox after"),
        before_primary
    );
    assert_eq!(
        fs::read_to_string(fixture.origin_inbox_path(PRIMARY_AGENT, "host-a"))
            .expect("origin inbox after"),
        before_origin
    );
}

#[test]
#[serial]
fn send_reports_non_contention_lock_failures_without_timeout() {
    let _env_lock = env_lock().lock().expect("env lock");
    let _fault = EnvGuard::set_raw("ATM_TEST_FORCE_LOCK_NON_CONTENTION_ERROR", "1");
    let fixture = Fixture::new();
    let observability = NullObservability;
    let started = Instant::now();

    let error = send_mail(
        fixture.send_request(TEAM_LEAD, &qualified(PRIMARY_AGENT), "lock failure"),
        &observability,
    )
    .expect_err("non-contention lock failure");

    assert_eq!(error.code, AtmErrorCode::MailboxLockFailed);
    assert!(
        started.elapsed() < TEST_LOCK_BUDGET_CEILING,
        "retain only a coarse non-blocking budget here; recv_timeout-based tests above already cover deadlock detection"
    );
}

enum CommandOp {
    Read(ReadQuery, Arc<NullObservability>),
    Clear(ClearQuery, Arc<NullObservability>),
}

// Serializes process-environment mutation inside this test module. This is
// process-local only; it does not coordinate with other test processes.
fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    // These tests mutate the process-global `ATM_TEST_MAILBOX_LOCK_TIMEOUT_MS`,
    // `ATM_TEST_FORCE_SOURCE_DISCOVERY_FAULT`, and
    // `ATM_TEST_FORCE_LOCK_NON_CONTENTION_ERROR` knobs while exercising
    // mailbox lock behavior. Keep a single process-wide mutex in addition to
    // `#[serial]` so a poisoned lock fails the suite closed instead of silently
    // continuing with inconsistent shared state.
    LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvGuard {
    key: &'static str,
    original: Option<OsString>,
}

impl EnvGuard {
    fn set_raw(key: &'static str, value: &str) -> Self {
        let original = std::env::var_os(key);
        set_env_var(key, value);
        Self { key, original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match self.original.take() {
            Some(value) => set_env_var(self.key, value),
            None => remove_env_var(self.key),
        }
    }
}

fn set_env_var<K: AsRef<OsStr>, V: AsRef<OsStr>>(key: K, value: V) {
    // SAFETY: these tests take a process-wide mutex and use #[serial] before
    // mutating the environment, so the mutation is serialized within this
    // process.
    unsafe { std::env::set_var(key, value) }
}

fn remove_env_var<K: AsRef<OsStr>>(key: K) {
    // SAFETY: these tests take a process-wide mutex and use #[serial] before
    // mutating the environment, so the mutation is serialized within this
    // process.
    unsafe { std::env::remove_var(key) }
}

struct Fixture {
    tempdir: TempDir,
    arch_message_id: LegacyMessageId,
    qa_message_id: LegacyMessageId,
}

impl Fixture {
    fn new() -> Self {
        let tempdir = tempfile::tempdir().expect("tempdir");
        create_team_with_config(
            tempdir.path(),
            PRIMARY_TEAM,
            &[TEAM_LEAD, PRIMARY_AGENT, SECONDARY_AGENT],
        );

        let arch_message_id = LegacyMessageId::from_atm_message_id(AtmMessageId::new());
        let qa_message_id = LegacyMessageId::from_atm_message_id(AtmMessageId::new());

        let fixture = Self {
            tempdir,
            arch_message_id,
            qa_message_id,
        };
        fixture.write_primary_inbox(
            PRIMARY_AGENT,
            &[pending_ack_message(
                SECONDARY_AGENT,
                "arch pending",
                arch_message_id,
                PRIMARY_TEAM,
            )],
        );
        fixture.write_primary_inbox(
            SECONDARY_AGENT,
            &[pending_ack_message(
                PRIMARY_AGENT,
                &format!("{SECONDARY_AGENT} pending"),
                qa_message_id,
                PRIMARY_TEAM,
            )],
        );

        fixture
    }

    fn ack_request(
        &self,
        actor: &str,
        message_id: LegacyMessageId,
        reply_body: &str,
    ) -> AckRequest {
        AckRequest {
            home_dir: self.tempdir.path().to_path_buf(),
            current_dir: self.tempdir.path().to_path_buf(),
            actor_override: Some(actor.parse().expect("actor")),
            team_override: Some(PRIMARY_TEAM.parse().expect("team")),
            message_id,
            reply_body: reply_body.to_string(),
        }
    }

    fn clear_query(&self, actor: &str) -> ClearQuery {
        ClearQuery {
            home_dir: self.tempdir.path().to_path_buf(),
            current_dir: self.tempdir.path().to_path_buf(),
            actor_override: Some(actor.parse().expect("actor")),
            target_address: None,
            team_override: Some(PRIMARY_TEAM.parse().expect("team")),
            older_than: None,
            idle_only: false,
            dry_run: false,
        }
    }

    fn read_query(&self, actor: &str) -> ReadQuery {
        ReadQuery::new(
            self.tempdir.path().to_path_buf(),
            self.tempdir.path().to_path_buf(),
            Some(actor),
            None,
            Some(PRIMARY_TEAM),
            ReadSelection::Actionable,
            false,
            false,
            AckActivationMode::ReadOnly,
            None,
            None,
            None,
            None,
        )
        .expect("read query")
    }

    fn send_request(&self, sender: &str, to: &str, text: &str) -> SendRequest {
        SendRequest::new(
            self.tempdir.path().to_path_buf(),
            self.tempdir.path().to_path_buf(),
            Some(sender),
            to,
            Some(PRIMARY_TEAM),
            SendMessageSource::Inline(text.to_string()),
            None,
            false,
            None,
            false,
        )
        .expect("send request")
    }

    fn inbox_contents(&self, agent: &str) -> Vec<MessageEnvelope> {
        self.inbox_contents_for_team(PRIMARY_TEAM, agent)
    }

    fn origin_inbox_contents(&self, agent: &str, suffix: &str) -> Vec<MessageEnvelope> {
        read_jsonl(self.origin_inbox_path(agent, suffix))
    }

    fn workflow_state_contents(&self, agent: &str) -> serde_json::Value {
        self.workflow_state_contents_for_team(PRIMARY_TEAM, agent)
    }

    fn inbox_contents_for_team(&self, team: &str, agent: &str) -> Vec<MessageEnvelope> {
        read_jsonl(self.primary_inbox_path_for_team(team, agent))
    }

    fn workflow_state_contents_for_team(&self, team: &str, agent: &str) -> serde_json::Value {
        let raw = fs::read_to_string(self.workflow_state_path_for_team(team, agent))
            .expect("workflow contents");
        serde_json::from_str(&raw).expect("workflow json")
    }

    fn write_workflow_state(&self, agent: &str, value: serde_json::Value) {
        let path = self.workflow_state_path(agent);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("workflow dir");
        }
        fs::write(path, serde_json::to_vec(&value).expect("workflow json"))
            .expect("write workflow");
    }

    fn write_primary_inbox(&self, agent: &str, messages: &[MessageEnvelope]) {
        write_inbox(&self.primary_inbox_path(agent), messages);
    }

    fn write_primary_inbox_for_team(&self, team: &str, agent: &str, messages: &[MessageEnvelope]) {
        write_inbox(&self.primary_inbox_path_for_team(team, agent), messages);
    }

    fn write_origin_inbox(&self, agent: &str, suffix: &str, messages: &[MessageEnvelope]) {
        write_inbox(&self.origin_inbox_path(agent, suffix), messages);
    }

    fn primary_inbox_path(&self, agent: &str) -> std::path::PathBuf {
        self.primary_inbox_path_for_team(PRIMARY_TEAM, agent)
    }

    fn primary_inbox_path_for_team(&self, team: &str, agent: &str) -> std::path::PathBuf {
        self.team_dir_for(team)
            .join("inboxes")
            .join(format!("{agent}.json"))
    }

    fn origin_inbox_path(&self, agent: &str, suffix: &str) -> std::path::PathBuf {
        self.tempdir
            .path()
            .join(".claude")
            .join("teams")
            .join(PRIMARY_TEAM)
            .join("inboxes")
            .join(format!("{agent}.{suffix}.json"))
    }

    fn workflow_state_path(&self, agent: &str) -> std::path::PathBuf {
        self.workflow_state_path_for_team(PRIMARY_TEAM, agent)
    }

    fn workflow_state_path_for_team(&self, team: &str, agent: &str) -> std::path::PathBuf {
        self.team_dir_for(team)
            .join(".atm-state")
            .join("workflow")
            .join(format!("{agent}.json"))
    }

    fn team_dir_for(&self, team: &str) -> std::path::PathBuf {
        self.tempdir.path().join(".claude").join("teams").join(team)
    }

    fn create_team_without_config(&self, team: &str) {
        fs::create_dir_all(self.team_dir_for(team).join("inboxes")).expect("team inboxes");
    }
}

fn create_team_with_config(home_dir: &std::path::Path, team: &str, members: &[&str]) {
    let team_dir = home_dir.join(".claude").join("teams").join(team);
    fs::create_dir_all(team_dir.join("inboxes")).expect("inboxes");
    let config = TeamConfig {
        members: members
            .iter()
            .map(|name| AgentMember::with_name((*name).parse().expect("agent")))
            .collect(),
        ..Default::default()
    };
    fs::write(
        team_dir.join("config.json"),
        serde_json::to_vec(&config).expect("team config"),
    )
    .expect("write team config");
}

fn message_atm_id(message: &MessageEnvelope) -> String {
    message
        .atm_message_id()
        .map(|message_id| message_id.to_string())
        .as_deref()
        .expect("atm message id")
        .to_string()
}

fn read_jsonl(path: std::path::PathBuf) -> Vec<MessageEnvelope> {
    let raw = fs::read_to_string(path).expect("inbox contents");
    if raw.trim().is_empty() {
        return Vec::new();
    }

    let values: Vec<serde_json::Value> = match raw.chars().find(|ch| !ch.is_whitespace()) {
        Some('[') => serde_json::from_str(&raw).expect("json array"),
        _ => raw
            .lines()
            .map(|line| serde_json::from_str(line).expect("json line"))
            .collect(),
    };

    values
        .into_iter()
        .map(|mut value| {
            hydrate_legacy_fields_from_metadata(&mut value);
            serde_json::from_value(value).expect("message envelope")
        })
        .collect()
}

fn find_inbox_json_line(raw: &str, text: &str) -> serde_json::Value {
    let values: Vec<serde_json::Value> = if raw.trim().is_empty() {
        Vec::new()
    } else {
        match raw.chars().find(|ch| !ch.is_whitespace()) {
            Some('[') => serde_json::from_str(raw).expect("json array"),
            _ => raw
                .lines()
                .map(|line| serde_json::from_str(line).expect("json line"))
                .collect(),
        }
    };

    values
        .into_iter()
        .find(|line| line["text"] == text)
        .expect("matching inbox json line")
}

fn write_inbox(path: &std::path::Path, messages: &[MessageEnvelope]) {
    let raw = serde_json::to_string_pretty(messages).expect("json array");
    fs::write(path, format!("{raw}\n")).expect("write inbox");
}

fn sentinel_path(path: &std::path::Path) -> std::path::PathBuf {
    let mut os = path.as_os_str().to_os_string();
    os.push(".lock");
    std::path::PathBuf::from(os)
}

fn pending_ack_message(
    from: &str,
    text: &str,
    message_id: LegacyMessageId,
    source_team: &str,
) -> MessageEnvelope {
    let mut extra = serde_json::Map::new();
    let mut metadata = serde_json::Map::new();
    let mut atm = serde_json::Map::new();
    let atm_message_id = message_id.into_atm_message_id();
    atm.insert(
        "messageId".to_string(),
        serde_json::Value::String(atm_message_id.to_string()),
    );
    atm.insert(
        "sourceTeam".to_string(),
        serde_json::Value::String(source_team.to_string()),
    );
    metadata.insert("atm".to_string(), serde_json::Value::Object(atm));
    extra.insert("metadata".to_string(), serde_json::Value::Object(metadata));
    assert_eq!(
        LegacyMessageId::from_atm_message_id(message_atm_id_from_extra(&extra).expect("atm id")),
        message_id,
        "mailbox fixture metadata.atm.messageId must match legacy message_id",
    );

    MessageEnvelope {
        from: from.parse::<AgentName>().expect("agent"),
        text: text.to_string(),
        timestamp: IsoTimestamp::from_datetime(Utc::now()),
        read: true,
        source_team: Some(source_team.parse::<TeamName>().expect("team")),
        summary: None,
        message_id: Some(message_id),
        pending_ack_at: Some(IsoTimestamp::from_datetime(Utc::now())),
        acknowledged_at: None,
        acknowledges_message_id: None,
        task_id: None,
        extra,
    }
}

fn read_message(from: &str, text: &str, message_id: LegacyMessageId) -> MessageEnvelope {
    let mut extra = serde_json::Map::new();
    let mut metadata = serde_json::Map::new();
    let mut atm = serde_json::Map::new();
    let atm_message_id = message_id.into_atm_message_id();
    atm.insert(
        "messageId".to_string(),
        serde_json::Value::String(atm_message_id.to_string()),
    );
    atm.insert(
        "sourceTeam".to_string(),
        serde_json::Value::String(PRIMARY_TEAM.to_string()),
    );
    metadata.insert("atm".to_string(), serde_json::Value::Object(atm));
    extra.insert("metadata".to_string(), serde_json::Value::Object(metadata));
    assert_eq!(
        LegacyMessageId::from_atm_message_id(message_atm_id_from_extra(&extra).expect("atm id")),
        message_id,
        "mailbox fixture metadata.atm.messageId must match legacy message_id",
    );

    MessageEnvelope {
        from: from.parse::<AgentName>().expect("agent"),
        text: text.to_string(),
        timestamp: IsoTimestamp::from_datetime(Utc::now()),
        read: true,
        source_team: Some(PRIMARY_TEAM.parse::<TeamName>().expect("team")),
        summary: None,
        message_id: Some(message_id),
        pending_ack_at: None,
        acknowledged_at: None,
        acknowledges_message_id: None,
        task_id: None,
        extra,
    }
}

fn unread_message(from: &str, text: &str, message_id: LegacyMessageId) -> MessageEnvelope {
    let mut extra = serde_json::Map::new();
    let mut metadata = serde_json::Map::new();
    let mut atm = serde_json::Map::new();
    let atm_message_id = message_id.into_atm_message_id();
    atm.insert(
        "messageId".to_string(),
        serde_json::Value::String(atm_message_id.to_string()),
    );
    atm.insert(
        "sourceTeam".to_string(),
        serde_json::Value::String(PRIMARY_TEAM.to_string()),
    );
    metadata.insert("atm".to_string(), serde_json::Value::Object(atm));
    extra.insert("metadata".to_string(), serde_json::Value::Object(metadata));
    assert_eq!(
        LegacyMessageId::from_atm_message_id(message_atm_id_from_extra(&extra).expect("atm id")),
        message_id,
        "mailbox fixture metadata.atm.messageId must match legacy message_id",
    );

    MessageEnvelope {
        from: from.parse::<AgentName>().expect("agent"),
        text: text.to_string(),
        timestamp: IsoTimestamp::from_datetime(Utc::now()),
        read: false,
        source_team: Some(PRIMARY_TEAM.parse::<TeamName>().expect("team")),
        summary: None,
        message_id: Some(message_id),
        pending_ack_at: None,
        acknowledged_at: None,
        acknowledges_message_id: None,
        task_id: None,
        extra,
    }
}

fn message_atm_id_from_extra(
    extra: &serde_json::Map<String, serde_json::Value>,
) -> Option<AtmMessageId> {
    extra
        .get("metadata")
        .and_then(serde_json::Value::as_object)
        .and_then(|metadata| metadata.get("atm"))
        .and_then(serde_json::Value::as_object)
        .and_then(|atm| atm.get("messageId"))
        .and_then(serde_json::Value::as_str)
        .and_then(|value| value.parse().ok())
}
