//! Process-neutral snapshots for retained-runtime diagnostic health.

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
