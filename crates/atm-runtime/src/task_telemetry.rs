//! Supervised, bounded, best-effort task telemetry worker.
//!
//! The runtime is the sole holder and caller of the [`TaskTelemetrySink`]
//! trait object (ADR-064). Producers call [`TaskTelemetryRuntime::try_emit`],
//! which never awaits; one worker forwards records to the sink.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use atm_core::{TaskTelemetryError, TaskTelemetryRecord, TaskTelemetrySink};
use atm_storage::AtmErrorCode;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::telemetry_limits::{DEFAULT_CAPACITY, DEFAULT_DRAIN, DEFAULT_TIMEOUT, within_limits};

/// Validated worker limits. Invalid configuration is converted to a disabled
/// runtime rather than making ATM task processing unavailable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskTelemetryConfig {
    pub queue_capacity: usize,
    pub emit_timeout: Duration,
    pub drain_timeout: Duration,
}

impl Default for TaskTelemetryConfig {
    fn default() -> Self {
        Self {
            queue_capacity: DEFAULT_CAPACITY,
            emit_timeout: DEFAULT_TIMEOUT,
            drain_timeout: DEFAULT_DRAIN,
        }
    }
}

impl TaskTelemetryConfig {
    /// Accepts a queue of 1..=4096 records and timeouts of 1 ms..=30 s.
    ///
    /// # Errors
    ///
    /// Returns [`AtmErrorCode::TelemetryExportConfigInvalid`] for any value
    /// outside those bounds.
    pub fn validate(&self) -> Result<(), AtmErrorCode> {
        if !within_limits(self.queue_capacity, self.emit_timeout, self.drain_timeout) {
            return Err(AtmErrorCode::TelemetryExportConfigInvalid);
        }
        Ok(())
    }
}

/// Bootstrap-owned exporter selection. Absence keeps telemetry inert.
#[derive(Clone)]
pub struct TaskTelemetrySetup {
    pub config: TaskTelemetryConfig,
    pub sink: Arc<dyn TaskTelemetrySink>,
}

/// Concurrent, non-fatal counters for doctor health composition.
#[derive(Debug, Default)]
pub struct TaskTelemetryDiagnostics {
    emitted: AtomicU64,
    dropped_full: AtomicU64,
    dropped_timeout: AtomicU64,
    dropped_failure: AtomicU64,
    dropped_shutdown: AtomicU64,
    config_invalid: AtomicBool,
    /// Admitted records not yet resolved by the worker; drained into
    /// `dropped_shutdown` if the worker is aborted.
    pending: AtomicU64,
    /// Set once a started runtime stops admission, so later producers are
    /// counted as shutdown drops rather than ignored.
    closed: AtomicBool,
}

/// A point-in-time copy of [`TaskTelemetryDiagnostics`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TaskTelemetryDiagnosticsSnapshot {
    pub emitted: u64,
    pub dropped_full: u64,
    pub dropped_timeout: u64,
    pub dropped_failure: u64,
    pub dropped_shutdown: u64,
    pub config_invalid: bool,
}

impl TaskTelemetryDiagnostics {
    #[must_use]
    pub fn snapshot(&self) -> TaskTelemetryDiagnosticsSnapshot {
        TaskTelemetryDiagnosticsSnapshot {
            emitted: self.emitted.load(Ordering::Relaxed),
            dropped_full: self.dropped_full.load(Ordering::Relaxed),
            dropped_timeout: self.dropped_timeout.load(Ordering::Relaxed),
            dropped_failure: self.dropped_failure.load(Ordering::Relaxed),
            dropped_shutdown: self.dropped_shutdown.load(Ordering::Relaxed),
            config_invalid: self.config_invalid.load(Ordering::Relaxed),
        }
    }

    fn resolve(&self, counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
        self.pending.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Shutdown signal and worker handle. Held under an async mutex across the
/// join so a cancelled `shutdown` leaves the handle in place for the next
/// caller instead of detaching the worker.
#[derive(Default)]
struct Lifecycle {
    stop: Option<oneshot::Sender<Instant>>,
    worker: Option<JoinHandle<()>>,
    drain_deadline: Option<Instant>,
}

#[derive(Clone)]
pub struct TaskTelemetryRuntime {
    sender: Arc<std::sync::Mutex<Option<mpsc::Sender<TaskTelemetryRecord>>>>,
    diagnostics: Arc<TaskTelemetryDiagnostics>,
    lifecycle: Arc<tokio::sync::Mutex<Lifecycle>>,
    drain_timeout: Duration,
}

impl TaskTelemetryRuntime {
    /// Starts one worker. Invalid config returns a disabled runtime with
    /// `config_invalid` set; it never prevents runtime construction.
    pub fn start(config: TaskTelemetryConfig, sink: Arc<dyn TaskTelemetrySink>) -> Self {
        if config.validate().is_err() {
            let runtime = Self::disabled();
            runtime
                .diagnostics
                .config_invalid
                .store(true, Ordering::Relaxed);
            return runtime;
        }
        // `assemble_runtime` is synchronous. Telemetry is best-effort, so a
        // caller outside Tokio gets an inert runtime rather than a panic.
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return Self::disabled();
        };
        let diagnostics = Arc::new(TaskTelemetryDiagnostics::default());
        let (sender, receiver) = mpsc::channel(config.queue_capacity);
        let (stop, stop_receiver) = oneshot::channel();
        let worker = handle.spawn(run_worker(
            sink,
            receiver,
            stop_receiver,
            config.emit_timeout,
            Arc::clone(&diagnostics),
        ));
        Self {
            sender: Arc::new(std::sync::Mutex::new(Some(sender))),
            diagnostics,
            lifecycle: Arc::new(tokio::sync::Mutex::new(Lifecycle {
                stop: Some(stop),
                worker: Some(worker),
                drain_deadline: None,
            })),
            drain_timeout: config.drain_timeout,
        }
    }

    /// Inert default used when no exporter is configured: no queue, no worker.
    pub fn disabled() -> Self {
        Self {
            sender: Arc::new(std::sync::Mutex::new(None)),
            diagnostics: Arc::new(TaskTelemetryDiagnostics::default()),
            lifecycle: Arc::new(tokio::sync::Mutex::new(Lifecycle::default())),
            drain_timeout: DEFAULT_DRAIN,
        }
    }

    /// Non-blocking producer path: telemetry never delays task processing.
    pub fn try_emit(&self, record: TaskTelemetryRecord) {
        let Some(sender) = self.sender.lock().ok().and_then(|sender| sender.clone()) else {
            if self.diagnostics.closed.load(Ordering::Relaxed) {
                self.diagnostics
                    .dropped_shutdown
                    .fetch_add(1, Ordering::Relaxed);
            }
            return;
        };
        self.diagnostics.pending.fetch_add(1, Ordering::Relaxed);
        match sender.try_send(record) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.diagnostics.resolve(&self.diagnostics.dropped_full);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.diagnostics.resolve(&self.diagnostics.dropped_shutdown);
            }
        }
    }

    #[must_use]
    pub fn diagnostics(&self) -> Arc<TaskTelemetryDiagnostics> {
        Arc::clone(&self.diagnostics)
    }

    /// Stops admission, drains until `min(drain_timeout, deadline)`, then
    /// aborts and joins the worker. Repeated calls are no-ops once the worker
    /// is joined; a cancelled call leaves the worker for the next caller.
    pub async fn shutdown(&self, deadline: Instant) {
        if let Ok(mut sender) = self.sender.lock()
            && sender.take().is_some()
        {
            self.diagnostics.closed.store(true, Ordering::Relaxed);
        }
        let mut lifecycle = self.lifecycle.lock().await;
        let drain_deadline = *lifecycle
            .drain_deadline
            .get_or_insert_with(|| deadline.min(Instant::now() + self.drain_timeout));
        if let Some(stop) = lifecycle.stop.take()
            && stop.send(drain_deadline).is_err()
        {
            // The worker has exited already; the join below observes that.
            tracing::debug!("task telemetry worker stopped before shutdown signal");
        }
        let wait_until = drain_deadline.min(deadline);
        let Some(worker) = lifecycle.worker.as_mut() else {
            return;
        };
        if tokio::time::timeout_at(wait_until, &mut *worker)
            .await
            .is_err()
        {
            worker.abort();
            if let Err(error) = (&mut *worker).await
                && error.is_panic()
            {
                tracing::warn!(%error, "task telemetry worker panicked during abort");
            }
            let abandoned = self.diagnostics.pending.swap(0, Ordering::Relaxed);
            self.diagnostics
                .dropped_shutdown
                .fetch_add(abandoned, Ordering::Relaxed);
        }
        lifecycle.worker = None;
    }
}

impl Drop for TaskTelemetryRuntime {
    fn drop(&mut self) {
        // Only the final owner aborts; the daemon path calls `shutdown` first.
        if Arc::strong_count(&self.lifecycle) != 1 {
            return;
        }
        if let Ok(mut lifecycle) = self.lifecycle.try_lock()
            && let Some(worker) = lifecycle.worker.take()
        {
            worker.abort();
        }
    }
}

async fn run_worker(
    sink: Arc<dyn TaskTelemetrySink>,
    mut receiver: mpsc::Receiver<TaskTelemetryRecord>,
    mut stop: oneshot::Receiver<Instant>,
    emit_timeout: Duration,
    diagnostics: Arc<TaskTelemetryDiagnostics>,
) {
    let drain_deadline = loop {
        tokio::select! {
            message = receiver.recv() => match message {
                Some(record) => emit_one(&*sink, record, emit_timeout, &diagnostics).await,
                None => return,
            },
            deadline = &mut stop => match deadline {
                Ok(deadline) => break deadline,
                // Every runtime handle is gone without a shutdown call.
                Err(_) => return,
            },
        }
    };
    receiver.close();
    while let Ok(Some(record)) = tokio::time::timeout_at(drain_deadline, receiver.recv()).await {
        emit_one(&*sink, record, emit_timeout, &diagnostics).await;
    }
    while receiver.try_recv().is_ok() {
        diagnostics.resolve(&diagnostics.dropped_shutdown);
    }
}

async fn emit_one(
    sink: &dyn TaskTelemetrySink,
    record: TaskTelemetryRecord,
    timeout: Duration,
    diagnostics: &TaskTelemetryDiagnostics,
) {
    let counter = match tokio::time::timeout(timeout, sink.emit(record)).await {
        Ok(Ok(())) => &diagnostics.emitted,
        Ok(Err(TaskTelemetryError::TimedOut)) | Err(_) => &diagnostics.dropped_timeout,
        Ok(Err(TaskTelemetryError::Unavailable | TaskTelemetryError::Rejected)) => {
            &diagnostics.dropped_failure
        }
    };
    diagnostics.resolve(counter);
}

impl atm_core::boundary::sealed::Sealed for TaskTelemetryRuntime {}

impl TaskTelemetrySink for TaskTelemetryRuntime {
    fn emit(
        &self,
        record: TaskTelemetryRecord,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), TaskTelemetryError>> + Send + '_>,
    > {
        self.try_emit(record);
        Box::pin(async { Ok(()) })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::telemetry_limits::{MAX_DURATION, MIN_DURATION};
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;

    use atm_core::TaskTelemetryKind;
    use atm_storage::TaskActor;
    use tokio::sync::Semaphore;

    const WAIT: Duration = Duration::from_secs(5);

    pub(crate) fn record(kind: TaskTelemetryKind) -> TaskTelemetryRecord {
        TaskTelemetryRecord {
            kind,
            team: "test-team".parse().expect("team"),
            task_id: "atm-bd-7".parse().expect("task id"),
            assignee: "fenix".parse().expect("assignee"),
            actor: TaskActor::Member("solar".parse().expect("actor")),
            seq: Some(7),
            at: "2026-10-08T03:00:00Z".parse().expect("timestamp"),
            from_state: None,
            to_state: None,
            close_outcome: None,
            message_id: None,
            reminder_outcome: None,
            marker: None,
            handoff: None,
        }
    }

    type EmitFuture<'a> = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), TaskTelemetryError>> + Send + 'a>,
    >;

    /// Waits on `gate` before recording each record, so tests control when
    /// the worker makes progress without sleeping.
    struct GatedSink {
        gate: Semaphore,
        started: AtomicUsize,
        records: Mutex<Vec<TaskTelemetryRecord>>,
        result: Result<(), TaskTelemetryError>,
    }

    impl GatedSink {
        fn open(result: Result<(), TaskTelemetryError>) -> Arc<Self> {
            Arc::new(Self {
                gate: Semaphore::new(Semaphore::MAX_PERMITS),
                started: AtomicUsize::new(0),
                records: Mutex::new(Vec::new()),
                result,
            })
        }

        fn closed() -> Arc<Self> {
            Arc::new(Self {
                gate: Semaphore::new(0),
                started: AtomicUsize::new(0),
                records: Mutex::new(Vec::new()),
                result: Ok(()),
            })
        }

        fn records(&self) -> Vec<TaskTelemetryRecord> {
            self.records.lock().expect("records lock").clone()
        }

        async fn wait_started(&self, expected: usize) {
            tokio::time::timeout(WAIT, async {
                while self.started.load(Ordering::Relaxed) < expected {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("worker reached the sink");
        }
    }

    impl atm_core::boundary::sealed::Sealed for GatedSink {}

    impl TaskTelemetrySink for GatedSink {
        fn emit(&self, record: TaskTelemetryRecord) -> EmitFuture<'_> {
            Box::pin(async move {
                self.started.fetch_add(1, Ordering::Relaxed);
                let _permit = self.gate.acquire().await.expect("gate open");
                self.records.lock().expect("records lock").push(record);
                self.result
            })
        }
    }

    async fn wait_for(
        runtime: &TaskTelemetryRuntime,
        done: impl Fn(&TaskTelemetryDiagnosticsSnapshot) -> bool,
    ) {
        tokio::time::timeout(WAIT, async {
            while !done(&runtime.diagnostics().snapshot()) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "diagnostics never matched: {:?}",
                runtime.diagnostics().snapshot()
            )
        });
    }

    fn has_worker(runtime: &TaskTelemetryRuntime) -> bool {
        runtime
            .lifecycle
            .try_lock()
            .expect("lifecycle unlocked")
            .worker
            .is_some()
    }

    #[test]
    fn diagnostics_snapshot_is_payload_free() {
        let TaskTelemetryDiagnosticsSnapshot {
            emitted,
            dropped_full,
            dropped_timeout,
            dropped_failure,
            dropped_shutdown,
            config_invalid,
        } = TaskTelemetryDiagnosticsSnapshot::default();

        assert_eq!(
            [
                emitted,
                dropped_full,
                dropped_timeout,
                dropped_failure,
                dropped_shutdown,
            ],
            [0; 5]
        );
        assert!(!config_invalid);
    }

    #[test]
    fn config_bounds_and_defaults() {
        let defaults = TaskTelemetryConfig::default();
        assert_eq!(defaults.queue_capacity, 256);
        assert_eq!(defaults.emit_timeout, Duration::from_secs(1));
        assert_eq!(defaults.drain_timeout, Duration::from_secs(2));
        for queue_capacity in [1, 256, 4096] {
            for timeout in [MIN_DURATION, MAX_DURATION] {
                let config = TaskTelemetryConfig {
                    queue_capacity,
                    emit_timeout: timeout,
                    drain_timeout: timeout,
                };
                assert_eq!(config.validate(), Ok(()), "{config:?}");
            }
        }
        for config in [
            TaskTelemetryConfig {
                queue_capacity: 0,
                ..defaults.clone()
            },
            TaskTelemetryConfig {
                queue_capacity: 4097,
                ..defaults.clone()
            },
            TaskTelemetryConfig {
                emit_timeout: Duration::ZERO,
                ..defaults.clone()
            },
            TaskTelemetryConfig {
                drain_timeout: Duration::from_secs(31),
                ..defaults.clone()
            },
        ] {
            assert_eq!(
                config.validate(),
                Err(AtmErrorCode::TelemetryExportConfigInvalid),
                "{config:?}"
            );
        }
    }

    #[tokio::test]
    async fn invalid_config_is_disabled_without_a_worker() {
        let sink = GatedSink::open(Ok(()));
        let runtime = TaskTelemetryRuntime::start(
            TaskTelemetryConfig {
                queue_capacity: 0,
                ..Default::default()
            },
            Arc::clone(&sink) as Arc<dyn TaskTelemetrySink>,
        );
        assert!(!has_worker(&runtime));
        runtime.try_emit(record(TaskTelemetryKind::Assigned));
        assert_eq!(
            runtime.diagnostics().snapshot(),
            TaskTelemetryDiagnosticsSnapshot {
                config_invalid: true,
                ..Default::default()
            }
        );
        assert_eq!(
            Arc::strong_count(&sink),
            1,
            "invalid config never holds the sink"
        );
        runtime.shutdown(Instant::now() + WAIT).await;
    }

    #[test]
    fn valid_config_outside_tokio_is_inert() {
        let runtime =
            TaskTelemetryRuntime::start(TaskTelemetryConfig::default(), GatedSink::open(Ok(())));
        assert!(!has_worker(&runtime));
        runtime.try_emit(record(TaskTelemetryKind::Assigned));
        assert_eq!(
            runtime.diagnostics().snapshot(),
            TaskTelemetryDiagnosticsSnapshot::default()
        );
    }

    #[tokio::test]
    async fn disabled_runtime_has_no_worker_and_counts_nothing() {
        let runtime = TaskTelemetryRuntime::disabled();
        assert!(!has_worker(&runtime));
        runtime.try_emit(record(TaskTelemetryKind::Started));
        runtime.shutdown(Instant::now() + WAIT).await;
        runtime.try_emit(record(TaskTelemetryKind::Completed));
        assert_eq!(
            runtime.diagnostics().snapshot(),
            TaskTelemetryDiagnosticsSnapshot::default()
        );
    }

    #[tokio::test]
    async fn configured_sink_receives_records_and_counts_emitted() {
        let sink = GatedSink::open(Ok(()));
        let runtime = TaskTelemetryRuntime::start(
            TaskTelemetryConfig::default(),
            Arc::clone(&sink) as Arc<dyn TaskTelemetrySink>,
        );
        let sent = [
            record(TaskTelemetryKind::Assigned),
            record(TaskTelemetryKind::Started),
            record(TaskTelemetryKind::PromptHandoff),
        ];
        for item in sent.clone() {
            runtime.try_emit(item);
        }
        wait_for(&runtime, |s| s.emitted == 3).await;
        assert_eq!(sink.records(), sent);
        assert_eq!(
            runtime.diagnostics().snapshot(),
            TaskTelemetryDiagnosticsSnapshot {
                emitted: 3,
                ..Default::default()
            }
        );
        runtime.shutdown(Instant::now() + WAIT).await;
    }

    #[tokio::test]
    async fn full_queue_drops_without_blocking_the_producer() {
        let sink = GatedSink::closed();
        let runtime = TaskTelemetryRuntime::start(
            TaskTelemetryConfig {
                queue_capacity: 1,
                emit_timeout: MAX_DURATION,
                ..Default::default()
            },
            Arc::clone(&sink) as Arc<dyn TaskTelemetrySink>,
        );
        runtime.try_emit(record(TaskTelemetryKind::Assigned));
        sink.wait_started(1).await;
        // One record is in flight and one fills the queue; the rest drop.
        for _ in 0..10 {
            runtime.try_emit(record(TaskTelemetryKind::Reminded));
        }
        assert_eq!(runtime.diagnostics().snapshot().dropped_full, 9);
        sink.gate.add_permits(Semaphore::MAX_PERMITS);
        wait_for(&runtime, |s| s.emitted == 2).await;
        runtime.shutdown(Instant::now() + WAIT).await;
    }

    #[tokio::test]
    async fn sink_failures_and_timeouts_are_counted_separately() {
        for (error, expect_failure) in [
            (TaskTelemetryError::Unavailable, true),
            (TaskTelemetryError::Rejected, true),
            (TaskTelemetryError::TimedOut, false),
        ] {
            let runtime = TaskTelemetryRuntime::start(
                TaskTelemetryConfig::default(),
                GatedSink::open(Err(error)),
            );
            runtime.try_emit(record(TaskTelemetryKind::Refused));
            wait_for(&runtime, |s| s.dropped_failure + s.dropped_timeout == 1).await;
            let snapshot = runtime.diagnostics().snapshot();
            assert_eq!(
                snapshot.dropped_failure,
                u64::from(expect_failure),
                "{error:?}"
            );
            assert_eq!(
                snapshot.dropped_timeout,
                u64::from(!expect_failure),
                "{error:?}"
            );
            assert_eq!(snapshot.emitted, 0);
            runtime.shutdown(Instant::now() + WAIT).await;
        }
    }

    #[tokio::test]
    async fn stuck_sink_hits_the_emit_timeout() {
        let sink = GatedSink::closed();
        let runtime = TaskTelemetryRuntime::start(
            TaskTelemetryConfig {
                emit_timeout: MIN_DURATION,
                ..Default::default()
            },
            Arc::clone(&sink) as Arc<dyn TaskTelemetrySink>,
        );
        runtime.try_emit(record(TaskTelemetryKind::Acked));
        runtime.try_emit(record(TaskTelemetryKind::Started));
        wait_for(&runtime, |s| s.dropped_timeout == 2).await;
        assert!(sink.records().is_empty());
        runtime.shutdown(Instant::now() + WAIT).await;
    }

    #[tokio::test]
    async fn shutdown_drains_queued_records_within_the_deadline() {
        let sink = GatedSink::closed();
        let runtime = TaskTelemetryRuntime::start(
            TaskTelemetryConfig {
                emit_timeout: MAX_DURATION,
                drain_timeout: MAX_DURATION,
                ..Default::default()
            },
            Arc::clone(&sink) as Arc<dyn TaskTelemetrySink>,
        );
        for _ in 0..4 {
            runtime.try_emit(record(TaskTelemetryKind::Moved));
        }
        sink.wait_started(1).await;
        let shutdown = tokio::spawn({
            let runtime = runtime.clone();
            async move { runtime.shutdown(Instant::now() + WAIT).await }
        });
        sink.gate.add_permits(Semaphore::MAX_PERMITS);
        shutdown.await.expect("shutdown task");
        assert_eq!(
            runtime.diagnostics().snapshot(),
            TaskTelemetryDiagnosticsSnapshot {
                emitted: 4,
                ..Default::default()
            }
        );
        assert!(!has_worker(&runtime));
        runtime.try_emit(record(TaskTelemetryKind::Moved));
        assert_eq!(
            runtime.diagnostics().snapshot().dropped_shutdown,
            1,
            "post-shutdown emit"
        );
    }

    #[tokio::test]
    async fn caller_deadline_bounds_shutdown_and_counts_abandoned_records() {
        let sink = GatedSink::closed();
        let runtime = TaskTelemetryRuntime::start(
            TaskTelemetryConfig {
                emit_timeout: MAX_DURATION,
                drain_timeout: MAX_DURATION,
                ..Default::default()
            },
            Arc::clone(&sink) as Arc<dyn TaskTelemetrySink>,
        );
        for _ in 0..3 {
            runtime.try_emit(record(TaskTelemetryKind::Cancelled));
        }
        sink.wait_started(1).await;
        tokio::time::timeout(
            WAIT,
            runtime.shutdown(Instant::now() + Duration::from_millis(20)),
        )
        .await
        .expect("shutdown obeys the caller deadline, not the 30 s drain");
        assert_eq!(
            runtime.diagnostics().snapshot(),
            TaskTelemetryDiagnosticsSnapshot {
                dropped_shutdown: 3,
                ..Default::default()
            }
        );
        assert!(!has_worker(&runtime));
        assert_eq!(
            Arc::strong_count(&sink),
            1,
            "aborted worker released the sink"
        );
    }

    #[tokio::test]
    async fn cancelled_shutdown_leaves_the_worker_for_the_next_call() {
        let sink = GatedSink::closed();
        let runtime = TaskTelemetryRuntime::start(
            TaskTelemetryConfig {
                emit_timeout: MAX_DURATION,
                drain_timeout: MAX_DURATION,
                ..Default::default()
            },
            Arc::clone(&sink) as Arc<dyn TaskTelemetrySink>,
        );
        runtime.try_emit(record(TaskTelemetryKind::Reopened));
        sink.wait_started(1).await;
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
        let terminal = runtime.diagnostics().snapshot();
        assert_eq!(terminal.dropped_shutdown, 1);

        runtime.shutdown(Instant::now() + WAIT).await;
        assert_eq!(
            runtime.diagnostics().snapshot(),
            terminal,
            "repeated shutdown keeps the terminal result"
        );
    }

    #[tokio::test]
    async fn runtime_is_usable_as_the_sink_trait_object() {
        let sink = GatedSink::open(Ok(()));
        let runtime = TaskTelemetryRuntime::start(
            TaskTelemetryConfig::default(),
            Arc::clone(&sink) as Arc<dyn TaskTelemetrySink>,
        );
        let as_sink: Arc<dyn TaskTelemetrySink> = Arc::new(runtime.clone());
        as_sink
            .emit(record(TaskTelemetryKind::LeadNotified))
            .await
            .expect("admission is best effort");
        wait_for(&runtime, |s| s.emitted == 1).await;
        runtime.shutdown(Instant::now() + WAIT).await;
    }
}
