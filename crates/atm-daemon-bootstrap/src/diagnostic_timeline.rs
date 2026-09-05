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
use atm_storage::{DiagnosticEvent, DiagnosticRecordError, DiagnosticTimelineStore};
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

/// Returns the process-owned retained-diagnostic counter projection after the
/// SQLite timeline has been attached at bootstrap.
pub(crate) fn active_counters() -> Option<Arc<dyn DiagnosticCountersSource>> {
    ACTIVE_COUNTERS
        .get()
        .map(|counters| Arc::clone(counters) as Arc<dyn DiagnosticCountersSource>)
}

/// Attaches the SQLite-backed timeline to the process-global tracing bridge.
///
/// Idempotent: a second call (whether a genuine re-attach attempt or a race
/// between concurrent bootstrap paths) is a no-op. Only the first caller to
/// win [`ACTIVE_TIMELINE`] swaps the bridge's diagnostic sink and starts the
/// flush worker; every later call returns immediately so the original writer
/// keeps draining on its cadence instead of being silently orphaned.
pub(crate) fn attach_timeline(store: Arc<atm_storage_rusqlite::SqliteDiagnosticTimeline>) {
    if ACTIVE_TIMELINE.get().is_some() {
        return;
    }
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
    if ACTIVE_TIMELINE.set(Arc::clone(&writer)).is_err() {
        // Lost a race with a concurrent attach call; the winner already owns
        // the bridge sink and flush worker, so leave both untouched.
        return;
    }
    let _ = ACTIVE_COUNTERS.set(counters);
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
    /// Last persistence-side `persist_error_total` observed by
    /// [`DiagnosticTimelineWriter::refresh_persisted_stats`], used to fold the
    /// writer worker's own persistence failures into
    /// `timeline_dropped_persist_error_total` as a delta instead of an
    /// overwrite, so bootstrap-side drops (`WriterClosed`/`InvalidBatch`) are
    /// never discarded.
    writer_persist_error_baseline: AtomicU64,
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
            Err(DiagnosticRecordError::QueueFull) => {
                self.stats
                    .timeline_dropped_queue_full_total
                    .fetch_add(count, Ordering::Relaxed);
                SinkOffer::Dropped(DropReason::QueueFull)
            }
            Err(
                DiagnosticRecordError::WriterClosed
                | DiagnosticRecordError::InvalidBatch
                | DiagnosticRecordError::PersistFailed(_),
            ) => {
                self.stats
                    .timeline_dropped_persist_error_total
                    .fetch_add(count, Ordering::Relaxed);
                SinkOffer::Dropped(DropReason::PersistFailed)
            }
        }
    }

    /// Folds the persistence-layer's own counters into the shared stats.
    ///
    /// `written_total` is solely owned by the writer worker so it is safe to
    /// mirror directly. `persist_error_total`, however, counts a disjoint
    /// population from `timeline_dropped_persist_error_total`: the writer
    /// worker only increments it for failures discovered *after* a batch was
    /// already accepted onto the queue, whereas `flush_locked` above
    /// increments `timeline_dropped_persist_error_total` for batches that
    /// never reached the writer at all (`WriterClosed`/`InvalidBatch`).
    /// Overwriting one with the other would silently discard whichever
    /// source was not the most recent poll, so the writer's contribution is
    /// folded in as a monotonic delta instead.
    fn refresh_persisted_stats(&self) {
        let Some(persistence) = &self.persistence_stats else {
            return;
        };
        self.stats
            .timeline_written_total
            .store(persistence.written_total(), Ordering::Relaxed);
        let observed = persistence.persist_error_total();
        let previous = self
            .stats
            .writer_persist_error_baseline
            .swap(observed, Ordering::Relaxed);
        if let Some(delta) = observed.checked_sub(previous)
            && delta > 0
        {
            self.stats
                .timeline_dropped_persist_error_total
                .fetch_add(delta, Ordering::Relaxed);
        }
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
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::{
        BufferedEvents, DEGRADATION_RECOVERY_WINDOW_SECS, DegradationMonitor, DiagnosticPolicy,
        DiagnosticTimelineStats, DiagnosticTimelineWriter,
    };
    use atm_observability::{DiagnosticSink, DropReason, RetainedEvent, SinkOffer};
    use atm_storage::{
        AtmError, DiagnosticEvent, DiagnosticQuery, DiagnosticRecordError, DiagnosticTimelineStore,
    };

    #[derive(Default)]
    struct FixtureTimeline {
        batches: std::sync::Mutex<Vec<Vec<DiagnosticEvent>>>,
        fail: AtomicBool,
        fail_queue_full: AtomicBool,
    }

    impl DiagnosticTimelineStore for FixtureTimeline {
        fn record_batch(&self, events: &[DiagnosticEvent]) -> Result<(), DiagnosticRecordError> {
            if self.fail_queue_full.load(Ordering::Relaxed) {
                return Err(DiagnosticRecordError::QueueFull);
            }
            if self.fail.load(Ordering::Relaxed) {
                return Err(DiagnosticRecordError::PersistFailed(
                    AtmError::daemon_unavailable("fixture persistence failure"),
                ));
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

    /// C3/A8: `record_batch` failures must classify by the typed
    /// [`DiagnosticRecordError`] variant, not by sniffing `AtmError` message
    /// text, so a queue-full drop is never miscounted as a persist error (or
    /// vice versa).
    #[test]
    fn queue_full_offer_is_classified_as_queue_full_not_persist_error() {
        let store = std::sync::Arc::new(FixtureTimeline::default());
        store.fail_queue_full.store(true, Ordering::Relaxed);
        let writer = fixture_writer(std::sync::Arc::clone(&store));
        let event = info_event(atm_observability::RETAINED_INFO_TARGETS[0]);

        for _ in 1..super::DIAGNOSTIC_BATCH_MAX {
            assert_eq!(writer.offer(&event), SinkOffer::Accepted);
        }
        assert_eq!(
            writer.offer(&event),
            SinkOffer::Dropped(DropReason::QueueFull)
        );
        let stats = writer.stats();
        assert_eq!(
            stats
                .timeline_dropped_queue_full_total
                .load(Ordering::Relaxed),
            super::DIAGNOSTIC_BATCH_MAX as u64
        );
        assert_eq!(
            stats
                .timeline_dropped_persist_error_total
                .load(Ordering::Relaxed),
            0,
            "a queue-full drop must not also be counted as a persist error"
        );
    }

    /// C4: the writer worker's own late persistence failures (visible only
    /// through `DiagnosticTimelinePersistenceStats::persist_error_total`)
    /// must be folded into `timeline_dropped_persist_error_total` alongside
    /// drops already recorded at offer time (`WriterClosed`/`InvalidBatch`),
    /// never overwrite them.
    #[test]
    fn refresh_persisted_stats_folds_writer_errors_without_discarding_offer_time_drops() {
        let store = std::sync::Arc::new(FixtureTimeline::default());
        store.fail.store(true, Ordering::Relaxed);
        let persistence_stats = std::sync::Arc::new(
            atm_storage_rusqlite::DiagnosticTimelinePersistenceStats::default(),
        );
        let writer = DiagnosticTimelineWriter::new_with_persistence(
            store,
            DiagnosticPolicy::default(),
            std::sync::Arc::new(DiagnosticTimelineStats::default()),
            std::sync::Arc::clone(&persistence_stats),
        );
        let event = info_event(atm_observability::RETAINED_INFO_TARGETS[0]);

        for _ in 0..super::DIAGNOSTIC_BATCH_MAX {
            writer.offer(&event);
        }
        // The offer-time (bootstrap-owned) drop is recorded first, before any
        // writer-thread persistence stats exist.
        assert_eq!(
            writer
                .stats()
                .timeline_dropped_persist_error_total
                .load(Ordering::Relaxed),
            super::DIAGNOSTIC_BATCH_MAX as u64
        );

        // The writer thread now separately observes two of its own
        // persistence failures.
        persistence_stats.increment_persist_error_total_for_test();
        persistence_stats.increment_persist_error_total_for_test();
        writer.flush_due();

        assert_eq!(
            writer
                .stats()
                .timeline_dropped_persist_error_total
                .load(Ordering::Relaxed),
            super::DIAGNOSTIC_BATCH_MAX as u64 + 2,
            "the writer's own persist errors must add to, not replace, the offer-time drop count"
        );

        // A repeat poll before any further writer-thread failures must not
        // double count the already-folded delta.
        writer.flush_due();
        assert_eq!(
            writer
                .stats()
                .timeline_dropped_persist_error_total
                .load(Ordering::Relaxed),
            super::DIAGNOSTIC_BATCH_MAX as u64 + 2
        );
    }

    #[test]
    fn ac6_logging_contract_constants_match_the_timeline_source() {
        let logging_contract = include_str!("../../../docs/atm-daemon/logging.md");
        for (name, value) in [
            ("DIAGNOSTIC_BATCH_MAX", super::DIAGNOSTIC_BATCH_MAX),
            (
                "DIAGNOSTIC_FLUSH_INTERVAL_MS",
                super::DIAGNOSTIC_FLUSH_INTERVAL_MS as usize,
            ),
            (
                "DEGRADATION_RECOVERY_WINDOW_SECS",
                super::DEGRADATION_RECOVERY_WINDOW_SECS as usize,
            ),
            (
                "DEGRADATION_RATE_LIMIT_SECS",
                super::DEGRADATION_RATE_LIMIT_SECS as usize,
            ),
        ] {
            assert!(
                logging_contract.contains(&format!("`{name} = {value}`")),
                "docs/atm-daemon/logging.md must document {name} from diagnostic_timeline.rs"
            );
        }
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

    fn test_retained_log_policy() -> atm_observability::RetainedLogPolicy {
        atm_observability::RetainedLogPolicy {
            rotation_max_bytes: 1_048_576,
            rotation_max_files: 1,
            retention_max_age: std::time::Duration::from_secs(60),
            maintenance_cadence: std::time::Duration::from_secs(60),
            writer_shutdown_timeout: std::time::Duration::from_secs(1),
            maintenance_max_work_per_pass: None,
        }
    }

    /// C6/A7: a second `attach_timeline` call (a genuine re-attach attempt or
    /// a race between concurrent bootstrap paths) must not replace the
    /// active writer, swap the bridge's diagnostic sink, or spawn a second
    /// flush thread that would leave the live writer never flushed on
    /// cadence. This is the only test in this module that touches the
    /// process-global `INSTALLED_BRIDGE`/`ACTIVE_TIMELINE` statics.
    #[test]
    fn attach_timeline_second_call_does_not_replace_the_active_writer() {
        let log_dir = tempfile::tempdir().expect("temp log dir");
        let service_name =
            sc_observability_types::ServiceName::new("atm-test").expect("valid service name");
        let logger = std::sync::Arc::new(
            atm_observability::build_retained_logger(
                service_name,
                log_dir.path(),
                test_retained_log_policy(),
                None,
            )
            .expect("retained logger builds"),
        );
        let bridge = std::sync::Arc::new(atm_observability::TracingBridgeLayer::new(logger));
        super::register_bridge(std::sync::Arc::clone(&bridge));

        let backend_a =
            atm_storage_rusqlite::SqliteStorageBackend::new(log_dir.path().join("a.db"))
                .expect("backend a opens");
        super::attach_timeline(backend_a.diagnostic_timeline());
        let first = super::ACTIVE_TIMELINE.get().cloned().expect("attached");

        let backend_b =
            atm_storage_rusqlite::SqliteStorageBackend::new(log_dir.path().join("b.db"))
                .expect("backend b opens");
        super::attach_timeline(backend_b.diagnostic_timeline());
        let second = super::ACTIVE_TIMELINE
            .get()
            .cloned()
            .expect("still attached");

        assert!(
            std::sync::Arc::ptr_eq(&first, &second),
            "a second attach_timeline call must not replace the active writer or its flush cadence"
        );
    }
}
