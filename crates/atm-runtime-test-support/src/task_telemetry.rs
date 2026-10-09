//! In-memory task telemetry doubles for downstream handler tests.
//!
//! These implement the first-party `TaskTelemetrySink` boundary only for
//! tests (`[testing]` in `boundaries/atm-core/task-telemetry-sink.toml`).

use std::sync::{Arc, Mutex, mpsc};

use atm_core::{TaskTelemetryError, TaskTelemetryRecord, TaskTelemetrySink};
use atm_runtime::TaskTelemetrySetup;

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

    /// A runtime setup that exports into `sink`.
    #[must_use]
    pub fn setup(sink: &Arc<Self>) -> TaskTelemetrySetup {
        TaskTelemetrySetup {
            sink: Arc::clone(sink) as Arc<dyn TaskTelemetrySink>,
        }
    }
}

impl atm_core::boundary::sealed::Sealed for RecordingTaskTelemetrySink {}

impl TaskTelemetrySink for RecordingTaskTelemetrySink {
    fn emit(&self, record: TaskTelemetryRecord) -> Result<(), TaskTelemetryError> {
        self.records
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(record);
        self.result
    }
}

/// Blocks every emit until its [`StallRelease`] is dropped, so the runtime's
/// bounded queue and shutdown deadline are what a stalled exporter leaves the
/// producer with. The sink is synchronous and holds a Tokio worker thread
/// while stalled: use it on a multi-thread runtime, and drop the release
/// before the runtime shuts down so that thread can be joined.
pub struct StalledTaskTelemetrySink {
    on_emit: Box<dyn Fn() + Send + Sync>,
    released: Mutex<mpsc::Receiver<()>>,
}

/// Unblocks every stalled emit, now and later, when dropped.
#[derive(Debug)]
pub struct StallRelease(#[allow(dead_code)] mpsc::Sender<()>);

impl StalledTaskTelemetrySink {
    #[must_use]
    pub fn new() -> (Self, StallRelease) {
        Self::reporting(|| {})
    }

    /// Calls `on_emit` as each emit starts, before it stalls.
    #[must_use]
    pub fn reporting(on_emit: impl Fn() + Send + Sync + 'static) -> (Self, StallRelease) {
        let (release, released) = mpsc::channel();
        (
            Self {
                on_emit: Box::new(on_emit),
                released: Mutex::new(released),
            },
            StallRelease(release),
        )
    }
}

impl std::fmt::Debug for StalledTaskTelemetrySink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StalledTaskTelemetrySink")
            .finish_non_exhaustive()
    }
}

impl atm_core::boundary::sealed::Sealed for StalledTaskTelemetrySink {}

impl TaskTelemetrySink for StalledTaskTelemetrySink {
    fn emit(&self, _record: TaskTelemetryRecord) -> Result<(), TaskTelemetryError> {
        (self.on_emit)();
        // Nothing is ever sent: `recv` returns once the release is dropped.
        let _ = self
            .released
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .recv();
        Err(TaskTelemetryError::Unavailable)
    }
}
