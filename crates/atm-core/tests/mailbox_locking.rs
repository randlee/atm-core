use std::ffi::{OsStr, OsString};
use std::fs;
use std::fs::OpenOptions;
use std::sync::{Arc, Barrier, Mutex, OnceLock, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use atm_core::ack::{AckRequest, ack_mail};
use atm_core::address::AgentAddress;
use atm_core::clear::{ClearQuery, clear_mail};
use atm_core::error::AtmErrorCode;
use atm_core::observability::NullObservability;
use atm_core::read::{ReadQuery, read_mail};
use atm_core::schema::{AgentMember, LegacyMessageId, MessageEnvelope, TeamConfig};
use atm_core::send::{SendMessageSource, SendRequest, send_mail};
use atm_core::types::{AckActivationMode, IsoTimestamp, ReadSelection};
use chrono::Utc;
use fs2::FileExt;
use serial_test::serial;
use tempfile::TempDir;
use uuid::Uuid;

#[test]
#[serial]
fn concurrent_ack_on_overlapping_inbox_sets_completes_without_deadlock() {
    let fixture = Fixture::new();
    let observability = Arc::new(NullObservability);
    let barrier = Arc::new(Barrier::new(3));
    let (tx, rx) = mpsc::channel();

    let arch_request = fixture.ack_request("arch-ctm", fixture.arch_message_id, "ack from arch");
    let qa_request = fixture.ack_request("qa", fixture.qa_message_id, "ack from qa");

    let started = Instant::now();
    for (label, request) in [("arch", arch_request), ("qa", qa_request)] {
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
        "second ack failed for {}: {:?}; arch inbox: {:?}; qa inbox: {:?}",
        second.0,
        second.1,
        fixture.inbox_contents("arch-ctm"),
        fixture.inbox_contents("qa")
    );
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "overlapping ack operations exceeded the deadlock budget"
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
    let started = Instant::now();
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
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "send + clear exceeded the deadlock budget"
    );
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
    let started = Instant::now();
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
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "send + ack exceeded the deadlock budget"
    );

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
            message.message_id == Some(pending_message_id) && message.acknowledged_at.is_some()
        }),
        "pending message was not acknowledged: {:?}",
        arch_inbox
    );
    let qa_inbox = ack_fixture.inbox_contents("qa");
    assert!(
        qa_inbox.iter().any(|message| message.text == "ack reply"),
        "ack reply was not persisted: {:?}",
        qa_inbox
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
    let started = Instant::now();
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
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "multi-source read/clear exceeded the deadlock budget"
    );

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
    let _env_lock = env_lock().lock().expect("env lock");
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
        started.elapsed() < Duration::from_secs(1),
        "lock-timeout coverage exceeded the deterministic budget"
    );
}

#[test]
#[serial]
fn clear_dry_run_does_not_wait_on_mailbox_lock() {
    let _env_lock = env_lock().lock().expect("env lock");
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
        started.elapsed() < Duration::from_secs(1),
        "read-only mailbox query should not wait on the mailbox lock"
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
        started.elapsed() < Duration::from_secs(1),
        "read should skip mailbox locks when no display mutation is needed"
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
    let _env_lock = env_lock().lock().expect("env lock");
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
        started.elapsed() < Duration::from_secs(1),
        "non-contention lock classification should fail fast"
    );
}

enum CommandOp {
    Read(ReadQuery, Arc<NullObservability>),
    Clear(ClearQuery, Arc<NullObservability>),
}

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
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
        let team_dir = tempdir.path().join(".claude").join("teams").join("atm-dev");
        fs::create_dir_all(team_dir.join("inboxes")).expect("inboxes");

        let config = TeamConfig {
            members: ["team-lead", "arch-ctm", "qa"]
                .into_iter()
                .map(|name| AgentMember {
                    name: name.to_string(),
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

        let arch_message_id = LegacyMessageId::from(Uuid::new_v4());
        let qa_message_id = LegacyMessageId::from(Uuid::new_v4());

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
            actor_override: Some(actor.into()),
            team_override: Some("atm-dev".into()),
            message_id,
            reply_body: reply_body.to_string(),
        }
    }

    fn clear_query(&self, actor: &str) -> ClearQuery {
        ClearQuery {
            home_dir: self.tempdir.path().to_path_buf(),
            current_dir: self.tempdir.path().to_path_buf(),
            actor_override: Some(actor.into()),
            target_address: None,
            team_override: Some("atm-dev".into()),
            older_than: None,
            idle_only: false,
            dry_run: false,
        }
    }

    fn read_query(&self, actor: &str) -> ReadQuery {
        ReadQuery {
            home_dir: self.tempdir.path().to_path_buf(),
            current_dir: self.tempdir.path().to_path_buf(),
            actor_override: Some(actor.into()),
            target_address: None,
            team_override: Some("atm-dev".into()),
            selection_mode: ReadSelection::Actionable,
            seen_state_filter: false,
            seen_state_update: false,
            ack_activation_mode: AckActivationMode::ReadOnly,
            limit: None,
            sender_filter: None,
            timestamp_filter: None,
            timeout_secs: None,
        }
    }

    fn send_request(&self, sender: &str, to: &str, text: &str) -> SendRequest {
        SendRequest {
            home_dir: self.tempdir.path().to_path_buf(),
            current_dir: self.tempdir.path().to_path_buf(),
            sender_override: Some(sender.into()),
            to: to.parse::<AgentAddress>().expect("address"),
            team_override: Some("atm-dev".into()),
            message_source: SendMessageSource::Inline(text.to_string()),
            summary_override: None,
            requires_ack: false,
            task_id: None,
            dry_run: false,
        }
    }

    fn inbox_contents(&self, agent: &str) -> Vec<MessageEnvelope> {
        read_jsonl(self.primary_inbox_path(agent))
    }

    fn origin_inbox_contents(&self, agent: &str, suffix: &str) -> Vec<MessageEnvelope> {
        read_jsonl(self.origin_inbox_path(agent, suffix))
    }

    fn write_primary_inbox(&self, agent: &str, messages: &[MessageEnvelope]) {
        write_inbox(&self.primary_inbox_path(agent), messages);
    }

    fn write_origin_inbox(&self, agent: &str, suffix: &str, messages: &[MessageEnvelope]) {
        write_inbox(&self.origin_inbox_path(agent, suffix), messages);
    }

    fn primary_inbox_path(&self, agent: &str) -> std::path::PathBuf {
        self.tempdir
            .path()
            .join(".claude")
            .join("teams")
            .join("atm-dev")
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
}

fn read_jsonl(path: std::path::PathBuf) -> Vec<MessageEnvelope> {
    let raw = fs::read_to_string(path).expect("inbox contents");
    raw.lines()
        .map(|line| serde_json::from_str(line).expect("json line"))
        .collect()
}

fn write_inbox(path: &std::path::Path, messages: &[MessageEnvelope]) {
    let raw = messages
        .iter()
        .map(|message| serde_json::to_string(message).expect("json line"))
        .collect::<Vec<_>>()
        .join("\n");
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
    MessageEnvelope {
        from: from.to_string(),
        text: text.to_string(),
        timestamp: IsoTimestamp::from_datetime(Utc::now()),
        read: true,
        source_team: Some(source_team.to_string()),
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
        from: from.to_string(),
        text: text.to_string(),
        timestamp: IsoTimestamp::from_datetime(Utc::now()),
        read: true,
        source_team: Some("atm-dev".to_string()),
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
        from: from.to_string(),
        text: text.to_string(),
        timestamp: IsoTimestamp::from_datetime(Utc::now()),
        read: false,
        source_team: Some("atm-dev".to_string()),
        summary: None,
        message_id: Some(message_id),
        pending_ack_at: None,
        acknowledged_at: None,
        acknowledges_message_id: None,
        task_id: None,
        extra: serde_json::Map::new(),
    }
}
