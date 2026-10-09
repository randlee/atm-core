//! Observable SDK failures only; private SDK queue/drop totals are unknown.

use std::sync::Mutex;

use atm_core::observability::{
    AtmTelemetryExportFailure, AtmTelemetryExportHealth, AtmTelemetryExportState,
};
use opentelemetry_sdk::error::{OTelSdkError, OTelSdkResult};

#[derive(Default)]
struct Failure {
    // Only the class is kept: SDK error text can contain credentials or
    // collector-supplied text and is never stored or formatted into logs.
    kind: Option<AtmTelemetryExportFailure>,
}

/// Failure evidence shared by the existing tracing bridge and bootstrap.
/// Success at admission or flush does not clear an observed transport failure.
#[derive(Default)]
pub struct ExportDiagnostics {
    // MUTEX: SDK worker diagnostics and process lifecycle complete concurrently.
    failure: Mutex<Failure>,
}

impl ExportDiagnostics {
    /// Records the only four qualified mid-run export failure event names.
    /// SDK tracing fields expose no typed source, so their class is Unavailable.
    pub fn observe_sdk_event(&self, name: &str) {
        if matches!(
            name,
            "BatchSpanProcessor.Export.Error"
                | "BatchSpanProcessor.Flush.ExportError"
                | "BatchLogProcessor.Export.Error"
                | "PeriodicReader.ExportFailed"
        ) {
            self.record(AtmTelemetryExportFailure::Unavailable);
        }
    }

    /// Records an explicit standard-provider lifecycle result separately from
    /// mid-run diagnostics. SDK strings are never used for classification.
    pub fn observe_result(&self, result: OTelSdkResult) {
        if let Err(source) = result {
            let kind = match source {
                OTelSdkError::Timeout(_) => AtmTelemetryExportFailure::TimedOut,
                OTelSdkError::AlreadyShutdown | OTelSdkError::InternalFailure(_) => {
                    AtmTelemetryExportFailure::Unavailable
                }
            };
            self.record(kind);
        }
    }

    /// A caller exceeded its wait deadline. This does not abort a blocking SDK
    /// shutdown call or prove that the collector received pending records.
    pub fn shutdown_wait_timed_out(&self) {
        self.record(AtmTelemetryExportFailure::ShutdownTimedOut);
    }

    pub(crate) fn setup_failed(&self) {
        self.record(AtmTelemetryExportFailure::ConfigInvalid);
    }

    fn record(&self, kind: AtmTelemetryExportFailure) {
        if let Ok(mut failure) = self.failure.lock()
            && failure.kind != Some(AtmTelemetryExportFailure::ShutdownTimedOut)
        {
            failure.kind = Some(kind);
        }
    }

    /// Adds observable failure evidence to runtime-owned counts. It never
    /// invents loss counts for the SDK's private queues.
    pub fn project(&self, health: &mut AtmTelemetryExportHealth) {
        let kind = self
            .failure
            .lock()
            .map_or(Some(AtmTelemetryExportFailure::Unavailable), |failure| {
                failure.kind
            });
        if let Some(kind) = kind {
            health.last_failure = Some(kind);
            health.state = AtmTelemetryExportState::Unavailable;
        }
    }
}
