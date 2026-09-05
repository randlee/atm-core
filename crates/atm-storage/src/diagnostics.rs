//! Bounded, redacted diagnostic timeline contract shared by storage adapters.

use std::fmt;

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

/// Why a diagnostic batch offer was not durably recorded.
///
/// Callers must match on this typed classification instead of inspecting
/// [`AtmError`] message text: every implementation of
/// [`DiagnosticTimelineStore::record_batch`] uses the same [`AtmError`] code
/// for more than one of these outcomes, so string- or code-sniffing cannot
/// reliably distinguish them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticRecordError {
    /// The lower-priority diagnostic lane is saturated; the batch was
    /// dropped without contending with mailbox durability.
    QueueFull,
    /// The persistence writer is no longer accepting diagnostic work.
    WriterClosed,
    /// The batch violated the store's batch-size contract.
    InvalidBatch,
    /// Any other persistence failure (for example a SQLite I/O error).
    PersistFailed(AtmError),
}

impl fmt::Display for DiagnosticRecordError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QueueFull => {
                write!(
                    formatter,
                    "diagnostic timeline queue is full; batch dropped"
                )
            }
            Self::WriterClosed => write!(
                formatter,
                "diagnostic timeline writer is unavailable; batch dropped"
            ),
            Self::InvalidBatch => write!(
                formatter,
                "diagnostic timeline batch violated the store's size contract"
            ),
            Self::PersistFailed(error) => {
                write!(formatter, "diagnostic timeline persistence failed: {error}")
            }
        }
    }
}

impl std::error::Error for DiagnosticRecordError {}

/// Lower-priority persistence boundary; failures here never alter mailbox work.
pub trait DiagnosticTimelineStore: Send + Sync {
    fn record_batch(&self, events: &[DiagnosticEvent]) -> Result<(), DiagnosticRecordError>;
    fn query(&self, query: &DiagnosticQuery) -> Result<Vec<DiagnosticEvent>, AtmError>;
    fn prune(&self, now_unix_ms: i64) -> Result<u64, AtmError>;
}
