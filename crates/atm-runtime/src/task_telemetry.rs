//! Supervised, bounded, best-effort task telemetry worker.
//!
//! The runtime is the sole holder and caller of the [`TaskTelemetrySink`]
//! trait object (ADR-064). Producers call [`TaskTelemetryRuntime::try_emit`],
//! which never awaits; one worker calls the synchronous sink for each record.
//! The exporter's SDK batch processor is the only export queue behind it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use atm_core::{TaskTelemetryRecord, TaskTelemetrySink};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::Instant;

/// Production worker limits: the queue holds 256 records and shutdown drains
/// for at most 2 s (further bounded by the caller's deadline).
pub const TASK_TELEMETRY_QUEUE_CAPACITY: usize = 256;
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

/// Bootstrap-owned exporter selection. Absence keeps telemetry inert.
#[derive(Clone)]
pub struct TaskTelemetrySetup {
    pub sink: Arc<dyn TaskTelemetrySink>,
}

/// Concurrent, non-fatal counters for doctor health composition.
#[derive(Debug, Default)]
pub struct TaskTelemetryDiagnostics {
    emitted: AtomicU64,
    dropped_full: AtomicU64,
    dropped_failure: AtomicU64,
    dropped_shutdown: AtomicU64,
    /// Set when `start` ran outside a Tokio runtime and left telemetry inert.
    no_runtime: AtomicBool,
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
    pub dropped_failure: u64,
    pub dropped_shutdown: u64,
    /// The runtime was started outside Tokio, so no worker exists and nothing
    /// will be exported.
    pub no_runtime: bool,
}

impl TaskTelemetryDiagnostics {
    #[must_use]
    pub fn snapshot(&self) -> TaskTelemetryDiagnosticsSnapshot {
        TaskTelemetryDiagnosticsSnapshot {
            emitted: self.emitted.load(Ordering::Relaxed),
            dropped_full: self.dropped_full.load(Ordering::Relaxed),
            dropped_failure: self.dropped_failure.load(Ordering::Relaxed),
            dropped_shutdown: self.dropped_shutdown.load(Ordering::Relaxed),
            no_runtime: self.no_runtime.load(Ordering::Relaxed),
        }
    }

    /// Counts one admitted record under `counter`, once. A record already
    /// counted as abandoned by `shutdown` (which zeroes `pending`) is not
    /// counted again when an abandoned sink call returns late.
    fn resolve(&self, counter: &AtomicU64) {
        if self
            .pending
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |pending| {
                pending.checked_sub(1)
            })
            .is_ok()
        {
            counter.fetch_add(1, Ordering::Relaxed);
        }
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

/// Runtime producer handle for best-effort task telemetry.
///
/// Producers use [`TaskTelemetryRuntime::try_emit`]; this handle intentionally
/// does not implement [`TaskTelemetrySink`], whose implementation belongs to
/// the exporter provided when the runtime starts.
///
/// ```compile_fail
/// use atm_core::TaskTelemetrySink;
/// use atm_runtime::TaskTelemetryRuntime;
///
/// fn runtime_is_not_a_sink(runtime: TaskTelemetryRuntime) -> Box<dyn TaskTelemetrySink> {
///     Box::new(runtime)
/// }
/// ```
#[derive(Clone)]
pub struct TaskTelemetryRuntime {
    // MUTEX: a std mutex is correct here: the guard only clones or takes the
    // sender and is never held across an await. Poison is recovered with
    // `into_inner`: the slot holds no invariant a panic could break.
    sender: Arc<std::sync::Mutex<Option<mpsc::Sender<TaskTelemetryRecord>>>>,
    diagnostics: Arc<TaskTelemetryDiagnostics>,
    lifecycle: Arc<tokio::sync::Mutex<Lifecycle>>,
}

impl TaskTelemetryRuntime {
    /// Starts one worker. A caller outside Tokio gets an inert runtime whose
    /// diagnostics report `no_runtime`; it never prevents runtime construction.
    pub fn start(sink: Arc<dyn TaskTelemetrySink>) -> Self {
        // `assemble_runtime` is synchronous. Telemetry is best-effort, so a
        // caller outside Tokio gets an inert runtime rather than a panic.
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            let runtime = Self::disabled();
            runtime
                .diagnostics
                .no_runtime
                .store(true, Ordering::Relaxed);
            return runtime;
        };
        let diagnostics = Arc::new(TaskTelemetryDiagnostics::default());
        let (sender, receiver) = mpsc::channel(TASK_TELEMETRY_QUEUE_CAPACITY);
        let (stop, stop_receiver) = oneshot::channel();
        let worker = handle.spawn(run_worker(
            sink,
            receiver,
            stop_receiver,
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
        }
    }

    /// Inert default used when no exporter is configured: no queue, no worker.
    pub fn disabled() -> Self {
        Self {
            sender: Arc::new(std::sync::Mutex::new(None)),
            diagnostics: Arc::new(TaskTelemetryDiagnostics::default()),
            lifecycle: Arc::new(tokio::sync::Mutex::new(Lifecycle::default())),
        }
    }

    /// Non-blocking producer path: telemetry never delays task processing.
    pub fn try_emit(&self, record: TaskTelemetryRecord) {
        let sender = self
            .sender
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let Some(sender) = sender else {
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

    /// Stops admission, drains until `min(DRAIN_TIMEOUT, deadline)`, then
    /// aborts the worker and joins it until `deadline`. A worker still inside
    /// a sink call at `deadline` is abandoned. Repeated calls are no-ops once
    /// the worker is gone; a cancelled call leaves the worker for the next
    /// caller.
    pub async fn shutdown(&self, deadline: Instant) {
        let taken = self
            .sender
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if taken.is_some() {
            self.diagnostics.closed.store(true, Ordering::Relaxed);
        }
        let mut lifecycle = self.lifecycle.lock().await;
        let drain_deadline = *lifecycle
            .drain_deadline
            .get_or_insert_with(|| deadline.min(Instant::now() + DRAIN_TIMEOUT));
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
            // The sink is synchronous, so an aborted worker stops only when
            // its current sink call returns.
            match tokio::time::timeout_at(deadline, &mut *worker).await {
                Ok(Err(error)) if error.is_panic() => {
                    tracing::warn!(%error, "task telemetry worker panicked during abort");
                }
                Ok(_) => {}
                Err(_) => tracing::warn!(
                    "task telemetry worker still inside a sink call at the shutdown deadline; abandoned"
                ),
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
    diagnostics: Arc<TaskTelemetryDiagnostics>,
) {
    let drain_deadline = loop {
        tokio::select! {
            message = receiver.recv() => match message {
                Some(record) => emit_one(&*sink, record, &diagnostics),
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
        emit_one(&*sink, record, &diagnostics);
    }
    while receiver.try_recv().is_ok() {
        diagnostics.resolve(&diagnostics.dropped_shutdown);
    }
}

fn emit_one(
    sink: &dyn TaskTelemetrySink,
    record: TaskTelemetryRecord,
    diagnostics: &TaskTelemetryDiagnostics,
) {
    let counter = match sink.emit(record) {
        Ok(()) => &diagnostics.emitted,
        Err(_) => &diagnostics.dropped_failure,
    };
    diagnostics.resolve(counter);
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::{Condvar, Mutex};

    use atm_core::{TaskTelemetryError, TaskTelemetryKind};
    use atm_storage::TaskActor;

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

    /// Blocks each sink call until the gate opens, so tests control when the
    /// worker makes progress without sleeping. A closed gate holds a Tokio
    /// worker thread, so tests that use one run on a multi-thread runtime.
    struct GatedSink {
        open: Mutex<bool>,
        opened: Condvar,
        started: AtomicUsize,
        records: Mutex<Vec<TaskTelemetryRecord>>,
        result: Result<(), TaskTelemetryError>,
    }

    impl GatedSink {
        fn with_gate(open: bool, result: Result<(), TaskTelemetryError>) -> Arc<Self> {
            Arc::new(Self {
                open: Mutex::new(open),
                opened: Condvar::new(),
                started: AtomicUsize::new(0),
                records: Mutex::new(Vec::new()),
                result,
            })
        }

        fn open(result: Result<(), TaskTelemetryError>) -> Arc<Self> {
            Self::with_gate(true, result)
        }

        fn closed() -> Arc<Self> {
            Self::with_gate(false, Ok(()))
        }

        fn release(&self) {
            *self.open.lock().expect("gate lock") = true;
            self.opened.notify_all();
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
        fn emit(&self, record: TaskTelemetryRecord) -> Result<(), TaskTelemetryError> {
            self.started.fetch_add(1, Ordering::Relaxed);
            let open = self.open.lock().expect("gate lock");
            drop(
                self.opened
                    .wait_while(open, |open| !*open)
                    .expect("gate lock"),
            );
            self.records.lock().expect("records lock").push(record);
            self.result
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

    /// Waits until the abandoned worker has returned from the sink and
    /// dropped its clone.
    async fn wait_sink_released(sink: &Arc<GatedSink>) {
        tokio::time::timeout(WAIT, async {
            while Arc::strong_count(sink) > 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("abandoned worker released the sink");
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
            dropped_failure,
            dropped_shutdown,
            no_runtime,
        } = TaskTelemetryDiagnosticsSnapshot::default();

        assert_eq!(
            [emitted, dropped_full, dropped_failure, dropped_shutdown],
            [0; 4]
        );
        assert!(!no_runtime);
    }

    #[test]
    fn start_outside_tokio_reports_no_runtime() {
        let runtime = TaskTelemetryRuntime::start(GatedSink::open(Ok(())));
        assert!(!has_worker(&runtime));
        runtime.try_emit(record(TaskTelemetryKind::Assigned));
        assert_eq!(
            runtime.diagnostics().snapshot(),
            TaskTelemetryDiagnosticsSnapshot {
                no_runtime: true,
                ..Default::default()
            },
            "an inert start must be reported, not look healthy"
        );
    }

    #[tokio::test]
    async fn start_inside_tokio_does_not_report_no_runtime() {
        let runtime = TaskTelemetryRuntime::start(GatedSink::open(Ok(())));
        assert!(has_worker(&runtime));
        assert!(!runtime.diagnostics().snapshot().no_runtime);
        runtime.shutdown(Instant::now() + WAIT).await;
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
        let runtime = TaskTelemetryRuntime::start(Arc::clone(&sink) as Arc<dyn TaskTelemetrySink>);
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn full_queue_drops_without_blocking_the_producer() {
        let sink = GatedSink::closed();
        let runtime = TaskTelemetryRuntime::start(Arc::clone(&sink) as Arc<dyn TaskTelemetrySink>);
        runtime.try_emit(record(TaskTelemetryKind::Assigned));
        sink.wait_started(1).await;
        // One record is in flight and the queue fills; the rest drop.
        for _ in 0..TASK_TELEMETRY_QUEUE_CAPACITY + 9 {
            runtime.try_emit(record(TaskTelemetryKind::Reminded));
        }
        assert_eq!(runtime.diagnostics().snapshot().dropped_full, 9);
        sink.release();
        wait_for(&runtime, |s| {
            s.emitted == 1 + TASK_TELEMETRY_QUEUE_CAPACITY as u64
        })
        .await;
        runtime.shutdown(Instant::now() + WAIT).await;
    }

    #[tokio::test]
    async fn sink_failures_are_counted_as_failure_drops() {
        for error in [
            TaskTelemetryError::Unavailable,
            TaskTelemetryError::Rejected,
        ] {
            let runtime = TaskTelemetryRuntime::start(GatedSink::open(Err(error)));
            runtime.try_emit(record(TaskTelemetryKind::Refused));
            wait_for(&runtime, |s| s.dropped_failure == 1).await;
            assert_eq!(
                runtime.diagnostics().snapshot(),
                TaskTelemetryDiagnosticsSnapshot {
                    dropped_failure: 1,
                    ..Default::default()
                },
                "{error:?}"
            );
            runtime.shutdown(Instant::now() + WAIT).await;
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_drains_queued_records_within_the_deadline() {
        let sink = GatedSink::closed();
        let runtime = TaskTelemetryRuntime::start(Arc::clone(&sink) as Arc<dyn TaskTelemetrySink>);
        for _ in 0..4 {
            runtime.try_emit(record(TaskTelemetryKind::Moved));
        }
        sink.wait_started(1).await;
        let shutdown = tokio::spawn({
            let runtime = runtime.clone();
            async move { runtime.shutdown(Instant::now() + WAIT).await }
        });
        sink.release();
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn caller_deadline_bounds_shutdown_and_counts_abandoned_records_once() {
        let sink = GatedSink::closed();
        let runtime = TaskTelemetryRuntime::start(Arc::clone(&sink) as Arc<dyn TaskTelemetrySink>);
        for _ in 0..3 {
            runtime.try_emit(record(TaskTelemetryKind::Cancelled));
        }
        sink.wait_started(1).await;
        tokio::time::timeout(
            WAIT,
            runtime.shutdown(Instant::now() + Duration::from_millis(20)),
        )
        .await
        .expect("shutdown obeys the caller deadline, not the 2 s drain or the held sink");
        let abandoned = TaskTelemetryDiagnosticsSnapshot {
            dropped_shutdown: 3,
            ..Default::default()
        };
        assert_eq!(runtime.diagnostics().snapshot(), abandoned);
        assert!(!has_worker(&runtime));

        // The abandoned sink call returns late; its record was already
        // counted as a shutdown drop and is not counted again.
        sink.release();
        wait_sink_released(&sink).await;
        assert_eq!(runtime.diagnostics().snapshot(), abandoned);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancelled_shutdown_leaves_the_worker_for_the_next_call() {
        let sink = GatedSink::closed();
        let runtime = TaskTelemetryRuntime::start(Arc::clone(&sink) as Arc<dyn TaskTelemetrySink>);
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
        let terminal = runtime.diagnostics().snapshot();
        assert_eq!(terminal.dropped_shutdown, 1);

        runtime.shutdown(Instant::now() + WAIT).await;
        assert_eq!(
            runtime.diagnostics().snapshot(),
            terminal,
            "repeated shutdown keeps the terminal result"
        );
        sink.release();
        wait_sink_released(&sink).await;
        assert_eq!(runtime.diagnostics().snapshot(), terminal);
    }
}
