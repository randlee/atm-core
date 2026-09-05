//! Bounded, redacted diagnostic timeline contract shared by storage adapters.

use crate::AtmError;

/// One already-redacted diagnostic retained by the optional SQLite timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticEvent {
    pub ts_unix_ms: i64,
    pub level: String,
    pub component: String,
    pub code: Option<String>,
    pub correlation_id: Option<String>,
    pub origin: String,
    pub message: String,
    /// Compact JSON containing only the tracing bridge's allowlisted fields.
    pub detail: Option<String>,
}

/// A bounded query over the diagnostic timeline.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiagnosticQuery {
    pub since: Option<i64>,
    pub until: Option<i64>,
    pub level_at_least: Option<String>,
    pub component_prefix: Option<String>,
    pub limit: Option<usize>,
}

/// Lower-priority persistence boundary; failures here never alter mailbox work.
pub trait DiagnosticTimelineStore: Send + Sync {
    fn record_batch(&self, events: &[DiagnosticEvent]) -> Result<(), AtmError>;
    fn query(&self, query: &DiagnosticQuery) -> Result<Vec<DiagnosticEvent>, AtmError>;
    fn prune(&self, now_unix_ms: i64) -> Result<u64, AtmError>;
}
