//! Bounded, redacted diagnostic timeline contract shared by storage adapters.

use std::fmt;

use crate::AtmError;

/// Shared upper bound on `/v1/diagnostics` page size, enforced identically by
/// every layer (HTTP route admission, storage query clamp, and CLI request
/// construction) so no layer can silently disagree about the effective cap.
pub const DIAGNOSTIC_QUERY_MAX_LIMIT: usize = 1_000;

/// Default page size used when a caller omits an explicit limit.
pub const DIAGNOSTIC_QUERY_DEFAULT_LIMIT: usize = 100;

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
    /// The row's SQLite `rowid`. Callers constructing an event prior to
    /// persistence (the tracing bridge -> writer path) always set this to
    /// `0`; the storage query path is the sole source of a real value, used
    /// only to build a stable opaque pagination cursor. It is never part of
    /// the batch insert statement.
    pub id: i64,
}

/// An opaque, stable keyset-pagination position over `(ts_unix_ms, id)`.
///
/// `id` is the SQLite `rowid` of `diagnostic_events`, which is monotonically
/// increasing with insertion order. Pairing it with `ts_unix_ms` in the
/// cursor keeps pagination stable even when many rows share one millisecond
/// timestamp, which a purely time-based cursor cannot guarantee.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticCursor {
    pub ts_unix_ms: i64,
    pub id: i64,
}

impl DiagnosticCursor {
    /// Encodes this position as an opaque, URL-safe token.
    #[must_use]
    pub fn encode(self) -> String {
        let raw = format!("{}:{}", self.ts_unix_ms, self.id);
        base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, raw)
    }

    /// Decodes a previously-issued cursor token.
    ///
    /// # Errors
    /// Returns [`AtmError::config`] when `token` is not a value this module
    /// previously produced.
    pub fn decode(token: &str) -> Result<Self, AtmError> {
        let invalid = || AtmError::config("diagnostic timeline cursor is invalid");
        let raw = base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, token)
            .map_err(|_| invalid())?;
        let raw = String::from_utf8(raw).map_err(|_| invalid())?;
        let (ts_unix_ms, id) = raw.split_once(':').ok_or_else(invalid)?;
        Ok(Self {
            ts_unix_ms: ts_unix_ms.parse().map_err(|_| invalid())?,
            id: id.parse().map_err(|_| invalid())?,
        })
    }
}

/// A bounded query over the diagnostic timeline.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiagnosticQuery {
    pub since: Option<i64>,
    pub until: Option<i64>,
    pub level_at_least: Option<String>,
    pub component_prefix: Option<String>,
    pub limit: Option<usize>,
    /// When present, restricts results to rows strictly older (in the
    /// `ts_unix_ms DESC, id DESC` result order) than this cursor position,
    /// continuing a previous bounded page.
    pub cursor: Option<DiagnosticCursor>,
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

#[cfg(test)]
mod tests {
    use super::DiagnosticCursor;

    #[test]
    fn cursor_round_trips_through_its_opaque_encoding() {
        let cursor = DiagnosticCursor {
            ts_unix_ms: 1_700_000_000_123,
            id: 42,
        };
        let decoded = DiagnosticCursor::decode(&cursor.encode()).expect("decode round trip");
        assert_eq!(decoded, cursor);
    }

    #[test]
    fn cursor_decode_rejects_garbage_tokens() {
        assert!(DiagnosticCursor::decode("not-a-cursor").is_err());
        assert!(DiagnosticCursor::decode("").is_err());
    }
}
