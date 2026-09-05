//! Allowlisted, non-blocking bridge from `tracing` into retained JSONL logs.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use atm_core::observability_counters::{DiagnosticCounters, DiagnosticCountersSource};
use sc_observability_types::{
    ActionName, CorrelationId, Level, LogEvent, ProcessIdentity, SchemaVersion, ServiceName,
    TargetCategory, Timestamp,
};
use serde_json::{Map, Value};
use tracing::field::{Field, Visit};
use tracing::{Event, Level as TracingLevel, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::{Layer, Registry};

use crate::{RetainedLogOffer, RetainedLogger};

/// Canonical retained event file shared by daemon and CLI projections.
pub const CANONICAL_LOG_FILE_NAME: &str = "atm.log.jsonl";
/// AW.4's separate graft fallback satellite file.
pub const GRAFT_FALLBACK_LOG_FILE_NAME: &str = "atm-graft-fallback.jsonl";
/// INFO targets deliberately retained in addition to every WARN and ERROR.
pub const RETAINED_INFO_TARGETS: &[&str] = &[
    "atm_daemon_bootstrap::lifecycle",
    "atm_http_runtime::listener",
    "atm_storage_rusqlite::maintenance",
];
/// The redaction boundary: only these structured keys may leave `tracing`.
pub const RETAINED_FIELD_ALLOWLIST: &[&str] = &[
    "ts",
    "level",
    "component",
    "code",
    "action",
    "correlation_id",
    "outcome",
    "elapsed_ms",
    "attempt",
    "strategy",
    "endpoint_kind",
    "failure_class",
    "refresh_error_code",
    "error_layer",
    "origin",
    "message",
    "detail",
];

/// JSON-compatible value carried to the optional AW.2 diagnostic timeline.
pub type FieldValue = Value;

/// One allowlisted, already-redacted event as the bridge saw it.
pub struct RetainedEvent<'a> {
    pub ts_unix_ms: i64,
    pub level: Level,
    pub component: &'a str,
    pub code: Option<&'a str>,
    pub correlation_id: Option<&'a str>,
    pub origin: &'a str,
    pub message: &'a str,
    pub fields: &'a [(&'static str, FieldValue)],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkOffer {
    Accepted,
    Dropped(DropReason),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropReason {
    QueueFull,
    Disabled,
    PersistFailed,
}

/// AW.2's bounded diagnostic timeline hook.
pub trait DiagnosticSink: Send + Sync {
    /// ```
    /// use std::sync::Arc;
    /// use atm_core::observability_counters::{DiagnosticCounters, DiagnosticCountersSource};
    /// use atm_observability::{DiagnosticSink, DropReason, RetainedEvent, SinkOffer};
    /// struct Timeline;
    /// impl DiagnosticSink for Timeline {
    ///     fn offer(&self, _: &RetainedEvent<'_>) -> SinkOffer {
    ///         SinkOffer::Dropped(DropReason::Disabled)
    ///     }
    /// }
    /// fn snapshot(source: &dyn DiagnosticCountersSource) -> DiagnosticCounters { source.snapshot() }
    /// let _: Arc<dyn DiagnosticSink> = Arc::new(Timeline);
    /// ```
    fn offer(&self, event: &RetainedEvent<'_>) -> SinkOffer;
}

#[derive(Debug, Default)]
pub struct TracingBridgeStats {
    pub forwarded_total: AtomicU64,
    pub dropped_queue_full_total: AtomicU64,
    pub dropped_reentrant_total: AtomicU64,
    /// Invalid IDs are not silently discarded when a tracing field cannot
    /// satisfy the shared correlation-ID contract.
    pub dropped_invalid_correlation_id_total: AtomicU64,
    pub sink_dropped_total: AtomicU64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EventOrigin {
    Tracing,
    Sqlite,
    Timeline,
    Other(String),
}

impl EventOrigin {
    fn from_field(value: Option<String>) -> Self {
        match value.as_deref() {
            None | Some("tracing") => Self::Tracing,
            Some("sqlite") => Self::Sqlite,
            Some("timeline") => Self::Timeline,
            Some(value) => Self::Other(value.to_owned()),
        }
    }

    fn as_str(&self) -> &str {
        match self {
            Self::Tracing => "tracing",
            Self::Sqlite => "sqlite",
            Self::Timeline => "timeline",
            Self::Other(value) => value,
        }
    }

    fn skips_diagnostic_sink(&self) -> bool {
        matches!(self, Self::Sqlite | Self::Timeline)
    }
}

impl DiagnosticCountersSource for TracingBridgeStats {
    fn snapshot(&self) -> DiagnosticCounters {
        DiagnosticCounters {
            jsonl_forwarded_total: self.forwarded_total.load(Ordering::Relaxed),
            jsonl_dropped_queue_full_total: self.dropped_queue_full_total.load(Ordering::Relaxed),
            jsonl_dropped_reentrant_total: self.dropped_reentrant_total.load(Ordering::Relaxed),
            ..DiagnosticCounters::default()
        }
    }
}

thread_local! { static EMITTING: Cell<bool> = const { Cell::new(false) }; }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeError {
    AlreadyInstalled,
}

/// The sole process-wide retained tracing layer installed by daemon bootstrap.
#[derive(Clone)]
pub struct TracingBridgeLayer {
    logger: Arc<RetainedLogger>,
    stats: Arc<TracingBridgeStats>,
    sink: Arc<RwLock<Option<Arc<dyn DiagnosticSink>>>>,
}

impl TracingBridgeLayer {
    pub fn new(logger: Arc<RetainedLogger>) -> Self {
        Self {
            logger,
            stats: Arc::new(TracingBridgeStats::default()),
            sink: Arc::new(RwLock::new(None)),
        }
    }

    pub fn stats(&self) -> Arc<TracingBridgeStats> {
        Arc::clone(&self.stats)
    }

    pub fn set_diagnostic_sink(&self, sink: Arc<dyn DiagnosticSink>) {
        if let Ok(mut slot) = self.sink.write() {
            *slot = Some(sink);
        }
    }

    /// Installs once as the process-global subscriber; a second subscriber is
    /// intentionally rejected rather than layered around an unknown sink.
    pub fn install(logger: Arc<RetainedLogger>) -> Result<Arc<Self>, BridgeError> {
        Self::new(logger).install_inner()
    }

    fn install_inner(self) -> Result<Arc<Self>, BridgeError> {
        let bridge = Arc::new(self);
        tracing::subscriber::set_global_default(Registry::default().with((*bridge).clone()))
            .map_err(|_| BridgeError::AlreadyInstalled)?;
        Ok(bridge)
    }

    fn emit(&self, event: &Event<'_>) {
        if !should_retain(event.metadata().level(), event.metadata().target()) {
            return;
        }
        let reentrant = EMITTING.with(|flag| {
            if flag.get() {
                true
            } else {
                flag.set(true);
                false
            }
        });
        if reentrant {
            self.stats
                .dropped_reentrant_total
                .fetch_add(1, Ordering::Relaxed);
            return;
        }
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                EMITTING.with(|flag| flag.set(false));
            }
        }
        let _reset = Reset;

        let retained = RetainedTracingEvent::from_event(event);
        self.forward_retained(retained);
    }

    fn forward_retained(&self, retained: RetainedTracingEvent) {
        let code = retained.field_string("code");
        let correlation_id = retained.field_string("correlation_id");
        let correlation_id =
            correlation_id.and_then(|value| match CorrelationId::new(value.to_owned()) {
                Ok(value) => Some(value),
                Err(_) => {
                    self.stats
                        .dropped_invalid_correlation_id_total
                        .fetch_add(1, Ordering::Relaxed);
                    None
                }
            });
        let log_event = retained.log_event(correlation_id.clone());
        match self.logger.try_log(log_event) {
            RetainedLogOffer::Accepted => {
                self.stats.forwarded_total.fetch_add(1, Ordering::Relaxed)
            }
            RetainedLogOffer::QueueFull => {
                self.stats
                    .dropped_queue_full_total
                    .fetch_add(1, Ordering::Relaxed);
                return;
            }
            RetainedLogOffer::Rejected { .. } => return,
        };
        if !retained.origin.skips_diagnostic_sink()
            && let Ok(slot) = self.sink.read()
            && let Some(sink) = slot.as_ref()
        {
            let event = RetainedEvent {
                ts_unix_ms: retained.timestamp.into_inner().unix_timestamp_nanos() as i64
                    / 1_000_000,
                level: retained.level,
                component: &retained.component,
                code,
                correlation_id: correlation_id.as_ref().map(|value| value.as_str()),
                origin: retained.origin.as_str(),
                message: &retained.message,
                fields: &retained.fields,
            };
            if matches!(sink.offer(&event), SinkOffer::Dropped(_)) {
                self.stats
                    .sink_dropped_total
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

struct RetainedTracingEvent {
    timestamp: Timestamp,
    level: Level,
    component: String,
    origin: EventOrigin,
    message: String,
    fields: Vec<(&'static str, FieldValue)>,
}

impl RetainedTracingEvent {
    fn from_event(event: &Event<'_>) -> Self {
        let mut visitor = RetainedVisitor::default();
        event.record(&mut visitor);
        let component = event.metadata().target().to_string();
        let origin = EventOrigin::from_field(visitor.take_string("origin"));
        let message = visitor.message.take().unwrap_or_default();
        let fields = visitor.into_fields(&component, origin.as_str());
        Self {
            timestamp: Timestamp::now_utc(),
            level: map_level(event.metadata().level()),
            component,
            origin,
            message,
            fields,
        }
    }
    fn field_string(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(key, _)| *key == name)
            .and_then(|(_, value)| value.as_str())
    }
    fn log_event(&self, correlation_id: Option<CorrelationId>) -> LogEvent {
        let mut json_fields = Map::new();
        for (key, value) in &self.fields {
            json_fields.insert((*key).to_string(), value.clone());
        }
        LogEvent {
            version: SchemaVersion::new(sc_observability_types::OBSERVATION_ENVELOPE_VERSION)
                .expect("literal schema version"),
            timestamp: self.timestamp,
            level: self.level,
            service: ServiceName::new("atm").expect("literal service name"),
            target: TargetCategory::new("atm.tracing").expect("literal target category"),
            action: ActionName::new("tracing.event").expect("literal action"),
            message: (!self.message.is_empty()).then_some(self.message.clone()),
            identity: ProcessIdentity::default(),
            trace: None,
            request_id: None,
            correlation_id,
            outcome: None,
            diagnostic: None,
            state_transition: None,
            fields: json_fields,
        }
    }
}

impl<S> Layer<S> for TracingBridgeLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        self.emit(event);
    }
}

fn should_retain(level: &TracingLevel, target: &str) -> bool {
    matches!(*level, TracingLevel::WARN | TracingLevel::ERROR)
        || (*level == TracingLevel::INFO
            && RETAINED_INFO_TARGETS
                .iter()
                .any(|prefix| target.starts_with(prefix)))
}
fn map_level(level: &TracingLevel) -> Level {
    match *level {
        TracingLevel::ERROR => Level::Error,
        TracingLevel::WARN => Level::Warn,
        TracingLevel::INFO => Level::Info,
        TracingLevel::DEBUG => Level::Debug,
        _ => Level::Trace,
    }
}

#[derive(Default)]
struct RetainedVisitor {
    message: Option<String>,
    fields: BTreeMap<&'static str, FieldValue>,
}
impl RetainedVisitor {
    fn keep(&mut self, field: &Field, value: FieldValue) {
        if field.name() == "message" {
            self.message = value.as_str().map(ToOwned::to_owned);
        } else if let Some(key) = RETAINED_FIELD_ALLOWLIST
            .iter()
            .copied()
            .find(|key| *key == field.name())
        {
            self.fields.insert(key, value);
        }
    }
    fn take_string(&mut self, key: &str) -> Option<String> {
        self.fields
            .remove(key)
            .and_then(|value| value.as_str().map(ToOwned::to_owned))
    }
    fn into_fields(mut self, component: &str, origin: &str) -> Vec<(&'static str, FieldValue)> {
        self.fields
            .insert("component", Value::String(component.to_string()));
        self.fields
            .insert("origin", Value::String(origin.to_string()));
        self.fields.into_iter().collect()
    }
}
impl Visit for RetainedVisitor {
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.keep(field, Value::from(value));
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.keep(field, Value::from(value));
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.keep(field, Value::from(value));
    }
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.keep(field, Value::from(value));
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        self.keep(field, Value::String(value.to_owned()));
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.keep(field, Value::String(format!("{value:?}")));
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use tempfile::TempDir;
    use tracing_subscriber::prelude::*;

    use super::{EMITTING, EventOrigin, RetainedVisitor, TracingBridgeLayer, should_retain};
    use crate::{RetainedLogPolicy, RetainedLogger, build_retained_logger};

    fn bridge() -> (TempDir, Arc<TracingBridgeLayer>) {
        let tempdir = TempDir::new().expect("tempdir");
        let logger = build_retained_logger(
            sc_observability_types::ServiceName::new("atm").expect("service"),
            &tempdir.path().join("logs"),
            RetainedLogPolicy {
                rotation_max_bytes: 1024 * 1024,
                rotation_max_files: 2,
                retention_max_age: Duration::from_secs(60),
                maintenance_cadence: Duration::from_secs(60),
                writer_shutdown_timeout: Duration::from_secs(1),
                maintenance_max_work_per_pass: Some(2),
            },
        )
        .expect("logger");
        (tempdir, Arc::new(TracingBridgeLayer::new(Arc::new(logger))))
    }

    fn with_bridge(bridge: &Arc<TracingBridgeLayer>, f: impl FnOnce()) {
        tracing::subscriber::with_default(
            tracing_subscriber::Registry::default().with((**bridge).clone()),
            f,
        );
    }

    fn lines(tempdir: &TempDir, expected: usize) -> String {
        let path = tempdir.path().join("logs/atm.log.jsonl");
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let content = fs::read_to_string(&path).unwrap_or_default();
            if content.lines().count() >= expected || Instant::now() >= deadline {
                return content;
            }
            std::thread::yield_now();
        }
    }

    #[test]
    fn ac1_warn_and_error_runtime_events_retain_contract_fields() {
        let (tempdir, bridge) = bridge();
        with_bridge(&bridge, || {
            tracing::warn!(target: "atm_http_runtime::delivery", code = "ATM_HTTP_WARN", correlation_id = "c-http", "http warning");
            tracing::error!(target: "atm_runtime::dispatch", code = "ATM_RUNTIME_ERROR", correlation_id = "c-runtime", "runtime error");
            tracing::warn!(target: "atm_storage_rusqlite::maintenance", code = "ATM_STORAGE_WARN", correlation_id = "c-storage", "storage warning");
        });
        let content = lines(&tempdir, 3);
        for expected in [
            "atm_http_runtime::delivery",
            "atm_runtime::dispatch",
            "atm_storage_rusqlite::maintenance",
            "\"origin\":\"tracing\"",
            "\"correlation_id\":\"c-http\"",
        ] {
            assert!(
                content.contains(expected),
                "missing {expected} in {content}"
            );
        }
    }

    #[test]
    fn ac2_filters_unlisted_info_and_retains_all_configured_targets() {
        let (tempdir, bridge) = bridge();
        with_bridge(&bridge, || {
            tracing::info!(target: "outside::allowlist", "excluded");
            tracing::info!(target: "atm_daemon_bootstrap::lifecycle", "daemon");
            tracing::info!(target: "atm_http_runtime::listener", "listener");
            tracing::info!(target: "atm_storage_rusqlite::maintenance", "maintenance");
        });
        let content = lines(&tempdir, 3);
        assert!(!content.contains("excluded"), "{content}");
        for target in [
            "atm_daemon_bootstrap::lifecycle",
            "atm_http_runtime::listener",
            "atm_storage_rusqlite::maintenance",
        ] {
            assert!(content.contains(target), "missing {target} in {content}");
        }
    }

    #[test]
    fn ac3_reentrancy_guard_counts_nested_event_without_recursion() {
        let (_tempdir, bridge) = bridge();
        EMITTING.with(|flag| {
            assert!(!flag.replace(true));
            with_bridge(
                &bridge,
                || tracing::warn!(target: "atm_http_runtime::listener", "nested"),
            );
            flag.set(false);
        });
        assert_eq!(
            bridge
                .stats()
                .dropped_reentrant_total
                .load(std::sync::atomic::Ordering::Relaxed),
            1
        );
    }

    #[test]
    fn ac4_queue_full_offer_is_non_blocking() {
        let (_tempdir, bridge) = bridge();
        let start = Instant::now();
        RetainedLogger::force_queue_full_for_test(|| {
            tracing::subscriber::with_default(
                tracing_subscriber::Registry::default().with((*bridge).clone()),
                || tracing::warn!(target: "atm_http_runtime::listener", "queue pressure"),
            );
        });
        assert!(start.elapsed() < Duration::from_secs(1));
        assert_eq!(
            bridge
                .stats()
                .dropped_queue_full_total
                .load(std::sync::atomic::Ordering::Relaxed),
            1
        );
    }

    #[test]
    fn ac5_removes_sensitive_fields_and_values() {
        let (tempdir, bridge) = bridge();
        with_bridge(
            &bridge,
            || tracing::warn!(target: "atm_http_runtime::delivery", body = "body-secret", recipient = "recipient-secret", token = "token-secret", env = "env-secret", code = "ATM_REDACTION", "redact"),
        );
        let content = lines(&tempdir, 1);
        for secret in [
            "body-secret",
            "recipient-secret",
            "token-secret",
            "env-secret",
        ] {
            assert!(!content.contains(secret), "secret leaked: {content}");
        }
    }

    #[test]
    fn ac7_second_global_install_is_rejected() {
        let (_tempdir, first) = bridge();
        assert!(
            TracingBridgeLayer::new(Arc::clone(&first.logger))
                .install_inner()
                .is_ok()
        );
        let (_tempdir, second) = bridge();
        assert!(matches!(
            TracingBridgeLayer::new(Arc::clone(&second.logger)).install_inner(),
            Err(super::BridgeError::AlreadyInstalled)
        ));
    }

    #[test]
    fn typed_origins_and_invalid_correlation_ids_are_explicit() {
        assert!(EventOrigin::from_field(Some("sqlite".to_string())).skips_diagnostic_sink());
        assert!(!EventOrigin::from_field(Some("tracing".to_string())).skips_diagnostic_sink());
        let (_tempdir, bridge) = bridge();
        with_bridge(
            &bridge,
            || tracing::warn!(target: "atm_http_runtime::listener", correlation_id = "", "bad correlation"),
        );
        assert_eq!(
            bridge
                .stats()
                .dropped_invalid_correlation_id_total
                .load(std::sync::atomic::Ordering::Relaxed),
            1
        );
        assert!(should_retain(&tracing::Level::WARN, "unlisted::target"));
        let _visitor = RetainedVisitor::default();
    }
}
