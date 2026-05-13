use atm_core::error::AtmError;
use atm_core::error_codes::AtmErrorCode;
use atm_core::observability::ObservabilityPort;

/// Daemon-runtime observability operations that stay daemon-specific above the
/// shared ATM observability boundary.
pub trait DaemonRuntimeObservability: ObservabilityPort + Send + Sync {
    /// Emit one daemon lifecycle/runtime event into the retained sink.
    fn emit_runtime_event(
        &self,
        action: &'static str,
        outcome: &'static str,
        message: &'static str,
    ) -> Result<(), AtmError>;

    /// Emit one generic subsystem event into the retained sink without
    /// introducing daemon-subsystem type dependencies at the observability
    /// boundary.
    fn emit_subsystem_event(
        &self,
        subsystem: &'static str,
        action: &'static str,
        outcome: &'static str,
        message: &str,
        error_code: Option<AtmErrorCode>,
    ) -> Result<(), AtmError>;

    /// Attempt one best-effort synchronous flush during daemon shutdown.
    fn best_effort_flush_blocking(&self) -> Result<(), AtmError>;
}
