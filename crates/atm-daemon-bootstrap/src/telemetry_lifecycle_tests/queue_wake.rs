//! Queue-wake producers (reminder, lead notification, reminder reset) through
//! the pump the daemon composition built, sharing its assembly and exporter.
//!
//! The composed pump runs on a test clock that the test advances by whole
//! reminder intervals instead of waiting for real idle minutes. Its own
//! polling task is parked at its next Herdr list call, so the test's ticks of
//! that same pump are the only ones that consume the fake Herdr observations.
#![cfg(test)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use atm_core::send::SendMessageSource;
use atm_core::test_support::FakeEnvSource;
use atm_core::types::IsoTimestamp;
use atm_herdr::testing::{FakeHerdrCall, FakeHerdrProcessAdapter};
use atm_herdr::{AgentSnapshot, HerdrAgentStatus, HerdrListOutcome};

use super::receiver::Receiver;
use super::{
    Daemon, EXPORT_WAIT, EventIdentity, endpoint_env, export_health, exported_counts,
    exported_events, sent_message_id,
};
use atm_core::observability::AtmTelemetryExportState;

const TASK: &str = "BD6-R1";
const MINUTE_MS: i64 = 60_000;

fn list_calls(herdr: &FakeHerdrProcessAdapter) -> usize {
    herdr
        .calls()
        .iter()
        .filter(|call| matches!(call, FakeHerdrCall::List { .. }))
        .count()
}

fn worker_status(status: HerdrAgentStatus) -> HerdrListOutcome {
    HerdrListOutcome {
        agents: vec![AgentSnapshot {
            name: Some("worker".to_owned()),
            pane_id: None,
            status,
            workspace_id: None,
        }],
    }
}

fn kind_counts(events: &[EventIdentity]) -> BTreeMap<String, i64> {
    let mut counts = BTreeMap::new();
    for event in events {
        *counts.entry(event.kind.clone()).or_insert(0) += 1;
    }
    counts
}

struct Driver<'a> {
    daemon: &'a Daemon,
    offset_ms: Arc<AtomicI64>,
}

impl Driver<'_> {
    /// One pass of the composed pump at `offset_ms` past real time.
    async fn tick(&self, offset_ms: i64, status: HerdrAgentStatus) {
        self.offset_ms.store(offset_ms, Ordering::SeqCst);
        self.daemon
            .herdr
            .queue_list_result(Ok(worker_status(status)));
        let pump = self.daemon.pump.as_ref().expect("composed pump");
        tokio::time::timeout(Duration::from_secs(20), pump.tick_once())
            .await
            .expect("a composed pump pass completes");
    }

    async fn has(&self, kind: &str) -> bool {
        self.daemon
            .committed(&[TASK])
            .await
            .iter()
            .any(|event| event.kind == kind)
    }
}

/// Composes the daemon with a test clock on its queue-wake pump.
async fn start_clocked(env: FakeEnvSource) -> (Daemon, Arc<AtomicI64>) {
    let offset_ms = Arc::new(AtomicI64::new(0));
    let clock = {
        let offset_ms = Arc::clone(&offset_ms);
        Arc::new(move || {
            IsoTimestamp::from_datetime(
                IsoTimestamp::now().into_inner()
                    + chrono::TimeDelta::milliseconds(offset_ms.load(Ordering::SeqCst)),
            )
        })
    };
    let (root, observability) = Daemon::bootstrap(env).await;
    let daemon = Daemon::compose_with(root, observability, Some(clock)).await;
    (daemon, offset_ms)
}

/// Assigns [`TASK`] to the Herdr worker, then ticks the composed pump through
/// reminder intervals until the lead notification, and on to the reset. The
/// running polling task stays parked until the returned gate is released.
async fn drive_through_lead_notification_and_reset(
    daemon: &Daemon,
    offset_ms: Arc<AtomicI64>,
) -> Arc<tokio::sync::Notify> {
    sent_message_id(
        daemon
            .write(daemon.request("sender", "worker", TASK, None))
            .await
            .expect("task assignment to the Herdr worker"),
    );

    // Park the polling task at its next list call (at most one 5s interval).
    let baseline = list_calls(&daemon.herdr);
    let parked = daemon.herdr.block_next_list();
    let mut cadence = tokio::time::interval(Duration::from_millis(50));
    tokio::time::timeout(Duration::from_secs(15), async {
        while list_calls(&daemon.herdr) == baseline {
            cadence.tick().await;
        }
    })
    .await
    .expect("the running pump reaches its next Herdr list call");

    let driver = Driver { daemon, offset_ms };
    let mut minute = 0;
    while !driver.has("lead_notified").await {
        assert!(
            minute <= 20,
            "no lead notification after {minute} reminder intervals"
        );
        driver
            .tick(minute * MINUTE_MS, HerdrAgentStatus::Idle)
            .await;
        minute += 1;
    }
    // Sustained activity for one full interval resets the reminders.
    driver
        .tick(
            minute * MINUTE_MS + MINUTE_MS / 2,
            HerdrAgentStatus::Working,
        )
        .await;
    driver
        .tick(
            (minute + 1) * MINUTE_MS + MINUTE_MS / 2,
            HerdrAgentStatus::Working,
        )
        .await;
    parked
}

/// Positive: the daemon-composed queue-wake pump commits reminders, the
/// stalled-task lead notification and the reminder reset, and each reaches
/// the collector as exactly its durable row (identity, kind, seq and time),
/// with the per-kind counter, while the daemon is still serving.
/// Negative: no span exists without its durable row.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn composed_queue_wake_pump_exports_reminders_lead_notification_and_reset() {
    let receiver = Receiver::start(false).await;
    let (daemon, offset_ms) = start_clocked(endpoint_env(&receiver.endpoint)).await;
    let parked = drive_through_lead_notification_and_reset(&daemon, offset_ms).await;

    let committed = daemon.committed(&[TASK]).await;
    let counts = kind_counts(&committed);
    assert_eq!(counts.get("assigned"), Some(&1), "{counts:?}");
    assert!(
        counts.get("reminded").is_some_and(|count| *count >= 1),
        "{counts:?}"
    );
    assert_eq!(counts.get("lead_notified"), Some(&1), "{counts:?}");
    assert_eq!(counts.get("reminders_reset"), Some(&1), "{counts:?}");
    receiver
        .capture
        .wait(EXPORT_WAIT, "every committed queue-wake row", || {
            exported_events(&receiver.capture) == committed
        })
        .await;
    receiver
        .capture
        .wait(EXPORT_WAIT, "the per-kind counter", || {
            exported_counts(&receiver.capture) == counts
        })
        .await;
    let health = export_health(&daemon.doctor_json().await);
    assert_eq!(health.state, AtmTelemetryExportState::Healthy);
    assert_eq!(health.emitted, committed.len() as u64);

    parked.notify_one();
    daemon.shutdown().await.expect("daemon shutdown");
    receiver.stop().await;
}

const ISOLATED_HOME_CHILD: &str =
    "telemetry_lifecycle_tests::queue_wake::composed_daemon_home_child";
const SENTINEL_HOME: &str = "ATM_BD6_SENTINEL_HOME";

/// Every path below `root`, relative to it; empty when `root` is absent.
fn tree(root: &Path) -> BTreeSet<PathBuf> {
    let mut paths = BTreeSet::new();
    if !root.exists() {
        return paths;
    }
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).expect("read directory") {
            let path = entry.expect("directory entry").path();
            paths.insert(path.strip_prefix(root).expect("below root").to_path_buf());
            if path.is_dir() {
                pending.push(path);
            }
        }
    }
    paths
}

/// Positive: the composed router and queue-wake pump write under the daemon
/// home that composition was given: a file-reference send lands its share
/// copy there, and the pump's lead-notification escalation mail commits.
/// Negative: with the ambient `ATM_HOME` pointing at a separate sentinel
/// directory, nothing is created there.
/// No-Claim: this scopes the composed daemon's home-derived writes, not every
/// host filesystem access.
/// The child process owns `ATM_HOME`, so no parallel test sees it change.
#[test]
fn composed_daemon_home_is_injected_not_ambient() {
    let sentinel = tempfile::tempdir().expect("sentinel home");
    let sentinel_path = sentinel.path().to_str().expect("utf-8 sentinel home");
    super::exit::run_child_scenario_with(
        ISOLATED_HOME_CHILD,
        &[("ATM_HOME", sentinel_path), (SENTINEL_HOME, sentinel_path)],
    );
    assert_eq!(
        tree(sentinel.path()),
        BTreeSet::new(),
        "the ambient ATM_HOME stays untouched"
    );
}

/// Child half of [`composed_daemon_home_is_injected_not_ambient`].
#[test]
fn composed_daemon_home_child() {
    if !super::exit::is_child_scenario(ISOLATED_HOME_CHILD) {
        return;
    }
    let sentinel = PathBuf::from(std::env::var_os(SENTINEL_HOME).expect("sentinel home"));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("child runtime");
    runtime.block_on(async {
        let (daemon, offset_ms) = start_clocked(FakeEnvSource::empty()).await;
        let attachment = daemon.root.path().join("outside").join("bd6-f12.txt");
        std::fs::create_dir_all(attachment.parent().unwrap()).expect("attachment dir");
        std::fs::write(&attachment, "bd6 f12 attachment").expect("attachment");
        let mut request = daemon.request("sender", "recipient", "BD6-F12", None);
        request.message_source = SendMessageSource::File {
            path: attachment.clone(),
            message: None,
        };
        sent_message_id(
            daemon
                .write(request)
                .await
                .expect("file-reference send through the daemon"),
        );
        let parked = drive_through_lead_notification_and_reset(&daemon, offset_ms).await;

        let home = daemon.root.path().join("home");
        assert!(
            home.join(".config/atm/share")
                .join(super::TEAM)
                .join("bd6-f12.txt")
                .is_file(),
            "the share copy lands in the injected daemon home: {:?}",
            tree(&home)
        );
        let counts = kind_counts(&daemon.committed(&[TASK]).await);
        assert_eq!(counts.get("lead_notified"), Some(&1), "{counts:?}");
        assert_eq!(
            tree(&sentinel),
            BTreeSet::new(),
            "nothing is written under the ambient ATM_HOME"
        );
        parked.notify_one();
        daemon.shutdown().await.expect("daemon shutdown");
    });
}
