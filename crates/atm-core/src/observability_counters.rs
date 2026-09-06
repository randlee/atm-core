//! Process-neutral snapshots for retained-runtime diagnostic health.

use serde::{Deserialize, Serialize};

// Re-exported so every consumer of the wire-level diagnostic timeline types
// (the HTTP route, the typed client, and the CLI) shares one definition of
// the effective page-size cap instead of each layer hard-coding its own.
pub use atm_storage::{DIAGNOSTIC_QUERY_DEFAULT_LIMIT, DIAGNOSTIC_QUERY_MAX_LIMIT};

/// Copyable retained-diagnostic counter snapshot. Timeline values remain zero
/// until AW.2 installs its timeline adapter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiagnosticCounters {
    pub jsonl_forwarded_total: u64,
    pub jsonl_dropped_queue_full_total: u64,
    pub jsonl_dropped_reentrant_total: u64,
    pub timeline_written_total: u64,
    pub timeline_dropped_queue_full_total: u64,
    pub timeline_dropped_persist_error_total: u64,
}

/// Supplies a non-blocking snapshot to the runtime-health projection.
pub trait DiagnosticCountersSource: Send + Sync {
    fn snapshot(&self) -> DiagnosticCounters;
}

/// Shared read-only representation returned by `/v1/health` and projected by
/// `atm doctor`.  Its one classifier prevents endpoint-specific degradation
/// semantics from drifting.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetainedObservabilityHealth {
    pub jsonl: JsonlDiagnosticCounters,
    pub timeline: TimelineDiagnosticCounters,
    pub degraded: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct JsonlDiagnosticCounters {
    pub forwarded_total: u64,
    pub dropped_queue_full_total: u64,
    pub dropped_reentrant_total: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimelineDiagnosticCounters {
    pub written_total: u64,
    pub dropped_queue_full_total: u64,
    pub dropped_persist_error_total: u64,
}

/// The dedicated, capability-authenticated response contract for the
/// read-only health route.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetainedObservabilityHealthResponse {
    pub observability: RetainedObservabilityHealth,
}

impl From<DiagnosticCounters> for RetainedObservabilityHealth {
    fn from(counters: DiagnosticCounters) -> Self {
        let mut degraded = Vec::new();
        if counters.jsonl_dropped_queue_full_total > 0 || counters.jsonl_dropped_reentrant_total > 0
        {
            degraded.push("jsonl".to_owned());
        }
        if counters.timeline_dropped_queue_full_total > 0
            || counters.timeline_dropped_persist_error_total > 0
        {
            degraded.push("timeline".to_owned());
        }
        Self {
            jsonl: JsonlDiagnosticCounters {
                forwarded_total: counters.jsonl_forwarded_total,
                dropped_queue_full_total: counters.jsonl_dropped_queue_full_total,
                dropped_reentrant_total: counters.jsonl_dropped_reentrant_total,
            },
            timeline: TimelineDiagnosticCounters {
                written_total: counters.timeline_written_total,
                dropped_queue_full_total: counters.timeline_dropped_queue_full_total,
                dropped_persist_error_total: counters.timeline_dropped_persist_error_total,
            },
            degraded,
        }
    }
}

impl From<DiagnosticCounters> for RetainedObservabilityHealthResponse {
    fn from(counters: DiagnosticCounters) -> Self {
        Self {
            observability: counters.into(),
        }
    }
}

/// Shared public record shape for the daemon's bounded diagnostic timeline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticTimelineRecord {
    pub ts_unix_ms: i64,
    pub level: String,
    pub component: String,
    pub code: Option<String>,
    pub correlation_id: Option<String>,
    pub origin: String,
    pub message: String,
    pub detail: Option<String>,
}

/// Dedicated read-only response contract for `/v1/diagnostics`.
///
/// `truncated` and `next_cursor` are additive fields: a client that only
/// reads `records` (as `atm log` did before cursor pagination existed)
/// continues to work unchanged, and an older client deserializing a newer
/// response tolerates them via `#[serde(default)]`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticTimelineResponse {
    pub records: Vec<DiagnosticTimelineRecord>,
    /// `true` when more rows exist beyond this page; page further with
    /// `next_cursor`.
    #[serde(default)]
    pub truncated: bool,
    /// Opaque cursor identifying the last record on this page. Present
    /// whenever `truncated` is `true`.
    #[serde(default)]
    pub next_cursor: Option<String>,
}

/// Typed request parameters for the bounded `/v1/diagnostics` timeline
/// query, shared by the typed client and the HTTP route so the query-string
/// encoding lives in exactly one place.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiagnosticTimelineQuery {
    pub since_unix_ms: Option<i64>,
    pub until_unix_ms: Option<i64>,
    pub level_at_least: Option<String>,
    pub component_prefix: Option<String>,
    pub limit: Option<usize>,
    /// Opaque cursor returned as `next_cursor` by a previous page.
    pub cursor: Option<String>,
}
