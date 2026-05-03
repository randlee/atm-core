use std::ffi::{OsStr, OsString};
use std::fs;
use std::fs::{File, OpenOptions};
use std::sync::{Arc, Barrier, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use atm_core::ack::{AckMessageId, AckRequest, ack_mail};
use atm_core::clear::{ClearQuery, clear_mail};
use atm_core::error::AtmErrorCode;
use atm_core::inbox_ingress;
use atm_core::mail_store::MailStore;
use atm_core::observability::NullObservability;
use atm_core::read::{ReadQuery, read_mail};
use atm_core::schema::{AgentMember, LegacyMessageId, MessageEnvelope, TeamConfig};
use atm_core::send::{SendMessageSource, SendRequest, send_mail};
use atm_core::task_store::TaskStore;
use atm_core::types::{AckActivationMode, AgentName, IsoTimestamp, ReadSelection, TeamName};
use atm_core::{read_messages as read_inbox_messages, write_messages as write_inbox_messages};
use atm_rusqlite::RusqliteStore;
use chrono::Utc;
use fs2::FileExt;
use serial_test::serial;
use tempfile::TempDir;
use uuid::Uuid;

// Test-side ceiling guard only; production lock timeout defaults to 5s per
// architecture §18.3.
const TEST_LOCK_BUDGET_CEILING: Duration = Duration::from_secs(2);

fn test_recv_timeout() -> Duration {
    std::env::var("ATM_TEST_RECV_TIMEOUT_SECS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(10))
}

#[test]
#[serial]
fn concurrent_ack_on_overlapping_inbox_sets_completes_without_deadlock() {
    let fixture = Fixture::new();
    let observability = Arc::new(NullObservability);
    let store = Arc::new(
        RusqliteStore::open_for_team_home(
            fixture.tempdir.path(),
            &"atm-dev".parse().expect("team"),
        )
        .expect("open store"),
    );
    let barrier = Arc::new(Barrier::new(3));
    let (tx, rx) = mpsc::channel();

    let arch_request = fixture.ack_request("arch-ctm", fixture.arch_message_id, "ack from arch");
    let qa_request = fixture.ack_request("qa", fixture.qa_message_id, "ack from qa");

    for (label, request) in [("arch", arch_request), ("qa", qa_request)] {
        let barrier = Arc::clone(&barrier);
        let tx = tx.clone();
        let observability = Arc::clone(&observability);
        let store = Arc::clone(&store);
        thread::spawn(move || {
            barrier.wait();
            tx.send((
                label,
                ack_mail(request, store.as_ref(), observability.as_ref()),
            ))
            .expect("send result");
        });
    }
    drop(tx);

    barrier.wait();
    let first = rx
        .recv_timeout(test_recv_timeout())
        .expect("first ack result");
    let second = rx
        .recv_timeout(test_recv_timeout())
        .expect("second ack result");

    assert!(
        first.1.is_ok(),
        "first ack failed for {}: {:?}",
        first.0,
        first.1
    );
    assert!(
        second.1.is_ok(),
        "second ack failed for {}: {:?}; arch inbox: {:?}; qa inbox: {:?}",
        second.0,
        second.1,
        fixture.inbox_contents("arch-ctm"),
        fixture.inbox_contents("qa")
    );
    let arch_inbox = fixture.inbox_contents("arch-ctm");
    let qa_inbox = fixture.inbox_contents("qa");
    assert!(
        arch_inbox
            .iter()
            .any(|message| message.text == "ack from qa")
    );
    assert!(
        qa_inbox
            .iter()
            .any(|message| message.text == "ack from arch")
    );
}

#[test]
#[serial]
fn ack_mail_imports_legacy_pending_message_through_store_service() {
    let fixture = Fixture::new();
    let observability = NullObservability;
    let store = RusqliteStore::open_for_team_home(
        fixture.tempdir.path(),
        &"atm-dev".parse().expect("team"),
    )
    .expect("open store");

    let outcome = ack_mail(
        fixture.ack_request("arch-ctm", fixture.arch_message_id, "ack from arch"),
        &store,
        &observability,
    )
    .expect("ack legacy message");

    assert_eq!(
        outcome.message_id,
        AckMessageId::Legacy(fixture.arch_message_id)
    );
    assert_eq!(outcome.reply_target.to_string(), "qa@atm-dev");
    assert!(
        fixture
            .inbox_contents("qa")
            .iter()
            .any(|message| message.text == "ack from arch")
    );
}

#[test]
#[serial]
fn ack_mail_acknowledges_task_linked_imported_message() {
    let fixture = Fixture::new();
    let observability = NullObservability;
    let task_id: atm_core::types::TaskId = "TASK-LINK-1".parse().expect("task id");
    let mut message = pending_ack_message(
        "qa",
        "task-linked pending",
        LegacyMessageId::new(),
        "atm-dev",
    );
    message.task_id = Some(task_id.clone());
    let message_id = message.message_id.expect("legacy message id");
    fixture.write_primary_inbox("arch-ctm", &[message]);
    let store = RusqliteStore::open_for_team_home(
        fixture.tempdir.path(),
        &"atm-dev".parse().expect("team"),
    )
    .expect("open store");

    let outcome = ack_mail(
        fixture.ack_request("arch-ctm", message_id, "ack task"),
        &store,
        &observability,
    )
    .expect("ack task-linked message");

    assert_eq!(outcome.task_id.as_ref(), Some(&task_id));
    let task = store
        .load_task(&task_id)
        .expect("load task")
        .expect("task row");
    assert_eq!(task.status, atm_core::task_store::TaskStatus::Acknowledged);
    assert!(task.acknowledged_at.is_some());
}

#[test]
#[serial]
fn ack_mail_export_failure_after_commit_surfaces_warning_and_preserves_sqlite_state() {
    let fixture = Fixture::new();
    let observability = NullObservability;
    let store = RusqliteStore::open_for_team_home(
        fixture.tempdir.path(),
        &"atm-dev".parse().expect("team"),
    )
    .expect("open store");
    let reply_inbox_path = fixture.primary_inbox_path("qa");
    fs::remove_file(&reply_inbox_path).expect("remove reply inbox file");
    fs::create_dir(&reply_inbox_path).expect("replace reply inbox with directory");

    let outcome = ack_mail(
        fixture.ack_request(
            "arch-ctm",
            fixture.arch_message_id,
            "ack with export failure",
        ),
        &store,
        &observability,
    )
    .expect("ack should commit even when export fails");

    assert!(!outcome.warnings.is_empty(), "expected export warning");
    let stored = store
        .load_message_by_legacy_id(&fixture.arch_message_id)
        .expect("load source row")
        .expect("stored source row");
    let ack_state = store
        .load_ack_state(&stored.message_key)
        .expect("load ack state")
        .expect("ack state row");
    assert!(ack_state.acknowledged_at.is_some());
}

#[test]
#[serial]
fn ack_mail_rejects_message_already_acknowledged_in_sqlite_before_ingest() {
    let fixture = Fixture::new();
    let observability = NullObservability;
    let store = RusqliteStore::open_for_team_home(
        fixture.tempdir.path(),
        &"atm-dev".parse().expect("team"),
    )
    .expect("open store");

    inbox_ingress::ingest_mailbox_state(
        fixture.tempdir.path(),
        &"atm-dev".parse().expect("team"),
        &"arch-ctm".parse().expect("agent"),
        &store,
        &observability,
    )
    .expect("ingest pending message");
    let stored = store
        .load_message_by_legacy_id(&fixture.arch_message_id)
        .expect("load stored row")
        .expect("stored row");
    store
        .upsert_ack_state(&atm_core::mail_store::AckStateRecord {
            message_key: stored.message_key.clone(),
            pending_ack_at: Some(stored.created_at),
            acknowledged_at: Some(IsoTimestamp::now()),
            ack_reply_message_key: None,
            ack_reply_team: None,
            ack_reply_agent: None,
        })
        .expect("seed acknowledged state");

    let error = ack_mail(
        fixture.ack_request("arch-ctm", fixture.arch_message_id, "should reject"),
        &store,
        &observability,
    )
    .expect_err("already-acknowledged sqlite state must reject");

    assert_eq!(error.code, AtmErrorCode::AckInvalidState);
}

#[test]
#[serial]
fn duplicate_ack_attempt_returns_ack_invalid_state_at_service_level() {
    let fixture = Fixture::new();
    let observability = NullObservability;
    let store = RusqliteStore::open_for_team_home(
        fixture.tempdir.path(),
        &"atm-dev".parse().expect("team"),
    )
    .expect("open store");
    let request = fixture.ack_request("arch-ctm", fixture.arch_message_id, "first ack");

    ack_mail(request.clone(), &store, &observability).expect("first ack");
    let error = ack_mail(request, &store, &observability).expect_err("duplicate ack must fail");

    assert_eq!(error.code, AtmErrorCode::AckInvalidState);
}

#[test]
#[serial]
fn concurrent_send_with_ack_and_clear_completes_without_deadlock_or_data_loss() {
    let observability = Arc::new(NullObservability);

    let clear_fixture = Fixture::new();
    clear_fixture.write_primary_inbox(
        "arch-ctm",
        &[read_message(
            "qa",
            "clearable history entry",
            LegacyMessageId::from(Uuid::new_v4()),
        )],
    );
    let barrier = Arc::new(Barrier::new(3));
    let (tx, rx) = mpsc::channel();
    let send_request = clear_fixture.send_request("team-lead", "arch-ctm@atm-dev", "new message");
    let clear_request = clear_fixture.clear_query("arch-ctm");
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
        .recv_timeout(test_recv_timeout())
        .expect("first send/clear result");
    let second = rx
        .recv_timeout(test_recv_timeout())
        .expect("second send/clear result");
    assert!(first.1.is_ok(), "{} failed: {:?}", first.0, first.1);
    assert!(second.1.is_ok(), "{} failed: {:?}", second.0, second.1);
    let arch_inbox = clear_fixture.inbox_contents("arch-ctm");
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
        "arch-ctm",
        &[pending_ack_message(
            "qa",
            "pending ack",
            pending_message_id,
            "atm-dev",
        )],
    );
    let barrier = Arc::new(Barrier::new(3));
    let (tx, rx) = mpsc::channel();
    let send_request = ack_fixture.send_request("team-lead", "arch-ctm@atm-dev", "new message");
    let ack_request = ack_fixture.ack_request("arch-ctm", pending_message_id, "ack reply");
    let ack_store = Arc::new(
        RusqliteStore::open_for_team_home(
            ack_fixture.tempdir.path(),
            &"atm-dev".parse().expect("team"),
        )
        .expect("open store"),
    );
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
        let store = Arc::clone(&ack_store);
        thread::spawn(move || {
            barrier.wait();
            tx.send((
                "send-ack/ack",
                ack_mail(ack_request, store.as_ref(), observability.as_ref()).map(|_| ()),
            ))
            .expect("ack result");
        });
    }
    drop(tx);
    barrier.wait();
    let first = rx
        .recv_timeout(test_recv_timeout())
        .expect("first send/ack result");
    let second = rx
        .recv_timeout(test_recv_timeout())
        .expect("second send/ack result");
    assert!(first.1.is_ok(), "{} failed: {:?}", first.0, first.1);
    assert!(second.1.is_ok(), "{} failed: {:?}", second.0, second.1);
    let arch_inbox = ack_fixture.inbox_contents("arch-ctm");
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
    let pending_store = RusqliteStore::open_for_team_home(
        ack_fixture.tempdir.path(),
        &"atm-dev".parse().expect("team"),
    )
    .expect("open store");
    let pending_row = pending_store
        .load_message_by_legacy_id(&pending_message_id)
        .expect("load pending row")
        .expect("pending row");
    let ack_state = pending_store
        .load_ack_state(&pending_row.message_key)
        .expect("load ack state")
        .expect("ack row");
    assert!(ack_state.acknowledged_at.is_some());
    let qa_inbox = ack_fixture.inbox_contents("qa");
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

    let plain_request = fixture.send_request("team-lead", "arch-ctm@atm-dev", "plain payload");
    let mut task_request = fixture.send_request("qa", "arch-ctm@atm-dev", "task payload");
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
        .recv_timeout(test_recv_timeout())
        .expect("first send result");
    let second = rx
        .recv_timeout(test_recv_timeout())
        .expect("second send result");
    assert!(first.1.is_ok(), "{} failed: {:?}", first.0, first.1);
    assert!(second.1.is_ok(), "{} failed: {:?}", second.0, second.1);

    let inbox = fixture.inbox_contents("arch-ctm");
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
    let workflow = fixture.workflow_state_contents("arch-ctm");
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
        "arch-ctm",
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
    let first_request = fixture.send_request("team-lead", "arch-ctm@atm-dev", "first payload");
    let second_request = fixture.send_request("qa", "arch-ctm@atm-dev", "second payload");

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
        .recv_timeout(test_recv_timeout())
        .expect("first send result");
    let second = rx
        .recv_timeout(test_recv_timeout())
        .expect("second send result");
    assert!(first.1.is_ok(), "{} failed: {:?}", first.0, first.1);
    assert!(second.1.is_ok(), "{} failed: {:?}", second.0, second.1);

    let inbox = fixture.inbox_contents("arch-ctm");
    let first_message = inbox
        .iter()
        .find(|message| message.text == "first payload")
        .expect("first inbox message");
    let second_message = inbox
        .iter()
        .find(|message| message.text == "second payload")
        .expect("second inbox message");
    let workflow = fixture.workflow_state_contents("arch-ctm");

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
    fixture.write_primary_inbox_for_team("broken-dev", "recipient", &[]);
    fixture.write_primary_inbox_for_team("broken-dev", "team-lead", &[]);

    send_mail(
        fixture.send_request("team-lead", "recipient@broken-dev", "broken send"),
        &observability,
    )
    .expect("missing-config send");

    let notices = fixture.inbox_contents_for_team("broken-dev", "team-lead");
    let notice = notices.first().expect("missing-config notice");
    assert_eq!(notice.from, "atm-identity-missing");
    assert_eq!(notice.source_team.as_deref(), Some("broken-dev"));
    let workflow = fixture.workflow_state_contents_for_team("broken-dev", "team-lead");
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
    fixture.write_primary_inbox_for_team("broken-dev", "recipient", &[]);
    fixture.write_primary_inbox_for_team("broken-dev", "team-lead", &[]);

    let barrier = Arc::new(Barrier::new(3));
    let (tx, rx) = mpsc::channel();
    let normal_request = fixture.send_request("team-lead", "arch-ctm@atm-dev", "normal send");
    let broken_request = fixture.send_request("qa", "recipient@broken-dev", "broken send");

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
        .recv_timeout(test_recv_timeout())
        .expect("first send result");
    let second = rx
        .recv_timeout(test_recv_timeout())
        .expect("second send result");
    assert!(first.1.is_ok(), "{} failed: {:?}", first.0, first.1);
    assert!(second.1.is_ok(), "{} failed: {:?}", second.0, second.1);

    assert!(
        fixture
            .inbox_contents("arch-ctm")
            .iter()
            .any(|message| message.text == "normal send"),
        "normal send missing from primary team inbox"
    );
    assert!(
        fixture
            .inbox_contents_for_team("broken-dev", "recipient")
            .iter()
            .any(|message| message.text == "broken send"),
        "missing-config recipient send was not persisted"
    );
    let notices = fixture.inbox_contents_for_team("broken-dev", "team-lead");
    let notice = notices.first().expect("missing-config notice");
    let workflow = fixture.workflow_state_contents_for_team("broken-dev", "team-lead");
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
        "arch-ctm",
        &[unread_message(
            "team-lead",
            "primary unread",
            LegacyMessageId::from(Uuid::new_v4()),
        )],
    );
    fixture.write_origin_inbox(
        "arch-ctm",
        "host-b",
        &[unread_message(
            "qa",
            "origin unread b",
            LegacyMessageId::from(Uuid::new_v4()),
        )],
    );
    fixture.write_origin_inbox(
        "arch-ctm",
        "host-a",
        &[read_message(
            "qa",
            "origin read a",
            LegacyMessageId::from(Uuid::new_v4()),
        )],
    );

    let barrier = Arc::new(Barrier::new(3));
    let (tx, rx) = mpsc::channel();
    let read_request = fixture.read_query("arch-ctm");
    let clear_request = fixture.clear_query("arch-ctm");
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
        .recv_timeout(test_recv_timeout())
        .expect("first read/clear result");
    let second = rx
        .recv_timeout(test_recv_timeout())
        .expect("second read/clear result");
    assert!(first.1.is_ok(), "{} failed: {:?}", first.0, first.1);
    assert!(second.1.is_ok(), "{} failed: {:?}", second.0, second.1);
    let arch_inbox = fixture.inbox_contents("arch-ctm");
    let host_a_inbox = fixture.origin_inbox_contents("arch-ctm", "host-a");
    let host_b_inbox = fixture.origin_inbox_contents("arch-ctm", "host-b");
    let _ = (arch_inbox, host_a_inbox, host_b_inbox);
    assert!(fixture.primary_inbox_path("arch-ctm").exists());
    assert!(fixture.origin_inbox_path("arch-ctm", "host-a").exists());
    assert!(fixture.origin_inbox_path("arch-ctm", "host-b").exists());
}

#[test]
#[serial]
fn send_times_out_under_bounded_lock_contention() {
    let _env_lock = acquire_env_lock();
    let _timeout = EnvGuard::set_raw("ATM_TEST_MAILBOX_LOCK_TIMEOUT_MS", "100");
    let fixture = Fixture::new();
    let observability = NullObservability;
    let lock_path = sentinel_path(&fixture.primary_inbox_path("arch-ctm"));
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
        fixture.send_request("team-lead", "arch-ctm@atm-dev", "blocked send"),
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
    let _env_lock = acquire_env_lock();
    let fixture = Fixture::new();
    let observability = NullObservability;
    fixture.write_primary_inbox(
        "arch-ctm",
        &[unread_message(
            "team-lead",
            "read without lock",
            LegacyMessageId::from(Uuid::new_v4()),
        )],
    );
    let lock_path = sentinel_path(&fixture.primary_inbox_path("arch-ctm"));
    let lock_file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .expect("open lock file");
    lock_file.lock_exclusive().expect("hold mailbox lock");

    let started = Instant::now();
    let mut clear_query = fixture.clear_query("arch-ctm");
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
    let _env_lock = acquire_env_lock();
    let _timeout = EnvGuard::set_raw("ATM_TEST_MAILBOX_LOCK_TIMEOUT_MS", "100");
    let observability = NullObservability;

    let mutation_fixture = Fixture::new();
    mutation_fixture.write_primary_inbox(
        "arch-ctm",
        &[unread_message(
            "team-lead",
            "needs mark-read",
            LegacyMessageId::from(Uuid::new_v4()),
        )],
    );
    let mutation_lock_path = sentinel_path(&mutation_fixture.primary_inbox_path("arch-ctm"));
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
    let mut mutation_query = mutation_fixture.read_query("arch-ctm");
    mutation_query.ack_activation_mode = AckActivationMode::PromoteDisplayedUnread;
    let error = read_mail(mutation_query, &observability).expect_err("lock timeout");
    assert_eq!(error.code, AtmErrorCode::MailboxLockTimeout);

    let no_mutation_fixture = Fixture::new();
    no_mutation_fixture.write_primary_inbox(
        "arch-ctm",
        &[read_message(
            "team-lead",
            "already read",
            LegacyMessageId::from(Uuid::new_v4()),
        )],
    );
    let no_mutation_lock_path = sentinel_path(&no_mutation_fixture.primary_inbox_path("arch-ctm"));
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
    let mut no_mutation_query = no_mutation_fixture.read_query("arch-ctm");
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
        fixture.send_request("team-lead", "arch-ctm@atm-dev", "hello sidecar"),
        &observability,
    )
    .expect("send ULID-authored message");

    let inbox_before =
        fs::read_to_string(fixture.primary_inbox_path("arch-ctm")).expect("raw inbox before read");
    let physical_before = find_inbox_json_line(&inbox_before, "hello sidecar");
    let atm_message_id = physical_before["metadata"]["atm"]["messageId"]
        .as_str()
        .expect("atm message id")
        .to_string();
    assert_eq!(physical_before["read"], false);

    let mut read_query = fixture.read_query("arch-ctm");
    read_query.ack_activation_mode = AckActivationMode::PromoteDisplayedUnread;
    let outcome = read_mail(read_query, &observability).expect("read mail");
    assert!(
        outcome
            .messages
            .iter()
            .any(|message| message.envelope.text == "hello sidecar"),
        "read outcome should include the ULID-authored message"
    );

    let inbox_after =
        fs::read_to_string(fixture.primary_inbox_path("arch-ctm")).expect("raw inbox after read");
    assert_eq!(inbox_after, inbox_before);
    let physical_after = find_inbox_json_line(&inbox_after, "hello sidecar");
    assert_eq!(
        physical_after["metadata"]["atm"]["messageId"],
        atm_message_id
    );
    assert_eq!(physical_after["read"], false);
    assert!(
        !sentinel_path(&fixture.primary_inbox_path("arch-ctm")).exists(),
        "read-only ULID sidecar path must not leave a lock sentinel behind",
    );

    let workflow = fixture.workflow_state_contents("arch-ctm");
    assert_eq!(
        workflow["messages"][format!("atm:{atm_message_id}")]["read"],
        true
    );
}

#[test]
#[serial]
fn clear_fails_closed_on_synthetic_source_discovery_fault() {
    let _env_lock = acquire_env_lock();
    let _fault = EnvGuard::set_raw("ATM_TEST_FORCE_SOURCE_DISCOVERY_FAULT", "1");
    let fixture = Fixture::new();
    let observability = NullObservability;
    fixture.write_origin_inbox(
        "arch-ctm",
        "host-a",
        &[read_message(
            "qa",
            "origin read a",
            LegacyMessageId::from(Uuid::new_v4()),
        )],
    );
    let before_primary =
        fs::read_to_string(fixture.primary_inbox_path("arch-ctm")).expect("primary inbox before");
    let before_origin = fs::read_to_string(fixture.origin_inbox_path("arch-ctm", "host-a"))
        .expect("origin inbox before");

    let error = clear_mail(fixture.clear_query("arch-ctm"), &observability).expect_err("fault");

    assert_eq!(error.code, AtmErrorCode::MailboxReadFailed);
    assert_eq!(
        fs::read_to_string(fixture.primary_inbox_path("arch-ctm")).expect("primary inbox after"),
        before_primary
    );
    assert_eq!(
        fs::read_to_string(fixture.origin_inbox_path("arch-ctm", "host-a"))
            .expect("origin inbox after"),
        before_origin
    );
}

#[test]
#[serial]
fn send_reports_non_contention_lock_failures_without_timeout() {
    let _env_lock = acquire_env_lock();
    let _fault = EnvGuard::set_raw("ATM_TEST_FORCE_LOCK_NON_CONTENTION_ERROR", "1");
    let fixture = Fixture::new();
    let observability = NullObservability;
    let started = Instant::now();

    let error = send_mail(
        fixture.send_request("team-lead", "arch-ctm@atm-dev", "lock failure"),
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

// Serializes process-environment mutation across both threads and test
// processes. nextest runs separate test binaries in parallel, so a plain
// process-local mutex is not sufficient here.
fn acquire_env_lock() -> File {
    let lock_path = std::env::temp_dir().join("atm-mailbox-locking-env.lock");
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .expect("open env lock file");
    file.lock_exclusive().expect("lock env file");
    file
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
    // SAFETY: these tests hold the process-wide env_lock mutex and use
    // #[serial] before mutating the environment, so the mutation is
    // serialized within this process.
    unsafe { std::env::set_var(key, value) }
}

fn remove_env_var<K: AsRef<OsStr>>(key: K) {
    // SAFETY: these tests hold the process-wide env_lock mutex and use
    // #[serial] before mutating the environment, so the mutation is
    // serialized within this process.
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
        create_team_with_config(tempdir.path(), "atm-dev", &["team-lead", "arch-ctm", "qa"]);

        let arch_message_id = LegacyMessageId::new();
        let qa_message_id = LegacyMessageId::new();

        let fixture = Self {
            tempdir,
            arch_message_id,
            qa_message_id,
        };
        fixture.write_primary_inbox(
            "arch-ctm",
            &[pending_ack_message(
                "qa",
                "arch pending",
                arch_message_id,
                "atm-dev",
            )],
        );
        fixture.write_primary_inbox(
            "qa",
            &[pending_ack_message(
                "arch-ctm",
                "qa pending",
                qa_message_id,
                "atm-dev",
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
            team_override: Some("atm-dev".parse().expect("team")),
            message_id: AckMessageId::Legacy(message_id),
            reply_body: reply_body.to_string(),
        }
    }

    fn clear_query(&self, actor: &str) -> ClearQuery {
        ClearQuery {
            home_dir: self.tempdir.path().to_path_buf(),
            current_dir: self.tempdir.path().to_path_buf(),
            actor_override: Some(actor.parse().expect("actor")),
            target_address: None,
            team_override: Some("atm-dev".parse().expect("team")),
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
            Some("atm-dev"),
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
            Some("atm-dev"),
            SendMessageSource::Inline(text.to_string()),
            None,
            false,
            None,
            false,
        )
        .expect("send request")
    }

    fn inbox_contents(&self, agent: &str) -> Vec<MessageEnvelope> {
        self.inbox_contents_for_team("atm-dev", agent)
    }

    fn origin_inbox_contents(&self, agent: &str, suffix: &str) -> Vec<MessageEnvelope> {
        read_jsonl(self.origin_inbox_path(agent, suffix))
    }

    fn workflow_state_contents(&self, agent: &str) -> serde_json::Value {
        self.workflow_state_contents_for_team("atm-dev", agent)
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
        self.primary_inbox_path_for_team("atm-dev", agent)
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
            .join("atm-dev")
            .join("inboxes")
            .join(format!("{agent}.{suffix}.json"))
    }

    fn workflow_state_path(&self, agent: &str) -> std::path::PathBuf {
        self.workflow_state_path_for_team("atm-dev", agent)
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
    read_inbox_messages(&path).expect("inbox contents")
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
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("inbox dir");
    }
    write_inbox_messages(path, messages).expect("write inbox");
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
        extra: serde_json::Map::new(),
    }
}

fn read_message(from: &str, text: &str, message_id: LegacyMessageId) -> MessageEnvelope {
    MessageEnvelope {
        from: from.parse::<AgentName>().expect("agent"),
        text: text.to_string(),
        timestamp: IsoTimestamp::from_datetime(Utc::now()),
        read: true,
        source_team: Some("atm-dev".parse::<TeamName>().expect("team")),
        summary: None,
        message_id: Some(message_id),
        pending_ack_at: None,
        acknowledged_at: None,
        acknowledges_message_id: None,
        task_id: None,
        extra: serde_json::Map::new(),
    }
}

fn unread_message(from: &str, text: &str, message_id: LegacyMessageId) -> MessageEnvelope {
    MessageEnvelope {
        from: from.parse::<AgentName>().expect("agent"),
        text: text.to_string(),
        timestamp: IsoTimestamp::from_datetime(Utc::now()),
        read: false,
        source_team: Some("atm-dev".parse::<TeamName>().expect("team")),
        summary: None,
        message_id: Some(message_id),
        pending_ack_at: None,
        acknowledged_at: None,
        acknowledges_message_id: None,
        task_id: None,
        extra: serde_json::Map::new(),
    }
}
