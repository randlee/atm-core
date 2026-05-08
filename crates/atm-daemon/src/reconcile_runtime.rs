use atm_core::boundary::{
    InboxIngress, NotificationSink, ReconcileRequest, ReconcileResult, WatchEventSource,
    WatchSubscriptionRequest,
};
use atm_core::error::AtmError;
use atm_core::protocol::NotificationEvent;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const DEFAULT_RECONCILE_DEBOUNCE: Duration = Duration::from_millis(25);
const DEFAULT_RECONCILE_COMPLETION_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_RECONCILE_DEBOUNCE_EXTENSIONS: u32 = 8;

#[derive(Clone)]
pub(crate) struct ReconcileRuntime {
    inner: Arc<ReconcileRuntimeInner>,
}

type ReconcileExecutor =
    Arc<dyn Fn(&ReconcileRequest) -> Result<ReconcileResult, AtmError> + Send + Sync>;

struct ReconcileRuntimeInner {
    state: Mutex<ReconcileState>,
    wake: Condvar,
    worker: Mutex<Option<JoinHandle<()>>>,
    debounce: Duration,
    executor: ReconcileExecutor,
}

#[derive(Default)]
struct ReconcileState {
    started: bool,
    shutdown: bool,
    next_waiter_id: u64,
    pending_epoch: u64,
    pending: HashMap<ReconcileKey, PendingReconcile>,
    pending_order: VecDeque<ReconcileKey>,
    completed: HashMap<u64, ReconcileOutcome>,
}

impl ReconcileState {
    fn release_waiter(&mut self, waiter_id: u64) {
        self.completed.remove(&waiter_id);
        for pending in self.pending.values_mut() {
            pending.waiters.retain(|candidate| *candidate != waiter_id);
        }
        self.pending
            .retain(|_, pending| !pending.waiters.is_empty());
        self.pending_order
            .retain(|key| self.pending.contains_key(key));
    }

    #[cfg(test)]
    fn counts(&self) -> (usize, usize, usize) {
        (
            self.pending.len(),
            self.pending_order.len(),
            self.completed.len(),
        )
    }
}

#[derive(Clone)]
enum ReconcileOutcome {
    Success(ReconcileResult),
    Failure(ReconcileFailureSnapshot),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ReconcileKey {
    home_dir: PathBuf,
    team: atm_core::types::TeamName,
    agent: atm_core::types::AgentName,
}

struct PendingReconcile {
    request: ReconcileRequest,
    waiters: Vec<u64>,
}

#[derive(Clone)]
struct ReconcileFailureSnapshot {
    message: String,
    recovery: Option<String>,
}

impl From<AtmError> for ReconcileFailureSnapshot {
    fn from(error: AtmError) -> Self {
        Self {
            message: error.message,
            recovery: error.recovery,
        }
    }
}

impl ReconcileFailureSnapshot {
    fn to_error(&self) -> AtmError {
        let error = AtmError::daemon_unavailable(self.message.clone());
        match &self.recovery {
            Some(recovery) => error.with_recovery(recovery.clone()),
            None => error,
        }
    }
}

impl ReconcileKey {
    fn from_request(request: &ReconcileRequest) -> Self {
        Self {
            home_dir: request.home_dir.clone(),
            team: request.team.clone(),
            agent: request.agent.clone(),
        }
    }
}

impl ReconcileRuntime {
    pub(crate) fn new(
        watch_source: Arc<dyn WatchEventSource + Send + Sync>,
        inbox_ingress: Arc<dyn InboxIngress + Send + Sync>,
        notification_sink: Arc<dyn NotificationSink + Send + Sync>,
    ) -> Self {
        Self::new_with_executor(
            Arc::new(move |request| {
                let batch = watch_source.poll(WatchSubscriptionRequest {
                    home_dir: request.home_dir.clone(),
                    team: request.team.clone(),
                    agent: request.agent.clone(),
                })?;
                let import = inbox_ingress.import_inbox_source(
                    atm_core::boundary::InboxIngressImportRequest {
                        home_dir: request.home_dir.clone(),
                        team: request.team.clone(),
                        agent: request.agent.clone(),
                    },
                )?;
                notification_sink.deliver(NotificationEvent {
                    kind: "reconcile_complete".to_string(),
                    detail: format!(
                        "observed_paths={} imported_sources={}",
                        batch.paths.len(),
                        import.source_files.len()
                    ),
                    team: Some(request.team.clone()),
                    agent: Some(request.agent.clone()),
                })?;
                Ok(ReconcileResult {
                    observed_paths: batch.paths.len(),
                    imported_sources: import.source_files.len(),
                })
            }),
            DEFAULT_RECONCILE_DEBOUNCE,
        )
    }

    fn new_with_executor(executor: ReconcileExecutor, debounce: Duration) -> Self {
        Self {
            inner: Arc::new(ReconcileRuntimeInner {
                state: Mutex::new(ReconcileState::default()),
                wake: Condvar::new(),
                worker: Mutex::new(None),
                debounce,
                executor,
            }),
        }
    }

    pub(crate) fn start(&self) -> Result<(), AtmError> {
        let mut state =
            self.inner.state.lock().map_err(|_| {
                AtmError::daemon_unavailable("reconcile runtime state lock poisoned")
            })?;
        if state.started {
            return Ok(());
        }
        state.started = true;
        state.shutdown = false;
        drop(state);

        let inner = Arc::clone(&self.inner);
        let handle = thread::Builder::new()
            .name("atm-daemon-reconcile".to_string())
            .spawn(move || reconcile_worker_loop(inner))
            .map_err(|source| {
                AtmError::daemon_unavailable("failed to spawn reconcile runtime worker")
                    .with_source(source)
            })?;
        *self.inner.worker.lock().map_err(|_| {
            AtmError::daemon_unavailable("reconcile runtime worker lock poisoned")
        })? = Some(handle);
        Ok(())
    }

    pub(crate) fn shutdown(&self) -> Result<(), AtmError> {
        {
            let mut state = self.inner.state.lock().map_err(|_| {
                AtmError::daemon_unavailable("reconcile runtime state lock poisoned")
            })?;
            state.shutdown = true;
            self.inner.wake.notify_all();
        }
        if let Some(handle) = self
            .inner
            .worker
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("reconcile runtime worker lock poisoned"))?
            .take()
        {
            handle.join().map_err(|_| {
                AtmError::daemon_unavailable("reconcile runtime worker panicked during shutdown")
            })?;
        }
        Ok(())
    }

    pub(crate) fn reconcile(&self, request: ReconcileRequest) -> Result<ReconcileResult, AtmError> {
        let waiter_id = {
            let mut state = self.inner.state.lock().map_err(|_| {
                AtmError::daemon_unavailable("reconcile runtime state lock poisoned")
            })?;
            if !state.started {
                return Err(AtmError::daemon_unavailable(
                    "reconcile runtime is unavailable before daemon startup",
                ));
            }
            if state.shutdown {
                return Err(AtmError::daemon_unavailable(
                    "reconcile runtime is unavailable during daemon shutdown",
                ));
            }
            let waiter_id = state.next_waiter_id;
            state.next_waiter_id += 1;
            state.pending_epoch = state.pending_epoch.saturating_add(1);
            let key = ReconcileKey::from_request(&request);
            if let Some(pending) = state.pending.get_mut(&key) {
                pending.request = request.clone();
                pending.waiters.push(waiter_id);
            } else {
                state.pending_order.push_back(key.clone());
                state.pending.insert(
                    key,
                    PendingReconcile {
                        request,
                        waiters: vec![waiter_id],
                    },
                );
            }
            self.inner.wake.notify_one();
            waiter_id
        };

        let mut state =
            self.inner.state.lock().map_err(|_| {
                AtmError::daemon_unavailable("reconcile runtime state lock poisoned")
            })?;
        loop {
            if let Some(outcome) = state.completed.remove(&waiter_id) {
                return match outcome {
                    ReconcileOutcome::Success(result) => Ok(result),
                    ReconcileOutcome::Failure(failure) => Err(failure.to_error()),
                };
            }
            if state.shutdown {
                state.release_waiter(waiter_id);
                return Err(AtmError::daemon_unavailable(
                    "reconcile runtime shut down before completion",
                ));
            }
            let wait = self
                .inner
                .wake
                .wait_timeout(state, DEFAULT_RECONCILE_COMPLETION_TIMEOUT)
                .map_err(|_| {
                    AtmError::daemon_unavailable("reconcile runtime state lock poisoned")
                })?;
            state = wait.0;
            if wait.1.timed_out() {
                state.release_waiter(waiter_id);
                return Err(AtmError::daemon_unavailable(
                    "reconcile runtime timed out waiting for background completion",
                ));
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(executor: ReconcileExecutor, debounce: Duration) -> Self {
        Self::new_with_executor(executor, debounce)
    }

    #[cfg(test)]
    pub(crate) fn state_counts_for_test(&self) -> (usize, usize, usize) {
        self.inner.state.lock().expect("state lock").counts()
    }
}

fn reconcile_worker_loop(inner: Arc<ReconcileRuntimeInner>) {
    loop {
        let pending = {
            let mut state = match inner.state.lock() {
                Ok(state) => state,
                Err(_) => return,
            };
            while state.pending.is_empty() && !state.shutdown {
                state = match inner.wake.wait(state) {
                    Ok(state) => state,
                    Err(_) => return,
                };
            }
            if state.shutdown {
                return;
            }
            let mut debounce_epoch = state.pending_epoch;
            let mut debounce_extensions = 0u32;
            loop {
                let wait = match inner.wake.wait_timeout(state, inner.debounce) {
                    Ok(wait) => wait,
                    Err(_) => return,
                };
                state = wait.0;
                if state.shutdown {
                    return;
                }
                if state.pending_epoch != debounce_epoch {
                    debounce_epoch = state.pending_epoch;
                    debounce_extensions = debounce_extensions.saturating_add(1);
                    if debounce_extensions >= MAX_RECONCILE_DEBOUNCE_EXTENSIONS {
                        break;
                    }
                    continue;
                }
                if wait.1.timed_out() {
                    break;
                }
            }
            let pending_order = std::mem::take(&mut state.pending_order);
            let mut drained = Vec::with_capacity(pending_order.len());
            for key in pending_order {
                if let Some(pending) = state.pending.remove(&key) {
                    drained.push(pending);
                }
            }
            drained
        };

        for pending_request in pending {
            let outcome = match (inner.executor)(&pending_request.request) {
                Ok(result) => ReconcileOutcome::Success(result),
                Err(error) => ReconcileOutcome::Failure(error.into()),
            };
            if let Ok(mut state) = inner.state.lock() {
                for waiter in pending_request.waiters {
                    state.completed.insert(waiter, outcome.clone());
                }
                inner.wake.notify_all();
            } else {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ReconcileRuntime;
    use atm_core::boundary::{
        self, InboxIngress, InboxIngressDiagnosticsRequest, InboxIngressDiagnosticsResponse,
        InboxIngressIdentityFingerprintRequest, InboxIngressIdentityFingerprintResponse,
        InboxIngressImportRequest, InboxIngressImportResponse, NotificationEvent, NotificationSink,
        ReconcileRequest, WatchEventBatch, WatchEventSource, WatchSubscriptionRequest,
    };
    use atm_core::protocol::ReconcileResult;
    use std::sync::{Arc, Barrier, Condvar, Mutex, mpsc};
    use std::time::Duration;

    fn request() -> ReconcileRequest {
        ReconcileRequest {
            home_dir: std::env::temp_dir().join("atm-reconcile-test"),
            team: "test-team".parse().expect("team"),
            agent: "test-agent".parse().expect("agent"),
        }
    }

    fn request_for(agent: &str) -> ReconcileRequest {
        ReconcileRequest {
            home_dir: std::env::temp_dir().join("atm-reconcile-test"),
            team: "test-team".parse().expect("team"),
            agent: agent.parse().expect("agent"),
        }
    }

    #[test]
    fn reconcile_runtime_coalesces_duplicate_requests() {
        let calls = Arc::new(Mutex::new(0usize));
        let barrier = Arc::new(Barrier::new(2));
        let runtime = ReconcileRuntime::new_for_test(
            Arc::new({
                let calls = Arc::clone(&calls);
                move |_| {
                    *calls.lock().expect("calls") += 1;
                    Ok(ReconcileResult {
                        observed_paths: 2,
                        imported_sources: 1,
                    })
                }
            }),
            Duration::from_millis(20),
        );
        runtime.start().expect("start");

        let runtime_a = runtime.clone();
        let runtime_b = runtime.clone();
        let request_a = request();
        let request_b = request();
        let first = std::thread::spawn({
            let barrier = Arc::clone(&barrier);
            move || {
                barrier.wait();
                runtime_a.reconcile(request_a).expect("first")
            }
        });
        let second = std::thread::spawn({
            let barrier = Arc::clone(&barrier);
            move || {
                barrier.wait();
                runtime_b.reconcile(request_b).expect("second")
            }
        });
        assert_eq!(first.join().expect("join").observed_paths, 2);
        assert_eq!(second.join().expect("join").imported_sources, 1);
        assert_eq!(*calls.lock().expect("calls"), 1);
        runtime.shutdown().expect("shutdown");
    }

    #[test]
    fn reconcile_runtime_cleans_up_pending_waiters_during_shutdown() {
        let runtime = ReconcileRuntime::new_for_test(
            Arc::new(|_| {
                Ok(ReconcileResult {
                    observed_paths: 1,
                    imported_sources: 1,
                })
            }),
            Duration::from_millis(250),
        );
        runtime.start().expect("start");

        let runtime_for_thread = runtime.clone();
        let join = std::thread::spawn(move || runtime_for_thread.reconcile(request()));
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while runtime.state_counts_for_test().0 == 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "reconcile request never entered the pending queue"
            );
            std::thread::yield_now();
        }
        runtime.shutdown().expect("shutdown");

        let error = join
            .join()
            .expect("join")
            .expect_err("shutdown interruption");
        assert!(error.message.contains("shut down before completion"));
        assert_eq!(runtime.state_counts_for_test(), (0, 0, 0));
    }

    #[test]
    fn reconcile_runtime_returns_executor_failures() {
        let runtime = ReconcileRuntime::new_for_test(
            Arc::new(|_| {
                Err(atm_core::error::AtmError::daemon_unavailable(
                    "reconcile failed",
                ))
            }),
            Duration::from_millis(10),
        );
        runtime.start().expect("start");
        let error = runtime.reconcile(request()).expect_err("failure");
        assert!(error.message.contains("reconcile failed"));
        runtime.shutdown().expect("shutdown");
    }

    #[test]
    fn reconcile_runtime_preserves_trigger_order_and_signals_completion() {
        let order = Arc::new(Mutex::new(Vec::new()));
        let (started_tx, started_rx) = mpsc::channel();
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let runtime = ReconcileRuntime::new_for_test(
            Arc::new({
                let order = Arc::clone(&order);
                let started_tx = started_tx.clone();
                let release = Arc::clone(&release);
                move |request| {
                    order
                        .lock()
                        .expect("order")
                        .push(request.agent.as_str().to_string());
                    started_tx
                        .send(request.agent.as_str().to_string())
                        .expect("started");
                    if request.agent.as_str() == "agent-a" {
                        let (released, wake) = &*release;
                        let mut released = released.lock().expect("released");
                        while !*released {
                            released = wake.wait(released).expect("wait release");
                        }
                    }
                    Ok(ReconcileResult {
                        observed_paths: 1,
                        imported_sources: 1,
                    })
                }
            }),
            Duration::from_millis(10),
        );
        runtime.start().expect("start");

        let runtime_a = runtime.clone();
        let runtime_b = runtime.clone();
        let first = std::thread::spawn(move || runtime_a.reconcile(request_for("agent-a")));
        assert_eq!(started_rx.recv().expect("first started"), "agent-a");
        let second = std::thread::spawn(move || runtime_b.reconcile(request_for("agent-b")));
        let (released, wake) = &*release;
        *released.lock().expect("released") = true;
        wake.notify_all();

        let first_result = first.join().expect("first join").expect("first result");
        let second_result = second.join().expect("second join").expect("second result");
        assert_eq!(first_result.observed_paths, 1);
        assert_eq!(second_result.imported_sources, 1);
        assert_eq!(
            order.lock().expect("order").as_slice(),
            ["agent-a".to_string(), "agent-b".to_string()]
        );
        runtime.shutdown().expect("shutdown");
    }

    #[derive(Clone)]
    struct FakeWatchSource;

    impl boundary::sealed::Sealed for FakeWatchSource {}

    impl WatchEventSource for FakeWatchSource {
        fn poll(
            &self,
            _request: WatchSubscriptionRequest,
        ) -> Result<WatchEventBatch, atm_core::error::AtmError> {
            Ok(WatchEventBatch {
                paths: vec![std::env::temp_dir().join("watch.json")],
            })
        }
    }

    #[derive(Clone)]
    struct FakeInboxIngress;

    impl boundary::sealed::Sealed for FakeInboxIngress {}

    impl InboxIngress for FakeInboxIngress {
        fn import_inbox_source(
            &self,
            _request: InboxIngressImportRequest,
        ) -> Result<InboxIngressImportResponse, atm_core::error::AtmError> {
            Ok(InboxIngressImportResponse {
                source_files: Vec::new(),
            })
        }

        fn compute_identity_fingerprint(
            &self,
            _request: InboxIngressIdentityFingerprintRequest,
        ) -> Result<InboxIngressIdentityFingerprintResponse, atm_core::error::AtmError> {
            Ok(InboxIngressIdentityFingerprintResponse { fingerprint: None })
        }

        fn report_diagnostics(
            &self,
            _request: InboxIngressDiagnosticsRequest,
        ) -> Result<InboxIngressDiagnosticsResponse, atm_core::error::AtmError> {
            Ok(InboxIngressDiagnosticsResponse {
                duplicate_legacy_message_ids: 0,
                messages_without_ids: 0,
            })
        }
    }

    #[derive(Clone)]
    struct FakeNotificationSink {
        delivered: Arc<Mutex<Vec<NotificationEvent>>>,
    }

    impl boundary::sealed::Sealed for FakeNotificationSink {}

    impl NotificationSink for FakeNotificationSink {
        fn deliver(&self, event: NotificationEvent) -> Result<(), atm_core::error::AtmError> {
            self.delivered.lock().expect("delivered").push(event);
            Ok(())
        }
    }

    #[test]
    fn reconcile_runtime_routes_notifications_through_notification_sink_boundary() {
        let delivered = Arc::new(Mutex::new(Vec::new()));
        let runtime = ReconcileRuntime::new(
            Arc::new(FakeWatchSource),
            Arc::new(FakeInboxIngress),
            Arc::new(FakeNotificationSink {
                delivered: Arc::clone(&delivered),
            }),
        );
        runtime.start().expect("start");

        let result = runtime.reconcile(request()).expect("reconcile result");
        assert_eq!(result.observed_paths, 1);
        assert_eq!(result.imported_sources, 0);

        let delivered = delivered.lock().expect("delivered");
        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0].kind, "reconcile_complete");

        runtime.shutdown().expect("shutdown");
    }
}
