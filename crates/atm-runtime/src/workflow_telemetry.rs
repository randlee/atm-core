//! Supervised, bounded, best-effort workflow telemetry worker.

use std::sync::Arc;
use std::time::Duration;

use atm_core::{WorkflowTelemetryError, WorkflowTelemetryRecord, WorkflowTelemetrySink};
use atm_storage::AtmErrorCode;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::telemetry_limits::{DEFAULT_CAPACITY, DEFAULT_DRAIN, DEFAULT_TIMEOUT, within_limits};

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
        if !within_limits(self.queue_capacity, self.emit_timeout, self.drain_timeout) {
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

/// Worker ownership. The join handle stays here until its join completes, so
/// a cancelled `shutdown` call never detaches the worker.
#[derive(Default)]
struct Lifecycle {
    stop: Option<oneshot::Sender<Instant>>,
    worker: Option<JoinHandle<()>>,
    drain_deadline: Option<Instant>,
}

/// Runtime producer handle for best-effort workflow telemetry.
///
/// Producers use [`WorkflowTelemetryRuntime::try_emit`]; this handle
/// intentionally does not implement [`WorkflowTelemetrySink`], whose
/// implementation belongs to the exporter provided when the runtime starts.
///
/// ```compile_fail
/// use atm_core::WorkflowTelemetrySink;
/// use atm_runtime::WorkflowTelemetryRuntime;
///
/// fn runtime_is_not_a_sink(runtime: WorkflowTelemetryRuntime) -> Box<dyn WorkflowTelemetrySink> {
///     Box::new(runtime)
/// }
/// ```
#[derive(Clone)]
pub struct WorkflowTelemetryRuntime {
    sender: Arc<std::sync::Mutex<Option<mpsc::Sender<WorkflowTelemetryRecord>>>>,
    diagnostics: Arc<WorkflowTelemetryDiagnostics>,
    lifecycle: Arc<tokio::sync::Mutex<Lifecycle>>,
    drain_timeout: Duration,
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
                lifecycle: Arc::new(tokio::sync::Mutex::new(Lifecycle::default())),
                drain_timeout: DEFAULT_DRAIN,
            };
        }
        // `assemble_runtime` is deliberately synchronous. Telemetry is
        // best-effort, so a caller outside Tokio must get an inert runtime
        // rather than a panic from `tokio::spawn`.
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return Self::disabled();
        };
        let (sender, mut receiver) = mpsc::channel(config.queue_capacity);
        let (shutdown_sender, mut shutdown_receiver) = oneshot::channel::<Instant>();
        let worker_diagnostics = Arc::clone(&diagnostics);
        let worker = runtime.spawn(async move {
            loop {
                tokio::select! {
                    message = receiver.recv() => match message {
                        Some(record) => emit_one(&*sink, record, config.emit_timeout, &worker_diagnostics).await,
                        None => break,
                    },
                    stop = &mut shutdown_receiver => {
                        receiver.close();
                        let deadline = stop.unwrap_or_else(|_| Instant::now() + config.drain_timeout);
                        while let Ok(Some(record)) = tokio::time::timeout_at(deadline, receiver.recv()).await {
                            emit_one(&*sink, record, config.emit_timeout, &worker_diagnostics).await;
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
            lifecycle: Arc::new(tokio::sync::Mutex::new(Lifecycle {
                stop: Some(shutdown_sender),
                worker: Some(worker),
                drain_deadline: None,
            })),
            drain_timeout: config.drain_timeout,
        }
    }

    /// Disabled default used when no exporter is configured.
    pub fn disabled() -> Self {
        Self {
            sender: Arc::new(std::sync::Mutex::new(None)),
            diagnostics: Arc::new(WorkflowTelemetryDiagnostics::default()),
            lifecycle: Arc::new(tokio::sync::Mutex::new(Lifecycle::default())),
            drain_timeout: DEFAULT_DRAIN,
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

    /// Closes intake, drains until `min(now + drain_timeout, deadline)`, then
    /// aborts and joins the supervised worker. The first call fixes the drain
    /// deadline; a repeated call, or one that follows a cancelled call, joins
    /// the same worker against that deadline. No exporter task is left
    /// detached after this returns.
    pub async fn shutdown(&self, deadline: Instant) {
        if let Ok(mut sender) = self.sender.lock() {
            sender.take();
        }
        let mut lifecycle = self.lifecycle.lock().await;
        let drain_deadline = *lifecycle
            .drain_deadline
            .get_or_insert_with(|| deadline.min(Instant::now() + self.drain_timeout));
        if let Some(stop) = lifecycle.stop.take()
            && stop.send(drain_deadline).is_err()
        {
            self.diagnostics
                .dropped_shutdown
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            tracing::warn!("workflow telemetry worker shutdown receiver was already closed");
        }
        let Some(worker) = lifecycle.worker.as_mut() else {
            return;
        };
        if tokio::time::timeout_at(drain_deadline.min(deadline), &mut *worker)
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
            if let Err(error) = (&mut *worker).await
                && !error.is_cancelled()
            {
                tracing::warn!(%error, "workflow telemetry worker abort join failed");
            }
        }
        lifecycle.worker = None;
    }
}

impl Drop for WorkflowTelemetryRuntime {
    fn drop(&mut self) {
        // A clone may be dropped while another handle is still admitting
        // records, so only the final owner performs a fail-closed abort. The
        // normal daemon path calls the async `shutdown` method and drains.
        if Arc::strong_count(&self.lifecycle) != 1 {
            return;
        }
        if let Ok(mut sender) = self.sender.lock() {
            sender.take();
        }
        if let Ok(mut lifecycle) = self.lifecycle.try_lock()
            && let Some(worker) = lifecycle.worker.take()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry_limits::MAX_DURATION;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    const DIAGNOSTIC_WAIT_TIMEOUT: Duration = Duration::from_secs(1);
    const WAIT: Duration = Duration::from_secs(5);

    async fn wait_for_diagnostic_count(counter: &AtomicU64, expected: u64, counter_name: &str) {
        let observed = tokio::time::timeout(DIAGNOSTIC_WAIT_TIMEOUT, async {
            loop {
                let observed = counter.load(Ordering::Relaxed);
                if observed >= expected {
                    return observed;
                }
                tokio::task::yield_now().await;
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
        runtime.shutdown(Instant::now() + WAIT).await;
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
        runtime.shutdown(Instant::now() + WAIT).await;
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
        runtime.shutdown(Instant::now() + WAIT).await;
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
        runtime.shutdown(Instant::now() + WAIT).await;
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
        runtime.shutdown(Instant::now() + WAIT).await;
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
        runtime.shutdown(Instant::now() + WAIT).await;
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
        tokio::time::timeout(
            Duration::from_millis(100),
            runtime.shutdown(Instant::now() + WAIT),
        )
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

    fn has_worker(runtime: &WorkflowTelemetryRuntime) -> bool {
        runtime
            .lifecycle
            .try_lock()
            .expect("no shutdown in progress")
            .worker
            .is_some()
    }

    fn stuck_runtime(sink: &Arc<BlockingSink>) -> WorkflowTelemetryRuntime {
        WorkflowTelemetryRuntime::start(
            WorkflowTelemetryConfig {
                emit_timeout: MAX_DURATION,
                drain_timeout: MAX_DURATION,
                ..Default::default()
            },
            Arc::clone(sink) as Arc<dyn WorkflowTelemetrySink>,
        )
    }

    async fn wait_started(sink: &BlockingSink) {
        tokio::time::timeout(WAIT, async {
            while sink.0.load(Ordering::Relaxed) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("worker started emit");
    }

    #[tokio::test]
    async fn caller_deadline_bounds_shutdown_below_the_drain_timeout() {
        let sink = Arc::new(BlockingSink(AtomicUsize::new(0)));
        let runtime = stuck_runtime(&sink);
        runtime.try_emit(record());
        wait_started(&sink).await;
        tokio::time::timeout(
            WAIT,
            runtime.shutdown(Instant::now() + Duration::from_millis(20)),
        )
        .await
        .expect("shutdown obeys the caller deadline, not the 30 s drain");
        assert!(!has_worker(&runtime));
        assert_eq!(
            Arc::strong_count(&sink),
            1,
            "aborted worker released the sink"
        );
    }

    #[tokio::test]
    async fn cancelled_shutdown_leaves_the_worker_for_the_next_call() {
        let sink = Arc::new(BlockingSink(AtomicUsize::new(0)));
        let runtime = stuck_runtime(&sink);
        runtime.try_emit(record());
        wait_started(&sink).await;
        let cancelled = tokio::time::timeout(
            Duration::from_millis(5),
            runtime.shutdown(Instant::now() + WAIT),
        )
        .await;
        assert!(
            cancelled.is_err(),
            "first shutdown is cancelled by its caller"
        );
        assert!(
            has_worker(&runtime),
            "cancellation must not detach the worker"
        );
        assert_eq!(
            Arc::strong_count(&sink),
            2,
            "worker still owns its sink clone"
        );

        runtime
            .shutdown(Instant::now() + Duration::from_millis(5))
            .await;
        assert!(!has_worker(&runtime));
        assert_eq!(Arc::strong_count(&sink), 1, "worker joined, not detached");
        let terminal = runtime
            .diagnostics()
            .dropped_shutdown
            .load(Ordering::Relaxed);
        assert_eq!(terminal, 1);

        runtime.shutdown(Instant::now() + WAIT).await;
        assert_eq!(
            runtime
                .diagnostics()
                .dropped_shutdown
                .load(Ordering::Relaxed),
            terminal,
            "repeated shutdown keeps the terminal result"
        );
    }
}
