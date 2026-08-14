//! Supervised, bounded, best-effort workflow telemetry worker.

use std::sync::Arc;
use std::time::Duration;

use atm_core::{WorkflowTelemetryError, WorkflowTelemetryRecord, WorkflowTelemetrySink};
use atm_storage::AtmErrorCode;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

const DEFAULT_CAPACITY: usize = 256;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(1);
const DEFAULT_DRAIN: Duration = Duration::from_secs(2);
const MIN_DURATION: Duration = Duration::from_millis(1);
const MAX_DURATION: Duration = Duration::from_secs(30);

/// Validated worker limits. Invalid configuration is intentionally converted
/// to a disabled runtime rather than making ATM admission unavailable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowTelemetryConfig {
    pub queue_capacity: usize,
    pub emit_timeout: Duration,
    pub drain_timeout: Duration,
}

/// Bootstrap-owned exporter selection. Absence keeps telemetry inert; a
/// present but invalid configuration is retained as a doctor-visible degraded
/// state and never blocks ATM's mail runtime.
#[derive(Clone)]
pub struct WorkflowTelemetrySetup {
    pub config: WorkflowTelemetryConfig,
    pub sink: Arc<dyn WorkflowTelemetrySink>,
}

impl Default for WorkflowTelemetryConfig {
    fn default() -> Self {
        Self {
            queue_capacity: DEFAULT_CAPACITY,
            emit_timeout: DEFAULT_TIMEOUT,
            drain_timeout: DEFAULT_DRAIN,
        }
    }
}

impl WorkflowTelemetryConfig {
    pub fn validate(&self) -> Result<(), AtmErrorCode> {
        if !(1..=4096).contains(&self.queue_capacity)
            || !(MIN_DURATION..=MAX_DURATION).contains(&self.emit_timeout)
            || !(MIN_DURATION..=MAX_DURATION).contains(&self.drain_timeout)
        {
            return Err(AtmErrorCode::WorkflowTelemetryConfigInvalid);
        }
        Ok(())
    }
}

/// Structured, non-fatal telemetry counters suitable for doctor diagnostics.
#[derive(Debug, Default)]
pub struct WorkflowTelemetryDiagnostics {
    pub dropped_full: std::sync::atomic::AtomicU64,
    pub dropped_timeout: std::sync::atomic::AtomicU64,
    pub dropped_failure: std::sync::atomic::AtomicU64,
    pub dropped_shutdown: std::sync::atomic::AtomicU64,
    pub config_invalid: std::sync::atomic::AtomicBool,
}

#[derive(Clone)]
pub struct WorkflowTelemetryRuntime {
    sender: Arc<std::sync::Mutex<Option<mpsc::Sender<WorkflowTelemetryRecord>>>>,
    diagnostics: Arc<WorkflowTelemetryDiagnostics>,
    shutdown: Arc<std::sync::Mutex<Option<oneshot::Sender<()>>>>,
    worker: Arc<std::sync::Mutex<Option<JoinHandle<()>>>>,
    shutdown_timeout: Duration,
}

impl WorkflowTelemetryRuntime {
    /// Starts one worker. Invalid config returns a disabled, diagnostically
    /// degraded runtime; it never prevents mail runtime construction.
    pub fn start(config: WorkflowTelemetryConfig, sink: Arc<dyn WorkflowTelemetrySink>) -> Self {
        let diagnostics = Arc::new(WorkflowTelemetryDiagnostics::default());
        if config.validate().is_err() {
            diagnostics
                .config_invalid
                .store(true, std::sync::atomic::Ordering::Relaxed);
            return Self {
                sender: Arc::new(std::sync::Mutex::new(None)),
                diagnostics,
                shutdown: Arc::new(std::sync::Mutex::new(None)),
                worker: Arc::new(std::sync::Mutex::new(None)),
                shutdown_timeout: DEFAULT_DRAIN,
            };
        }
        // `assemble_runtime` is deliberately synchronous. Telemetry is
        // best-effort, so a caller outside Tokio must get an inert runtime
        // rather than a panic from `tokio::spawn`.
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return Self::disabled();
        };
        let (sender, mut receiver) = mpsc::channel(config.queue_capacity);
        let (shutdown_sender, mut shutdown_receiver) = oneshot::channel::<()>();
        let worker_diagnostics = Arc::clone(&diagnostics);
        let worker = runtime.spawn(async move {
            loop {
                tokio::select! {
                    message = receiver.recv() => match message {
                        Some(record) => emit_one(&*sink, record, config.emit_timeout, &worker_diagnostics).await,
                        None => break,
                    },
                    _ = &mut shutdown_receiver => {
                        let deadline = tokio::time::Instant::now() + config.drain_timeout;
                        loop {
                            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                            if remaining.is_zero() { break; }
                            match tokio::time::timeout(remaining, receiver.recv()).await {
                                Ok(Some(record)) => emit_one(&*sink, record, config.emit_timeout, &worker_diagnostics).await,
                                Ok(None) | Err(_) => break,
                            }
                        }
                        while receiver.try_recv().is_ok() { worker_diagnostics.dropped_shutdown.fetch_add(1, std::sync::atomic::Ordering::Relaxed); }
                        break;
                    }
                }
            }
        });
        Self {
            sender: Arc::new(std::sync::Mutex::new(Some(sender))),
            diagnostics,
            shutdown: Arc::new(std::sync::Mutex::new(Some(shutdown_sender))),
            worker: Arc::new(std::sync::Mutex::new(Some(worker))),
            shutdown_timeout: config.drain_timeout,
        }
    }

    /// Disabled default used when no exporter is configured.
    pub fn disabled() -> Self {
        Self {
            sender: Arc::new(std::sync::Mutex::new(None)),
            diagnostics: Arc::new(WorkflowTelemetryDiagnostics::default()),
            shutdown: Arc::new(std::sync::Mutex::new(None)),
            worker: Arc::new(std::sync::Mutex::new(None)),
            shutdown_timeout: DEFAULT_DRAIN,
        }
    }

    /// Non-blocking producer path: telemetry can never delay admission/routing.
    pub fn try_emit(&self, record: WorkflowTelemetryRecord) {
        let Some(sender) = self.sender.lock().ok().and_then(|sender| sender.clone()) else {
            return;
        };
        match sender.try_send(record) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.diagnostics
                    .dropped_full
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.diagnostics
                    .dropped_shutdown
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        };
    }

    pub fn diagnostics(&self) -> &Arc<WorkflowTelemetryDiagnostics> {
        &self.diagnostics
    }

    /// Closes intake, drains through the configured deadline, and joins the
    /// supervised worker. No exporter task is left detached after this returns.
    pub async fn shutdown(&self) {
        if let Ok(mut sender) = self.sender.lock() {
            sender.take();
        }
        if let Ok(mut sender) = self.shutdown.lock()
            && let Some(sender) = sender.take()
            && sender.send(()).is_err()
        {
            self.diagnostics
                .dropped_shutdown
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            tracing::warn!("workflow telemetry worker shutdown receiver was already closed");
        }
        let worker = self.worker.lock().ok().and_then(|mut worker| worker.take());
        if let Some(mut worker) = worker
            && tokio::time::timeout(self.shutdown_timeout, &mut worker)
                .await
                .is_err()
        {
            // The worker is supervised by this runtime. Abort only after its
            // bounded drain window expires, then join the cancellation so no
            // detached task can outlive daemon shutdown.
            self.diagnostics
                .dropped_shutdown
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            worker.abort();
            if let Err(error) = worker.await {
                tracing::warn!(%error, "workflow telemetry worker abort join failed");
            }
        }
    }
}

impl Drop for WorkflowTelemetryRuntime {
    fn drop(&mut self) {
        // A clone may be dropped while another handle is still admitting
        // records, so only the final owner performs a fail-closed abort. The
        // normal daemon path calls the async `shutdown` method and drains.
        if Arc::strong_count(&self.worker) != 1 {
            return;
        }
        if let Ok(mut sender) = self.sender.lock() {
            sender.take();
        }
        if let Ok(mut shutdown) = self.shutdown.lock()
            && let Some(shutdown) = shutdown.take()
            && shutdown.send(()).is_err()
        {
            tracing::warn!(
                "workflow telemetry worker shutdown receiver was already closed during drop"
            );
        }
        if let Ok(mut worker) = self.worker.lock()
            && let Some(worker) = worker.take()
        {
            worker.abort();
        }
    }
}

async fn emit_one(
    sink: &dyn WorkflowTelemetrySink,
    record: WorkflowTelemetryRecord,
    timeout: Duration,
    diagnostics: &WorkflowTelemetryDiagnostics,
) {
    match tokio::time::timeout(timeout, sink.emit(record)).await {
        Ok(Ok(())) => {}
        Ok(Err(WorkflowTelemetryError::TimedOut)) | Err(_) => {
            diagnostics
                .dropped_timeout
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        Ok(Err(WorkflowTelemetryError::Unavailable | WorkflowTelemetryError::Rejected)) => {
            diagnostics
                .dropped_failure
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

impl atm_core::boundary::sealed::Sealed for WorkflowTelemetryRuntime {}

impl WorkflowTelemetrySink for WorkflowTelemetryRuntime {
    fn emit(
        &self,
        record: WorkflowTelemetryRecord,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), WorkflowTelemetryError>> + Send + '_>,
    > {
        self.try_emit(record);
        Box::pin(async { Ok(()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    const DIAGNOSTIC_WAIT_TIMEOUT: Duration = Duration::from_secs(1);
    const DIAGNOSTIC_POLL_INTERVAL: Duration = Duration::from_millis(2);

    async fn wait_for_diagnostic_count(counter: &AtomicU64, expected: u64, counter_name: &str) {
        let observed = tokio::time::timeout(DIAGNOSTIC_WAIT_TIMEOUT, async {
            loop {
                let observed = counter.load(Ordering::Relaxed);
                if observed >= expected {
                    return observed;
                }
                tokio::time::sleep(DIAGNOSTIC_POLL_INTERVAL).await;
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "{counter_name} did not reach {expected} within {DIAGNOSTIC_WAIT_TIMEOUT:?}; observed {}",
                counter.load(Ordering::Relaxed)
            )
        });
        assert_eq!(
            observed, expected,
            "{counter_name} must not exceed its expected count in this single-record test"
        );
    }

    fn record() -> WorkflowTelemetryRecord {
        WorkflowTelemetryRecord {
            observation: atm_core::WorkflowTelemetryObservation::Incomplete,
            scope_kind: atm_storage::WorkflowScopeKind::new("sprint").expect("kind"),
            scope_id: atm_storage::WorkflowScopeId::new("an-11").expect("scope"),
            state: atm_storage::WorkflowState::new("opened").expect("state"),
            stage: atm_storage::WorkflowStage::new("dev").expect("stage"),
            transition: atm_storage::WorkflowTransition::new("start").expect("transition"),
            iteration: None,
            start_message_id: atm_storage::AtmMessageId::new(),
            start_timestamp: atm_storage::IsoTimestamp::now(),
            end_message_id: None,
            end_timestamp: None,
            duration_millis: None,
        }
    }

    struct FailingSink;
    impl atm_core::boundary::sealed::Sealed for FailingSink {}
    impl WorkflowTelemetrySink for FailingSink {
        fn emit(
            &self,
            _: WorkflowTelemetryRecord,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<(), WorkflowTelemetryError>> + Send + '_>,
        > {
            Box::pin(async { Err(WorkflowTelemetryError::Unavailable) })
        }
    }
    struct BlockingSink(AtomicUsize);
    impl atm_core::boundary::sealed::Sealed for BlockingSink {}
    impl WorkflowTelemetrySink for BlockingSink {
        fn emit(
            &self,
            _: WorkflowTelemetryRecord,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<(), WorkflowTelemetryError>> + Send + '_>,
        > {
            self.0.fetch_add(1, Ordering::Relaxed);
            Box::pin(async { std::future::pending::<Result<(), WorkflowTelemetryError>>().await })
        }
    }

    #[derive(Default)]
    struct RecordingSink(Mutex<Vec<WorkflowTelemetryRecord>>);

    impl atm_core::boundary::sealed::Sealed for RecordingSink {}

    impl WorkflowTelemetrySink for RecordingSink {
        fn emit(
            &self,
            record: WorkflowTelemetryRecord,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<(), WorkflowTelemetryError>> + Send + '_>,
        > {
            self.0.lock().expect("recording sink lock").push(record);
            Box::pin(async { Ok(()) })
        }
    }

    #[test]
    fn all_documented_capacity_and_deadline_boundaries_are_valid() {
        for queue_capacity in [1, WorkflowTelemetryConfig::default().queue_capacity, 4096] {
            for drain_timeout in [
                Duration::from_millis(1),
                WorkflowTelemetryConfig::default().drain_timeout,
                Duration::from_secs(30),
            ] {
                let config = WorkflowTelemetryConfig {
                    queue_capacity,
                    emit_timeout: Duration::from_millis(1),
                    drain_timeout,
                };
                assert_eq!(config.validate(), Ok(()), "{config:?}");
            }
        }
    }
    #[tokio::test]
    async fn invalid_configuration_fails_closed_to_disabled_diagnostics() {
        let runtime = WorkflowTelemetryRuntime::start(
            WorkflowTelemetryConfig {
                queue_capacity: 0,
                ..Default::default()
            },
            Arc::new(FailingSink),
        );
        assert!(runtime.diagnostics().config_invalid.load(Ordering::Relaxed));
        runtime.shutdown().await;
    }

    #[test]
    fn valid_configuration_outside_tokio_is_inert_instead_of_panicking() {
        let runtime = WorkflowTelemetryRuntime::start(
            WorkflowTelemetryConfig::default(),
            Arc::new(FailingSink),
        );
        runtime.try_emit(record());
        assert_eq!(
            runtime.diagnostics().dropped_full.load(Ordering::Relaxed),
            0
        );
    }

    #[tokio::test]
    async fn disabled_telemetry_is_inert_and_has_no_worker_side_effects() {
        let runtime = WorkflowTelemetryRuntime::disabled();
        runtime.try_emit(record());
        assert_eq!(
            runtime.diagnostics().dropped_full.load(Ordering::Relaxed),
            0,
            "disabled telemetry has no queue to fill"
        );
        assert_eq!(
            runtime
                .diagnostics()
                .dropped_failure
                .load(Ordering::Relaxed),
            0,
            "disabled telemetry does not attempt an export"
        );
        runtime.shutdown().await;
    }
    #[tokio::test]
    async fn timeout_and_failure_remain_best_effort() {
        let runtime = WorkflowTelemetryRuntime::start(
            WorkflowTelemetryConfig {
                emit_timeout: Duration::from_millis(1),
                ..Default::default()
            },
            Arc::new(BlockingSink(AtomicUsize::new(0))),
        );
        runtime.try_emit(record());
        wait_for_diagnostic_count(&runtime.diagnostics().dropped_timeout, 1, "dropped_timeout")
            .await;
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn full_queue_is_counted_without_blocking_the_producer() {
        let runtime = WorkflowTelemetryRuntime::start(
            WorkflowTelemetryConfig {
                queue_capacity: 1,
                ..Default::default()
            },
            Arc::new(BlockingSink(AtomicUsize::new(0))),
        );
        for _ in 0..32 {
            runtime.try_emit(record());
        }
        assert!(
            runtime.diagnostics().dropped_full.load(Ordering::Relaxed) > 0,
            "a bounded telemetry queue must drop rather than delay producers"
        );
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn failing_sink_isolated_as_a_diagnostic() {
        let runtime = WorkflowTelemetryRuntime::start(
            WorkflowTelemetryConfig::default(),
            Arc::new(FailingSink),
        );
        runtime.try_emit(record());
        wait_for_diagnostic_count(&runtime.diagnostics().dropped_failure, 1, "dropped_failure")
            .await;
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn configured_sink_receives_only_the_redacted_record_contract() {
        let sink = Arc::new(RecordingSink::default());
        let runtime = WorkflowTelemetryRuntime::start(
            WorkflowTelemetryConfig::default(),
            Arc::clone(&sink) as Arc<dyn WorkflowTelemetrySink>,
        );
        runtime.try_emit(record());
        for _ in 0..32 {
            if sink.0.lock().expect("recording sink lock").len() == 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        {
            let records = sink.0.lock().expect("recording sink lock");
            assert_eq!(records.len(), 1, "configured sink receives the record");
            let exported = serde_json::to_string(&records[0]).expect("redacted record JSON");
            for forbidden in ["body", "message_text", "merged_vars", "vars_json"] {
                assert!(
                    !exported.contains(forbidden),
                    "telemetry export must never contain {forbidden}"
                );
            }
        }
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn shutdown_is_bounded_when_an_exporter_is_stuck() {
        let sink = Arc::new(BlockingSink(AtomicUsize::new(0)));
        let runtime = WorkflowTelemetryRuntime::start(
            WorkflowTelemetryConfig {
                emit_timeout: Duration::from_secs(30),
                drain_timeout: Duration::from_millis(1),
                ..Default::default()
            },
            Arc::clone(&sink) as Arc<dyn WorkflowTelemetrySink>,
        );
        runtime.try_emit(record());
        for _ in 0..32 {
            if sink.0.load(Ordering::Relaxed) == 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(sink.0.load(Ordering::Relaxed), 1, "worker started emit");
        tokio::time::timeout(Duration::from_millis(100), runtime.shutdown())
            .await
            .expect("shutdown must honor the drain deadline");
        assert!(
            runtime
                .diagnostics()
                .dropped_shutdown
                .load(Ordering::Relaxed)
                > 0
        );
    }
}
