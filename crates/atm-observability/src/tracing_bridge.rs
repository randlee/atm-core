//! Allowlisted, non-blocking bridge from `tracing` into retained JSONL logs.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use atm_core::observability_counters::{DiagnosticCounters, DiagnosticCountersSource};
use sc_observability::Logger;
use sc_observability_types::{
    ActionName, CorrelationId, Level, LogEvent, ProcessIdentity, SchemaVersion, ServiceName,
    TargetCategory, Timestamp,
};
use serde_json::{Map, Value};
use tracing::field::{Field, Visit};
use tracing::{Event, Level as TracingLevel, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::{Layer, Registry};

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
    fn offer(&self, event: &RetainedEvent<'_>) -> SinkOffer;
}

#[derive(Debug, Default)]
pub struct TracingBridgeStats {
    pub forwarded_total: AtomicU64,
    pub dropped_queue_full_total: AtomicU64,
    pub dropped_reentrant_total: AtomicU64,
    pub sink_dropped_total: AtomicU64,
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
    logger: Arc<Logger>,
    stats: Arc<TracingBridgeStats>,
    sink: Arc<RwLock<Option<Arc<dyn DiagnosticSink>>>>,
}

impl TracingBridgeLayer {
    pub fn new(logger: Arc<Logger>) -> Self {
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
    pub fn install(logger: Arc<Logger>) -> Result<Arc<Self>, BridgeError> {
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

        let mut visitor = RetainedVisitor::default();
        event.record(&mut visitor);
        let component = event.metadata().target().to_string();
        let origin = visitor
            .take_string("origin")
            .unwrap_or_else(|| "tracing".to_string());
        let message = visitor.message.take().unwrap_or_default();
        let fields = visitor.into_fields(&component, &origin);
        let code = fields
            .iter()
            .find(|(key, _)| *key == "code")
            .and_then(|(_, value)| value.as_str());
        let correlation_id = fields
            .iter()
            .find(|(key, _)| *key == "correlation_id")
            .and_then(|(_, value)| value.as_str());
        let level = map_level(event.metadata().level());
        let mut json_fields = Map::new();
        for (key, value) in &fields {
            json_fields.insert((*key).to_string(), value.clone());
        }
        let timestamp = Timestamp::now_utc();
        let log_event = LogEvent {
            version: SchemaVersion::new(sc_observability_types::OBSERVATION_ENVELOPE_VERSION)
                .expect("literal schema version"),
            timestamp,
            level,
            service: ServiceName::new("atm").expect("literal service name"),
            target: TargetCategory::new("atm.tracing").expect("literal target category"),
            action: ActionName::new("tracing.event").expect("literal action"),
            message: (!message.is_empty()).then_some(message.clone()),
            identity: ProcessIdentity::default(),
            trace: None,
            request_id: None,
            correlation_id: correlation_id
                .and_then(|value| CorrelationId::new(value.to_owned()).ok()),
            outcome: None,
            diagnostic: None,
            state_transition: None,
            fields: json_fields,
        };
        match self.logger.try_log(log_event) {
            Ok(()) => self.stats.forwarded_total.fetch_add(1, Ordering::Relaxed),
            Err(sc_observability::TryLogError::QueueFull(_)) => {
                self.stats
                    .dropped_queue_full_total
                    .fetch_add(1, Ordering::Relaxed);
                return;
            }
            Err(_) => return,
        };
        if origin != "sqlite" && origin != "timeline" {
            if let Ok(slot) = self.sink.read() {
                if let Some(sink) = slot.as_ref() {
                    let retained = RetainedEvent {
                        ts_unix_ms: timestamp.into_inner().unix_timestamp_nanos() as i64
                            / 1_000_000,
                        level,
                        component: &component,
                        code,
                        correlation_id,
                        origin: &origin,
                        message: &message,
                        fields: &fields,
                    };
                    if matches!(sink.offer(&retained), SinkOffer::Dropped(_)) {
                        self.stats
                            .sink_dropped_total
                            .fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
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
