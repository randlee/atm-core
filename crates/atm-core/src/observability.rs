use serde::Serialize;
use uuid::Uuid;

use crate::error::AtmError;
use crate::log::{LogQuery, LogQueryResult, LogRecord};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CommandEvent {
    pub command: &'static str,
    pub action: &'static str,
    pub outcome: &'static str,
    pub team: String,
    pub agent: String,
    pub sender: String,
    pub message_id: Option<Uuid>,
    pub requires_ack: bool,
    pub dry_run: bool,
    pub task_id: Option<String>,
}

pub trait LogFollowSession: Send {
    fn next_record(&mut self) -> Result<Option<LogRecord>, AtmError>;
}

pub trait ObservabilityPort {
    /// Best-effort emit path used by mail commands. Failures must not block
    /// send/read/ack/clear correctness.
    fn emit_command_event(&self, event: CommandEvent) -> Result<(), AtmError>;

    /// Explicit log-consumer query path. Unlike `emit_command_event`, failures
    /// here are surfaced to the caller because `atm log` is itself an
    /// observability consumer.
    fn query_logs(&self, query: &LogQuery) -> Result<LogQueryResult, AtmError>;

    /// Explicit follow/tail path for `atm log --follow`. The returned session
    /// owns any adapter-specific state needed to yield matching records.
    fn follow_logs(&self, query: &LogQuery) -> Result<Box<dyn LogFollowSession>, AtmError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NullObservability;

impl ObservabilityPort for NullObservability {
    fn emit_command_event(&self, _event: CommandEvent) -> Result<(), AtmError> {
        Ok(())
    }

    fn query_logs(&self, _query: &LogQuery) -> Result<LogQueryResult, AtmError> {
        Err(
            AtmError::observability_query("observability query API is unavailable").with_recovery(
                "Inject a concrete observability port that supports ATM log queries.",
            ),
        )
    }

    fn follow_logs(&self, _query: &LogQuery) -> Result<Box<dyn LogFollowSession>, AtmError> {
        Err(
            AtmError::observability_query("observability follow API is unavailable").with_recovery(
                "Inject a concrete observability port that supports ATM log follow.",
            ),
        )
    }
}
