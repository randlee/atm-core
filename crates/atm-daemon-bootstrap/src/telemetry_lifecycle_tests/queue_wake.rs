//! Queue-wake producers (reminder, lead notification, reminder reset) through
//! the pump the daemon composition built, sharing its assembly and exporter.
//!
//! The composed pump runs on a test clock that the test advances by whole
//! reminder intervals instead of waiting for real idle minutes. Its own
//! polling task is parked at its next Herdr list call, so the test's ticks of
//! that same pump are the only ones that consume the fake Herdr observations.
#![cfg(test)]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

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

/// Positive: the daemon-composed queue-wake pump commits reminders, the
/// stalled-task lead notification and the reminder reset, and each reaches
/// the collector as exactly its durable row (identity, kind, seq and time),
/// with the per-kind counter, while the daemon is still serving.
/// Negative: no span exists without its durable row.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn composed_queue_wake_pump_exports_reminders_lead_notification_and_reset() {
    let receiver = Receiver::start(false).await;
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
    let (root, observability) = Daemon::bootstrap(endpoint_env(&receiver.endpoint)).await;
    let daemon = Daemon::compose_with(root, observability, Some(clock)).await;
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

    let driver = Driver {
        daemon: &daemon,
        offset_ms,
    };
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
