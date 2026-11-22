#![forbid(unsafe_code)]
//! Backend-neutral runtime/store composition for ATM callers.

mod composition;
pub mod doctor_projection;
pub mod mailbox_runtime;
mod peer_port;
mod storage_backends;
pub mod task_telemetry;

pub use atm_storage::{
    DIAGNOSTIC_QUERY_DEFAULT_LIMIT, DIAGNOSTIC_QUERY_MAX_LIMIT, DiagnosticCursor, DiagnosticEvent,
    DiagnosticQuery, DiagnosticRecordError, DiagnosticTimelineStore, StorageFactory,
    StorageHandles,
};
pub use composition::{
    RuntimeAssembly, RuntimeAssemblyInputs, assemble_runtime, validate_enabled_peer_configuration,
    validate_enabled_peer_configuration_for_reload,
};
pub use doctor_projection::{
    DoctorProjection, DoctorProjectionConfig, DoctorProjectionContext, StorageDoctorProjection,
};
pub use mailbox_runtime::{
    AsyncMailboxRuntime, HandoffConfig, StateHandoffDiagnostics, StateHandoffSupervisor,
    SupervisorState, read_deadline,
};
pub use peer_port::PeerPortResolver;
pub use task_telemetry::{
    TASK_TELEMETRY_QUEUE_CAPACITY, TaskTelemetryDiagnostics, TaskTelemetryDiagnosticsSnapshot,
    TaskTelemetryRuntime, TaskTelemetrySetup,
};
