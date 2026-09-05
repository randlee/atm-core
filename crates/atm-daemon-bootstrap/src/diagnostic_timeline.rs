//! Bootstrap-owned non-blocking adapter from the tracing bridge to SQLite.

use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use atm_core::observability_counters::{DiagnosticCounters, DiagnosticCountersSource};
use atm_observability::{
    DiagnosticSink, DropReason, RETAINED_FIELD_ALLOWLIST, RETAINED_INFO_TARGETS, RetainedEvent,
    SinkOffer, TracingBridgeStats,
};
use atm_storage::{DiagnosticEvent, DiagnosticTimelineStore};
use atm_storage_rusqlite::DIAGNOSTIC_DETAIL_MAX_BYTES;
use atm_storage_rusqlite::DiagnosticTimelinePersistenceStats;
use serde_json::{Map, Value};

pub const DIAGNOSTIC_BATCH_MAX: usize = 128;
pub const DIAGNOSTIC_FLUSH_INTERVAL_MS: u64 = 250;
pub const DEGRADATION_RECOVERY_WINDOW_SECS: u64 = 60;
pub const DEGRADATION_RATE_LIMIT_SECS: u64 = 60;

static INSTALLED_BRIDGE: OnceLock<Arc<atm_observability::TracingBridgeLayer>> = OnceLock::new();
static ACTIVE_TIMELINE: OnceLock<Arc<DiagnosticTimelineWriter>> = OnceLock::new();
static ACTIVE_COUNTERS: OnceLock<Arc<CombinedDiagnosticCounters>> = OnceLock::new();
static DEGRADATION_MONITOR: OnceLock<DegradationMonitor> = OnceLock::new();

pub(crate) fn register_bridge(bridge: Arc<atm_observability::TracingBridgeLayer>) {
    let _ = INSTALLED_BRIDGE.set(bridge);
}

pub(crate) fn attach_timeline(store: Arc<atm_storage_rusqlite::SqliteDiagnosticTimeline>) {
    let Some(bridge) = INSTALLED_BRIDGE.get() else {
        return;
    };
    let persistence_stats = store.persistence_stats();
    let store: Arc<dyn DiagnosticTimelineStore> = store;
    let stats = Arc::new(DiagnosticTimelineStats::default());
    let writer = Arc::new(DiagnosticTimelineWriter::new_with_persistence(
        store,
        DiagnosticPolicy::default(),
        stats,
        persistence_stats,
    ));
    writer.flush_due();
    let counters = Arc::new(CombinedDiagnosticCounters::new(
        bridge.stats(),
        writer.stats(),
    ));
    let monitor = DEGRADATION_MONITOR.get_or_init(DegradationMonitor::default);
    monitor.observe(
        "timeline",
        counters.snapshot().timeline_dropped_queue_full_total,
    );
    monitor.observe("jsonl", counters.snapshot().jsonl_dropped_queue_full_total);
    let _ = ACTIVE_COUNTERS.set(counters);
    let _ = ACTIVE_TIMELINE.set(Arc::clone(&writer));
    bridge.set_diagnostic_sink(writer);
    start_flush_worker();
}

fn start_flush_worker() {
    let _ = std::thread::Builder::new()
        .name("atm-diagnostic-timeline-flush".to_owned())
        .spawn(|| {
            loop {
                std::thread::park_timeout(Duration::from_millis(DIAGNOSTIC_FLUSH_INTERVAL_MS));
                let Some(writer) = ACTIVE_TIMELINE.get() else {
                    return;
                };
                writer.flush_due();
                if let Some(counters) = ACTIVE_COUNTERS.get()
                    && let Some(monitor) = DEGRADATION_MONITOR.get()
                {
                    monitor.observe(
                        "timeline",
                        counters.snapshot().timeline_dropped_queue_full_total,
                    );
                    monitor.observe("jsonl", counters.snapshot().jsonl_dropped_queue_full_total);
                }
            }
        });
}

/// Explicit selection policy for retained INFO diagnostics. WARN and ERROR
/// are always eligible because the bridge only offers retained events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticPolicy {
    info_targets: BTreeSet<String>,
}

impl Default for DiagnosticPolicy {
    fn default() -> Self {
        Self {
            info_targets: RETAINED_INFO_TARGETS
                .iter()
                .map(|target| (*target).to_owned())
                .collect(),
        }
    }
}

impl DiagnosticPolicy {
    pub fn permits(&self, event: &RetainedEvent<'_>) -> bool {
        !matches!(event.level, sc_observability_types::Level::Info)
            || self.info_targets.contains(event.component)
    }
}

/// Shared, non-blocking counters for the best-effort timeline path.
#[derive(Debug, Default)]
pub struct DiagnosticTimelineStats {
    pub timeline_written_total: AtomicU64,
    pub timeline_dropped_queue_full_total: AtomicU64,
    pub timeline_dropped_persist_error_total: AtomicU64,
}

/// AW.3 consumes a single snapshot source without importing either concrete
/// logging implementation.
#[derive(Debug)]
pub struct CombinedDiagnosticCounters {
    bridge: Arc<TracingBridgeStats>,
    timeline: Arc<DiagnosticTimelineStats>,
}

impl CombinedDiagnosticCounters {
    pub fn new(bridge: Arc<TracingBridgeStats>, timeline: Arc<DiagnosticTimelineStats>) -> Self {
        Self { bridge, timeline }
    }
}

impl DiagnosticCountersSource for CombinedDiagnosticCounters {
    fn snapshot(&self) -> DiagnosticCounters {
        let mut counters = self.bridge.snapshot();
        counters.timeline_written_total =
            self.timeline.timeline_written_total.load(Ordering::Relaxed);
        counters.timeline_dropped_queue_full_total = self
            .timeline
            .timeline_dropped_queue_full_total
            .load(Ordering::Relaxed);
        counters.timeline_dropped_persist_error_total = self
            .timeline
            .timeline_dropped_persist_error_total
            .load(Ordering::Relaxed);
        counters
    }
}

#[derive(Debug)]
struct BufferedEvents {
    events: Vec<DiagnosticEvent>,
    last_flush: Instant,
}

/// One bounded upstream batch. `offer` only takes a try-lock and delegates to
/// the store's lower-priority `try_send` lane, so observability cannot delay a
/// request path.
pub struct DiagnosticTimelineWriter {
    store: Arc<dyn DiagnosticTimelineStore>,
    policy: DiagnosticPolicy,
    stats: Arc<DiagnosticTimelineStats>,
    persistence_stats: Option<Arc<DiagnosticTimelinePersistenceStats>>,
    buffered: Mutex<BufferedEvents>,
}

impl std::fmt::Debug for DiagnosticTimelineWriter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DiagnosticTimelineWriter")
            .finish_non_exhaustive()
    }
}

impl DiagnosticTimelineWriter {
    pub fn new_with_persistence(
        store: Arc<dyn DiagnosticTimelineStore>,
        policy: DiagnosticPolicy,
        stats: Arc<DiagnosticTimelineStats>,
        persistence_stats: Arc<DiagnosticTimelinePersistenceStats>,
    ) -> Self {
        Self {
            store,
            policy,
            stats,
            persistence_stats: Some(persistence_stats),
            buffered: Mutex::new(BufferedEvents {
                events: Vec::with_capacity(DIAGNOSTIC_BATCH_MAX),
                last_flush: Instant::now(),
            }),
        }
    }

    pub fn stats(&self) -> Arc<DiagnosticTimelineStats> {
        Arc::clone(&self.stats)
    }

    /// Drives the fixed flush cadence from bootstrap's owned maintenance tick.
    /// It is separately callable in tests and never waits for SQLite.
    pub fn flush_due(&self) {
        let Ok(mut buffered) = self.buffered.try_lock() else {
            return;
        };
        if !buffered.events.is_empty()
            && buffered.last_flush.elapsed() >= Duration::from_millis(DIAGNOSTIC_FLUSH_INTERVAL_MS)
        {
            self.flush_locked(&mut buffered);
        }
        self.refresh_persisted_stats();
    }

    fn flush_locked(&self, buffered: &mut BufferedEvents) -> SinkOffer {
        let events = std::mem::take(&mut buffered.events);
        buffered.last_flush = Instant::now();
        let count = events.len() as u64;
        match self.store.record_batch(&events) {
            Ok(()) => SinkOffer::Accepted,
            Err(error) if error.message().contains("queue is full") => {
                self.stats
                    .timeline_dropped_queue_full_total
                    .fetch_add(count, Ordering::Relaxed);
                SinkOffer::Dropped(DropReason::QueueFull)
            }
            Err(_) => {
                self.stats
                    .timeline_dropped_persist_error_total
                    .fetch_add(count, Ordering::Relaxed);
                SinkOffer::Dropped(DropReason::PersistFailed)
            }
        }
    }

    fn refresh_persisted_stats(&self) {
        let Some(persistence) = &self.persistence_stats else {
            return;
        };
        self.stats
            .timeline_written_total
            .store(persistence.written_total(), Ordering::Relaxed);
        self.stats
            .timeline_dropped_persist_error_total
            .store(persistence.persist_error_total(), Ordering::Relaxed);
    }
}

impl DiagnosticSink for DiagnosticTimelineWriter {
    fn offer(&self, event: &RetainedEvent<'_>) -> SinkOffer {
        if !self.policy.permits(event) {
            return SinkOffer::Dropped(DropReason::Disabled);
        }
        let Ok(mut buffered) = self.buffered.try_lock() else {
            self.stats
                .timeline_dropped_queue_full_total
                .fetch_add(1, Ordering::Relaxed);
            return SinkOffer::Dropped(DropReason::QueueFull);
        };
        if buffered.events.len() == DIAGNOSTIC_BATCH_MAX {
            let offer = self.flush_locked(&mut buffered);
            if matches!(offer, SinkOffer::Dropped(_)) {
                return offer;
            }
        }
        buffered.events.push(diagnostic_event(event));
        if buffered.events.len() == DIAGNOSTIC_BATCH_MAX {
            let offer = self.flush_locked(&mut buffered);
            self.refresh_persisted_stats();
            offer
        } else {
            SinkOffer::Accepted
        }
    }
}

fn diagnostic_event(event: &RetainedEvent<'_>) -> DiagnosticEvent {
    let detail = retained_detail(event.fields);
    DiagnosticEvent {
        ts_unix_ms: event.ts_unix_ms,
        level: format!("{:?}", event.level).to_lowercase(),
        component: event.component.to_owned(),
        code: event.code.map(str::to_owned),
        correlation_id: event.correlation_id.map(str::to_owned),
        origin: event.origin.to_owned(),
        message: event.message.to_owned(),
        detail,
    }
}

fn retained_detail(fields: &[(&'static str, Value)]) -> Option<String> {
    let allowlist = RETAINED_FIELD_ALLOWLIST;
    let mut detail = Map::new();
    for (key, value) in fields {
        if allowlist.contains(key)
            && !matches!(
                *key,
                "ts" | "level" | "component" | "code" | "correlation_id" | "origin" | "message"
            )
        {
            detail.insert((*key).to_owned(), value.clone());
        }
    }
    if detail.is_empty() {
        return None;
    }
    let encoded = Value::Object(detail).to_string();
    Some(truncate_utf8(&encoded, DIAGNOSTIC_DETAIL_MAX_BYTES))
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let marker = "…";
    let limit = max_bytes.saturating_sub(marker.len());
    let end = value
        .char_indices()
        .take_while(|(index, _)| *index <= limit)
        .map(|(index, _)| index)
        .last()
        .unwrap_or_default();
    format!("{}{marker}", &value[..end])
}

/// Rate-limited saturation state machine. The emitted timeline-origin events
/// are retained JSONL records but the bridge's recursion rule excludes them
/// from the timeline.
#[derive(Debug, Default)]
pub struct DegradationMonitor {
    sinks: Mutex<HashMap<&'static str, SinkState>>,
}

#[derive(Debug, Default)]
struct SinkState {
    last_dropped: u64,
    degraded_since: Option<Instant>,
    last_transition: Option<Instant>,
    #[cfg(test)]
    emitted_codes: Vec<&'static str>,
}

impl DegradationMonitor {
    pub fn observe(&self, sink: &'static str, dropped_total: u64) {
        let Ok(mut sinks) = self.sinks.try_lock() else {
            return;
        };
        let state = sinks.entry(sink).or_default();
        let now = Instant::now();
        let rate_limited = state.last_transition.is_some_and(|last| {
            now.duration_since(last) < Duration::from_secs(DEGRADATION_RATE_LIMIT_SECS)
        });
        if dropped_total > state.last_dropped && state.degraded_since.is_none() && !rate_limited {
            state.degraded_since = Some(now);
            state.last_transition = Some(now);
            #[cfg(test)]
            state.emitted_codes.push("ATM_LOG_SINK_DEGRADED");
            tracing::warn!(
                origin = "timeline",
                code = "ATM_LOG_SINK_DEGRADED",
                sink,
                dropped = dropped_total,
                "diagnostic sink is degraded"
            );
        } else if state.degraded_since.is_some_and(|since| {
            now.duration_since(since) >= Duration::from_secs(DEGRADATION_RECOVERY_WINDOW_SECS)
        }) && dropped_total == state.last_dropped
            && !rate_limited
        {
            state.degraded_since = None;
            state.last_transition = Some(now);
            #[cfg(test)]
            state.emitted_codes.push("ATM_LOG_SINK_RECOVERED");
            tracing::warn!(
                origin = "timeline",
                code = "ATM_LOG_SINK_RECOVERED",
                sink,
                dropped = dropped_total,
                "diagnostic sink recovered"
            );
        }
        state.last_dropped = dropped_total;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::{
        BufferedEvents, DEGRADATION_RECOVERY_WINDOW_SECS, DegradationMonitor, DiagnosticPolicy,
        DiagnosticTimelineStats, DiagnosticTimelineWriter,
    };
    use atm_observability::{DiagnosticSink, DropReason, RetainedEvent, SinkOffer};
    use atm_storage::{AtmError, DiagnosticEvent, DiagnosticQuery, DiagnosticTimelineStore};

    #[derive(Default)]
    struct FixtureTimeline {
        batches: std::sync::Mutex<Vec<Vec<DiagnosticEvent>>>,
        fail: AtomicBool,
    }

    impl DiagnosticTimelineStore for FixtureTimeline {
        fn record_batch(&self, events: &[DiagnosticEvent]) -> Result<(), AtmError> {
            if self.fail.load(Ordering::Relaxed) {
                return Err(AtmError::daemon_unavailable("fixture persistence failure"));
            }
            self.batches
                .lock()
                .expect("fixture batches")
                .push(events.to_vec());
            Ok(())
        }

        fn query(&self, _query: &DiagnosticQuery) -> Result<Vec<DiagnosticEvent>, AtmError> {
            Ok(Vec::new())
        }

        fn prune(&self, _now_unix_ms: i64) -> Result<u64, AtmError> {
            Ok(0)
        }
    }

    /// Models the real lower-priority lane while its worker is paused in the
    /// first batch: one batch is in flight and the bounded queue holds the
    /// remaining `DIAGNOSTIC_QUEUE_BATCHES` batches.
    #[derive(Default)]
    struct PausedTimeline {
        accepted_batches: AtomicUsize,
    }

    impl DiagnosticTimelineStore for PausedTimeline {
        fn record_batch(&self, _events: &[DiagnosticEvent]) -> Result<(), AtmError> {
            let accepted = self.accepted_batches.fetch_add(1, Ordering::Relaxed);
            if accepted < atm_runtime_test_support::diagnostic_queue_batches_for_test() + 1 {
                Ok(())
            } else {
                Err(AtmError::daemon_unavailable(
                    "diagnostic timeline queue is full; batch dropped",
                ))
            }
        }

        fn query(&self, _query: &DiagnosticQuery) -> Result<Vec<DiagnosticEvent>, AtmError> {
            Ok(Vec::new())
        }

        fn prune(&self, _now_unix_ms: i64) -> Result<u64, AtmError> {
            Ok(0)
        }
    }

    fn fixture_writer(store: std::sync::Arc<FixtureTimeline>) -> DiagnosticTimelineWriter {
        DiagnosticTimelineWriter {
            store,
            policy: DiagnosticPolicy::default(),
            stats: std::sync::Arc::new(DiagnosticTimelineStats::default()),
            persistence_stats: None,
            buffered: std::sync::Mutex::new(BufferedEvents {
                events: Vec::new(),
                last_flush: std::time::Instant::now(),
            }),
        }
    }

    fn info_event(component: &'static str) -> RetainedEvent<'static> {
        RetainedEvent {
            ts_unix_ms: 42,
            level: sc_observability_types::Level::Info,
            component,
            code: Some("ATM_FIXTURE"),
            correlation_id: None,
            origin: "tracing",
            message: "fixture diagnostic",
            fields: &[],
        }
    }

    #[test]
    fn ac2_policy_selected_info_rows_reach_the_timeline() {
        let store = std::sync::Arc::new(FixtureTimeline::default());
        let writer = fixture_writer(std::sync::Arc::clone(&store));
        let selected = info_event(atm_observability::RETAINED_INFO_TARGETS[0]);
        let unselected = info_event("fixture.unselected");

        assert_eq!(
            writer.offer(&unselected),
            SinkOffer::Dropped(DropReason::Disabled)
        );
        for _ in 0..super::DIAGNOSTIC_BATCH_MAX {
            assert_eq!(writer.offer(&selected), SinkOffer::Accepted);
        }

        let batches = store.batches.lock().expect("fixture batches");
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].len(), super::DIAGNOSTIC_BATCH_MAX);
        assert!(
            batches[0]
                .iter()
                .all(|event| event.component == selected.component && event.level == "info")
        );
    }

    #[test]
    fn ac3_diagnostic_persistence_failure_does_not_poison_later_offers() {
        let store = std::sync::Arc::new(FixtureTimeline::default());
        store.fail.store(true, Ordering::Relaxed);
        let writer = fixture_writer(std::sync::Arc::clone(&store));
        let event = info_event(atm_observability::RETAINED_INFO_TARGETS[0]);

        for _ in 1..super::DIAGNOSTIC_BATCH_MAX {
            assert_eq!(writer.offer(&event), SinkOffer::Accepted);
        }
        assert_eq!(
            writer.offer(&event),
            SinkOffer::Dropped(DropReason::PersistFailed)
        );
        assert_eq!(
            writer
                .stats()
                .timeline_dropped_persist_error_total
                .load(Ordering::Relaxed),
            super::DIAGNOSTIC_BATCH_MAX as u64
        );

        store.fail.store(false, Ordering::Relaxed);
        for _ in 0..super::DIAGNOSTIC_BATCH_MAX {
            assert_eq!(writer.offer(&event), SinkOffer::Accepted);
        }
        assert_eq!(store.batches.lock().expect("fixture batches").len(), 1);
    }

    #[test]
    fn ac7_saturation_state_transitions_from_degraded_to_recovered_after_quiet_window() {
        let monitor = DegradationMonitor::default();
        monitor.observe("timeline", 1);
        {
            let mut sinks = monitor.sinks.lock().expect("monitor state");
            let state = sinks.get_mut("timeline").expect("degraded state");
            assert!(state.degraded_since.is_some());
            state.degraded_since = Some(
                std::time::Instant::now()
                    - std::time::Duration::from_secs(DEGRADATION_RECOVERY_WINDOW_SECS + 1),
            );
            state.last_transition = None;
        }

        monitor.observe("timeline", 1);
        let sinks = monitor.sinks.lock().expect("monitor state");
        assert!(
            sinks
                .get("timeline")
                .expect("recovered state")
                .degraded_since
                .is_none()
        );
    }

    #[test]
    fn ac7_offer_overflow_counts_the_paused_writer_excess() {
        let store = std::sync::Arc::new(PausedTimeline::default());
        let writer = DiagnosticTimelineWriter {
            store,
            policy: DiagnosticPolicy::default(),
            stats: std::sync::Arc::new(DiagnosticTimelineStats::default()),
            persistence_stats: None,
            buffered: std::sync::Mutex::new(BufferedEvents {
                events: Vec::new(),
                last_flush: std::time::Instant::now(),
            }),
        };
        let event = info_event(atm_observability::RETAINED_INFO_TARGETS[0]);
        let accepted_events = (atm_runtime_test_support::diagnostic_queue_batches_for_test() + 1)
            * super::DIAGNOSTIC_BATCH_MAX;
        for _ in 0..accepted_events {
            assert_eq!(writer.offer(&event), SinkOffer::Accepted);
        }

        for _ in 1..super::DIAGNOSTIC_BATCH_MAX {
            assert_eq!(writer.offer(&event), SinkOffer::Accepted);
        }
        assert_eq!(
            writer.offer(&event),
            SinkOffer::Dropped(DropReason::QueueFull)
        );
        assert_eq!(
            writer
                .stats()
                .timeline_dropped_queue_full_total
                .load(Ordering::Relaxed),
            super::DIAGNOSTIC_BATCH_MAX as u64,
            "the only excess beyond the one in-flight and bounded queued batches is dropped"
        );

        let monitor = DegradationMonitor::default();
        let dropped_total = writer
            .stats()
            .timeline_dropped_queue_full_total
            .load(Ordering::Relaxed);
        monitor.observe("timeline", dropped_total);
        monitor.observe("timeline", dropped_total);
        {
            let mut sinks = monitor.sinks.lock().expect("monitor state");
            let state = sinks.get_mut("timeline").expect("degraded state");
            assert_eq!(state.emitted_codes, ["ATM_LOG_SINK_DEGRADED"]);
            state.degraded_since = Some(
                std::time::Instant::now()
                    - std::time::Duration::from_secs(DEGRADATION_RECOVERY_WINDOW_SECS + 1),
            );
            state.last_transition = None;
        }
        monitor.observe("timeline", dropped_total);
        let sinks = monitor.sinks.lock().expect("monitor state");
        assert_eq!(
            sinks
                .get("timeline")
                .expect("recovered state")
                .emitted_codes,
            ["ATM_LOG_SINK_DEGRADED", "ATM_LOG_SINK_RECOVERED"]
        );
    }
}
