//! In-memory task telemetry doubles for downstream handler tests.
//!
//! These implement the first-party `TaskTelemetrySink` and
//! `WorkflowTelemetrySink` boundaries only for tests (`[testing]` in
//! `boundaries/atm-core/task-telemetry-sink.toml` and
//! `workflow-telemetry-sink.toml`).

use std::sync::{Arc, Mutex};

use atm_core::{
    TaskTelemetryError, TaskTelemetryRecord, TaskTelemetrySink, WorkflowTelemetryError,
    WorkflowTelemetryRecord, WorkflowTelemetrySink,
};
use atm_runtime::{TaskTelemetryConfig, TaskTelemetrySetup};

/// Records every emitted task telemetry record, in order, and answers each
/// emit with a fixed result.
#[derive(Debug)]
pub struct RecordingTaskTelemetrySink {
    records: Mutex<Vec<TaskTelemetryRecord>>,
    result: Result<(), TaskTelemetryError>,
}

impl Default for RecordingTaskTelemetrySink {
    fn default() -> Self {
        Self::answering(Ok(()))
    }
}

impl RecordingTaskTelemetrySink {
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// A sink that records each record and then returns `result`, for tests
    /// of failure counting.
    #[must_use]
    pub fn answering(result: Result<(), TaskTelemetryError>) -> Self {
        Self {
            records: Mutex::new(Vec::new()),
            result,
        }
    }

    /// The records received so far, in emit order.
    #[must_use]
    pub fn records(&self) -> Vec<TaskTelemetryRecord> {
        self.records
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// A runtime setup with default limits that exports into `sink`.
    #[must_use]
    pub fn setup(sink: &Arc<Self>) -> TaskTelemetrySetup {
        TaskTelemetrySetup {
            config: TaskTelemetryConfig::default(),
            sink: Arc::clone(sink) as Arc<dyn TaskTelemetrySink>,
        }
    }
}

impl atm_core::boundary::sealed::Sealed for RecordingTaskTelemetrySink {}

impl TaskTelemetrySink for RecordingTaskTelemetrySink {
    fn emit(
        &self,
        record: TaskTelemetryRecord,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), TaskTelemetryError>> + Send + '_>,
    > {
        self.records
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(record);
        let result = self.result;
        Box::pin(async move { result })
    }
}

/// Never completes an emit, so the runtime's emit timeout and bounded queue
/// are what a stalled exporter leaves the producer with.
#[derive(Debug, Default)]
pub struct StalledTaskTelemetrySink;

impl atm_core::boundary::sealed::Sealed for StalledTaskTelemetrySink {}

impl TaskTelemetrySink for StalledTaskTelemetrySink {
    fn emit(
        &self,
        _record: TaskTelemetryRecord,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), TaskTelemetryError>> + Send + '_>,
    > {
        Box::pin(std::future::pending())
    }
}

/// Calls `on_emit` for each task or workflow record it receives, then never
/// completes the emit, so only a deadline can end a drain that waits on it.
pub struct ReportingStalledTelemetrySink {
    on_emit: Box<dyn Fn() + Send + Sync>,
}

impl ReportingStalledTelemetrySink {
    #[must_use]
    pub fn new(on_emit: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            on_emit: Box::new(on_emit),
        }
    }
}

impl std::fmt::Debug for ReportingStalledTelemetrySink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReportingStalledTelemetrySink")
            .finish_non_exhaustive()
    }
}

impl atm_core::boundary::sealed::Sealed for ReportingStalledTelemetrySink {}

impl TaskTelemetrySink for ReportingStalledTelemetrySink {
    fn emit(
        &self,
        _record: TaskTelemetryRecord,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), TaskTelemetryError>> + Send + '_>,
    > {
        (self.on_emit)();
        Box::pin(std::future::pending())
    }
}

impl WorkflowTelemetrySink for ReportingStalledTelemetrySink {
    fn emit(
        &self,
        _record: WorkflowTelemetryRecord,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), WorkflowTelemetryError>> + Send + '_>,
    > {
        (self.on_emit)();
        Box::pin(std::future::pending())
    }
}
