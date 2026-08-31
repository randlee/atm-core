#![forbid(unsafe_code)]
#![allow(
    deprecated,
    reason = "Phase AC runtime composition still consumes the transitional shared storage traits while backend adoption settles"
)]
//! Backend-neutral runtime/store composition for ATM callers.

mod composition;
pub mod doctor_projection;
mod legacy_storage_adapters;
pub mod mailbox_runtime;
pub mod workflow_telemetry;

pub use atm_storage::{StorageFactory, StorageHandles};
pub use composition::{
    RuntimeAssembly, RuntimeAssemblyInputs, assemble_runtime, validate_enabled_peer_configuration,
    validate_enabled_peer_configuration_for_reload, with_installed_roster_store,
};
pub use doctor_projection::{
    DoctorProjection, DoctorProjectionConfig, DoctorProjectionContext, StorageDoctorProjection,
};
#[cfg(feature = "test-utils")]
pub use mailbox_runtime::test_utils;
pub use mailbox_runtime::{
    AsyncMailboxRuntime, HandoffConfig, StateHandoffDiagnostics, StateHandoffSupervisor,
    SupervisorState,
};
pub use workflow_telemetry::{
    WorkflowTelemetryConfig, WorkflowTelemetryDiagnostics, WorkflowTelemetryRuntime,
    WorkflowTelemetrySetup,
};
