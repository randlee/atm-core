use std::fs;
#[cfg(unix)]
use std::fs::OpenOptions;
use std::sync::{Arc, Barrier, mpsc};
use std::thread;
use std::time::Duration;
use std::time::Instant;

use atm_core::ack::{AckRequest, ack_mail};
use atm_core::clear::{ClearQuery, clear_mail};
#[cfg(unix)]
use atm_core::error::AtmErrorCode;
use atm_core::observability::NullObservability;
use atm_core::read::{ReadQuery, read_mail};
use atm_core::roles::ROLE_TEAM_LEAD;
use atm_core::schema::{AgentMember, AtmMessageId, MessageEnvelope, TeamConfig};
use atm_core::send::{SendMessageSource, SendRequest, send_mail};
#[cfg(unix)]
use atm_core::test_support::EnvGuard;
use atm_core::types::{AckActivationMode, AgentName, IsoTimestamp, ReadSelection, TeamName};
use atm_runtime_test_support::{
    SqliteRuntimeGuard, install_sqlite_retained_runtime_factory,
    open_sqlite_boundary,
};
#[cfg(unix)]
use atm_runtime_test_support::hold_sqlite_writer_lock;
use chrono::Utc;
#[cfg(unix)]
use fs2::FileExt;
use tempfile::TempDir;
use uuid::Uuid;

// Test-side ceiling guard only; production lock timeout defaults to 5s per
// architecture §18.3.
#[cfg(unix)]
const TEST_LOCK_BUDGET_CEILING: Duration = Duration::from_secs(10);
const TEST_RESULT_TIMEOUT: Duration = Duration::from_secs(30);
const TEST_TEAM: &str = "test-team";
const TEST_SENDER: &str = "sender-a";
const TEST_RECIPIENT: &str = "recipient";
const TEST_QA: &str = "qa-a";
const PRIMARY_TEAM: &str = TEST_TEAM;
const PRIMARY_AGENT: &str = TEST_SENDER;
const SECONDARY_AGENT: &str = TEST_QA;
const TEAM_LEAD: &str = ROLE_TEAM_LEAD;

fn qualified(agent: &str) -> String {
    format!("{agent}@{PRIMARY_TEAM}")
}

#[test]
#[serial_test::serial(env)]
fn concurrent_ack_on_overlapping_inbox_sets_completes_without_deadlock() {
    let fixture = Fixture::new();
    let observability = Arc::new(NullObservability);
    let barrier = Arc::new(Barrier::new(3));
    let (tx, rx) = mpsc::channel();
    fixture.write_primary_inbox(
        PRIMARY_AGENT,
        &[pending_ack_message_at(
            SECONDARY_AGENT,
            "arch pending",
            fixture.arch_message_id,
            PRIMARY_TEAM,
            Utc::now() - chrono::Duration::seconds(1),
        )],
    );
    fixture.write_primary_inbox(
        SECONDARY_AGENT,
        &[pending_ack_message_at(
            PRIMARY_AGENT,
            &format!("{SECONDARY_AGENT} pending"),
            fixture.qa_message_id,
            PRIMARY_TEAM,
            Utc::now() - chrono::Duration::seconds(1),
        )],
    );

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
        .recv_timeout(TEST_RESULT_TIMEOUT)
        .expect("first ack result");
    let second = rx
        .recv_timeout(TEST_RESULT_TIMEOUT)
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
#[serial_test::serial(env)]
fn concurrent_send_with_ack_and_clear_completes_without_deadlock_or_data_loss() {
    let observability = Arc::new(NullObservability);

    let clear_fixture = Fixture::new();
    clear_fixture.write_primary_inbox(
        PRIMARY_AGENT,
        &[read_message(
            SECONDARY_AGENT,
            "clearable history entry",
            AtmMessageId::from(Uuid::new_v4()),
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
        .recv_timeout(TEST_RESULT_TIMEOUT)
        .expect("first send/clear result");
    let second = rx
        .recv_timeout(TEST_RESULT_TIMEOUT)
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
    drop(clear_fixture);
    let ack_fixture = Fixture::new();
    let pending_message_id = AtmMessageId::from(Uuid::new_v4());
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
        .recv_timeout(TEST_RESULT_TIMEOUT)
        .expect("first send/ack result");
    let second = rx
        .recv_timeout(TEST_RESULT_TIMEOUT)
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
        arch_workflow["messages"]
            .as_object()
            .is_some_and(|messages| !messages.is_empty()),
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
#[serial_test::serial(env)]
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
        .recv_timeout(TEST_RESULT_TIMEOUT)
        .expect("first send result");
    let second = rx
        .recv_timeout(TEST_RESULT_TIMEOUT)
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

    let plain_workflow_key = message_workflow_key(plain_message);
    let task_workflow_key = message_workflow_key(task_message);
    let workflow = fixture.workflow_state_contents(PRIMARY_AGENT);
    assert!(
        workflow["messages"][plain_workflow_key.clone()]
            .as_object()
            .is_some(),
        "plain workflow entry missing: {workflow:?}"
    );
    assert!(
        workflow["messages"][plain_workflow_key]["pendingAckAt"].is_null(),
        "plain workflow state should not require ack: {workflow:?}"
    );
    assert!(
        workflow["messages"][task_workflow_key]["pendingAckAt"]
            .as_str()
            .is_some(),
        "task workflow state should preserve pending ack: {workflow:?}"
    );
}

#[test]
#[serial_test::serial(env)]
fn concurrent_same_recipient_sends_preserve_preseeded_workflow_entries() {
    let fixture = Fixture::new();
    let observability = Arc::new(NullObservability);
    let preseeded_workflow_key = "atm:01KRFK5QTF2R6NRS3Q0F8Z9K0S";
    fixture.write_workflow_state(
        PRIMARY_AGENT,
        serde_json::json!({
            "messages": {
                preseeded_workflow_key: {
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
        .recv_timeout(TEST_RESULT_TIMEOUT)
        .expect("first send result");
    let second = rx
        .recv_timeout(TEST_RESULT_TIMEOUT)
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
        workflow["messages"][preseeded_workflow_key]
            .as_object()
            .is_some(),
        "preseeded workflow entry was dropped: {workflow:?}"
    );
    assert!(
        workflow["messages"][message_workflow_key(first_message)]
            .as_object()
            .is_some(),
        "first send workflow entry missing after concurrent update: {workflow:?}"
    );
    assert!(
        workflow["messages"][message_workflow_key(second_message)]
            .as_object()
            .is_some(),
        "second send workflow entry missing after concurrent update: {workflow:?}"
    );
}

#[test]
#[serial_test::serial(env)]
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

    let notice = fixture.wait_for_missing_config_notice("broken-dev");
    assert_eq!(notice.from, "atm-identity-missing");
    assert_eq!(notice.source_team.as_deref(), Some("broken-dev"));
    let workflow = fixture.wait_for_workflow_state_for_message("broken-dev", TEAM_LEAD, &notice);
    assert!(
        workflow["messages"][message_workflow_key(&notice)]
            .as_object()
            .is_some(),
        "missing-config workflow entry missing: {workflow:?}"
    );
}

#[test]
#[serial_test::serial(env)]
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
        .recv_timeout(TEST_RESULT_TIMEOUT)
        .expect("first send result");
    let second = rx
        .recv_timeout(TEST_RESULT_TIMEOUT)
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
    let notice = fixture.wait_for_missing_config_notice("broken-dev");
    let workflow = fixture.wait_for_workflow_state_for_message("broken-dev", TEAM_LEAD, &notice);
    assert!(
        workflow["messages"][message_workflow_key(&notice)]["pendingAckAt"].is_null(),
        "missing-config notice workflow state missing after concurrent send: {workflow:?}"
    );
}

#[test]
#[serial_test::serial(env)]
fn multi_source_read_and_clear_complete_without_deadlock() {
    let fixture = Fixture::new();
    let observability = Arc::new(NullObservability);
    fixture.write_primary_inbox(
        PRIMARY_AGENT,
        &[unread_message(
            TEAM_LEAD,
            "primary unread",
            AtmMessageId::from(Uuid::new_v4()),
        )],
    );
    fixture.write_origin_inbox(
        PRIMARY_AGENT,
        "host-b",
        &[unread_message(
            SECONDARY_AGENT,
            "origin unread b",
            AtmMessageId::from(Uuid::new_v4()),
        )],
    );
    fixture.write_origin_inbox(
        PRIMARY_AGENT,
        "host-a",
        &[read_message(
            SECONDARY_AGENT,
            "origin read a",
            AtmMessageId::from(Uuid::new_v4()),
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
        .recv_timeout(TEST_RESULT_TIMEOUT)
        .expect("first read/clear result");
    let second = rx
        .recv_timeout(TEST_RESULT_TIMEOUT)
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
#[cfg(unix)]
#[serial_test::serial(env)]
fn send_times_out_under_bounded_lock_contention() {
    let _timeout = EnvGuard::set_raw("ATM_TEST_MAILBOX_LOCK_TIMEOUT_MS", "100");
    let fixture = Fixture::new();
    let observability = NullObservability;
    fixture.write_primary_inbox(PRIMARY_AGENT, &[]);
    let _writer_lock = hold_sqlite_writer_lock(fixture.sqlite_db_path()).expect("hold sqlite lock");

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
#[cfg(unix)]
#[serial_test::serial(env)]
fn clear_dry_run_does_not_wait_on_mailbox_lock() {
    let fixture = Fixture::new();
    let observability = NullObservability;
    fixture.write_primary_inbox(
        PRIMARY_AGENT,
        &[unread_message(
            TEAM_LEAD,
            "read without lock",
            AtmMessageId::from(Uuid::new_v4()),
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
#[cfg(unix)]
#[serial_test::serial(env)]
fn read_store_backed_display_mutation_ignores_mailbox_file_lock() {
    let observability = NullObservability;

    let mutation_fixture = Fixture::new();
    mutation_fixture.write_primary_inbox(
        PRIMARY_AGENT,
        &[unread_message(
            TEAM_LEAD,
            "needs mark-read",
            AtmMessageId::from(Uuid::new_v4()),
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
    let mutation_query = mutation_fixture
        .read_query(PRIMARY_AGENT)
        .with_ack_activation_mode(AckActivationMode::PromoteDisplayedUnread);
    let mutation_outcome = read_mail(mutation_query, &observability).expect("read with mutation");
    assert_eq!(mutation_outcome.count, 1);
    assert!(mutation_outcome.mutation_applied);
    drop(mutation_fixture);
    let no_mutation_fixture = Fixture::new();
    no_mutation_fixture.write_primary_inbox(
        PRIMARY_AGENT,
        &[read_message(
            TEAM_LEAD,
            "already read",
            AtmMessageId::from(Uuid::new_v4()),
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
    let no_mutation_query = no_mutation_fixture
        .read_query(PRIMARY_AGENT)
        .with_ack_activation_mode(AckActivationMode::PromoteDisplayedUnread)
        .with_selection_mode(ReadSelection::All);
    let started = Instant::now();
    let outcome = read_mail(no_mutation_query, &observability).expect("read without mutation");
    assert_eq!(outcome.count, 1);
    assert_eq!(
        outcome.message.expect("selected message").envelope.text,
        "already read"
    );
    assert!(
        started.elapsed() < TEST_LOCK_BUDGET_CEILING,
        "retain only a coarse non-blocking budget here; recv_timeout-based tests above already cover deadlock detection"
    );
}

#[test]
#[serial_test::serial(env)]
fn read_mail_updates_sidecar_for_ulid_authored_message_without_mutating_inbox() {
    let fixture = Fixture::new();
    let observability = NullObservability;

    send_mail(
        fixture.send_request(TEAM_LEAD, &qualified(PRIMARY_AGENT), "hello sidecar"),
        &observability,
    )
    .expect("send ULID-authored message");

    let inbox_before = fs::read_to_string(fixture.primary_inbox_path(PRIMARY_AGENT))
        .expect("raw inbox before read");
    let physical_before = find_inbox_json_line(&inbox_before, "hello sidecar");
    let message_id = physical_before["message_id"]
        .as_str()
        .expect("message id")
        .to_string();
    let logical_message_id = message_id
        .parse::<AtmMessageId>()
        .expect("logical message id");
    assert_eq!(physical_before["read"], false);

    let read_query = fixture
        .read_query(PRIMARY_AGENT)
        .with_ack_activation_mode(AckActivationMode::PromoteDisplayedUnread);
    let outcome = read_mail(read_query, &observability).expect("read mail");
    assert!(
        outcome
            .message
            .as_ref()
            .is_some_and(|message| message.envelope.text == "hello sidecar"),
        "read outcome should include the ULID-authored message"
    );

    let inbox_after = fs::read_to_string(fixture.primary_inbox_path(PRIMARY_AGENT))
        .expect("raw inbox after read");
    assert_eq!(inbox_after, inbox_before);
    let physical_after = find_inbox_json_line(&inbox_after, "hello sidecar");
    assert_eq!(physical_after["message_id"], message_id);
    assert_eq!(physical_after["read"], false);
    assert!(
        !sentinel_path(&fixture.primary_inbox_path(PRIMARY_AGENT)).exists(),
        "read-only ULID sidecar path must not leave a lock sentinel behind",
    );
    assert!(
        outcome
            .message
            .as_ref()
            .is_some_and(|message| message.envelope.read),
        "read outcome should surface the promoted read state"
    );

    let workflow = fixture.workflow_state_contents(PRIMARY_AGENT);
    assert!(
        workflow["messages"][format!("atm:{logical_message_id}")]
            .as_object()
            .is_some(),
        "workflow entry missing for ULID-authored message: {workflow:?}"
    );
}

#[test]
#[cfg(unix)]
#[serial_test::serial(env)]
fn clear_ignores_synthetic_source_discovery_fault_in_store_only_mode() {
    let _fault = EnvGuard::set_raw("ATM_TEST_FORCE_SOURCE_DISCOVERY_FAULT", "1");
    let fixture = Fixture::new();
    let observability = NullObservability;
    fixture.write_primary_inbox(PRIMARY_AGENT, &[]);
    fixture.write_origin_inbox(
        PRIMARY_AGENT,
        "host-a",
        &[read_message(
            SECONDARY_AGENT,
            "origin read a",
            AtmMessageId::from(Uuid::new_v4()),
        )],
    );
    let before_primary = fs::read_to_string(fixture.primary_inbox_path(PRIMARY_AGENT))
        .expect("primary inbox before");
    let before_origin = fs::read_to_string(fixture.origin_inbox_path(PRIMARY_AGENT, "host-a"))
        .expect("origin inbox before");

    let outcome = clear_mail(fixture.clear_query(PRIMARY_AGENT), &observability).expect("clear");
    assert_eq!(outcome.removed_total, 0);
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
#[cfg(unix)]
#[serial_test::serial(env)]
fn send_reports_non_contention_lock_failures_without_timeout() {
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

struct Fixture {
    tempdir: TempDir,
    _sqlite_runtime_guard: SqliteRuntimeGuard,
    arch_message_id: AtmMessageId,
    qa_message_id: AtmMessageId,
}

impl Fixture {
    fn new() -> Self {
        install_sqlite_retained_runtime_factory();
        let tempdir = tempfile::tempdir().expect("tempdir");
        let sqlite_db_path = tempdir.path().join("runtime").join("mail.sqlite3");
        let sqlite_runtime_guard = SqliteRuntimeGuard::install(sqlite_db_path);
        create_team_with_config(
            tempdir.path(),
            PRIMARY_TEAM,
            &[TEAM_LEAD, PRIMARY_AGENT, SECONDARY_AGENT],
        );

        let arch_message_id = AtmMessageId::new();
        let qa_message_id = AtmMessageId::new();

        Self {
            tempdir,
            _sqlite_runtime_guard: sqlite_runtime_guard,
            arch_message_id,
            qa_message_id,
        }
    }

    fn ack_request(&self, actor: &str, message_id: AtmMessageId, reply_body: &str) -> AckRequest {
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

    fn try_workflow_state_contents_for_team(
        &self,
        team: &str,
        agent: &str,
    ) -> Result<serde_json::Value, String> {
        let path = self.workflow_state_path_for_team(team, agent);
        let raw = fs::read_to_string(&path)
            .map_err(|error| format!("read {}: {error}", path.display()))?;
        serde_json::from_str(&raw).map_err(|error| format!("parse {}: {error}", path.display()))
    }

    fn wait_for_missing_config_notice(&self, team: &str) -> MessageEnvelope {
        let deadline = Instant::now() + TEST_RESULT_TIMEOUT;
        let mut attempts = 0usize;
        loop {
            if let Some(notice) = self
                .inbox_contents_for_team(team, TEAM_LEAD)
                .into_iter()
                .find(|message| {
                    message.from.as_str() == "atm-identity-missing"
                        && message.source_team.as_deref() == Some(team)
                })
            {
                return notice;
            }
            if Instant::now() >= deadline {
                let notices = self.inbox_contents_for_team(team, TEAM_LEAD);
                panic!(
                    "missing-config notice not observed before timeout after {attempts} attempts: {notices:?}"
                );
            }
            attempts = attempts.saturating_add(1);
            // lint-fixed-sleep: allow-next-line
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn wait_for_workflow_state_for_message(
        &self,
        team: &str,
        agent: &str,
        message: &MessageEnvelope,
    ) -> serde_json::Value {
        let workflow_path = self.workflow_state_path_for_team(team, agent);
        let message_key = message_workflow_key(message);
        let deadline = Instant::now() + TEST_RESULT_TIMEOUT;
        let mut attempts = 0usize;
        let mut last_error = None;
        loop {
            if workflow_path.exists() {
                match self.try_workflow_state_contents_for_team(team, agent) {
                    Ok(workflow) => {
                        last_error = None;
                        if workflow["messages"][&message_key].as_object().is_some() {
                            return workflow;
                        }
                    }
                    Err(error) => {
                        last_error = Some(error);
                    }
                }
            }
            if Instant::now() >= deadline {
                let workflow = if workflow_path.exists() {
                    self.try_workflow_state_contents_for_team(team, agent)
                        .unwrap_or_else(|error| serde_json::json!({ "workflow_error": error }))
                } else {
                    serde_json::json!({})
                };
                panic!(
                    "workflow state for missing-config notice not observed before timeout after {attempts} attempts: {workflow:?}; last_error={last_error:?}"
                );
            }
            attempts = attempts.saturating_add(1);
            // lint-fixed-sleep: allow-next-line
            thread::sleep(Duration::from_millis(5));
        }
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
        self.write_primary_inbox_for_team(PRIMARY_TEAM, agent, messages);
    }

    fn write_primary_inbox_for_team(&self, team: &str, agent: &str, messages: &[MessageEnvelope]) {
        write_inbox(&self.primary_inbox_path_for_team(team, agent), messages);
        self.seed_sqlite_mailbox_for_team(team, agent, messages);
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

    fn sqlite_db_path(&self) -> std::path::PathBuf {
        self.tempdir.path().join("runtime").join("mail.sqlite3")
    }

    fn seed_sqlite_mailbox_for_team(&self, team: &str, agent: &str, messages: &[MessageEnvelope]) {
        let assembly = open_sqlite_boundary(self.sqlite_db_path()).expect("sqlite db");
        let mail_store = assembly.mail_store();
        let team = team.parse::<TeamName>().expect("team");
        let agent_name = agent.parse::<AgentName>().expect("agent");

        for (index, message) in messages.iter().enumerate() {
            let message_key = if let Some(message_id) = message.message_id {
                atm_core::boundary::MessageKey::new(format!("atm:{message_id}"))
                    .expect("message key")
            } else {
                atm_core::boundary::MessageKey::new(format!("ext:{agent}:{index}"))
                    .expect("message key")
            };
            mail_store
                .upsert_message(atm_core::boundary::MailStoreUpsertMessageRequest {
                    record: atm_core::boundary::MailStoreMessageRecord {
                        team: team.clone(),
                        agent: agent_name.clone(),
                        message_key: message_key.clone(),
                        envelope: message.clone(),
                    },
                })
                .expect("seed sqlite message");
            mail_store
                .upsert_message_state(atm_core::boundary::UpsertMailMessageStateRequest {
                    team: team.clone(),
                    agent: agent_name.clone(),
                    actor: agent_name.clone(),
                    state: atm_core::boundary::MailMessageState {
                        team: team.clone(),
                        agent: agent_name.clone(),
                        actor: agent_name.clone(),
                        message_key,
                        read: message.read,
                        pending_ack_at: message.pending_ack_at,
                        acknowledged_at: message.acknowledged_at,
                        expires_at: message.expires_at,
                        deleted_at: None,
                        updated_at: Some(message.timestamp),
                    },
                })
                .expect("seed sqlite message state");
        }
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

fn message_workflow_key(message: &MessageEnvelope) -> String {
    atm_core::boundary::MessageKey::new(format!("atm:{}", message.message_id.expect("message id")))
        .expect("message key")
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
        .map(|value| serde_json::from_value(value).expect("message envelope"))
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
    message_id: AtmMessageId,
    source_team: &str,
) -> MessageEnvelope {
    pending_ack_message_at(from, text, message_id, source_team, Utc::now())
}

fn pending_ack_message_at(
    from: &str,
    text: &str,
    message_id: AtmMessageId,
    source_team: &str,
    timestamp: chrono::DateTime<Utc>,
) -> MessageEnvelope {
    MessageEnvelope {
        from: from.parse::<AgentName>().expect("agent"),
        text: text.to_string(),
        timestamp: IsoTimestamp::from_datetime(timestamp),
        read: true,
        source_team: Some(source_team.parse::<TeamName>().expect("team")),
        summary: None,
        message_id: Some(message_id),
        pending_ack_at: Some(IsoTimestamp::from_datetime(timestamp)),
        acknowledged_at: None,
        acknowledges_message_id: None,
        parent_message_id: None,
        thread_mode: None,
        expires_at: None,
        task_id: None,
        extra: serde_json::Map::new(),
    }
}

fn read_message(from: &str, text: &str, message_id: AtmMessageId) -> MessageEnvelope {
    read_message_at(from, text, message_id, Utc::now())
}

fn read_message_at(
    from: &str,
    text: &str,
    message_id: AtmMessageId,
    timestamp: chrono::DateTime<Utc>,
) -> MessageEnvelope {
    MessageEnvelope {
        from: from.parse::<AgentName>().expect("agent"),
        text: text.to_string(),
        timestamp: IsoTimestamp::from_datetime(timestamp),
        read: true,
        source_team: Some(PRIMARY_TEAM.parse::<TeamName>().expect("team")),
        summary: None,
        message_id: Some(message_id),
        pending_ack_at: None,
        acknowledged_at: None,
        acknowledges_message_id: None,
        parent_message_id: None,
        thread_mode: None,
        expires_at: None,
        task_id: None,
        extra: serde_json::Map::new(),
    }
}

fn unread_message(from: &str, text: &str, message_id: AtmMessageId) -> MessageEnvelope {
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
        parent_message_id: None,
        thread_mode: None,
        expires_at: None,
        task_id: None,
        extra: serde_json::Map::new(),
    }
}
