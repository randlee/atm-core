use atm_core::boundary::{
    InboxIngress, NotificationSink, ReconcileRequest, ReconcileResult, WatchEventSource,
    WatchSubscriptionRequest,
};
use atm_core::error::AtmError;
use atm_core::protocol::{NotificationEvent, ProtocolErrorEnvelope};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const DEFAULT_RECONCILE_DEBOUNCE: Duration = Duration::from_millis(25);
const DEFAULT_RECONCILE_COMPLETION_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_RECONCILE_IDLE_INTERVAL: Duration = Duration::from_millis(50);
const MAX_RECONCILE_DEBOUNCE_EXTENSIONS: u32 = 8;
const MAX_RECONCILE_FINGERPRINT_KEYS: usize = 1024;
#[cfg(not(test))]
const RECONCILE_SHUTDOWN_DEADLINE: Duration = Duration::from_secs(2);
#[cfg(test)]
const RECONCILE_SHUTDOWN_DEADLINE: Duration = Duration::from_millis(100);

#[derive(Clone)]
pub(crate) struct ReconcileRuntime {
    inner: Arc<ReconcileRuntimeInner>,
}

type ReconcileExecutor =
    Arc<dyn Fn(&ReconcileRequest) -> Result<ReconcileResult, AtmError> + Send + Sync>;

struct ReconcileRuntimeInner {
    // Worker thread, reconcile callers, and shutdown path all access state
    // concurrently; Mutex+Condvar guards the lifecycle and pending/completed
    // reconcile registries.
    state: Mutex<ReconcileState>,
    wake: Condvar,
    #[cfg(test)]
    pending_changed: Condvar,
    worker: Mutex<Option<JoinHandle<()>>>,
    debounce: Duration,
    executor: ReconcileExecutor,
    #[cfg_attr(not(test), allow(dead_code))]
    notification_fingerprints: Arc<Mutex<NotificationFingerprintRegistry>>,
}

#[derive(Default)]
struct ReconcileState {
    started: bool,
    shutdown: bool,
    next_waiter_id: u64,
    pending_epoch: u64,
    pending: HashMap<ReconcileKey, PendingReconcile>,
    pending_order: VecDeque<ReconcileKey>,
    active_waiters: HashSet<u64>,
    completed: HashMap<u64, ReconcileOutcome>,
}

impl ReconcileState {
    fn release_waiter(&mut self, waiter_id: u64) {
        self.active_waiters.remove(&waiter_id);
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

    #[cfg(test)]
    fn waiter_count(&self) -> usize {
        self.pending
            .values()
            .map(|pending| pending.waiters.len())
            .sum()
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

#[derive(Default)]
struct NotificationFingerprintRegistry {
    entries: HashMap<ReconcileKey, HashSet<String>>,
    order: VecDeque<ReconcileKey>,
}

#[derive(Clone)]
struct ReconcileFailureSnapshot {
    code: atm_core::error_codes::AtmErrorCode,
    message: String,
    recovery: Option<String>,
}

impl From<AtmError> for ReconcileFailureSnapshot {
    fn from(error: AtmError) -> Self {
        Self {
            code: error.code,
            message: error.message,
            recovery: error.recovery,
        }
    }
}

impl ReconcileFailureSnapshot {
    fn to_error(&self) -> AtmError {
        ProtocolErrorEnvelope {
            code: self.code,
            message: self.message.clone(),
            recovery: self.recovery.clone(),
        }
        .into_atm_error()
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
        // Fingerprints must survive across executor invocations so duplicate
        // reconcile cycles can compare the newest inbox projection with the
        // previous one before deciding whether to emit a notification.
        let notification_fingerprints =
            Arc::new(Mutex::new(NotificationFingerprintRegistry::default()));
        let notification_fingerprints_for_executor = Arc::clone(&notification_fingerprints);
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
                if should_emit_reconcile_notification(
                    request,
                    &import,
                    inbox_ingress.as_ref(),
                    notification_fingerprints_for_executor.as_ref(),
                )? {
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
                }
                Ok(ReconcileResult {
                    observed_paths: batch.paths.len(),
                    imported_sources: import.source_files.len(),
                })
            }),
            DEFAULT_RECONCILE_DEBOUNCE,
            notification_fingerprints,
        )
    }

    fn new_with_executor(
        executor: ReconcileExecutor,
        debounce: Duration,
        notification_fingerprints: Arc<Mutex<NotificationFingerprintRegistry>>,
    ) -> Self {
        Self {
            inner: Arc::new(ReconcileRuntimeInner {
                state: Mutex::new(ReconcileState::default()),
                wake: Condvar::new(),
                #[cfg(test)]
                pending_changed: Condvar::new(),
                worker: Mutex::new(None),
                debounce,
                executor,
                notification_fingerprints,
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
        let mut worker = match self.inner.worker.lock() {
            Ok(worker) => worker,
            Err(_) => {
                let _ = handle.join();
                return Err(AtmError::daemon_unavailable(
                    "reconcile runtime worker lock poisoned",
                ));
            }
        };
        *worker = Some(handle);
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
            let (result_tx, result_rx) = mpsc::sync_channel(1);
            let join_helper = thread::spawn(move || {
                let _ = result_tx.send(handle.join());
            });
            match result_rx.recv_timeout(RECONCILE_SHUTDOWN_DEADLINE) {
                Ok(Ok(())) => {
                    let _ = join_helper.join();
                }
                Ok(Err(_)) => {
                    let _ = join_helper.join();
                    return Err(AtmError::daemon_unavailable(
                        "reconcile runtime worker panicked during shutdown",
                    ));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    drop(join_helper);
                    tracing::warn!(
                        timeout_ms = RECONCILE_SHUTDOWN_DEADLINE.as_millis(),
                        "reconcile runtime worker exceeded shutdown deadline; detaching join helper"
                    );
                    return Err(AtmError::daemon_unavailable(
                        "reconcile runtime worker exceeded the bounded shutdown deadline",
                    )
                    .with_recovery(
                        "Restart atm-daemon after the reconcile background lane becomes responsive again.",
                    ));
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let _ = join_helper.join();
                    tracing::warn!(
                        "reconcile runtime worker join helper exited before reporting shutdown status"
                    );
                }
            }
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
            state.active_waiters.insert(waiter_id);
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
            #[cfg(test)]
            self.inner.pending_changed.notify_all();
            waiter_id
        };

        let mut state =
            self.inner.state.lock().map_err(|_| {
                AtmError::daemon_unavailable("reconcile runtime state lock poisoned")
            })?;
        loop {
            if state.shutdown {
                state.release_waiter(waiter_id);
                return Err(AtmError::daemon_unavailable(
                    "reconcile runtime shut down before completion",
                ));
            }
            if let Some(outcome) = state.completed.remove(&waiter_id) {
                state.active_waiters.remove(&waiter_id);
                return match outcome {
                    ReconcileOutcome::Success(result) => Ok(result),
                    ReconcileOutcome::Failure(failure) => Err(failure.to_error()),
                };
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
        Self::new_with_executor(
            executor,
            debounce,
            Arc::new(Mutex::new(NotificationFingerprintRegistry::default())),
        )
    }

    #[cfg(test)]
    pub(crate) fn state_counts_for_test(&self) -> (usize, usize, usize) {
        self.inner.state.lock().expect("state lock").counts()
    }

    #[cfg(test)]
    pub(crate) fn wait_for_pending_count_for_test(
        &self,
        minimum_pending: usize,
        timeout: Duration,
    ) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        let mut state = self.inner.state.lock().expect("state lock");
        while state.pending.len() < minimum_pending {
            let now = std::time::Instant::now();
            if now >= deadline {
                return false;
            }
            let wait = self
                .inner
                .pending_changed
                .wait_timeout(state, deadline.saturating_duration_since(now))
                .expect("pending change wait");
            state = wait.0;
            if wait.1.timed_out() {
                return state.pending.len() >= minimum_pending;
            }
        }
        true
    }

    #[cfg(test)]
    pub(crate) fn wait_for_pending_waiter_count_for_test(
        &self,
        minimum_waiters: usize,
        timeout: Duration,
    ) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        let mut state = self.inner.state.lock().expect("state lock");
        while state.waiter_count() < minimum_waiters {
            let now = std::time::Instant::now();
            if now >= deadline {
                return false;
            }
            let wait = self
                .inner
                .pending_changed
                .wait_timeout(state, deadline.saturating_duration_since(now))
                .expect("pending change wait");
            state = wait.0;
            if wait.1.timed_out() {
                return state.waiter_count() >= minimum_waiters;
            }
        }
        true
    }

    #[cfg(test)]
    pub(crate) fn wait_for_pending_agent_for_test(&self, agent: &str, timeout: Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        let mut state = self.inner.state.lock().expect("state lock");
        while !state
            .pending
            .values()
            .any(|pending| pending.request.agent.as_str() == agent)
        {
            let now = std::time::Instant::now();
            if now >= deadline {
                return false;
            }
            let wait = self
                .inner
                .pending_changed
                .wait_timeout(state, deadline.saturating_duration_since(now))
                .expect("pending change wait");
            state = wait.0;
            if wait.1.timed_out() {
                return state
                    .pending
                    .values()
                    .any(|pending| pending.request.agent.as_str() == agent);
            }
        }
        true
    }
}

fn should_emit_reconcile_notification(
    request: &ReconcileRequest,
    import: &atm_core::boundary::InboxIngressImportResponse,
    inbox_ingress: &dyn InboxIngress,
    notification_fingerprints: &Mutex<NotificationFingerprintRegistry>,
) -> Result<bool, AtmError> {
    let mut current_fingerprints = HashSet::new();
    for source in &import.source_files {
        for message in &source.messages {
            let fingerprint = inbox_ingress
                .compute_identity_fingerprint(
                    atm_core::boundary::InboxIngressIdentityFingerprintRequest {
                        message: message.clone(),
                    },
                )?
                .fingerprint;
            let Some(fingerprint) = fingerprint else {
                return Ok(true);
            };
            current_fingerprints.insert(fingerprint);
        }
    }

    let key = ReconcileKey::from_request(request);
    let mut fingerprints = notification_fingerprints.lock().map_err(|_| {
        AtmError::daemon_unavailable("reconcile notification fingerprint state lock poisoned")
    })?;
    let changed = fingerprints
        .entries
        .get(&key)
        .map(|previous| previous != &current_fingerprints)
        .unwrap_or(true);
    let is_new_key = !fingerprints.entries.contains_key(&key);
    if is_new_key && fingerprints.entries.len() >= MAX_RECONCILE_FINGERPRINT_KEYS {
        while let Some(evicted_key) = fingerprints.order.pop_front() {
            if fingerprints.entries.remove(&evicted_key).is_some() {
                tracing::warn!(
                    team = %evicted_key.team,
                    agent = %evicted_key.agent,
                    cap = MAX_RECONCILE_FINGERPRINT_KEYS,
                    "evicted oldest reconcile notification fingerprint entry after reaching the bounded cap"
                );
                break;
            }
        }
    }
    if is_new_key {
        fingerprints.order.push_back(key.clone());
    }
    fingerprints.entries.insert(key, current_fingerprints);
    Ok(changed)
}

fn reconcile_worker_loop(inner: Arc<ReconcileRuntimeInner>) {
    loop {
        let pending = {
            let mut state = match inner.state.lock() {
                Ok(state) => state,
                Err(_) => return,
            };
            while state.pending.is_empty() && !state.shutdown {
                let wait = match inner
                    .wake
                    .wait_timeout(state, DEFAULT_RECONCILE_IDLE_INTERVAL)
                {
                    Ok(wait) => wait,
                    Err(_) => return,
                };
                state = wait.0;
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
            #[cfg(test)]
            inner.pending_changed.notify_all();
            drained
        };

        for pending_request in pending {
            let outcome = match (inner.executor)(&pending_request.request) {
                Ok(result) => ReconcileOutcome::Success(result),
                Err(error) => {
                    tracing::warn!(
                        team = %pending_request.request.team,
                        agent = %pending_request.request.agent,
                        %error,
                        "reconcile runtime executor failed"
                    );
                    ReconcileOutcome::Failure(error.into())
                }
            };
            if let Ok(mut state) = inner.state.lock() {
                for waiter in pending_request.waiters {
                    if state.active_waiters.contains(&waiter) {
                        state.completed.insert(waiter, outcome.clone());
                    }
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
    use super::{MAX_RECONCILE_FINGERPRINT_KEYS, ReconcileRuntime};
    use atm_core::boundary::{
        self, InboxIngress, InboxIngressDiagnosticsRequest, InboxIngressDiagnosticsResponse,
        InboxIngressIdentityFingerprintRequest, InboxIngressIdentityFingerprintResponse,
        InboxIngressImportRequest, InboxIngressImportResponse, NotificationEvent, NotificationSink,
        ReconcileRequest, WatchEventBatch, WatchEventSource, WatchSubscriptionRequest,
    };
    use atm_core::protocol::ReconcileResult;
    use atm_core::roles::ROLE_TEAM_LEAD;
    use atm_core::schema::{AtmMessageId, LegacyMessageId, MessageEnvelope};
    use atm_core::types::IsoTimestamp;
    use chrono::Utc;
    use serde_json::{Map, Value};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Condvar, Mutex, mpsc};
    use std::time::Duration;

    fn unique_home_dir() -> std::path::PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "atm-reconcile-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn request() -> ReconcileRequest {
        ReconcileRequest {
            home_dir: unique_home_dir(),
            team: "test-team".parse().expect("team"),
            agent: "test-agent".parse().expect("agent"),
        }
    }

    fn request_for(agent: &str) -> ReconcileRequest {
        ReconcileRequest {
            home_dir: unique_home_dir(),
            team: "test-team".parse().expect("team"),
            agent: agent.parse().expect("agent"),
        }
    }

    #[test]
    fn reconcile_runtime_coalesces_duplicate_requests() {
        let calls = Arc::new(Mutex::new(0usize));
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
            Duration::from_millis(200),
        );
        runtime.start().expect("start");

        let runtime_a = runtime.clone();
        let runtime_b = runtime.clone();
        let request_a = request();
        let request_b = request_a.clone();
        let first = std::thread::spawn(move || runtime_a.reconcile(request_a).expect("first"));
        assert!(
            runtime.wait_for_pending_count_for_test(1, Duration::from_secs(1)),
            "first reconcile request never entered the pending queue"
        );
        assert!(
            runtime.wait_for_pending_waiter_count_for_test(1, Duration::from_secs(1)),
            "first reconcile waiter never entered the pending queue"
        );
        let second = std::thread::spawn(move || runtime_b.reconcile(request_b).expect("second"));
        assert!(
            runtime.wait_for_pending_waiter_count_for_test(2, Duration::from_secs(1)),
            "duplicate reconcile requests never shared the same pending work item"
        );
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
        assert!(
            runtime.wait_for_pending_count_for_test(1, Duration::from_secs(1)),
            "reconcile request never entered the pending queue"
        );
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
                            let wait = wake
                                .wait_timeout(released, Duration::from_secs(1))
                                .expect("wait release");
                            released = wait.0;
                            assert!(!wait.1.timed_out(), "agent-a release timed out");
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
        assert!(
            runtime.wait_for_pending_agent_for_test("agent-b", Duration::from_secs(1)),
            "agent-b never entered the pending queue before agent-a was released"
        );
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
    struct FakeInboxIngress {
        imports: Arc<Mutex<Vec<InboxIngressImportResponse>>>,
    }

    impl FakeInboxIngress {
        fn new(imports: Vec<InboxIngressImportResponse>) -> Self {
            Self {
                imports: Arc::new(Mutex::new(imports)),
            }
        }
    }

    impl boundary::sealed::Sealed for FakeInboxIngress {}

    impl InboxIngress for FakeInboxIngress {
        fn import_inbox_source(
            &self,
            _request: InboxIngressImportRequest,
        ) -> Result<InboxIngressImportResponse, atm_core::error::AtmError> {
            let mut imports = self.imports.lock().expect("imports");
            if imports.is_empty() {
                return Ok(InboxIngressImportResponse {
                    source_files: Vec::new(),
                });
            }
            Ok(imports.remove(0))
        }

        fn compute_identity_fingerprint(
            &self,
            request: InboxIngressIdentityFingerprintRequest,
        ) -> Result<InboxIngressIdentityFingerprintResponse, atm_core::error::AtmError> {
            Ok(InboxIngressIdentityFingerprintResponse {
                fingerprint: request
                    .message
                    .message_id
                    .map(|message_id| message_id.to_string()),
            })
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
        let ingress = FakeInboxIngress::new(vec![InboxIngressImportResponse {
            source_files: vec![inbox_source_with_message(sample_message(
                "projected message",
            ))],
        }]);
        let runtime = ReconcileRuntime::new(
            Arc::new(FakeWatchSource),
            Arc::new(ingress),
            Arc::new(FakeNotificationSink {
                delivered: Arc::clone(&delivered),
            }),
        );
        runtime.start().expect("start");

        let result = runtime.reconcile(request()).expect("reconcile result");
        assert_eq!(result.observed_paths, 1);
        assert_eq!(result.imported_sources, 1);

        let delivered = delivered.lock().expect("delivered");
        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0].kind, "reconcile_complete");

        runtime.shutdown().expect("shutdown");
    }

    #[test]
    fn reconcile_runtime_suppresses_duplicate_notifications_for_same_message_snapshot() {
        let delivered = Arc::new(Mutex::new(Vec::new()));
        let watch_polls = Arc::new(AtomicU64::new(0));
        let repeated_message = sample_message("same logical message");
        let repeated_source = inbox_source_with_message(repeated_message);
        let runtime = ReconcileRuntime::new(
            Arc::new(CountingWatchSource {
                calls: Arc::clone(&watch_polls),
            }),
            Arc::new(FakeInboxIngress::new(vec![
                InboxIngressImportResponse {
                    source_files: vec![repeated_source.clone()],
                },
                InboxIngressImportResponse {
                    source_files: vec![repeated_source],
                },
            ])),
            Arc::new(FakeNotificationSink {
                delivered: Arc::clone(&delivered),
            }),
        );
        runtime.start().expect("start");

        let request = request();
        let first = runtime.reconcile(request.clone()).expect("first reconcile");
        let second = runtime.reconcile(request).expect("second reconcile");
        assert_eq!(first.imported_sources, 1);
        assert_eq!(second.imported_sources, 1);
        assert_eq!(watch_polls.load(Ordering::Relaxed), 2);

        let delivered = delivered.lock().expect("delivered");
        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0].kind, "reconcile_complete");

        runtime.shutdown().expect("shutdown");
    }

    #[test]
    fn reconcile_runtime_bounds_notification_fingerprint_registry_and_re_emits_after_eviction() {
        let delivered = Arc::new(Mutex::new(Vec::new()));
        let imports = (0..=MAX_RECONCILE_FINGERPRINT_KEYS)
            .map(|index| InboxIngressImportResponse {
                source_files: vec![inbox_source_with_message(sample_message(&format!(
                    "message-{index}"
                )))],
            })
            .collect::<Vec<_>>();
        let runtime = ReconcileRuntime::new(
            Arc::new(FakeWatchSource),
            Arc::new(FakeInboxIngress::new(imports)),
            Arc::new(FakeNotificationSink {
                delivered: Arc::clone(&delivered),
            }),
        );
        runtime.start().expect("start");

        let first_request = request_for("agent-0");
        runtime
            .reconcile(first_request.clone())
            .expect("first reconcile");
        for index in 1..=MAX_RECONCILE_FINGERPRINT_KEYS {
            runtime
                .reconcile(request_for(&format!("agent-{index}")))
                .expect("bounded reconcile");
        }
        runtime
            .reconcile(first_request)
            .expect("reconcile after eviction");

        let fingerprints = runtime
            .inner
            .notification_fingerprints
            .lock()
            .expect("fingerprints");
        assert_eq!(fingerprints.entries.len(), MAX_RECONCILE_FINGERPRINT_KEYS);
        assert_eq!(fingerprints.order.len(), MAX_RECONCILE_FINGERPRINT_KEYS);
        drop(fingerprints);

        let delivered = delivered.lock().expect("delivered");
        assert_eq!(delivered.len(), MAX_RECONCILE_FINGERPRINT_KEYS + 2);

        runtime.shutdown().expect("shutdown");
    }

    #[test]
    fn reconcile_runtime_discards_completed_entries_for_timed_out_waiters() {
        let (started_tx, started_rx) = mpsc::channel();
        let release: Arc<(Mutex<bool>, Condvar)> = Arc::new((Mutex::new(false), Condvar::new()));
        let runtime = ReconcileRuntime::new_for_test(
            Arc::new({
                let release = Arc::clone(&release);
                move |_| {
                    started_tx.send(()).expect("started");
                    let (released, wake) = &*release;
                    let mut released = released.lock().expect("released");
                    while !*released {
                        let wait = wake
                            .wait_timeout(released, Duration::from_secs(1))
                            .expect("wait release");
                        released = wait.0;
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

        let runtime_for_thread = runtime.clone();
        let join = std::thread::spawn(move || runtime_for_thread.reconcile(request()));
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("started");
        let error = join.join().expect("join").expect_err("timeout should fail");
        assert!(error.message.contains("timed out"));

        let (released, wake) = &*release;
        *released.lock().expect("released") = true;
        wake.notify_all();
        assert!(
            runtime.wait_for_pending_count_for_test(0, Duration::from_secs(1)),
            "pending reconcile work did not drain after release"
        );
        assert_eq!(runtime.state_counts_for_test(), (0, 0, 0));

        runtime.shutdown().expect("shutdown");
    }

    #[derive(Clone)]
    struct CountingWatchSource {
        calls: Arc<AtomicU64>,
    }

    impl boundary::sealed::Sealed for CountingWatchSource {}

    impl WatchEventSource for CountingWatchSource {
        fn poll(
            &self,
            _request: WatchSubscriptionRequest,
        ) -> Result<WatchEventBatch, atm_core::error::AtmError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(WatchEventBatch {
                paths: vec![std::env::temp_dir().join("watch.json")],
            })
        }
    }

    fn inbox_source_with_message(
        message: MessageEnvelope,
    ) -> atm_core::boundary::InboxSourceFileRecord {
        atm_core::boundary::InboxSourceFileRecord {
            path: std::env::temp_dir().join("watch.json"),
            messages: vec![message],
        }
    }

    fn sample_message(text: &str) -> MessageEnvelope {
        let atm_message_id = AtmMessageId::new();
        let message_id = LegacyMessageId::from_atm_message_id(atm_message_id);
        let mut atm = Map::new();
        atm.insert(
            "messageId".to_string(),
            Value::String(atm_message_id.to_string()),
        );
        let mut metadata = Map::new();
        metadata.insert("atm".to_string(), Value::Object(atm));
        let mut extra = Map::new();
        extra.insert("metadata".to_string(), Value::Object(metadata));

        MessageEnvelope {
            from: ROLE_TEAM_LEAD.parse().expect("agent"),
            text: text.to_string(),
            timestamp: IsoTimestamp::from_datetime(Utc::now()),
            read: false,
            source_team: Some("test-team".parse().expect("team")),
            summary: Some("summary".to_string()),
            message_id: Some(message_id),
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            stale_at: None,
            task_id: None,
            extra,
        }
    }
}
