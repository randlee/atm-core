//! First-party, best-effort telemetry seam for workflow lifecycle facts.
//!
//! The contract deliberately excludes message payloads and merged variables.
//! It is sealed because ATM runtime composition, not third-party plug-ins,
//! owns exporter selection and failure isolation.

use std::future::Future;
use std::pin::Pin;

use atm_storage::{
    AtmMessageId, IsoTimestamp, WorkflowIteration, WorkflowScopeId, WorkflowScopeKind,
    WorkflowStage, WorkflowState, WorkflowTransition,
};
use serde::{Deserialize, Serialize};

/// One redacted lifecycle projection suitable for a telemetry exporter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowTelemetryRecord {
    pub observation: WorkflowTelemetryObservation,
    pub scope_kind: WorkflowScopeKind,
    pub scope_id: WorkflowScopeId,
    pub state: WorkflowState,
    pub stage: WorkflowStage,
    pub transition: WorkflowTransition,
    pub iteration: Option<WorkflowIteration>,
    pub start_message_id: AtmMessageId,
    pub start_timestamp: IsoTimestamp,
    pub end_message_id: Option<AtmMessageId>,
    pub end_timestamp: Option<IsoTimestamp>,
    pub duration_millis: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkflowTelemetryObservation {
    Completed,
    Incomplete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkflowTelemetryError {
    Unavailable,
    Rejected,
    TimedOut,
}

/// BOUNDARY-WorkflowTelemetrySink — an object-safe, first-party-only sink.
pub trait WorkflowTelemetrySink: crate::boundary::sealed::Sealed + Send + Sync {
    fn emit(
        &self,
        record: WorkflowTelemetryRecord,
    ) -> Pin<Box<dyn Future<Output = Result<(), WorkflowTelemetryError>> + Send + '_>>;
}

/// Inert built-in default used when no valid runtime exporter is configured.
#[derive(Debug, Default)]
pub struct NoopWorkflowTelemetrySink;

impl crate::boundary::sealed::Sealed for NoopWorkflowTelemetrySink {}

impl WorkflowTelemetrySink for NoopWorkflowTelemetrySink {
    fn emit(
        &self,
        _record: WorkflowTelemetryRecord,
    ) -> Pin<Box<dyn Future<Output = Result<(), WorkflowTelemetryError>> + Send + '_>> {
        Box::pin(async { Ok(()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn boundary_is_object_safe() {
        fn accepts_dyn(_: &dyn WorkflowTelemetrySink) {}
        let sink: Arc<dyn WorkflowTelemetrySink> = Arc::new(NoopWorkflowTelemetrySink);
        accepts_dyn(&*sink);
    }
}
