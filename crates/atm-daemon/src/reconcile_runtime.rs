mod notification_fingerprints;
mod projection_write_journal;

use arc_swap::ArcSwap;
use atm_core::boundary::{
    InboxIngress, NotificationSink, ReconcileRequest, ReconcileResult, ReplaySource, RosterStore,
    RosterStoreReplaceRosterRequest, WatchEventBatch, WatchEventSource, WatchSubscriptionRequest,
};
use atm_core::error::AtmError;
use atm_core::protocol::{NotificationEvent, NotificationKind, ProtocolErrorEnvelope};
use std::collections::{HashMap, HashSet, VecDeque};
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::worker_support::{JoinHandleOwner, reap_retained_join_helpers, retain_join_helper};
use crate::{DaemonSubsystem, SubsystemObservability};
use notification_fingerprints::{
    NotificationFingerprint, NotificationFingerprintRegistry, should_emit_reconcile_notification,
};
#[cfg(test)]
use projection_write_journal::remember_projected_config_write;
use projection_write_journal::{
    ProjectionWriteJournal, config_document_digest, consume_projected_config_write,
};

const DEFAULT_RECONCILE_QUEUE_CAPACITY: usize = 64;
const DEFAULT_RECONCILE_DEBOUNCE: Duration = Duration::from_millis(25);
const DEFAULT_RECONCILE_COMPLETION_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_RECONCILE_IDLE_INTERVAL: Duration = Duration::from_millis(50);
const MAX_RECONCILE_DEBOUNCE_EXTENSIONS: u32 = 8;
const MAX_RECONCILE_FINGERPRINT_KEYS: usize = 1024;
const MAX_RECONCILE_FINGERPRINTS_PER_KEY: usize = 256;
const MAX_RECONCILE_WAITERS: usize = 1024;
const MAX_PROJECTION_WRITE_JOURNAL_ENTRIES: usize = 256;
const RECONCILE_SHUTDOWN_DEADLINE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReconcileWorkerLiveness {
    Live,
    Degraded,
    Stopped,
}

#[derive(Debug, Clone)]
pub(crate) struct ReconcileRuntimeStatus {
    started: bool,
    shutdown_requested: bool,
    shutdown_started_at: Option<Instant>,
    degraded_message: Option<Arc<str>>,
    worker_liveness: ReconcileWorkerLiveness,
}

impl Default for ReconcileRuntimeStatus {
    fn default() -> Self {
        Self {
            started: false,
            shutdown_requested: false,
            shutdown_started_at: None,
            degraded_message: None,
            worker_liveness: ReconcileWorkerLiveness::Stopped,
        }
    }
}

#[derive(Clone)]
pub(crate) struct ReconcileRuntime {
    inner: Arc<ReconcileRuntimeInner>,
}

struct ReconcileRuntimeInner {
    command_tx: OnceLock<SyncSender<ReconcileCommand>>,
    status: Arc<ArcSwap<ReconcileRuntimeStatus>>,
    worker: Arc<JoinHandleOwner>,
    #[cfg_attr(not(test), allow(dead_code))]
    projection_write_journal: ProjectionWriteJournal,
    queue_capacity: usize,
    debounce: Duration,
    executor: ReconcileExecutor,
    notification_sink: Arc<dyn NotificationSink + Send + Sync>,
    shutdown_deadline: Duration,
    observability: SubsystemObservability,
    start_claimed: AtomicBool,
}

type ReconcileExecutor =
    Arc<dyn Fn(&ReconcileRequest) -> Result<ReconcileExecution, AtmError> + Send + Sync>;
type ReconcileReplyRx = Receiver<Result<ReconcileResult, AtmError>>;
type ReconcileDispatch = (
    ReconcileReplyRx,
    atm_core::types::TeamName,
    atm_core::types::AgentName,
);

pub(crate) struct ReconcileExecution {
    result: ReconcileResult,
    current_fingerprints: Option<HashSet<NotificationFingerprint>>,
}

pub(crate) enum ReconcileCommand {
    Reconcile {
        request: ReconcileRequest,
        reply_tx: SyncSender<Result<ReconcileResult, AtmError>>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ReconcileKey {
    home_dir: PathBuf,
    team: atm_core::types::TeamName,
    agent: atm_core::types::AgentName,
}

struct PendingReconcile {
    request: ReconcileRequest,
    replies: Vec<SyncSender<Result<ReconcileResult, AtmError>>>,
}

#[derive(Default)]
struct ReconcileWorkerState {
    pending_epoch: u64,
    pending: HashMap<ReconcileKey, PendingReconcile>,
    pending_order: VecDeque<ReconcileKey>,
    notification_fingerprints: NotificationFingerprintRegistry,
}

impl ReconcileWorkerState {
    fn waiter_count(&self) -> usize {
        self.pending
            .values()
            .map(|pending| pending.replies.len())
            .sum()
    }
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
        roster_store: Arc<dyn RosterStore + Send + Sync>,
        notification_sink: Arc<dyn NotificationSink + Send + Sync>,
    ) -> Self {
        Self::new_with_observability(
            watch_source,
            inbox_ingress,
            roster_store,
            notification_sink,
            SubsystemObservability::disabled(DaemonSubsystem::ReconcileRuntime),
        )
    }

    pub(crate) fn new_with_observability(
        watch_source: Arc<dyn WatchEventSource + Send + Sync>,
        inbox_ingress: Arc<dyn InboxIngress + Send + Sync>,
        roster_store: Arc<dyn RosterStore + Send + Sync>,
        notification_sink: Arc<dyn NotificationSink + Send + Sync>,
        observability: SubsystemObservability,
    ) -> Self {
        let projection_write_journal: ProjectionWriteJournal = Arc::new(Mutex::new(HashMap::new()));
        let projection_write_journal_for_executor = Arc::clone(&projection_write_journal);
        Self::new_with_executor_and_sink(
            Arc::new(move |request| {
                let batch = watch_source.poll(WatchSubscriptionRequest {
                    home_dir: request.home_dir.clone(),
                    team: request.team.clone(),
                    agent: request.agent.clone(),
                })?;
                ingest_claude_team_config_from_watch_batch(
                    request,
                    &batch,
                    roster_store.as_ref(),
                    &projection_write_journal_for_executor,
                )?;
                let import = inbox_ingress.import_inbox_source(
                    atm_core::boundary::InboxIngressImportRequest {
                        home_dir: request.home_dir.clone(),
                        team: request.team.clone(),
                        agent: request.agent.clone(),
                    },
                )?;
                Ok(ReconcileExecution {
                    result: ReconcileResult {
                        observed_paths: batch.paths.len(),
                        imported_sources: import.source_files.len(),
                    },
                    current_fingerprints: compute_reconcile_notification_fingerprints(
                        &import,
                        inbox_ingress.as_ref(),
                    ),
                })
            }),
            projection_write_journal,
            notification_sink,
            DEFAULT_RECONCILE_QUEUE_CAPACITY,
            DEFAULT_RECONCILE_DEBOUNCE,
            RECONCILE_SHUTDOWN_DEADLINE,
            observability,
        )
    }

    fn new_with_executor_and_sink(
        executor: ReconcileExecutor,
        projection_write_journal: ProjectionWriteJournal,
        notification_sink: Arc<dyn NotificationSink + Send + Sync>,
        queue_capacity: usize,
        debounce: Duration,
        shutdown_deadline: Duration,
        observability: SubsystemObservability,
    ) -> Self {
        assert!(
            queue_capacity >= 1,
            "reconcile runtime queue_capacity must be at least 1"
        );
        assert!(
            !shutdown_deadline.is_zero(),
            "reconcile runtime shutdown_deadline must be greater than zero"
        );
        Self {
            inner: Arc::new(ReconcileRuntimeInner {
                command_tx: OnceLock::new(),
                status: Arc::new(ArcSwap::from_pointee(ReconcileRuntimeStatus::default())),
                worker: Arc::new(JoinHandleOwner::default()),
                projection_write_journal,
                queue_capacity,
                debounce,
                executor,
                notification_sink,
                shutdown_deadline,
                observability,
                start_claimed: AtomicBool::new(false),
            }),
        }
    }

    pub(crate) fn start(&self) -> Result<(), AtmError> {
        reap_retained_join_helpers();
        if self.inner.start_claimed.swap(true, Ordering::AcqRel) {
            return Ok(());
        }

        let (command_tx, command_rx) = mpsc::sync_channel(self.inner.queue_capacity);

        let inner = Arc::clone(&self.inner);
        let handle = thread::Builder::new()
            .name("atm-daemon-reconcile".to_string())
            .spawn(move || reconcile_worker_loop(inner, command_rx))
            .map_err(|source| {
                self.inner.start_claimed.store(false, Ordering::Release);
                self.inner.observability.emit_or_warn(
                    "start",
                    "failed",
                    "failed to spawn reconcile runtime worker",
                );
                AtmError::daemon_unavailable("failed to spawn reconcile runtime worker")
                    .with_source(source)
            })?;

        if self.inner.command_tx.set(command_tx).is_err() {
            self.inner.start_claimed.store(false, Ordering::Release);
            return Err(AtmError::daemon_unavailable(
                "reconcile runtime command sender was already initialized during startup",
            )
            .with_recovery(
                "Restart atm-daemon; the reconcile worker lane already claimed its bounded command-channel handoff.",
            ));
        }
        self.inner.worker.install(handle).inspect_err(|_| {
            self.inner.start_claimed.store(false, Ordering::Release);
        })?;
        self.inner.mark_started();
        tracing::info!(
            subsystem = "reconcile",
            action = "worker_start",
            outcome = "configured",
            queue_capacity = self.inner.queue_capacity,
            debounce_ms = self.inner.debounce.as_millis(),
            "reconcile runtime worker configuration accepted"
        );
        self.inner
            .observability
            .emit_or_warn("start", "ok", "reconcile runtime worker started");
        Ok(())
    }

    pub(crate) fn shutdown(&self) -> Result<(), AtmError> {
        let Some(handle) = self.take_worker_for_shutdown()? else {
            self.inner.mark_shutdown_requested();
            self.inner.mark_worker_stopped();
            return Ok(());
        };
        self.await_worker_shutdown(handle)
    }

    #[cfg(test)]
    pub(crate) fn worker_liveness(&self) -> ReconcileWorkerLiveness {
        self.inner.status_snapshot().worker_liveness
    }

    pub(crate) fn reconcile(&self, request: ReconcileRequest) -> Result<ReconcileResult, AtmError> {
        let (reply_rx, request_team, request_agent) = self.dispatch_reconcile_command(request)?;
        self.wait_for_reconcile_result(reply_rx, &request_team, &request_agent)
    }

    fn dispatch_reconcile_command(
        &self,
        request: ReconcileRequest,
    ) -> Result<ReconcileDispatch, AtmError> {
        let request_team_owned = request.team.clone();
        let request_agent_owned = request.agent.clone();
        let request_team = &request_team_owned;
        let request_agent = &request_agent_owned;
        let status = self.inner.status_snapshot();
        if !status.started {
            return Err(self.reconcile_unavailable_error(
                "rejected",
                "reconcile runtime is unavailable before daemon startup",
                request_team,
                request_agent,
            ));
        }
        if status.shutdown_requested {
            return Err(self.reconcile_unavailable_error(
                "rejected",
                "reconcile runtime is unavailable during daemon shutdown",
                request_team,
                request_agent,
            ));
        }
        if let Some(message) = &status.degraded_message {
            return Err(
                AtmError::daemon_unavailable(message.as_ref()).with_recovery(
                    "Restart atm-daemon; the reconcile runtime worker lane is degraded.",
                ),
            );
        }

        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        let command_tx = self.inner.command_tx.get().ok_or_else(|| {
            self.reconcile_unavailable_error(
                "rejected",
                "reconcile runtime command channel is unavailable before daemon startup",
                request_team,
                request_agent,
            )
        })?;
        match command_tx.try_send(ReconcileCommand::Reconcile { request, reply_tx }) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                self.inner.observability.emit_or_warn(
                    "reconcile",
                    "rejected",
                    "reconcile runtime command queue is full",
                );
                return Err(
                    AtmError::daemon_unavailable(
                        "reconcile runtime command queue is full; requests are backpressured",
                    )
                    .with_recovery(
                        "Retry after earlier reconcile requests complete or reduce concurrent reconcile load.",
                    ),
                );
            }
            Err(TrySendError::Disconnected(_)) => {
                return Err(self.handle_send_error(
                    self.inner.status_snapshot(),
                    &request_team_owned,
                    &request_agent_owned,
                ));
            }
        }
        Ok((reply_rx, request_team_owned, request_agent_owned))
    }

    fn wait_for_reconcile_result(
        &self,
        reply_rx: Receiver<Result<ReconcileResult, AtmError>>,
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

    fn handle_send_error(
        &self,
        latest_status: Arc<ReconcileRuntimeStatus>,
        request_team: &atm_core::types::TeamName,
        request_agent: &atm_core::types::AgentName,
    ) -> AtmError {
        if let Some(message) = &latest_status.degraded_message {
            return AtmError::daemon_unavailable(message.as_ref()).with_recovery(
                "Restart atm-daemon; the reconcile runtime worker lane is degraded.",
            );
        }
        if latest_status.shutdown_requested {
            return self.reconcile_unavailable_error(
                "rejected",
                "reconcile runtime is unavailable during daemon shutdown",
                request_team,
                request_agent,
            );
        }
        if latest_status.worker_liveness == ReconcileWorkerLiveness::Stopped {
            return self.reconcile_unavailable_error(
                "rejected",
                "reconcile runtime worker stopped receiving commands",
                request_team,
                request_agent,
            );
        }
        AtmError::daemon_unavailable("reconcile runtime command channel is unavailable")
            .with_recovery(
                "Restart atm-daemon; the reconcile worker lane is no longer receiving commands.",
            )
    }

    fn take_worker_for_shutdown(&self) -> Result<Option<JoinHandle<()>>, AtmError> {
        self.inner.mark_shutdown_requested();
        self.inner.worker.take()
    }

    fn await_worker_shutdown(&self, handle: JoinHandle<()>) -> Result<(), AtmError> {
        let worker_thread_id = handle.thread().id();
        let (join_helper, result_rx) = spawn_reconcile_join_helper(handle)?;
        match result_rx.recv_timeout(self.inner.shutdown_deadline) {
            Ok(Ok(())) => self.handle_shutdown_joined(join_helper),
            Ok(Err(_)) => self.handle_shutdown_panic(join_helper),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.handle_shutdown_timeout(join_helper, worker_thread_id)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.handle_shutdown_disconnect(join_helper, worker_thread_id)
            }
        }
    }

    fn handle_shutdown_joined(&self, join_helper: JoinHandle<()>) -> Result<(), AtmError> {
        let _ = join_helper.join();
        self.inner.mark_worker_stopped();
        self.inner.observability.emit_or_warn(
            "shutdown",
            "ok",
            "reconcile runtime worker shut down cleanly",
        );
        Ok(())
    }

    fn handle_shutdown_panic(&self, join_helper: JoinHandle<()>) -> Result<(), AtmError> {
        let _ = join_helper.join();
        self.inner
            .mark_worker_degraded("reconcile runtime worker panicked during shutdown");
        self.inner.observability.emit_or_warn(
            "shutdown",
            "failed",
            "reconcile runtime worker panicked during shutdown",
        );
        Err(AtmError::daemon_unavailable(
            "reconcile runtime worker panicked during shutdown",
        )
        .with_recovery(
            "Restart atm-daemon; the reconcile background lane crashed while shutting down.",
        ))
    }

    fn handle_shutdown_timeout(
        &self,
        join_helper: JoinHandle<()>,
        worker_thread_id: thread::ThreadId,
    ) -> Result<(), AtmError> {
        let timeout_elapsed = self
            .inner
            .status_snapshot()
            .shutdown_started_at
            .map(|started_at| started_at.elapsed())
            .unwrap_or(self.inner.shutdown_deadline);
        self.inner.observability.emit_or_warn(
            "shutdown",
            "degraded",
            "reconcile runtime worker exceeded its shutdown deadline",
        );
        tracing::warn!(
            subsystem = "reconcile",
            action = "shutdown_retain",
            outcome = "deadline_exceeded",
            thread_id = ?worker_thread_id,
            timeout_ms = timeout_elapsed.as_millis(),
            "reconcile runtime worker exceeded shutdown deadline; retaining join helper for later cleanup"
        );
        retain_join_helper(
            "reconcile_runtime_worker",
            join_helper,
            self.inner.shutdown_deadline,
        );
        Err(AtmError::daemon_unavailable(
            "reconcile runtime worker exceeded the bounded shutdown deadline",
        )
        .with_recovery(
            "Restart atm-daemon after the reconcile background lane becomes responsive again.",
        ))
    }

    fn handle_shutdown_disconnect(
        &self,
        join_helper: JoinHandle<()>,
        worker_thread_id: thread::ThreadId,
    ) -> Result<(), AtmError> {
        let _ = join_helper.join();
        self.inner
            .mark_worker_degraded("reconcile runtime join helper disconnected during shutdown");
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
            "reconcile runtime join helper exited before reporting shutdown status"
        );
        Err(AtmError::daemon_unavailable(
            "reconcile runtime join helper disconnected during shutdown",
        )
        .with_recovery(
            "Restart atm-daemon; the reconcile background lane did not report bounded shutdown status cleanly.",
        ))
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(executor: ReconcileExecutor, debounce: Duration) -> Self {
        Self::new_with_executor_and_sink(
            executor,
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(TestNotificationSink),
            DEFAULT_RECONCILE_QUEUE_CAPACITY,
            debounce,
            RECONCILE_SHUTDOWN_DEADLINE,
            SubsystemObservability::disabled(DaemonSubsystem::ReconcileRuntime),
        )
    }

    #[cfg(test)]
    pub(crate) fn record_projected_config_write_for_test(
        &self,
        path: &Path,
    ) -> Result<(), AtmError> {
        let digest = config_document_digest(path)?;
        remember_projected_config_write(&self.inner.projection_write_journal, path, digest)
    }
}

fn ingest_claude_team_config_from_watch_batch(
    request: &ReconcileRequest,
    batch: &WatchEventBatch,
    roster_store: &dyn RosterStore,
    projection_write_journal: &ProjectionWriteJournal,
) -> Result<(), AtmError> {
    let team_dir = atm_core::home::team_dir_from_home(&request.home_dir, &request.team).map_err(
        |error| {
            AtmError::daemon_unavailable(format!(
                "reconcile runtime could not resolve team {} from {} for Claude config ingest",
                request.team,
                request.home_dir.display()
            ))
            .with_recovery(
                "Verify the ATM home directory and Claude team layout before retrying reconcile ingest.",
            )
            .with_source(error)
        },
    )?;
    let config_path = team_dir.join("config.json");
    if !batch.paths.iter().any(|path| path == &config_path) || !config_path.is_file() {
        return Ok(());
    }

    let digest = config_document_digest(&config_path)?;
    if consume_projected_config_write(projection_write_journal, &config_path, digest)? {
        return Ok(());
    }

    let team_config = atm_core::load_claude_team_config_document(&team_dir).map_err(|error| {
        AtmError::daemon_unavailable(format!(
            "reconcile runtime could not load Claude team config from {}",
            config_path.display()
        ))
        .with_recovery(
            "Repair the Claude team config document before retrying watcher-owned roster ingest.",
        )
        .with_source(error)
    })?;
    let members = team_config
        .members
        .into_iter()
        .map(|member| {
            atm_core::boundary::RosterMemberRecord::from_claude_code_member(
                request.team.clone(),
                member,
            )
        })
        .collect::<Vec<_>>();
    roster_store
        .replace_roster(RosterStoreReplaceRosterRequest {
            team: request.team.clone(),
            members,
            source: Some(ReplaySource::new("watcher-config-ingress").expect("replay source")),
        })
        .map_err(|error| {
            AtmError::daemon_unavailable(format!(
                "reconcile runtime could not replace canonical ATM roster state from {}",
                config_path.display()
            ))
            .with_recovery(
                "Repair the ATM roster store or Claude config document before retrying watcher-owned ingest.",
            )
            .with_source(error)
        })?;
    Ok(())
}

impl ReconcileRuntimeInner {
    fn status_snapshot(&self) -> Arc<ReconcileRuntimeStatus> {
        self.status.load_full()
    }

    fn publish_status(
        &self,
        mutate: impl FnOnce(&ReconcileRuntimeStatus) -> ReconcileRuntimeStatus,
    ) {
        let current = self.status.load_full();
        self.status.store(Arc::new(mutate(current.as_ref())));
    }

    fn mark_started(&self) {
        self.status.store(Arc::new(ReconcileRuntimeStatus {
            started: true,
            shutdown_requested: false,
            shutdown_started_at: None,
            degraded_message: None,
            worker_liveness: ReconcileWorkerLiveness::Live,
        }));
    }

    fn mark_shutdown_requested(&self) {
        self.publish_status(|status| ReconcileRuntimeStatus {
            shutdown_requested: true,
            shutdown_started_at: status.shutdown_started_at.or(Some(Instant::now())),
            ..status.clone()
        });
    }

    fn mark_worker_stopped(&self) {
        self.publish_status(|status| ReconcileRuntimeStatus {
            worker_liveness: ReconcileWorkerLiveness::Stopped,
            ..status.clone()
        });
    }

    fn mark_worker_degraded(&self, message: &'static str) {
        self.publish_status(|status| ReconcileRuntimeStatus {
            degraded_message: Some(Arc::<str>::from(message)),
            worker_liveness: ReconcileWorkerLiveness::Degraded,
            ..status.clone()
        });
    }
}

fn compute_reconcile_notification_fingerprints(
    import: &atm_core::boundary::InboxIngressImportResponse,
    inbox_ingress: &dyn InboxIngress,
) -> Option<HashSet<NotificationFingerprint>> {
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
            let fingerprint = fingerprint?;
            current_fingerprints.insert(NotificationFingerprint::new(fingerprint)?);
        }
    }
    Some(current_fingerprints)
}

fn reconcile_worker_loop(
    inner: Arc<ReconcileRuntimeInner>,
    command_rx: Receiver<ReconcileCommand>,
) {
    let mut worker_state = ReconcileWorkerState::default();
    loop {
        if inner.status_snapshot().shutdown_requested {
            inner.mark_worker_stopped();
            return;
        }

        let first_command = match command_rx.recv_timeout(DEFAULT_RECONCILE_IDLE_INTERVAL) {
            Ok(command) => command,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                inner.mark_worker_stopped();
                return;
            }
        };
        enqueue_pending_reconcile_command(inner.as_ref(), &mut worker_state, first_command);

        let pending = match debounce_pending_reconcile_batch(
            inner.as_ref(),
            &command_rx,
            &mut worker_state,
        ) {
            Some(pending) => pending,
            None => {
                inner.mark_worker_stopped();
                return;
            }
        };

        let mut pending_iter = pending.into_iter();
        while let Some(pending_request) = pending_iter.next() {
            if inner.status_snapshot().shutdown_requested {
                interrupt_pending_reconcile_batch(pending_request, pending_iter);
                inner.mark_worker_stopped();
                return;
            }
            let outcome = execute_reconcile_request(
                inner.as_ref(),
                &mut worker_state,
                &pending_request.request,
            );
            record_reconcile_outcome(pending_request, outcome);
        }
    }
}

fn interrupt_pending_reconcile_batch(
    pending_request: PendingReconcile,
    remaining: impl IntoIterator<Item = PendingReconcile>,
) {
    record_reconcile_outcome(pending_request, Err(reconcile_shutdown_interrupted_error()));
    for pending_request in remaining {
        record_reconcile_outcome(pending_request, Err(reconcile_shutdown_interrupted_error()));
    }
}

fn reconcile_shutdown_interrupted_error() -> AtmError {
    AtmError::daemon_unavailable("reconcile runtime is unavailable during daemon shutdown")
        .with_recovery(
            "Wait for the daemon reconcile runtime to finish shutting down, then retry the reconcile request.",
        )
}

fn enqueue_pending_reconcile_command(
    inner: &ReconcileRuntimeInner,
    worker_state: &mut ReconcileWorkerState,
    command: ReconcileCommand,
) {
    match command {
        ReconcileCommand::Reconcile { request, reply_tx } => {
            if worker_state.waiter_count() >= MAX_RECONCILE_WAITERS {
                let _ = reply_tx.send(Err(AtmError::daemon_unavailable(
                        "reconcile runtime hit its concurrent waiter capacity",
                    )
                    .with_recovery(
                        "Reduce concurrent reconcile waiters or retry after earlier reconcile requests complete.",
                    )));
                inner.observability.emit_or_warn(
                    "reconcile",
                    "rejected",
                    "reconcile runtime hit its concurrent waiter capacity",
                );
                return;
            }
            worker_state.pending_epoch = worker_state.pending_epoch.saturating_add(1);
            let key = ReconcileKey::from_request(&request);
            if let Some(pending) = worker_state.pending.get_mut(&key) {
                pending.request = request;
                pending.replies.push(reply_tx);
            } else {
                worker_state.pending_order.push_back(key.clone());
                worker_state.pending.insert(
                    key,
                    PendingReconcile {
                        request,
                        replies: vec![reply_tx],
                    },
                );
            }
        }
    }
}

fn debounce_pending_reconcile_batch(
    inner: &ReconcileRuntimeInner,
    command_rx: &Receiver<ReconcileCommand>,
    worker_state: &mut ReconcileWorkerState,
) -> Option<Vec<PendingReconcile>> {
    let mut debounce_epoch = worker_state.pending_epoch;
    let mut debounce_extensions = 0u32;
    loop {
        if inner.status_snapshot().shutdown_requested {
            let mut pending = drain_pending_reconcile_batch(worker_state).into_iter();
            if let Some(first_pending) = pending.next() {
                interrupt_pending_reconcile_batch(first_pending, pending);
            }
            return None;
        }
        match command_rx.recv_timeout(inner.debounce) {
            Ok(command) => {
                enqueue_pending_reconcile_command(inner, worker_state, command);
                if worker_state.pending_epoch != debounce_epoch {
                    debounce_epoch = worker_state.pending_epoch;
                    debounce_extensions = debounce_extensions.saturating_add(1);
                    if debounce_extensions >= MAX_RECONCILE_DEBOUNCE_EXTENSIONS {
                        break;
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => break,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Some(drain_pending_reconcile_batch(worker_state))
}

fn drain_pending_reconcile_batch(worker_state: &mut ReconcileWorkerState) -> Vec<PendingReconcile> {
    let pending_order = std::mem::take(&mut worker_state.pending_order);
    let mut drained = Vec::with_capacity(pending_order.len());
    for key in pending_order {
        if let Some(pending) = worker_state.pending.remove(&key) {
            drained.push(pending);
        }
    }
    drained
}

fn execute_reconcile_request(
    inner: &ReconcileRuntimeInner,
    worker_state: &mut ReconcileWorkerState,
    request: &ReconcileRequest,
) -> Result<ReconcileResult, AtmError> {
    // `Y.22` accepts one actor-owned request lane here; the caller-facing
    // bounded command-channel handoff has already ended before execution.
    let execution = (inner.executor)(request)?;
    if should_emit_reconcile_notification(worker_state, request, execution.current_fingerprints) {
        inner.notification_sink.deliver(NotificationEvent {
            kind: NotificationKind::ReconcileComplete,
            detail: format!(
                "observed_paths={} imported_sources={}",
                execution.result.observed_paths, execution.result.imported_sources
            ),
            team: Some(request.team.clone()),
            agent: Some(request.agent.clone()),
        })?;
    }
    Ok(execution.result)
}

fn record_reconcile_outcome(
    pending_request: PendingReconcile,
    outcome: Result<ReconcileResult, AtmError>,
) {
    for reply in pending_request.replies {
        let _ = reply.send(clone_reconcile_outcome(&outcome));
    }
}

fn clone_reconcile_outcome(
    outcome: &Result<ReconcileResult, AtmError>,
) -> Result<ReconcileResult, AtmError> {
    match outcome {
        Ok(result) => Ok(result.clone()),
        Err(error) => Err(ReconcileReplyErrorSnapshot::from(error).into_error()),
    }
}

fn spawn_reconcile_join_helper(
    handle: JoinHandle<()>,
) -> Result<(JoinHandle<()>, Receiver<std::thread::Result<()>>), AtmError> {
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let join_helper = thread::Builder::new()
        .name("atm-daemon-reconcile-join".to_string())
        .spawn(move || {
            let _ = result_tx.send(handle.join());
            #[cfg(test)]
            crate::worker_support::signal_retained_join_helper_exit_for_test();
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
    Ok((join_helper, result_rx))
}

#[cfg(test)]
struct TestNotificationSink;

#[cfg(test)]
impl atm_core::boundary::sealed::Sealed for TestNotificationSink {}

#[cfg(test)]
impl NotificationSink for TestNotificationSink {
    fn deliver(&self, _event: NotificationEvent) -> Result<(), AtmError> {
        Ok(())
    }
}

#[cfg(test)]
#[path = "reconcile_runtime_tests.rs"]
mod tests;
