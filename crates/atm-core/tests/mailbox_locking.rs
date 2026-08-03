use std::fs;
#[cfg(unix)]
use std::fs::OpenOptions;
use std::sync::{Arc, Barrier, mpsc};
use std::thread;
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

use atm_core::ack::{AckRequest, ack_mail};
use atm_core::clear::{ClearQuery, clear_mail};
use atm_core::error_codes::AtmErrorCode;
use atm_core::list::{ListQuery, list_mail};
use atm_core::observability::NullObservability;
use atm_core::read::{PeekQuery, ReadQuery, peek_mail, read_mail};
use atm_core::roles::ROLE_TEAM_LEAD;
use atm_core::schema::{AgentMember, AtmMessageId, InboxMessage, TeamConfig};
use atm_core::send::{SendMessageSource, SendRequest, send_mail};
#[cfg(unix)]
use atm_core::test_support::EnvGuard;
use atm_core::types::{AgentName, IsoTimestamp, ReadSelection, TeamName};
#[cfg(unix)]
use atm_runtime_test_support::hold_sqlite_writer_lock;
use atm_runtime_test_support::{
    SqliteRuntimeGuard, install_sqlite_retained_runtime_factory, open_sqlite_boundary,
};
use chrono::Utc;
#[cfg(unix)]
use fs2::FileExt;
use tempfile::TempDir;

// Test-side ceiling guard only; production lock timeout defaults to 5s per
// architecture §18.3.
#[cfg(unix)]
const TEST_LOCK_BUDGET_CEILING: Duration = Duration::from_secs(10);
// The operation under test has a 100 ms SQLite busy timeout. This outer
// channel deadline only detects a wedged worker; it must retain enough
// scheduler headroom for heavily contended macOS CI runners.
#[cfg(unix)]
const TEST_LOCK_COMPLETION_CEILING: Duration = Duration::from_secs(30);
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
            AtmMessageId::new(),
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
    let pending_message_id = AtmMessageId::new();
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
        arch_inbox
            .iter()
            .any(|message| message.message_id == Some(pending_message_id)),
        "pending message disappeared from compatibility export: {:?}",
        arch_inbox
    );
    let arch_state = ack_fixture.mailbox_state_contents(PRIMARY_AGENT);
    assert!(
        arch_state["messages"]
            .as_object()
            .is_some_and(|messages| !messages.is_empty()),
        "pending message was not acknowledged in SQLite state: {arch_state:?}"
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
fn concurrent_same_recipient_sends_preserve_mixed_payloads_and_sqlite_state() {
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

    let plain_state_key = message_workflow_key(plain_message);
    let task_state_key = message_workflow_key(task_message);
    let state = fixture.mailbox_state_contents(PRIMARY_AGENT);
    assert!(
        state["messages"][plain_state_key.clone()]
            .as_object()
            .is_some(),
        "plain SQLite state missing: {state:?}"
    );
    assert!(
        state["messages"][plain_state_key]["pendingAckAt"].is_null(),
        "plain SQLite state should not require ack: {state:?}"
    );
    assert!(
        state["messages"][task_state_key]["pendingAckAt"]
            .as_str()
            .is_some(),
        "task SQLite state should preserve pending ack: {state:?}"
    );
}

#[test]
#[serial_test::serial(env)]
fn concurrent_same_recipient_sends_preserve_sqlite_state() {
    let fixture = Fixture::new();
    let observability = Arc::new(NullObservability);

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
    let state = fixture.mailbox_state_contents(PRIMARY_AGENT);
    assert!(
        state["messages"][message_workflow_key(first_message)]
            .as_object()
            .is_some(),
        "first send SQLite state missing after concurrent update: {state:?}"
    );
    assert!(
        state["messages"][message_workflow_key(second_message)]
            .as_object()
            .is_some(),
        "second send SQLite state missing after concurrent update: {state:?}"
    );
}

#[test]
#[serial_test::serial(env)]
fn missing_team_config_no_longer_seeds_team_lead_notice_state() {
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

    assert!(
        fixture
            .inbox_contents_for_team("broken-dev", TEAM_LEAD)
            .is_empty(),
        "team-lead inbox should not receive a synthetic missing-config notice"
    );
}

#[test]
#[serial_test::serial(env)]
fn concurrent_normal_send_and_sqlite_only_delivery_complete_without_data_loss() {
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
    assert!(
        fixture
            .inbox_contents_for_team("broken-dev", TEAM_LEAD)
            .is_empty(),
        "team-lead inbox should not receive a synthetic missing-config notice during concurrent send"
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
            AtmMessageId::new(),
        )],
    );
    fixture.write_origin_inbox(
        PRIMARY_AGENT,
        "host-b",
        &[unread_message(
            SECONDARY_AGENT,
            "origin unread b",
            AtmMessageId::new(),
        )],
    );
    fixture.write_origin_inbox(
        PRIMARY_AGENT,
        "host-a",
        &[read_message(
            SECONDARY_AGENT,
            "origin read a",
            AtmMessageId::new(),
        )],
    );

    let barrier = Arc::new(Barrier::new(3));
    let (tx, rx) = mpsc::channel();
    let read_request = fixture.read_query(PRIMARY_AGENT);
    let clear_request = fixture.clear_query(PRIMARY_AGENT);
    for (label, op) in [
        (
            "read",
            CommandOp::Read(Box::new(read_request), Arc::clone(&observability)),
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
                    read_mail(*request, observability.as_ref()).map(|_| ())
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
    let fixture = Fixture::new();
    let _timeout = EnvGuard::set_raw("ATM_TEST_MAILBOX_LOCK_TIMEOUT_MS", "100");
    fixture.write_primary_inbox(PRIMARY_AGENT, &[]);
    let _writer_lock = hold_sqlite_writer_lock(fixture.sqlite_db_path()).expect("hold sqlite lock");
    let request = fixture.send_request(TEAM_LEAD, &qualified(PRIMARY_AGENT), "blocked send");
    let (tx, rx) = mpsc::sync_channel(1);

    let join = thread::spawn(move || {
        let result = send_mail(request, &NullObservability);
        tx.send(result).expect("send result");
    });

    let error = rx
        .recv_timeout(TEST_LOCK_COMPLETION_CEILING)
        .expect("bounded send completion")
        .expect_err("timeout");
    join.join().expect("join send thread");

    assert_eq!(error.code(), AtmErrorCode::MailboxLockTimeout);
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
            AtmMessageId::new(),
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
            AtmMessageId::new(),
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
    let mutation_query = mutation_fixture.read_query(PRIMARY_AGENT);
    let mutation_outcome = read_mail(mutation_query, &observability).expect("read with mutation");
    assert_eq!(mutation_outcome.count, 1);
    assert!(mutation_outcome.mutation_applied);
    drop(mutation_fixture);
    let no_mutation_fixture = Fixture::new();
    no_mutation_fixture.write_primary_inbox(
        PRIMARY_AGENT,
        &[read_message(TEAM_LEAD, "already read", AtmMessageId::new())],
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
fn read_unread_output_stays_consistent_with_the_mutated_message() {
    let fixture = Fixture::new();
    let observability = NullObservability;
    let older_id = AtmMessageId::new();
    let newer_id = AtmMessageId::new();
    let now = Utc::now();
    fixture.write_primary_inbox(
        PRIMARY_AGENT,
        &[
            unread_message_at(
                TEAM_LEAD,
                "older unread",
                older_id,
                now - chrono::Duration::seconds(5),
            ),
            unread_message_at(TEAM_LEAD, "newer unread", newer_id, now),
        ],
    );

    let unread_query = fixture
        .read_query(PRIMARY_AGENT)
        .with_selection_mode(ReadSelection::Unread);

    let first = read_mail(unread_query.clone(), &observability).expect("first unread read");
    assert!(first.mutation_applied);
    assert_eq!(first.selected_message_id, Some(newer_id));
    assert_eq!(
        first
            .message
            .as_ref()
            .and_then(|message| message.envelope.message_id),
        Some(newer_id)
    );
    assert_eq!(
        first
            .message
            .as_ref()
            .map(|message| message.envelope.text.as_str()),
        Some("newer unread")
    );
    assert_eq!(
        first.message.as_ref().map(|message| message.envelope.read),
        Some(true)
    );
    assert_eq!(first.bucket_counts.unread, 1);

    let second = read_mail(unread_query, &observability).expect("second unread read");
    assert!(second.mutation_applied);
    assert_eq!(second.selected_message_id, Some(older_id));
    assert_eq!(
        second
            .message
            .as_ref()
            .and_then(|message| message.envelope.message_id),
        Some(older_id)
    );
    assert_eq!(
        second
            .message
            .as_ref()
            .map(|message| message.envelope.text.as_str()),
        Some("older unread")
    );
    assert_eq!(
        second.message.as_ref().map(|message| message.envelope.read),
        Some(true)
    );
    assert_eq!(second.bucket_counts.unread, 0);
}

#[test]
#[serial_test::serial(env)]
fn ack_persists_read_state_and_acknowledged_timestamp() {
    let fixture = Fixture::new();
    let observability = NullObservability;
    let message_id = AtmMessageId::new();
    fixture.write_primary_inbox(
        PRIMARY_AGENT,
        &[pending_ack_message(
            TEAM_LEAD,
            "needs ack",
            message_id,
            PRIMARY_TEAM,
        )],
    );

    let ack_outcome = ack_mail(
        fixture.ack_request(PRIMARY_AGENT, message_id, "ack reply"),
        &observability,
    )
    .expect("ack outcome");
    assert_eq!(ack_outcome.message_id, message_id);

    let query = ReadQuery::new(
        fixture.tempdir.path().to_path_buf(),
        fixture.tempdir.path().to_path_buf(),
        PRIMARY_AGENT.parse().expect("caller"),
        None,
        PRIMARY_TEAM.parse().expect("team"),
        ReadSelection::All,
        false,
        false,
        Some(&message_id.to_string()),
        None,
        None,
        None,
        None,
        None,
    )
    .expect("read query");
    let outcome = read_mail(query, &observability).expect("read acked message");
    let message = outcome.message.expect("acked message");

    assert_eq!(message.envelope.message_id, Some(message_id));
    assert!(message.envelope.read);
    assert!(message.envelope.pending_ack_at.is_none());
    assert!(message.envelope.acknowledged_at.is_some());
}

#[test]
#[serial_test::serial(env)]
fn ack_self_addressed_empty_host_target_rejects_without_mutating_source() {
    let fixture = Fixture::new();
    let observability = NullObservability;
    let message_id = AtmMessageId::new();
    fixture.write_primary_inbox(
        PRIMARY_AGENT,
        &[pending_ack_message(
            PRIMARY_AGENT,
            "historical self poison",
            message_id,
            PRIMARY_TEAM,
        )],
    );

    let error = ack_mail(
        fixture.ack_request(PRIMARY_AGENT, message_id, "resolved"),
        &observability,
    )
    .expect_err("empty-host self acknowledgement must be rejected");
    assert_eq!(error.code(), AtmErrorCode::SelfAddressedSendInvalid);

    let inbox = fixture.inbox_contents(PRIMARY_AGENT);
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].message_id, Some(message_id));
    assert!(inbox[0].pending_ack_at.is_some());
    assert!(inbox[0].acknowledged_at.is_none());
    assert!(inbox[0].acknowledges_message_id.is_none());
}

#[test]
#[serial_test::serial(env)]
fn peek_cross_agent_target_store_backed_keeps_read_and_ack_fields_unchanged() {
    let fixture = Fixture::new();
    let observability = NullObservability;
    let message_id = AtmMessageId::new();
    fixture.write_primary_inbox(
        PRIMARY_AGENT,
        &[pending_ack_message(
            TEAM_LEAD,
            "peek without mutation",
            message_id,
            PRIMARY_TEAM,
        )],
    );

    let before = fixture
        .inbox_contents(PRIMARY_AGENT)
        .into_iter()
        .find(|message| message.message_id == Some(message_id))
        .expect("message before peek");

    let outcome = peek_mail(
        fixture.peek_query(SECONDARY_AGENT, Some(PRIMARY_AGENT), message_id),
        &observability,
    )
    .expect("peek outcome");

    assert!(!outcome.mutation_applied);
    assert_eq!(outcome.selected_message_id, Some(message_id));

    let after = fixture
        .inbox_contents(PRIMARY_AGENT)
        .into_iter()
        .find(|message| message.message_id == Some(message_id))
        .expect("message after peek");

    assert_eq!(after.read, before.read);
    assert_eq!(after.pending_ack_at, before.pending_ack_at);
    assert_eq!(after.acknowledged_at, before.acknowledged_at);
    assert_eq!(
        after.acknowledges_message_id,
        before.acknowledges_message_id
    );
}

#[test]
#[serial_test::serial(env)]
fn read_contains_matches_summary_only_and_body_only_on_store_backed_path() {
    let fixture = Fixture::new();
    let observability = NullObservability;
    let summary_match_id = AtmMessageId::new();
    let body_match_id = AtmMessageId::new();
    let now = Utc::now();
    let mut summary_match = unread_message_at(
        TEAM_LEAD,
        "durable body without the term",
        summary_match_id,
        now - chrono::Duration::seconds(5),
    );
    summary_match.summary = Some("summary needle".to_string());
    let mut body_match = unread_message_at(TEAM_LEAD, "durable body needle", body_match_id, now);
    body_match.summary = Some("summary miss".to_string());
    fixture.write_primary_inbox(PRIMARY_AGENT, &[summary_match, body_match]);

    let summary_query = ReadQuery::new(
        fixture.tempdir.path().to_path_buf(),
        fixture.tempdir.path().to_path_buf(),
        PRIMARY_AGENT.parse().expect("caller"),
        None,
        PRIMARY_TEAM.parse().expect("team"),
        ReadSelection::All,
        false,
        false,
        None,
        None,
        None,
        None,
        Some("summary needle"),
        None,
    )
    .expect("summary query");
    let summary_outcome = read_mail(summary_query, &observability).expect("summary outcome");
    assert_eq!(summary_outcome.selected_message_id, Some(summary_match_id));
    assert_eq!(
        summary_outcome
            .message
            .as_ref()
            .and_then(|message| message.envelope.message_id),
        Some(summary_match_id)
    );

    let body_query = ReadQuery::new(
        fixture.tempdir.path().to_path_buf(),
        fixture.tempdir.path().to_path_buf(),
        PRIMARY_AGENT.parse().expect("caller"),
        None,
        PRIMARY_TEAM.parse().expect("team"),
        ReadSelection::All,
        false,
        false,
        None,
        None,
        None,
        None,
        Some("body needle"),
        None,
    )
    .expect("body query");
    let body_outcome = read_mail(body_query, &observability).expect("body outcome");
    assert_eq!(body_outcome.selected_message_id, Some(body_match_id));
    assert_eq!(
        body_outcome
            .message
            .as_ref()
            .map(|message| message.envelope.text.as_str()),
        Some("durable body needle")
    );
}

#[test]
#[serial_test::serial(env)]
fn list_contains_matches_body_only_on_store_backed_path() {
    let fixture = Fixture::new();
    let observability = NullObservability;
    let message_id = AtmMessageId::new();
    let mut body_match = unread_message(TEAM_LEAD, "body only needle", message_id);
    body_match.summary = Some("summary miss".to_string());
    fixture.write_primary_inbox(PRIMARY_AGENT, &[body_match]);

    let outcome = list_mail(
        ListQuery::new(
            fixture.tempdir.path().to_path_buf(),
            fixture.tempdir.path().to_path_buf(),
            PRIMARY_AGENT.parse().expect("caller"),
            None,
            PRIMARY_TEAM.parse().expect("team"),
            ReadSelection::All,
            false,
            None,
            None,
            None,
            None,
            Some("only needle"),
        )
        .expect("list query"),
        &observability,
    )
    .expect("list outcome");

    assert_eq!(outcome.count, 1);
    assert_eq!(outcome.rows[0].message_id, Some(message_id));
    assert_eq!(outcome.rows[0].summary, "summary miss");
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

    let inbox_before = fixture.inbox_contents(PRIMARY_AGENT);
    let physical_before = inbox_before
        .iter()
        .find(|message| message.text == "hello sidecar")
        .expect("store-backed inbox before read");
    let logical_message_id = physical_before.message_id.expect("logical message id");
    assert!(!physical_before.read);
    assert!(
        !fixture.primary_inbox_path(PRIMARY_AGENT).exists(),
        "AD.3 should not recreate the retired primary compatibility inbox file on send",
    );

    let read_query = fixture.read_query(PRIMARY_AGENT);
    let outcome = read_mail(read_query, &observability).expect("read mail");
    assert!(
        outcome
            .message
            .as_ref()
            .is_some_and(|message| message.envelope.text == "hello sidecar"),
        "read outcome should include the ULID-authored message"
    );

    let inbox_after = fixture.inbox_contents(PRIMARY_AGENT);
    let physical_after = inbox_after
        .iter()
        .find(|message| message.text == "hello sidecar")
        .expect("store-backed inbox after read");
    assert_eq!(physical_after.message_id, Some(logical_message_id));
    assert!(physical_after.read);
    assert!(
        !sentinel_path(&fixture.primary_inbox_path(PRIMARY_AGENT)).exists(),
        "read-only ULID sidecar path must not leave a lock sentinel behind",
    );
    assert!(
        !fixture.primary_inbox_path(PRIMARY_AGENT).exists(),
        "AD.3 read-sidecar flow should not create the retired compatibility inbox file",
    );
    assert!(
        outcome
            .message
            .as_ref()
            .is_some_and(|message| message.envelope.read),
        "read outcome should surface the promoted read state"
    );

    let state = fixture.mailbox_state_contents(PRIMARY_AGENT);
    let logical_message_key = atm_core::boundary::MessageKey::from(logical_message_id).into_inner();
    assert!(
        state["messages"][logical_message_key].as_object().is_some(),
        "SQLite state missing for ULID-authored message: {state:?}"
    );
}

#[test]
#[cfg(unix)]
#[serial_test::serial(env)]
fn clear_ignores_synthetic_source_discovery_fault_in_store_only_mode() {
    let fixture = Fixture::new();
    let _fault = EnvGuard::set_raw("ATM_TEST_FORCE_SOURCE_DISCOVERY_FAULT", "1");
    let observability = NullObservability;
    fixture.write_primary_inbox(PRIMARY_AGENT, &[]);
    fixture.write_origin_inbox(
        PRIMARY_AGENT,
        "host-a",
        &[read_message(
            SECONDARY_AGENT,
            "origin read a",
            AtmMessageId::new(),
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
fn send_ignores_retired_file_lock_faults_on_sqlite_path() {
    let fixture = Fixture::new();
    let _fault = EnvGuard::set_raw("ATM_TEST_FORCE_LOCK_NON_CONTENTION_ERROR", "1");
    let observability = NullObservability;
    let started = Instant::now();

    let outcome = send_mail(
        fixture.send_request(TEAM_LEAD, &qualified(PRIMARY_AGENT), "lock failure"),
        &observability,
    )
    .expect("SQLite send must not consult retired file locks");

    assert!(!outcome.message_id.to_string().is_empty());
    assert!(
        started.elapsed() < TEST_LOCK_BUDGET_CEILING,
        "retain only a coarse non-blocking budget here; recv_timeout-based tests above already cover deadlock detection"
    );
}

enum CommandOp {
    Read(Box<ReadQuery>, Arc<NullObservability>),
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
            tempdir
                .path()
                .join("runtime")
                .join("mail.sqlite3")
                .as_path(),
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
            caller_identity: actor.parse().expect("caller"),
            caller_chat_id: None,
            caller_team: PRIMARY_TEAM.parse().expect("team"),
            message_id,
            reply_body: reply_body.to_string(),
        }
    }

    fn clear_query(&self, actor: &str) -> ClearQuery {
        ClearQuery {
            home_dir: self.tempdir.path().to_path_buf(),
            current_dir: self.tempdir.path().to_path_buf(),
            caller_identity: actor.parse().expect("caller"),
            caller_team: PRIMARY_TEAM.parse().expect("team"),
            older_than: None,
            idle_only: false,
            dry_run: false,
        }
    }

    fn read_query(&self, actor: &str) -> ReadQuery {
        ReadQuery::new(
            self.tempdir.path().to_path_buf(),
            self.tempdir.path().to_path_buf(),
            actor.parse().expect("caller"),
            None,
            PRIMARY_TEAM.parse().expect("team"),
            ReadSelection::Actionable,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("read query")
    }

    fn peek_query(&self, actor: &str, target: Option<&str>, message_id: AtmMessageId) -> PeekQuery {
        PeekQuery::new(
            self.tempdir.path().to_path_buf(),
            self.tempdir.path().to_path_buf(),
            actor.parse().expect("caller"),
            target,
            PRIMARY_TEAM.parse().expect("team"),
            ReadSelection::All,
            false,
            Some(&message_id.to_string()),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("peek query")
    }

    fn send_request(&self, sender: &str, to: &str, text: &str) -> SendRequest {
        SendRequest::new(
            self.tempdir.path().to_path_buf(),
            self.tempdir.path().to_path_buf(),
            sender.parse().expect("caller"),
            to,
            PRIMARY_TEAM.parse().expect("team"),
            SendMessageSource::Inline(text.to_string()),
            None,
            false,
            None,
            false,
        )
        .expect("send request")
    }

    fn inbox_contents(&self, agent: &str) -> Vec<InboxMessage> {
        self.inbox_contents_for_team(PRIMARY_TEAM, agent)
    }

    fn origin_inbox_contents(&self, agent: &str, suffix: &str) -> Vec<InboxMessage> {
        read_jsonl(self.origin_inbox_path(agent, suffix))
    }

    fn mailbox_state_contents(&self, agent: &str) -> serde_json::Value {
        self.mailbox_state_contents_for_team(PRIMARY_TEAM, agent)
    }

    #[allow(
        deprecated,
        reason = "mailbox locking tests still inspect the retained sqlite runtime through legacy core boundary shims"
    )]
    fn inbox_contents_for_team(&self, team: &str, agent: &str) -> Vec<InboxMessage> {
        let assembly = open_sqlite_boundary(self.sqlite_db_path()).expect("sqlite db");
        let mail_store = assembly.mail_store_arc();
        let team = team.parse::<TeamName>().expect("team");
        let agent_name = agent.parse::<AgentName>().expect("agent");
        let mut metadata_rows = mail_store
            .query_mailbox_metadata(&team, &agent_name, None)
            .expect("mailbox rows");
        metadata_rows.sort_by(|left, right| {
            left.message_at
                .cmp(&right.message_at)
                .then_with(|| left.message_key.as_ref().cmp(right.message_key.as_ref()))
        });
        metadata_rows
            .into_iter()
            .filter_map(|row| {
                mail_store
                    .load_message(&team, &agent_name, &row.message_key)
                    .expect("message record")
            })
            .map(|record| record.envelope)
            .collect()
    }

    fn mailbox_state_contents_for_team(&self, team: &str, agent: &str) -> serde_json::Value {
        let messages = self.inbox_contents_for_team(team, agent);
        let states = messages
            .into_iter()
            .filter_map(|message| {
                message.message_id.map(|message_id| {
                    (
                        atm_core::boundary::MessageKey::from(message_id).into_inner(),
                        serde_json::json!({
                            "read": message.read,
                            "pendingAckAt": message.pending_ack_at,
                            "acknowledgedAt": message.acknowledged_at,
                        }),
                    )
                })
            })
            .collect::<serde_json::Map<_, _>>();
        serde_json::json!({ "messages": states })
    }

    fn write_primary_inbox(&self, agent: &str, messages: &[InboxMessage]) {
        self.write_primary_inbox_for_team(PRIMARY_TEAM, agent, messages);
    }

    fn write_primary_inbox_for_team(&self, team: &str, agent: &str, messages: &[InboxMessage]) {
        write_inbox(&self.primary_inbox_path_for_team(team, agent), messages);
        self.seed_sqlite_mailbox_for_team(team, agent, messages);
    }

    fn write_origin_inbox(&self, agent: &str, suffix: &str, messages: &[InboxMessage]) {
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

    fn team_dir_for(&self, team: &str) -> std::path::PathBuf {
        self.tempdir.path().join(".claude").join("teams").join(team)
    }

    fn create_team_without_config(&self, team: &str) {
        fs::create_dir_all(self.team_dir_for(team).join("inboxes")).expect("team inboxes");
        seed_sqlite_roster(
            self.sqlite_db_path().as_path(),
            team,
            &[TEAM_LEAD, TEST_RECIPIENT],
        );
    }

    fn sqlite_db_path(&self) -> std::path::PathBuf {
        self.tempdir.path().join("runtime").join("mail.sqlite3")
    }

    #[allow(
        deprecated,
        reason = "mailbox locking tests still seed the retained sqlite runtime through legacy core boundary shims"
    )]
    fn seed_sqlite_mailbox_for_team(&self, team: &str, agent: &str, messages: &[InboxMessage]) {
        let assembly = open_sqlite_boundary(self.sqlite_db_path()).expect("sqlite db");
        let mail_store = assembly.mail_store_arc();
        let team = team.parse::<TeamName>().expect("team");
        let agent_name = agent.parse::<AgentName>().expect("agent");

        for (index, message) in messages.iter().enumerate() {
            let message_key = if let Some(message_id) = message.message_id {
                atm_core::boundary::MessageKey::from(message_id)
            } else {
                atm_core::boundary::MessageKey::new(format!("ext:{agent}:{index}"))
                    .expect("message key")
            };
            mail_store
                .upsert_message(atm_core::boundary::Message {
                    team: team.clone(),
                    agent: agent_name.clone(),
                    message_key: message_key.clone(),
                    envelope: message.clone(),
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

fn create_team_with_config(
    home_dir: &std::path::Path,
    sqlite_db_path: &std::path::Path,
    team: &str,
    members: &[&str],
) {
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
    seed_sqlite_roster(sqlite_db_path, team, members);
}

#[allow(
    deprecated,
    reason = "mailbox locking tests still seed the retained sqlite runtime through legacy core boundary shims"
)]
fn seed_sqlite_roster(sqlite_db_path: &std::path::Path, team: &str, members: &[&str]) {
    let assembly = open_sqlite_boundary(sqlite_db_path).expect("sqlite db");
    let roster_store = assembly.roster_store_arc();
    let team = team.parse::<TeamName>().expect("team");
    let members = members
        .iter()
        .map(|name| atm_core::boundary::RosterEntry {
            team_name: team.clone(),
            agent_name: (*name).parse::<AgentName>().expect("agent"),
            member_kind: atm_core::boundary::RosterMemberKind::Permanent,
            harness: atm_core::boundary::RosterHarness::ClaudeCode,
            agent_type: atm_core::schema::AgentType::default(),
            model: atm_core::types::ModelName::default(),
            recipient_pane_id: None,
            metadata_json: serde_json::Map::new(),
        })
        .collect::<Vec<_>>();
    roster_store
        .replace_roster(&team, &members)
        .expect("seed sqlite roster");
}

fn message_workflow_key(message: &InboxMessage) -> String {
    message
        .message_id
        .map(|message_id| atm_core::boundary::MessageKey::from(message_id).into_inner())
        .expect("message id")
}

fn read_jsonl(path: std::path::PathBuf) -> Vec<InboxMessage> {
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

fn write_inbox(path: &std::path::Path, messages: &[InboxMessage]) {
    let mut raw = String::new();
    for message in messages {
        raw.push_str(&serde_json::to_string(message).expect("json line"));
        raw.push('\n');
    }
    fs::write(path, raw).expect("write inbox");
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
) -> InboxMessage {
    pending_ack_message_at(from, text, message_id, source_team, Utc::now())
}

fn pending_ack_message_at(
    from: &str,
    text: &str,
    message_id: AtmMessageId,
    source_team: &str,
    timestamp: chrono::DateTime<Utc>,
) -> InboxMessage {
    InboxMessage {
        from: from.parse::<AgentName>().expect("agent"),
        source_chat_id: None,
        text: text.to_string(),
        timestamp: IsoTimestamp::from_datetime(timestamp),
        read: true,
        source_team: Some(source_team.parse::<TeamName>().expect("team")),
        destination_chat_id: None,
        summary: None,
        message_id: Some(message_id),
        requires_ack: true,
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

fn read_message(from: &str, text: &str, message_id: AtmMessageId) -> InboxMessage {
    read_message_at(from, text, message_id, Utc::now())
}

fn read_message_at(
    from: &str,
    text: &str,
    message_id: AtmMessageId,
    timestamp: chrono::DateTime<Utc>,
) -> InboxMessage {
    InboxMessage {
        from: from.parse::<AgentName>().expect("agent"),
        source_chat_id: None,
        text: text.to_string(),
        timestamp: IsoTimestamp::from_datetime(timestamp),
        read: true,
        source_team: Some(PRIMARY_TEAM.parse::<TeamName>().expect("team")),
        destination_chat_id: None,
        summary: None,
        message_id: Some(message_id),
        requires_ack: false,
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

fn unread_message(from: &str, text: &str, message_id: AtmMessageId) -> InboxMessage {
    unread_message_at(from, text, message_id, Utc::now())
}

fn unread_message_at(
    from: &str,
    text: &str,
    message_id: AtmMessageId,
    timestamp: chrono::DateTime<Utc>,
) -> InboxMessage {
    InboxMessage {
        from: from.parse::<AgentName>().expect("agent"),
        source_chat_id: None,
        text: text.to_string(),
        timestamp: IsoTimestamp::from_datetime(timestamp),
        read: false,
        source_team: Some(PRIMARY_TEAM.parse::<TeamName>().expect("team")),
        destination_chat_id: None,
        summary: None,
        message_id: Some(message_id),
        requires_ack: false,
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
