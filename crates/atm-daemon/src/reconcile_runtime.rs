use atm_core::boundary::{
    InboxIngress, NotificationSink, ReconcileRequest, ReconcileResult, WatchEventSource,
    WatchSubscriptionRequest,
};
use atm_core::error::AtmError;
use atm_core::protocol::{NotificationEvent, NotificationKind, ProtocolErrorEnvelope};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
#[cfg(test)]
use std::sync::Condvar;
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::worker_support::JoinHandleOwner;
use crate::{DaemonSubsystem, SubsystemObservability};

const DEFAULT_RECONCILE_DEBOUNCE: Duration = Duration::from_millis(25);
const DEFAULT_RECONCILE_COMPLETION_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_RECONCILE_DEBOUNCE_EXTENSIONS: u32 = 8;
const MAX_RECONCILE_FINGERPRINT_KEYS: usize = 1024;
const MAX_RECONCILE_FINGERPRINTS_PER_KEY: usize = 256;
const MAX_RECONCILE_WAITERS: usize = 1024;
#[cfg(not(test))]
const RECONCILE_SHUTDOWN_DEADLINE: Duration = Duration::from_secs(2);
#[cfg(test)]
const RECONCILE_SHUTDOWN_DEADLINE: Duration = Duration::from_millis(100);

#[derive(Clone)]
pub(crate) struct ReconcileRuntime {
    inner: Arc<ReconcileRuntimeInner>,
}

type ReconcileExecutor = Arc<
    dyn Fn(
            &ReconcileRequest,
            &mut NotificationFingerprintRegistry,
        ) -> Result<ReconcileResult, AtmError>
        + Send
        + Sync,
>;

struct ReconcileRuntimeInner {
    state: Mutex<ReconcileState>,
    command_tx: Mutex<Option<mpsc::SyncSender<ReconcileCommand>>>,
    #[cfg(test)]
    pending_changed: Condvar,
    debounce: Duration,
    executor: ReconcileExecutor,
    observability: SubsystemObservability,
    worker: Arc<JoinHandleOwner>,
}

#[derive(Default)]
struct ReconcileState {
    started: bool,
    shutdown: bool,
    worker_state: ReconcileWorkerState,
}

impl ReconcileState {
    #[cfg(test)]
    fn counts(&self) -> (usize, usize, usize) {
        (
            self.worker_state.pending.len(),
            self.worker_state.pending_order.len(),
            0,
        )
    }

    fn waiter_count(&self) -> usize {
        self.worker_state
            .pending
            .values()
            .map(|pending| pending.replies.len())
            .sum()
    }
}

#[derive(Default)]
struct ReconcileWorkerState {
    pending_epoch: u64,
    pending: HashMap<ReconcileKey, PendingReconcile>,
    pending_order: VecDeque<ReconcileKey>,
    notification_fingerprints: NotificationFingerprintRegistry,
}

pub(crate) enum ReconcileCommand {
    Reconcile {
        request: ReconcileRequest,
        reply_tx: mpsc::SyncSender<Result<ReconcileResult, AtmError>>,
    },
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ReconcileKey {
    home_dir: PathBuf,
    team: atm_core::types::TeamName,
    agent: atm_core::types::AgentName,
}

struct PendingReconcile {
    request: ReconcileRequest,
    replies: Vec<mpsc::SyncSender<Result<ReconcileResult, AtmError>>>,
}

#[derive(Clone)]
struct ReconcileReplyErrorSnapshot {
    code: atm_core::error_codes::AtmErrorCode,
    message: String,
    recovery: Option<String>,
}

impl From<&AtmError> for ReconcileReplyErrorSnapshot {
    fn from(error: &AtmError) -> Self {
        Self {
            code: error.code,
            message: error.message.clone(),
            recovery: error.recovery.clone(),
        }
    }
}

impl ReconcileReplyErrorSnapshot {
    fn into_error(self) -> AtmError {
        ProtocolErrorEnvelope {
            code: self.code,
            message: self.message,
            recovery: self.recovery,
        }
        .into_atm_error()
    }
}

#[derive(Default)]
pub(crate) struct NotificationFingerprintRegistry {
    // Each key retains the distinct fingerprint strings already emitted for one
    // reconcile target so duplicate inbox projections do not re-notify until a
    // genuinely new fingerprint appears.
    entries: HashMap<ReconcileKey, HashSet<String>>,
    order: VecDeque<ReconcileKey>,
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
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn new(
        watch_source: Arc<dyn WatchEventSource + Send + Sync>,
        inbox_ingress: Arc<dyn InboxIngress + Send + Sync>,
        notification_sink: Arc<dyn NotificationSink + Send + Sync>,
    ) -> Self {
        Self::new_with_observability(
            watch_source,
            inbox_ingress,
            notification_sink,
            SubsystemObservability::disabled(DaemonSubsystem::ReconcileRuntime),
        )
    }

    pub(crate) fn new_with_observability(
        watch_source: Arc<dyn WatchEventSource + Send + Sync>,
        inbox_ingress: Arc<dyn InboxIngress + Send + Sync>,
        notification_sink: Arc<dyn NotificationSink + Send + Sync>,
        observability: SubsystemObservability,
    ) -> Self {
        Self::new_with_executor(
            Arc::new(move |request, notification_fingerprints| {
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
                    notification_fingerprints,
                )? {
                    notification_sink.deliver(NotificationEvent {
                        kind: NotificationKind::ReconcileComplete,
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
            observability,
        )
    }

    fn new_with_executor(
        executor: ReconcileExecutor,
        debounce: Duration,
        observability: SubsystemObservability,
    ) -> Self {
        Self {
            inner: Arc::new(ReconcileRuntimeInner {
                state: Mutex::new(ReconcileState::default()),
                command_tx: Mutex::new(None),
                #[cfg(test)]
                pending_changed: Condvar::new(),
                debounce,
                executor,
                observability,
                worker: Arc::new(JoinHandleOwner::default()),
            }),
        }
    }

    pub(crate) fn start(&self) -> Result<(), AtmError> {
        let (command_tx, command_rx) = mpsc::sync_channel(MAX_RECONCILE_WAITERS);
        let mut state = self.inner.state.lock().map_err(|_| {
            AtmError::daemon_unavailable("reconcile runtime state lock poisoned").with_recovery(
                "Restart the daemon; reconcile lifecycle state can no longer be trusted.",
            )
        })?;
        if state.started {
            return Ok(());
        }
        state.started = true;
        state.shutdown = false;
        state.worker_state = ReconcileWorkerState::default();
        drop(state);
        self.install_command_tx(command_tx)?;

        let inner = Arc::clone(&self.inner);
        let handle = thread::Builder::new()
            .name("atm-daemon-reconcile".to_string())
            .spawn(move || reconcile_worker_loop(inner, command_rx))
            .map_err(|source| {
                let _ = self.take_command_tx();
                self.inner.observability.emit_or_warn(
                    "start",
                    "failed",
                    "failed to spawn reconcile runtime worker",
                );
                AtmError::daemon_unavailable("failed to spawn reconcile runtime worker")
                    .with_source(source)
            })?;
        let mut state = match self.inner.state.lock() {
            Ok(state) => state,
            Err(_) => {
                let _ = self.take_command_tx();
                let _ = handle.join();
                return Err(
                    AtmError::daemon_unavailable("reconcile runtime state lock poisoned")
                        .with_recovery(
                            "Restart the daemon; reconcile lifecycle state can no longer be trusted.",
                        ),
                );
            }
        };
        self.inner.worker.install(handle).inspect_err(|_| {
            let _ = self.take_command_tx();
            state.shutdown = true;
        })?;
        self.inner
            .observability
            .emit_or_warn("start", "ok", "reconcile runtime worker started");
        Ok(())
    }

    pub(crate) fn shutdown(&self) -> Result<(), AtmError> {
        let command_tx = {
            let mut state = self.inner.state.lock().map_err(|_| {
                AtmError::daemon_unavailable("reconcile runtime state lock poisoned").with_recovery(
                    "Restart the daemon; reconcile lifecycle state can no longer be trusted.",
                )
            })?;
            state.shutdown = true;
            self.take_command_tx()?
        };
        if let Some(command_tx) = command_tx {
            let _ = command_tx.send(ReconcileCommand::Shutdown);
        }
        let handle = self.inner.worker.take()?;
        if let Some(handle) = handle {
            let worker_thread_id = handle.thread().id();
            let (result_rx, join_helper) = spawn_reconcile_join_helper(handle)?;
            self.complete_reconcile_shutdown(result_rx, join_helper, worker_thread_id)?;
        }
        if let Ok(mut state) = self.inner.state.lock() {
            state.started = false;
            state.worker_state = ReconcileWorkerState::default();
            #[cfg(test)]
            self.inner.pending_changed.notify_all();
        }
        Ok(())
    }

    fn complete_reconcile_shutdown(
        &self,
        result_rx: mpsc::Receiver<thread::Result<()>>,
        join_helper: JoinHandle<()>,
        worker_thread_id: thread::ThreadId,
    ) -> Result<(), AtmError> {
        match wait_for_reconcile_join(result_rx) {
            Ok(Ok(())) => {
                let _ = join_helper.join();
                self.inner.observability.emit_or_warn(
                    "shutdown",
                    "ok",
                    "reconcile runtime worker shut down cleanly",
                );
            }
            Ok(Err(_)) => {
                let _ = join_helper.join();
                self.inner.observability.emit_or_warn(
                    "shutdown",
                    "failed",
                    "reconcile runtime worker panicked during shutdown",
                );
                return Err(AtmError::daemon_unavailable(
                    "reconcile runtime worker panicked during shutdown",
                )
                .with_recovery(
                    "Restart atm-daemon; the reconcile background lane crashed while shutting down.",
                ));
            }
            Err(ReconcileJoinStatus::Timeout) => {
                drop(join_helper);
                tracing::warn!(
                    subsystem = "reconcile",
                    action = "shutdown_detach",
                    outcome = "deadline_exceeded",
                    thread_id = ?worker_thread_id,
                    timeout_ms = RECONCILE_SHUTDOWN_DEADLINE.as_millis(),
                    "reconcile runtime worker exceeded shutdown deadline; detaching join helper"
                );
                self.inner.observability.emit_or_warn(
                    "shutdown",
                    "degraded",
                    "reconcile runtime worker exceeded its shutdown deadline",
                );
                return Err(AtmError::daemon_unavailable(
                    "reconcile runtime worker exceeded the bounded shutdown deadline",
                )
                .with_recovery(
                    "Restart atm-daemon after the reconcile background lane becomes responsive again.",
                ));
            }
            Err(ReconcileJoinStatus::Disconnected) => {
                let _ = join_helper.join();
                self.inner.observability.emit_or_warn(
                    "shutdown",
                    "failed",
                    "reconcile runtime join helper disconnected during shutdown",
                );
                tracing::warn!(
                    subsystem = "reconcile",
                    action = "shutdown_join_helper",
                    outcome = "disconnected",
                    thread_id = ?worker_thread_id,
                    "reconcile runtime worker join helper exited before reporting shutdown status"
                );
                return Err(AtmError::daemon_unavailable(
                    "reconcile runtime join helper disconnected during shutdown",
                )
                .with_recovery(
                    "Restart atm-daemon; the reconcile background lane did not report bounded shutdown status cleanly.",
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn reconcile(&self, request: ReconcileRequest) -> Result<ReconcileResult, AtmError> {
        let request_team = request.team.clone();
        let request_agent = request.agent.clone();
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.enqueue_reconcile_command(
            ReconcileCommand::Reconcile { request, reply_tx },
            &request_team,
            &request_agent,
        )?;
        self.wait_for_reconcile_result(reply_rx, &request_team, &request_agent)
    }

    fn enqueue_reconcile_command(
        &self,
        command: ReconcileCommand,
        request_team: &atm_core::types::TeamName,
        request_agent: &atm_core::types::AgentName,
    ) -> Result<(), AtmError> {
        let command_tx = {
            let state = self.inner.state.lock().map_err(|_| {
                AtmError::daemon_unavailable("reconcile runtime state lock poisoned").with_recovery(
                    "Restart the daemon; reconcile lifecycle state can no longer be trusted.",
                )
            })?;
            self.validate_reconcile_runtime_state(&state, request_team, request_agent)?;
            let slot = self.inner.command_tx.lock().map_err(|_| {
                AtmError::daemon_unavailable("reconcile runtime command sender lock poisoned")
                    .with_recovery(
                        "Restart atm-daemon; reconcile command-lane ownership can no longer be trusted.",
                    )
            })?;
            slot.clone().ok_or_else(|| {
                self.reconcile_unavailable_error(
                    "rejected",
                    "reconcile runtime command lane is unavailable",
                    request_team,
                    request_agent,
                )
            })?
        };
        command_tx.send(command).map_err(|_| {
            self.reconcile_unavailable_error(
                "degraded",
                "reconcile runtime command lane is unavailable",
                request_team,
                request_agent,
            )
        })?;
        Ok(())
    }

    fn install_command_tx(
        &self,
        command_tx: mpsc::SyncSender<ReconcileCommand>,
    ) -> Result<(), AtmError> {
        let mut slot = self.inner.command_tx.lock().map_err(|_| {
            AtmError::daemon_unavailable("reconcile runtime command sender lock poisoned")
                .with_recovery(
                    "Restart atm-daemon; reconcile command-lane ownership can no longer be trusted.",
                )
        })?;
        *slot = Some(command_tx);
        Ok(())
    }

    fn take_command_tx(&self) -> Result<Option<mpsc::SyncSender<ReconcileCommand>>, AtmError> {
        self.inner.take_command_tx()
    }

    fn validate_reconcile_runtime_state(
        &self,
        state: &ReconcileState,
        request_team: &atm_core::types::TeamName,
        request_agent: &atm_core::types::AgentName,
    ) -> Result<(), AtmError> {
        if !state.started {
            return Err(self.reconcile_unavailable_error(
                "rejected",
                "reconcile runtime is unavailable before daemon startup",
                request_team,
                request_agent,
            ));
        }
        if state.shutdown {
            return Err(self.reconcile_unavailable_error(
                "rejected",
                "reconcile runtime is unavailable during daemon shutdown",
                request_team,
                request_agent,
            ));
        }
        if state.waiter_count() >= MAX_RECONCILE_WAITERS {
            return Err(self
                .reconcile_unavailable_error(
                    "rejected",
                    "reconcile runtime hit its concurrent waiter capacity",
                    request_team,
                    request_agent,
                )
                .with_recovery(
                    "Reduce concurrent reconcile waiters or retry after earlier reconcile requests complete.",
                ));
        }
        Ok(())
    }

    fn wait_for_reconcile_result(
        &self,
        reply_rx: mpsc::Receiver<Result<ReconcileResult, AtmError>>,
        request_team: &atm_core::types::TeamName,
        request_agent: &atm_core::types::AgentName,
    ) -> Result<ReconcileResult, AtmError> {
        match reply_rx.recv_timeout(DEFAULT_RECONCILE_COMPLETION_TIMEOUT) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => Err(self.reconcile_unavailable_error(
                "degraded",
                "reconcile runtime timed out waiting for background completion",
                request_team,
                request_agent,
            )),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(self.reconcile_unavailable_error(
                "degraded",
                "reconcile runtime shut down before completion",
                request_team,
                request_agent,
            )),
        }
    }

    fn reconcile_unavailable_error(
        &self,
        outcome: &'static str,
        message: &'static str,
        request_team: &atm_core::types::TeamName,
        request_agent: &atm_core::types::AgentName,
    ) -> AtmError {
        let event = self
            .inner
            .observability
            .event("reconcile", outcome, message)
            .with_team(request_team.clone())
            .with_agent(request_agent.clone());
        self.inner.observability.emit_event_or_warn(event);
        AtmError::daemon_unavailable(message).with_recovery(
            "Wait for the daemon reconcile runtime to return to a serving state, then retry the reconcile request.",
        )
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(executor: ReconcileExecutor, debounce: Duration) -> Self {
        Self::new_with_executor(
            executor,
            debounce,
            SubsystemObservability::disabled(DaemonSubsystem::ReconcileRuntime),
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
        while state.worker_state.pending.len() < minimum_pending {
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
                return state.worker_state.pending.len() >= minimum_pending;
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
    pub(crate) fn notification_fingerprint_registry_counts_for_test(&self) -> (usize, usize) {
        let state = self.inner.state.lock().expect("state lock");
        (
            state.worker_state.notification_fingerprints.entries.len(),
            state.worker_state.notification_fingerprints.order.len(),
        )
    }

    #[cfg(test)]
    pub(crate) fn notification_fingerprint_count_for_key_for_test(
        &self,
        request: &ReconcileRequest,
    ) -> usize {
        let state = self.inner.state.lock().expect("state lock");
        state
            .worker_state
            .notification_fingerprints
            .entries
            .get(&ReconcileKey::from_request(request))
            .expect("entry")
            .len()
    }
}

fn should_emit_reconcile_notification(
    request: &ReconcileRequest,
    import: &atm_core::boundary::InboxIngressImportResponse,
    inbox_ingress: &dyn InboxIngress,
    notification_fingerprints: &mut NotificationFingerprintRegistry,
) -> Result<bool, AtmError> {
    let mut current_fingerprints = HashSet::new();
    for source in &import.source_files {
        for message in &source.messages {
            let fingerprint = inbox_ingress
                .compute_identity_fingerprint(
                    atm_core::boundary::InboxIngressIdentityFingerprintRequest {
                        message: message.clone(),
                    },
                )
                .fingerprint;
            let Some(fingerprint) = fingerprint else {
                return Ok(true);
            };
            current_fingerprints.insert(fingerprint);
        }
    }

    let key = ReconcileKey::from_request(request);
    if current_fingerprints.len() > MAX_RECONCILE_FINGERPRINTS_PER_KEY {
        let mut ordered = current_fingerprints.drain().collect::<Vec<_>>();
        ordered.sort();
        let dropped = ordered
            .len()
            .saturating_sub(MAX_RECONCILE_FINGERPRINTS_PER_KEY);
        ordered.truncate(MAX_RECONCILE_FINGERPRINTS_PER_KEY);
        current_fingerprints.extend(ordered);
        tracing::warn!(
            subsystem = "reconcile",
            action = "fingerprint_truncate",
            outcome = "cap_exceeded",
            team = %key.team,
            agent = %key.agent,
            retained = MAX_RECONCILE_FINGERPRINTS_PER_KEY,
            dropped,
            "reconcile notification fingerprint set exceeded the per-key bounded cap; truncating deterministically"
        );
    }
    let changed = notification_fingerprints
        .entries
        .get(&key)
        .map(|previous| previous != &current_fingerprints)
        .unwrap_or(true);
    let is_new_key = !notification_fingerprints.entries.contains_key(&key);
    if is_new_key && notification_fingerprints.entries.len() >= MAX_RECONCILE_FINGERPRINT_KEYS {
        while let Some(evicted_key) = notification_fingerprints.order.pop_front() {
            if notification_fingerprints
                .entries
                .remove(&evicted_key)
                .is_some()
            {
                tracing::warn!(
                    subsystem = "reconcile",
                    action = "fingerprint_evict",
                    outcome = "cap_exceeded",
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
        notification_fingerprints.order.push_back(key.clone());
    }
    notification_fingerprints
        .entries
        .insert(key, current_fingerprints);
    Ok(changed)
}

fn reconcile_worker_loop(
    inner: Arc<ReconcileRuntimeInner>,
    command_rx: mpsc::Receiver<ReconcileCommand>,
) {
    loop {
        let command = match command_rx.recv() {
            Ok(command) => command,
            Err(_) => {
                clear_pending_reconcile_state(inner.as_ref());
                return;
            }
        };
        if !handle_reconcile_command(inner.as_ref(), command) {
            clear_pending_reconcile_state(inner.as_ref());
            return;
        }
        if !debounce_reconcile_command_batch(inner.as_ref(), &command_rx) {
            clear_pending_reconcile_state(inner.as_ref());
            return;
        }

        let (pending, mut notification_fingerprints) =
            match take_pending_reconcile_batch(inner.as_ref()) {
                Some(pending) => pending,
                None => return,
            };

        for pending_request in pending {
            let outcome =
                match (inner.executor)(&pending_request.request, &mut notification_fingerprints) {
                    Ok(result) => Ok(result),
                    Err(error) => {
                        tracing::warn!(
                            subsystem = "reconcile",
                            action = "executor",
                            outcome = "failed",
                            team = %pending_request.request.team,
                            agent = %pending_request.request.agent,
                            %error,
                            "reconcile runtime executor failed"
                        );
                        Err(error)
                    }
                };
            if record_reconcile_outcome(pending_request, outcome).is_none() {
                return;
            }
        }

        if restore_notification_fingerprints(inner.as_ref(), notification_fingerprints).is_none() {
            return;
        }
    }
}

enum ReconcileJoinStatus {
    Timeout,
    Disconnected,
}

fn spawn_reconcile_join_helper(
    handle: JoinHandle<()>,
) -> Result<(mpsc::Receiver<std::thread::Result<()>>, JoinHandle<()>), AtmError> {
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let join_helper = thread::Builder::new()
        .name("atm-daemon-reconcile-join".to_string())
        .spawn(move || {
            let _ = result_tx.send(handle.join());
        })
        .map_err(|source| {
            AtmError::daemon_unavailable(
                "failed to spawn reconcile runtime join helper during shutdown",
            )
            .with_recovery(
                "Restart atm-daemon; reconcile shutdown could not create its bounded join helper.",
            )
            .with_source(source)
        })?;
    Ok((result_rx, join_helper))
}

fn wait_for_reconcile_join(
    result_rx: mpsc::Receiver<std::thread::Result<()>>,
) -> Result<std::thread::Result<()>, ReconcileJoinStatus> {
    result_rx
        .recv_timeout(RECONCILE_SHUTDOWN_DEADLINE)
        .map_err(|error| match error {
            mpsc::RecvTimeoutError::Timeout => ReconcileJoinStatus::Timeout,
            mpsc::RecvTimeoutError::Disconnected => ReconcileJoinStatus::Disconnected,
        })
}

impl ReconcileRuntimeInner {
    fn take_command_tx(&self) -> Result<Option<mpsc::SyncSender<ReconcileCommand>>, AtmError> {
        let mut slot = self.command_tx.lock().map_err(|_| {
            AtmError::daemon_unavailable("reconcile runtime command sender lock poisoned")
                .with_recovery(
                    "Restart atm-daemon; reconcile command-lane ownership can no longer be trusted.",
                )
        })?;
        Ok(slot.take())
    }
}

fn handle_reconcile_command(inner: &ReconcileRuntimeInner, command: ReconcileCommand) -> bool {
    match command {
        ReconcileCommand::Reconcile { request, reply_tx } => {
            let mut state = match inner.state.lock() {
                Ok(state) => state,
                Err(_) => return false,
            };
            state.worker_state.pending_epoch = state.worker_state.pending_epoch.saturating_add(1);
            let key = ReconcileKey::from_request(&request);
            if let Some(pending) = state.worker_state.pending.get_mut(&key) {
                pending.request = request;
                pending.replies.push(reply_tx);
            } else {
                state.worker_state.pending_order.push_back(key.clone());
                state.worker_state.pending.insert(
                    key,
                    PendingReconcile {
                        request,
                        replies: vec![reply_tx],
                    },
                );
            }
            #[cfg(test)]
            inner.pending_changed.notify_all();
            true
        }
        ReconcileCommand::Shutdown => false,
    }
}

fn debounce_reconcile_command_batch(
    inner: &ReconcileRuntimeInner,
    command_rx: &mpsc::Receiver<ReconcileCommand>,
) -> bool {
    let mut debounce_extensions = 0u32;
    loop {
        match command_rx.recv_timeout(inner.debounce) {
            Ok(command) => {
                if !handle_reconcile_command(inner, command) {
                    return false;
                }
                debounce_extensions = debounce_extensions.saturating_add(1);
                if debounce_extensions >= MAX_RECONCILE_DEBOUNCE_EXTENSIONS {
                    while let Ok(command) = command_rx.try_recv() {
                        if !handle_reconcile_command(inner, command) {
                            return false;
                        }
                    }
                    match command_rx.recv_timeout(inner.debounce) {
                        Ok(command) => {
                            if !handle_reconcile_command(inner, command) {
                                return false;
                            }
                            while let Ok(command) = command_rx.try_recv() {
                                if !handle_reconcile_command(inner, command) {
                                    return false;
                                }
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => return false,
                    }
                    return true;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => return true,
            Err(mpsc::RecvTimeoutError::Disconnected) => return false,
        }
    }
}

fn clear_pending_reconcile_state(inner: &ReconcileRuntimeInner) {
    if let Ok(mut state) = inner.state.lock() {
        state.worker_state = ReconcileWorkerState::default();
        #[cfg(test)]
        inner.pending_changed.notify_all();
    }
}

fn take_pending_reconcile_batch(
    inner: &ReconcileRuntimeInner,
) -> Option<(Vec<PendingReconcile>, NotificationFingerprintRegistry)> {
    let mut state = inner.state.lock().ok()?;
    let drained = drain_pending_reconcile_batch(&mut state);
    let notification_fingerprints =
        std::mem::take(&mut state.worker_state.notification_fingerprints);
    #[cfg(test)]
    inner.pending_changed.notify_all();
    Some((drained, notification_fingerprints))
}

fn restore_notification_fingerprints(
    inner: &ReconcileRuntimeInner,
    notification_fingerprints: NotificationFingerprintRegistry,
) -> Option<()> {
    let mut state = inner.state.lock().ok()?;
    state.worker_state.notification_fingerprints = notification_fingerprints;
    Some(())
}

fn drain_pending_reconcile_batch(state: &mut ReconcileState) -> Vec<PendingReconcile> {
    let pending_order = std::mem::take(&mut state.worker_state.pending_order);
    let mut drained = Vec::with_capacity(pending_order.len());
    for key in pending_order {
        if let Some(pending) = state.worker_state.pending.remove(&key) {
            drained.push(pending);
        }
    }
    drained
}

fn record_reconcile_outcome(
    pending_request: PendingReconcile,
    outcome: Result<ReconcileResult, AtmError>,
) -> Option<()> {
    for reply in pending_request.replies {
        let _ = reply.send(clone_reconcile_outcome(&outcome));
    }
    Some(())
}

fn clone_reconcile_outcome(
    outcome: &Result<ReconcileResult, AtmError>,
) -> Result<ReconcileResult, AtmError> {
    match outcome {
        Ok(result) => Ok(result.clone()),
        Err(error) => Err(ReconcileReplyErrorSnapshot::from(error).into_error()),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_RECONCILE_DEBOUNCE_EXTENSIONS, MAX_RECONCILE_FINGERPRINT_KEYS,
        MAX_RECONCILE_FINGERPRINTS_PER_KEY, ReconcileRuntime,
    };
    use atm_core::boundary::{
        self, InboxIngress, InboxIngressDiagnosticsRequest, InboxIngressDiagnosticsResponse,
        InboxIngressIdentityFingerprintRequest, InboxIngressIdentityFingerprintResponse,
        InboxIngressImportRequest, InboxIngressImportResponse, NotificationEvent, NotificationSink,
        ReconcileRequest, WatchEventBatch, WatchEventSource, WatchSubscriptionRequest,
    };
    use atm_core::protocol::ReconcileResult;
    use atm_core::roles::ROLE_TEAM_LEAD;
    use atm_core::schema::{AtmMessageId, MessageEnvelope};
    use atm_core::types::IsoTimestamp;
    use chrono::Utc;
    use serde_json::Map;
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
    fn reconcile_runtime_actor_coalesces_identical_requests_into_one_worker_run() {
        let calls = Arc::new(Mutex::new(0usize));
        let runtime = ReconcileRuntime::new_for_test(
            Arc::new({
                let calls = Arc::clone(&calls);
                move |_, _| {
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
    fn reconcile_runtime_actor_fans_one_result_to_all_waiters_for_a_key() {
        let calls = Arc::new(Mutex::new(0usize));
        let runtime = ReconcileRuntime::new_for_test(
            Arc::new({
                let calls = Arc::clone(&calls);
                move |_, _| {
                    *calls.lock().expect("calls") += 1;
                    Ok(ReconcileResult {
                        observed_paths: 7,
                        imported_sources: 3,
                    })
                }
            }),
            Duration::from_millis(200),
        );
        runtime.start().expect("start");

        let request = request();
        let runtime_a = runtime.clone();
        let runtime_b = runtime.clone();
        let request_a = request.clone();
        let request_b = request;
        let first = std::thread::spawn(move || runtime_a.reconcile(request_a).expect("first"));
        assert!(
            runtime.wait_for_pending_count_for_test(1, Duration::from_secs(1)),
            "first reconcile request never entered the pending queue"
        );
        let second = std::thread::spawn(move || runtime_b.reconcile(request_b).expect("second"));
        assert!(
            runtime.wait_for_pending_waiter_count_for_test(2, Duration::from_secs(1)),
            "duplicate reconcile requests never shared one worker-owned pending reply fanout"
        );

        let first_result = first.join().expect("join");
        let second_result = second.join().expect("join");
        assert_eq!(first_result, second_result);
        assert_eq!(first_result.observed_paths, 7);
        assert_eq!(first_result.imported_sources, 3);
        assert_eq!(*calls.lock().expect("calls"), 1);
        runtime.shutdown().expect("shutdown");
    }

    #[test]
    fn reconcile_runtime_actor_preserves_bounded_debounce_extensions() {
        let calls = Arc::new(Mutex::new(0usize));
        let runtime = ReconcileRuntime::new_for_test(
            Arc::new({
                let calls = Arc::clone(&calls);
                move |_, _| {
                    *calls.lock().expect("calls") += 1;
                    Ok(ReconcileResult {
                        observed_paths: 1,
                        imported_sources: 1,
                    })
                }
            }),
            Duration::from_millis(25),
        );
        runtime.start().expect("start");

        let request = request();
        let started = std::time::Instant::now();
        let mut joins = Vec::new();
        joins.push({
            let runtime = runtime.clone();
            let request = request.clone();
            std::thread::spawn(move || runtime.reconcile(request).expect("first"))
        });
        assert!(
            runtime.wait_for_pending_count_for_test(1, Duration::from_secs(1)),
            "first reconcile request never entered the pending queue"
        );
        for _ in 0..=MAX_RECONCILE_DEBOUNCE_EXTENSIONS {
            let runtime = runtime.clone();
            let request = request.clone();
            joins.push(std::thread::spawn(move || {
                runtime.reconcile(request).expect("duplicate")
            }));
        }

        for join in joins {
            let result = join.join().expect("join");
            assert_eq!(result.observed_paths, 1);
            assert_eq!(result.imported_sources, 1);
        }
        assert_eq!(*calls.lock().expect("calls"), 1);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "bounded debounce extensions never converged to one worker run"
        );
        runtime.shutdown().expect("shutdown");
    }

    #[test]
    fn reconcile_runtime_cleans_up_pending_waiters_during_shutdown() {
        let runtime = ReconcileRuntime::new_for_test(
            Arc::new(|_, _| {
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
            Arc::new(|_, _| {
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
                move |request, _| {
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
        ) -> InboxIngressIdentityFingerprintResponse {
            InboxIngressIdentityFingerprintResponse {
                fingerprint: request
                    .message
                    .message_id
                    .map(|message_id| message_id.to_string()),
            }
        }

        fn report_diagnostics(
            &self,
            _request: InboxIngressDiagnosticsRequest,
        ) -> InboxIngressDiagnosticsResponse {
            InboxIngressDiagnosticsResponse {
                duplicate_message_ids: 0,
                messages_without_ids: 0,
            }
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
        assert_eq!(
            delivered[0].kind,
            atm_core::protocol::NotificationKind::ReconcileComplete
        );

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
        assert_eq!(
            delivered[0].kind,
            atm_core::protocol::NotificationKind::ReconcileComplete
        );

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

        let (entry_count, order_count) =
            runtime.notification_fingerprint_registry_counts_for_test();
        assert_eq!(entry_count, MAX_RECONCILE_FINGERPRINT_KEYS);
        assert_eq!(order_count, MAX_RECONCILE_FINGERPRINT_KEYS);

        let delivered = delivered.lock().expect("delivered");
        assert_eq!(delivered.len(), MAX_RECONCILE_FINGERPRINT_KEYS + 2);

        runtime.shutdown().expect("shutdown");
    }

    #[test]
    fn reconcile_runtime_bounds_per_key_fingerprint_sets() {
        let runtime = ReconcileRuntime::new(
            Arc::new(FakeWatchSource),
            Arc::new(FakeInboxIngress::new(vec![InboxIngressImportResponse {
                source_files: (0..=MAX_RECONCILE_FINGERPRINTS_PER_KEY)
                    .map(|index| {
                        inbox_source_with_message(sample_message(&format!("message-{index}")))
                    })
                    .collect(),
            }])),
            Arc::new(FakeNotificationSink {
                delivered: Arc::new(Mutex::new(Vec::new())),
            }),
        );
        runtime.start().expect("start");
        let request = request();
        runtime.reconcile(request.clone()).expect("reconcile");

        assert_eq!(
            runtime.notification_fingerprint_count_for_key_for_test(&request),
            MAX_RECONCILE_FINGERPRINTS_PER_KEY
        );

        runtime.shutdown().expect("shutdown");
    }

    #[test]
    fn reconcile_runtime_discards_completed_entries_for_timed_out_waiters() {
        let (started_tx, started_rx) = mpsc::channel();
        let release: Arc<(Mutex<bool>, Condvar)> = Arc::new((Mutex::new(false), Condvar::new()));
        let runtime = ReconcileRuntime::new_for_test(
            Arc::new({
                let release = Arc::clone(&release);
                move |_, _| {
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
        let message_id = AtmMessageId::new();

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
            expires_at: None,
            task_id: None,
            extra: Map::new(),
        }
    }
}
