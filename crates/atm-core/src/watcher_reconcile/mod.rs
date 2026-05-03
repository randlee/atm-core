use crate::types::{AgentName, TeamName};

/// Explicit reconcile triggers. Watch/file-ingest logic stays behind this
/// boundary instead of leaking into CLI, transport, or store modules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileReason {
    Startup,
    ReadRefresh,
    ExportBeforeRewrite,
    ExplicitRefresh,
}

/// Canonical reconcile request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcileRequest {
    pub team_name: TeamName,
    pub recipient_agent: Option<AgentName>,
    pub reason: ReconcileReason,
}

/// Canonical reconcile result summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcileOutcome {
    pub imported_messages: usize,
    pub skipped_records: usize,
    pub roster_updated: bool,
}

/// Typed reconcile error kept separate from transport/store errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcileError {
    pub message: String,
}

/// File-watch and reconcile boundary shared by daemon read/export flows.
pub trait WatcherReconcile: Send + Sync {
    fn reconcile(&self, request: ReconcileRequest) -> Result<ReconcileOutcome, ReconcileError>;
}
