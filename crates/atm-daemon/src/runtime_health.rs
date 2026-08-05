use crate::AtmHomeDir;
use crate::daemon_runtime_observability::{
    DaemonRuntimeObservability, DaemonSubsystem, SubsystemObservability,
};
use arc_swap::ArcSwapOption;
use atm_core::{
    LocalServiceRuntime, RequestDeadline, RequestEnvelope, ResponseEnvelope, boundary,
    clear::clear_mail_with_runtime,
    doctor::{self, DaemonRuntimeDoctorReport, DoctorExecutionContext, DoctorQuery, DoctorReport},
    error::{AtmError, AtmErrorCode},
    list::list_mail,
    process::process_is_alive,
    protocol::{
        CompatibilityVerdict, ReleaseVersion, RuntimeLivenessState, RuntimeStatusSnapshot,
        SendResponseEnvelope, TeamMemberHeartbeatRequest, TeamMemberHeartbeatResponse,
    },
    read::{peek_mail_with_runtime, read_mail_with_runtime},
    send::{PreparedWrite, WriteOutcome, WriteRequest},
};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;
mod admission_view;
use admission_view::AdmissionRuntimeView;
mod doctor_reporting;
mod post_commit_work;
use crate::peer_http_listener::{PeerHttpBindConfig, PeerHttpListenerSet, PeerHttpRuntimeConfig};
#[cfg(test)]
pub(crate) use crate::runtime_status_cache::MAX_STATUS_CACHE_ENTRIES;
pub(crate) use crate::runtime_status_cache::RuntimeStatusCache;
use crate::runtime_status_cache::{build_runtime_status_cache_state, runtime_status_finding};
use atm_runtime::RuntimeAssembly;
use atm_storage::RosterStore;
use atm_storage::{MessageStore, OutboundMessageQuery, PeerConfigStore, PeerResendCacheSetting};
use doctor_reporting::{daemon_observability_finding, finalize_doctor_report};
use post_commit_work::{LocalPostCommitWorkQueue, PostCommitWorkKey, PostCommitWorkQueue};
mod peer_delivery_router;
mod peer_resend_scheduler;
mod request_router;
use peer_resend_scheduler::PeerResendScheduler;
// The retained observability flush is best-effort during shutdown; Phase S records this bounded
// 2-second deadline as an accepted production exception in the anti-flake contract docs.
const SHUTDOWN_OBSERVABILITY_FLUSH_DEADLINE: Duration = Duration::from_secs(2);
const MAX_SHUTDOWN_FINALIZER_THREADS: usize = 16;
// Timed-out shutdown workers are retained in one process-wide registry instead of being dropped
// orphaned; this must be static because the bounded finalizer helper can outlive any one
// dispatcher instance after timeout, while orderly shutdown and serial tests still need one place
// to recover and join those retained workers later.
static SHUTDOWN_FINALIZER_THREADS: std::sync::Mutex<Vec<std::thread::JoinHandle<()>>> =
    std::sync::Mutex::new(Vec::new());
pub(crate) struct DaemonRequestDispatcher {
    // Invariant: this is the validated ATM_HOME root for the running daemon,
    // not an arbitrary workspace path.
    home_dir: AtmHomeDir,
    observability: Arc<dyn DaemonRuntimeObservability>,
    runtime_health_observability: SubsystemObservability,
    status_cache: RuntimeStatusCache,
    service_runtime: LocalServiceRuntime,
    admission_runtime_view: AdmissionRuntimeView,
    peer_http_runtime_config: Arc<ArcSwapOption<PeerHttpRuntimeConfig>>,
    /// `None` is the explicit cache-disabled direct path. A present scheduler
    /// owns only transient endpoint state and is rebuilt on config reload.
    peer_resend_scheduler: Arc<ArcSwapOption<PeerResendScheduler>>,
    message_store: Arc<dyn MessageStore + Send + Sync>,
    outbound_message_query: Arc<dyn OutboundMessageQuery + Send + Sync>,
    doctor_ports: atm_core::doctor::RuntimeDoctorPorts,
    roster_store: Option<Arc<dyn RosterStore + Send + Sync>>,
    peer_config_store: Arc<dyn PeerConfigStore + Send + Sync>,
    post_commit_signals: Arc<LocalPostCommitWorkQueue>,
    post_commit_work_queue: Arc<dyn PostCommitWorkQueue>,
}

impl std::fmt::Debug for DaemonRequestDispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonRequestDispatcher")
            .field("home_dir", &self.home_dir.as_path())
            .field("status_cache", &self.status_cache)
            .field("service_runtime", &self.service_runtime)
            .field("doctor_ports", &self.doctor_ports)
            .field("roster_store_present", &self.roster_store.is_some())
            .field("peer_config_store", &"dyn PeerConfigStore")
            .field("message_store", &"dyn MessageStore")
            .finish()
    }
}

impl DaemonRequestDispatcher {
    pub(crate) fn next_peer_resend_due(&self) -> Option<std::time::Instant> {
        let scheduler = self.peer_resend_scheduler.load_full()?;
        match scheduler.next_due() {
            Ok(due_at) => due_at,
            Err(error) => {
                tracing::error!(
                    subsystem = "peer_resend",
                    action = "next_due",
                    outcome = "state_lock_failed",
                    %error,
                    "peer resend schedule is unavailable because its state lock is poisoned"
                );
                None
            }
        }
    }

    pub(crate) fn poll_due_peer_resends(&self, now: std::time::Instant) -> Result<(), AtmError> {
        let Some(scheduler) = self.peer_resend_scheduler.load_full() else {
            return Ok(());
        };
        // A due callback is best-effort recovery for already durable mail.
        // Its bounded delivery/query failure re-arms the endpoint and must not
        // terminate the daemon's local admission loop.
        if let Err(error) = scheduler.poll_due_peer_resends(now) {
            tracing::warn!(
                subsystem = "peer_resend",
                action = "due_callback",
                outcome = "rearmed",
                %error,
                "peer resend callback failed; retained immutable writes remain queued"
            );
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn drain_shutdown_finalizer_threads_for_test() {
        let mut deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let handle = with_shutdown_finalizer_registry(|handles| {
                handles
                    .iter()
                    .position(std::thread::JoinHandle::is_finished)
                    .map(|index| handles.swap_remove(index))
            });
            if let Some(handle) = handle {
                handle.join().expect("join shutdown finalizer thread");
                deadline = std::time::Instant::now() + Duration::from_secs(5);
                continue;
            }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let has_pending = with_shutdown_finalizer_registry(|handles| !handles.is_empty());
            if !has_pending {
                break;
            }
            assert!(
                !remaining.is_zero(),
                "shutdown finalizer thread failed to join within 5s"
            );
            std::thread::park_timeout(remaining.min(Duration::from_millis(10)));
        }
        let still_pending = with_shutdown_finalizer_registry(|handles| handles.len());
        assert_eq!(
            still_pending, 0,
            "shutdown finalizer join helper left retained worker handles behind"
        );
    }

    fn spawn_shutdown_step(
        label: &'static str,
        step: impl FnOnce() -> Result<(), AtmError> + Send + 'static,
    ) -> Result<std::thread::JoinHandle<()>, AtmError> {
        std::thread::Builder::new()
            .name(format!("shutdown-finalizer-{label}"))
            .spawn(move || {
                step().unwrap_or_else(|error| {
                    tracing::warn!(
                        subsystem = "runtime_health",
                        action = "shutdown_finalizer_step",
                        outcome = "failed",
                        %error,
                        step = label,
                        "daemon shutdown finalizer step failed; restart atm-daemon and inspect the retained observability log before retrying shutdown-sensitive work"
                    );
                });
            })
            .map_err(|source| {
                AtmError::daemon_unavailable(format!(
                    "failed to spawn daemon shutdown finalizer step `{label}`: {source}"
                ))
            })
    }

    fn retain_shutdown_step(
        label: &'static str,
        shutdown_handle: std::thread::JoinHandle<()>,
        deadline: Duration,
    ) {
        let retained = with_shutdown_finalizer_registry(|handles| {
            if handles.len() < MAX_SHUTDOWN_FINALIZER_THREADS {
                handles.push(shutdown_handle);
                true
            } else {
                false
            }
        });
        if !retained {
            tracing::warn!(
                subsystem = "runtime_health",
                action = "shutdown_retain",
                outcome = "capacity_exceeded",
                step = label,
                cap = MAX_SHUTDOWN_FINALIZER_THREADS,
                "shutdown finalizer thread cap reached; dropping retained worker handle"
            );
        } else {
            tracing::warn!(
                subsystem = "runtime_health",
                action = "shutdown_retain",
                outcome = "deadline_exceeded",
                step = label,
                timeout_ms = deadline.as_millis(),
                "daemon shutdown finalizer step exceeded its deadline; worker retained for later join"
            );
        }
    }

    fn spawn_shutdown_join_helper(
        label: &'static str,
        shutdown_handle: std::thread::JoinHandle<()>,
    ) -> Result<
        (
            std::sync::mpsc::Receiver<std::thread::Result<()>>,
            std::thread::JoinHandle<()>,
        ),
        AtmError,
    > {
        let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
        let join_helper = std::thread::Builder::new()
            .name(format!("shutdown-finalizer-join-{label}"))
            .spawn(move || {
                let _ = result_tx.send(shutdown_handle.join());
            })
            .map_err(|source| {
                AtmError::daemon_unavailable(format!(
                    "failed to spawn daemon shutdown finalizer join helper `{label}`: {source}"
                ))
            })?;
        Ok((result_rx, join_helper))
    }

    fn run_bounded_shutdown_step(
        label: &'static str,
        deadline: Duration,
        step: impl FnOnce() -> Result<(), AtmError> + Send + 'static,
    ) {
        let shutdown_handle = match Self::spawn_shutdown_step(label, step) {
            Ok(handle) => handle,
            Err(error) => {
                tracing::warn!(
                    subsystem = "runtime_health",
                    action = "shutdown_spawn",
                    outcome = "failed",
                    %error,
                    step = label,
                    "daemon shutdown finalizer step could not start; restart atm-daemon because shutdown cleanup could not be scheduled"
                );
                return;
            }
        };
        let (result_rx, join_helper) = match Self::spawn_shutdown_join_helper(
            label,
            shutdown_handle,
        ) {
            Ok(helper) => helper,
            Err(error) => {
                tracing::warn!(
                    subsystem = "runtime_health",
                    action = "shutdown_join_helper_spawn",
                    outcome = "failed",
                    %error,
                    step = label,
                    "daemon shutdown finalizer step could not start its bounded join helper; restart atm-daemon because shutdown cleanup could not be monitored"
                );
                return;
            }
        };
        Self::handle_bounded_shutdown_result(label, deadline, result_rx, join_helper);
    }

    fn handle_bounded_shutdown_result(
        label: &'static str,
        deadline: Duration,
        result_rx: std::sync::mpsc::Receiver<std::thread::Result<()>>,
        join_helper: JoinHandle<()>,
    ) {
        match result_rx.recv_timeout(deadline) {
            Ok(join_result) => {
                Self::finalize_bounded_shutdown_result(label, join_result, join_helper)
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // SQLite WAL checkpoint timeout is the highest-risk caller here: a checkpoint can
                // outlive the bounded shutdown window, but retaining the worker JoinHandle is still
                // safer than dropping it orphaned because tests and orderly process teardown can
                // join it later once the blocking storage step finishes.
                Self::retain_shutdown_step(label, join_helper, deadline);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                Self::report_bounded_shutdown_disconnect(label, join_helper);
            }
        }
    }

    fn finalize_bounded_shutdown_result(
        label: &'static str,
        join_result: std::thread::Result<()>,
        join_helper: JoinHandle<()>,
    ) {
        if join_helper.join().is_err() {
            Self::warn_shutdown_join_helper(label, "panic");
        }
        if join_result.is_err() {
            tracing::warn!(
                subsystem = "runtime_health",
                action = "shutdown_finalize",
                outcome = "panic",
                step = label,
                "daemon shutdown finalizer step panicked before reporting completion; restart atm-daemon and inspect the retained observability log for the failing shutdown step"
            );
        }
    }

    fn report_bounded_shutdown_disconnect(label: &'static str, join_helper: JoinHandle<()>) {
        if join_helper.join().is_err() {
            Self::warn_shutdown_join_helper(label, "panic");
        } else {
            Self::warn_shutdown_join_helper(label, "disconnected");
        }
    }

    fn warn_shutdown_join_helper(label: &'static str, outcome: &'static str) {
        tracing::warn!(
            subsystem = "runtime_health",
            action = "shutdown_join_helper",
            outcome,
            step = label,
            "daemon shutdown finalizer join helper failed before reporting completion"
        );
    }

    pub(crate) fn new(
        // Must be the validated ATM home dir for this daemon runtime.
        home_dir: AtmHomeDir,
        status_cache: RuntimeStatusCache,
        observability: Arc<dyn DaemonRuntimeObservability>,
        runtime_assembly: RuntimeAssembly,
    ) -> Result<Self, AtmError> {
        let runtime_health_observability =
            SubsystemObservability::new(DaemonSubsystem::RuntimeHealth, Arc::clone(&observability));
        let roster_store = runtime_assembly.shared_roster_store_arc();
        let message_store = runtime_assembly.message_store_arc();
        let outbound_message_query = runtime_assembly.outbound_message_query();
        let peer_config_store = runtime_assembly.peer_config_store();
        let peer_directory = peer_config_store.peer_directory()?;
        let peer_http_runtime_config = peer_http_runtime_config(peer_config_store.as_ref())?;
        let peer_resend_scheduler = peer_resend_scheduler(
            peer_config_store.peer_resend_cache_setting()?,
            peer_http_runtime_config.as_ref(),
            peer_directory.clone(),
            Arc::clone(&outbound_message_query),
            Arc::clone(&message_store),
        )?;
        match build_runtime_status_cache_state(None, roster_store.as_ref()) {
            Ok(state) => status_cache.publish_state(state),
            Err(error) => {
                tracing::warn!(
                    subsystem = "runtime_health",
                    action = "sqlite_cache_hydration",
                    outcome = "degraded",
                    %error,
                    "failed to hydrate runtime status cache from runtime-bound roster state"
                );
                runtime_health_observability.emit_or_warn(
                    "sqlite_cache_hydration",
                    "degraded",
                    "failed to hydrate runtime status cache from runtime-bound roster state",
                );
            }
        }
        let service_runtime = runtime_assembly.service_runtime;
        let post_commit_signals = Arc::new(LocalPostCommitWorkQueue::new(
            service_runtime.clone(),
            home_dir.clone(),
            Arc::clone(&observability),
        ));
        let post_commit_work_queue: Arc<dyn PostCommitWorkQueue> = post_commit_signals.clone();
        let admission_runtime_view =
            AdmissionRuntimeView::new(service_runtime.clone(), peer_directory);
        Ok(Self {
            home_dir,
            observability: Arc::clone(&observability),
            runtime_health_observability: runtime_health_observability.clone(),
            status_cache,
            service_runtime,
            admission_runtime_view,
            peer_http_runtime_config: Arc::new(ArcSwapOption::from(
                peer_http_runtime_config.map(Arc::new),
            )),
            peer_resend_scheduler: Arc::new(ArcSwapOption::from(peer_resend_scheduler)),
            message_store,
            outbound_message_query,
            doctor_ports: runtime_assembly.doctor_ports,
            roster_store: Some(roster_store),
            peer_config_store,
            post_commit_signals,
            post_commit_work_queue,
        })
    }

    pub(crate) fn start_local_post_write_executor(&self) -> Result<(), AtmError> {
        self.post_commit_signals.start()
    }

    pub(crate) fn stop_local_post_write_executor(&self) -> Result<(), AtmError> {
        self.post_commit_signals.stop()
    }

    pub(crate) fn prepare_peer_http_listener(
        &self,
    ) -> Result<Option<PeerHttpListenerSet>, AtmError> {
        let bind_addrs: Vec<std::net::SocketAddr> = self
            .peer_config_store
            .list_interfaces()?
            .into_iter()
            .filter(|interface| interface.enabled)
            .map(|interface| interface.bind_addr)
            .collect();
        if bind_addrs.is_empty() {
            return Ok(None);
        }
        PeerHttpListenerSet::bind(&PeerHttpBindConfig { bind_addrs }).map(Some)
    }

    #[cfg(test)]
    pub(crate) fn home_dir_for_test(&self) -> &std::path::Path {
        self.home_dir.as_path()
    }
}

fn with_shutdown_finalizer_registry<R>(
    f: impl FnOnce(&mut Vec<std::thread::JoinHandle<()>>) -> R,
) -> R {
    match SHUTDOWN_FINALIZER_THREADS.lock() {
        Ok(mut handles) => f(&mut handles),
        Err(poisoned) => {
            tracing::warn!(
                subsystem = "runtime_health",
                action = "shutdown_registry_lock",
                outcome = "poison_recovered",
                "shutdown finalizer thread registry lock poisoned; recovering retained worker handles"
            );
            // The registry only owns JoinHandles for timed-out shutdown helpers; recovering the
            // inner vector preserves later joins instead of dropping retained worker ownership.
            let mut handles = poisoned.into_inner();
            f(&mut handles)
        }
    }
}

impl boundary::sealed::Sealed for DaemonRequestDispatcher {}

pub(super) struct MessageRecord {
    prepared: PreparedWrite,
    outbound_request: WriteRequest,
}

trait MessageWriter: Send + Sync {
    fn write(&self, request: WriteRequest) -> Result<MessageRecord, AtmError>;
}

pub(super) trait PostWriteRouter: Send + Sync {
    /// Routes one already-committed write. Local notification stays
    /// best-effort; direct peer delivery can instead return the explicit
    /// persisted-but-undelivered result to the initiating caller.
    fn dispatch(
        &self,
        message: &mut MessageRecord,
        deadline: RequestDeadline,
    ) -> Result<(), AtmError>;
}

impl DaemonRequestDispatcher {
    #[cfg(test)]
    pub(crate) fn dispatch(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        self.dispatch_with_deadline(request, RequestDeadline::after(Duration::from_secs(5)))
    }

    fn dispatch_with_deadline(
        &self,
        request: RequestEnvelope,
        deadline: RequestDeadline,
    ) -> Result<ResponseEnvelope, AtmError> {
        let side_effecting = request_may_have_side_effects(&request);
        require_dispatch_budget(deadline, false)?;
        let response = match request {
            RequestEnvelope::Write(request) => self.route_write(*request, deadline),
            request => self.dispatch_non_write(request),
        }?;
        require_dispatch_budget(deadline, side_effecting)?;
        Ok(response)
    }

    fn route_write(
        &self,
        request: WriteRequest,
        deadline: RequestDeadline,
    ) -> Result<ResponseEnvelope, AtmError> {
        let mut message = MessageWriter::write(self, request)?;
        let requires_post_commit_signal = message.prepared.requires_post_write_route();
        // Admission is complete before any post-commit work is even signalled.
        // In particular, an acknowledgement's source transition completes here,
        // before the peer worker can observe or deliver the reply.
        let outcome = message
            .prepared
            .finish(&self.service_runtime, self.observability.as_ref())?;
        let response = write_response(outcome);
        if requires_post_commit_signal {
            PostWriteRouter::dispatch(self, &mut message, deadline)?;
        }
        Ok(response)
    }

    fn route_peer_acknowledgement(
        &self,
        request: WriteRequest,
        deadline: RequestDeadline,
    ) -> Result<ResponseEnvelope, AtmError> {
        let mut message = MessageWriter::write(self, request)?;
        let requires_post_commit_signal = message.prepared.requires_post_write_route();
        let outcome = message
            .prepared
            .finish(&self.service_runtime, self.observability.as_ref())?;
        let response = write_response(outcome);
        if requires_post_commit_signal
            && let Err(error) = PostWriteRouter::dispatch(self, &mut message, deadline)
        {
            tracing::warn!(
                subsystem = "runtime_health",
                action = "peer_ack_post_commit",
                outcome = "warning",
                message_id = ?message.prepared.persisted_message_id(),
                error = %error,
                "peer acknowledgement committed before its best-effort local post-write effect failed"
            );
        }
        Ok(response)
    }

    fn route_peer_messages(
        &self,
        requests: Vec<WriteRequest>,
        deadline: RequestDeadline,
    ) -> Result<ResponseEnvelope, AtmError> {
        let mut prepared = self
            .admission_runtime_view
            .prepare_peer_writes_atomically(requests, self.observability.as_ref())?;
        let mut response = None;
        for prepared_write in prepared.drain(..) {
            let mut message = MessageRecord {
                outbound_request: prepared_write.outbound_request(),
                prepared: prepared_write,
            };
            let requires_post_commit_signal = message.prepared.requires_post_write_route();
            let outcome = message
                .prepared
                .finish(&self.service_runtime, self.observability.as_ref())?;
            let WriteOutcome::Sent(outcome) = outcome else {
                return Err(AtmError::validation(
                    "peer message arrays cannot contain acknowledgement writes",
                ));
            };
            if response.is_none() {
                response = Some(ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)));
            }
            if requires_post_commit_signal {
                // The peer-receipt branch of the canonical router only queues
                // a local best-effort post-write effect.  Its result is never
                // used to redefine the already-committed receive response.
                if let Err(error) = PostWriteRouter::dispatch(self, &mut message, deadline) {
                    tracing::warn!(
                        subsystem = "runtime_health",
                        action = "peer_array_post_commit",
                        outcome = "warning",
                        message_id = ?message.prepared.persisted_message_id(),
                        error = %error,
                        "peer receive committed before its best-effort local post-write effect failed"
                    );
                }
            }
        }
        response.ok_or_else(|| AtmError::validation("peer message array must not be empty"))
    }

    fn persist_local_write(&self, request: WriteRequest) -> Result<PreparedWrite, AtmError> {
        self.admission_runtime_view
            .prepare_write(request, self.observability.as_ref())
    }

    fn dispatch_non_write(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        match request {
            RequestEnvelope::Heartbeat(request) => {
                Ok(ResponseEnvelope::Heartbeat(self.record_heartbeat(request)?))
            }
            RequestEnvelope::CompatibilityPreflight(preflight) => Ok(
                ResponseEnvelope::CompatibilityVerdict(Self::compatibility_verdict(preflight)?),
            ),
            RequestEnvelope::List(query) => Ok(ResponseEnvelope::List(list_mail(
                query,
                self.observability.as_ref(),
            )?)),
            RequestEnvelope::Peek(query) => Ok(ResponseEnvelope::Peek(Box::new(
                peek_mail_with_runtime(query, self.observability.as_ref(), &self.service_runtime)?,
            ))),
            RequestEnvelope::Receive(query) => Ok(ResponseEnvelope::Receive(Box::new(
                read_mail_with_runtime(query, self.observability.as_ref(), &self.service_runtime)?,
            ))),
            RequestEnvelope::Clear(query) => Ok(ResponseEnvelope::Clear(clear_mail_with_runtime(
                query,
                self.observability.as_ref(),
                &self.service_runtime,
            )?)),
            RequestEnvelope::Doctor(query) => Ok(ResponseEnvelope::Doctor(Box::new(
                self.project_doctor_report(query)?,
            ))),
            RequestEnvelope::ReloadRuntimeView => {
                self.reload_runtime_view()?;
                Ok(ResponseEnvelope::RuntimeViewReloaded)
            }
            RequestEnvelope::Write(_) => unreachable!("writes are handled by route_write"),
        }
    }
}

fn write_response(outcome: WriteOutcome) -> ResponseEnvelope {
    match outcome {
        WriteOutcome::Sent(outcome) => ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)),
        WriteOutcome::Acknowledged(outcome) => {
            ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(outcome))
        }
    }
}

/// The router owns one absolute request budget across validation, persistence,
/// and response construction.  Work that has already crossed a mutable
/// boundary is allowed to finish for consistency, but its caller receives the
/// durable retry-safe uncertainty instead of a stale success response.
fn require_dispatch_budget(
    deadline: RequestDeadline,
    side_effecting_work_may_have_started: bool,
) -> Result<(), AtmError> {
    if !deadline.expired() {
        return Ok(());
    }
    if side_effecting_work_may_have_started {
        return Err(AtmError::daemon_may_have_executed(
            "daemon request exceeded its shared deadline after side-effecting dispatch work may have started",
        ));
    }
    Err(AtmError::daemon_unavailable(
        "daemon request exceeded its shared deadline before dispatch work started",
    ))
}

fn request_may_have_side_effects(request: &RequestEnvelope) -> bool {
    !matches!(
        request,
        RequestEnvelope::CompatibilityPreflight(_)
            | RequestEnvelope::List(_)
            | RequestEnvelope::Peek(_)
            | RequestEnvelope::Doctor(_)
    )
}

fn peer_http_runtime_config(
    peer_config_store: &dyn PeerConfigStore,
) -> Result<Option<PeerHttpRuntimeConfig>, AtmError> {
    let mut advertised_hosts = peer_config_store
        .list_interfaces()?
        .into_iter()
        .filter(|interface| interface.enabled)
        .map(|interface| interface.advertise_host)
        .collect::<Vec<_>>();
    advertised_hosts.sort();
    advertised_hosts.dedup();
    match advertised_hosts.as_slice() {
        [] => Ok(None),
        [source_host] => Ok(Some(PeerHttpRuntimeConfig {
            source_host: source_host.clone(),
        })),
        _ => Err(AtmError::peer_config_validation(
            "enabled peer interfaces must advertise one canonical source host for direct peer HTTP delivery",
        )),
    }
}

fn peer_resend_scheduler(
    setting: PeerResendCacheSetting,
    http: Option<&PeerHttpRuntimeConfig>,
    directory: atm_storage::PeerDirectory,
    outbound: Arc<dyn OutboundMessageQuery + Send + Sync>,
    messages: Arc<dyn MessageStore + Send + Sync>,
) -> Result<Option<Arc<PeerResendScheduler>>, AtmError> {
    if !setting.enabled {
        return Ok(None);
    }
    let Some(http) = http else {
        return Ok(None);
    };
    let scheduler = Arc::new(PeerResendScheduler::new(
        http.clone(),
        directory,
        outbound,
        messages,
    ));
    scheduler.bootstrap_pending_peer_resends()?;
    Ok(Some(scheduler))
}

impl MessageWriter for DaemonRequestDispatcher {
    fn write(&self, request: WriteRequest) -> Result<MessageRecord, AtmError> {
        self.persist_local_write(request).map(|prepared| {
            let message_id = prepared.persisted_message_id();
            MessageRecord {
                outbound_request: prepared
                    .outbound_request()
                    .with_origin_metadata(message_id, prepared.persisted_timestamp()),
                prepared,
            }
        })
    }
}

impl DaemonRequestDispatcher {
    fn compatibility_verdict(
        preflight: atm_core::protocol::CompatibilityPreflight,
    ) -> Result<CompatibilityVerdict, AtmError> {
        let daemon_release = ReleaseVersion::current();
        let daemon_schema_version = atm_core::protocol::CLI_SCHEMA_VERSION;
        let daemon_http_api_version = atm_core::protocol::HttpApiVersion::current();
        if preflight.cli_schema_version == daemon_schema_version
            && preflight.http_api_version.major() == daemon_http_api_version.major()
        {
            return Ok(CompatibilityVerdict::Compatible {
                daemon_release,
                daemon_schema_version,
                daemon_http_api_version,
            });
        }
        Ok(CompatibilityVerdict::Incompatible {
            client_release: preflight.client_release,
            daemon_release,
            client_schema_version: preflight.cli_schema_version,
            daemon_schema_version,
            client_http_api_version: preflight.http_api_version,
            daemon_http_api_version,
            code: AtmErrorCode::ClientDaemonVersionIncompatible,
        })
    }

    pub(crate) fn reload_runtime_view(&self) -> Result<(), AtmError> {
        let roster_store = self.roster_store.as_ref().cloned().ok_or_else(|| {
            self.runtime_health_observability.emit_or_warn(
                "reload_unavailable",
                "failed",
                "daemon runtime reload is unavailable because the roster store is not assembled",
            );
            AtmError::daemon_unavailable(
                "daemon runtime reload is unavailable because the roster store is not assembled",
            )
        })?;
        let current_state = self.status_cache.clone_state();
        let next_state =
            build_runtime_status_cache_state(Some(&current_state), roster_store.as_ref())?;
        let peer_directory = self.peer_config_store.peer_directory()?;
        let peer_http_runtime_config = peer_http_runtime_config(self.peer_config_store.as_ref())?;
        let peer_resend_scheduler = peer_resend_scheduler(
            self.peer_config_store.peer_resend_cache_setting()?,
            peer_http_runtime_config.as_ref(),
            peer_directory.clone(),
            Arc::clone(&self.outbound_message_query),
            Arc::clone(&self.message_store),
        )?;
        self.admission_runtime_view
            .reload(self.service_runtime.clone(), peer_directory);
        self.peer_http_runtime_config
            .store(peer_http_runtime_config.map(Arc::new));
        self.peer_resend_scheduler.store(peer_resend_scheduler);
        let reloaded_members = next_state.member_count();
        self.status_cache.publish_state(next_state);
        tracing::info!(
            reloaded_members,
            "bounded daemon config/roster reload applied successfully"
        );
        Ok(())
    }

    pub(crate) fn finalize_observability_shutdown(&self) {
        let observability = self.observability.clone();
        Self::run_bounded_shutdown_step(
            "observability_flush",
            SHUTDOWN_OBSERVABILITY_FLUSH_DEADLINE,
            // The finalizer step already runs on a dedicated shutdown thread,
            // so the retained-log flush remains in a sync context.
            move || observability.best_effort_flush_blocking(),
        );
    }

    pub(crate) fn preflush_observability_shutdown(&self) {
        let observability = self.observability.clone();
        Self::run_bounded_shutdown_step(
            "observability_preflush",
            SHUTDOWN_OBSERVABILITY_FLUSH_DEADLINE,
            move || observability.best_effort_preflush_blocking(),
        );
    }

    fn record_heartbeat(
        &self,
        request: TeamMemberHeartbeatRequest,
    ) -> Result<TeamMemberHeartbeatResponse, AtmError> {
        let roster_store = self.roster_store.as_ref().cloned().ok_or_else(|| {
            self.runtime_health_observability.emit_or_warn(
                "heartbeat_unavailable",
                "failed",
                "daemon heartbeats are unavailable because the roster store is not assembled",
            );
            AtmError::daemon_unavailable(
                "daemon heartbeats are unavailable because the roster store is not assembled",
            )
        })?;
        let membership = roster_store
            .load_roster(&request.team)?
            .members
            .into_iter()
            .find(|entry| entry.agent_name == request.member);
        if membership.is_none() {
            return Err(AtmError::agent_not_found(
                request.member.as_str(),
                request.team.as_str(),
            ));
        }
        let cached_pid = self.status_cache.cached_pid(&request.team, &request.member);
        if let Some(existing_pid) = cached_pid.filter(|pid| *pid != request.pid)
            && process_is_alive(existing_pid)
        {
            self.status_cache
                .record_identity_conflict(&request, existing_pid);
            return Err(AtmError::identity_conflict(
                "ATM_IDENTITY_CONFLICT: stop and report to user immediately",
            ));
        }
        Ok(self
            .status_cache
            .record_heartbeat(&request, cached_pid.is_some_and(|pid| pid != request.pid)))
    }

    fn project_doctor_report(&self, query: DoctorQuery) -> Result<DoctorReport, AtmError> {
        let daemon_observability_finding = match self.observability.health() {
            Ok(health) => daemon_observability_finding(&health),
            Err(error) => doctor::health::observability_finding_from_error(&error),
        };
        let (peer_config, mut peer_findings) =
            doctor::peer_config_doctor_report(self.peer_config_store.as_ref());
        if let Some(scheduler) = self.peer_resend_scheduler.load_full() {
            peer_findings.extend(scheduler.doctor_findings()?);
        }
        peer_findings.insert(0, daemon_observability_finding);
        let daemon_runtime = DaemonRuntimeDoctorReport {
            findings: peer_findings,
            peer_config: Some(peer_config),
        };
        let mut report = doctor::run_doctor_with_runtime_ports(
            query,
            self.observability.as_ref(),
            &self.service_runtime,
            &self.doctor_ports,
            Some(daemon_runtime),
        )?;
        let runtime_status = match &report.member_roster {
            Some(roster) => self.status_cache.snapshot_for_members(
                roster
                    .members
                    .iter()
                    .map(|member| (roster.team.clone(), member.name.clone())),
            ),
            None => self.status_cache.snapshot(),
        };
        let runtime_status_finding = runtime_status_finding(&runtime_status);
        report.findings.push(runtime_status_finding.clone());
        if let Some(daemon_runtime) = report.daemon_runtime.as_mut() {
            daemon_runtime.findings.push(runtime_status_finding);
        } else {
            report.daemon_runtime = Some(DaemonRuntimeDoctorReport {
                findings: vec![runtime_status_finding],
                peer_config: None,
            });
        }
        report.runtime_status = Some(runtime_status);
        // Daemon context is process metadata only. Caller identity and team
        // remain request-scoped in `client_context`; the daemon never reads
        // ambient caller variables to populate either context.
        report.daemon_context = Some(DoctorExecutionContext {
            team: None,
            identity: None,
            version: Some(ReleaseVersion::current()),
            cli_schema_version: Some(atm_core::protocol::CLI_SCHEMA_VERSION),
            http_api_version: Some(atm_core::protocol::HttpApiVersion::current()),
        });
        finalize_doctor_report(&mut report);
        Ok(report)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DaemonStatusSource {
    status_cache: RuntimeStatusCache,
}

impl DaemonStatusSource {
    pub(crate) fn new(status_cache: RuntimeStatusCache) -> Self {
        Self { status_cache }
    }
}

impl boundary::sealed::Sealed for DaemonStatusSource {}

impl boundary::StatusSource for DaemonStatusSource {
    // The boundary contract is fallible even though the in-memory snapshot read
    // is currently infallible; keeping the `Result` preserves the shared
    // status-source seam for implementations that may need to surface IO or
    // transport failures.
    fn snapshot(&self) -> Result<RuntimeStatusSnapshot, AtmError> {
        let snapshot = self.status_cache.snapshot();
        if matches!(snapshot.liveness, RuntimeLivenessState::Unavailable)
            && snapshot.singleton_owner_pid.is_none()
        {
            return Err(AtmError::daemon_unavailable(
                "daemon runtime status snapshot is unavailable because no owner process is recorded",
            ));
        }
        Ok(snapshot)
    }
}

#[cfg(test)]
impl DaemonRequestDispatcher {
    pub(crate) fn new_for_test(
        home_dir: std::path::PathBuf,
        status_cache: RuntimeStatusCache,
        roster_db_path: std::path::PathBuf,
    ) -> Self {
        let observability = std::sync::Arc::new(
            crate::test_observability::TestDaemonObservability::new(
                atm_core::home::host_log_dir_from_home(&home_dir),
            )
            .expect("daemon test observability"),
        );
        let runtime_observability: std::sync::Arc<dyn crate::DaemonRuntimeObservability> =
            observability.clone();
        let runtime_assembly =
            crate::test_support::sqlite_runtime_assembly_for_test(&roster_db_path)
                .expect("assemble sqlite runtime for daemon dispatcher test");
        let daemon_home = crate::AtmHomeDir::from_path_for_test(home_dir.clone());
        let dispatcher = Self::new(
            daemon_home,
            status_cache,
            runtime_observability,
            runtime_assembly,
        )
        .expect("assemble daemon dispatcher test dependencies");
        dispatcher
            .post_commit_signals
            .start()
            .expect("start post-commit worker for dispatcher test");
        dispatcher
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DaemonRequestDispatcher, MAX_SHUTDOWN_FINALIZER_THREADS, SHUTDOWN_FINALIZER_THREADS,
    };
    use atm_core::api::RequestDeadline;
    use atm_core::protocol::RequestEnvelope;
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

    #[test]
    fn expired_router_deadline_prevents_downstream_dispatch() {
        let tempdir = tempfile::TempDir::new().expect("tempdir");
        let dispatcher = DaemonRequestDispatcher::new_for_test(
            tempdir.path().join("home"),
            super::RuntimeStatusCache::default(),
            tempdir.path().join("runtime.db"),
        );

        let error = dispatcher
            .dispatch_with_deadline(
                RequestEnvelope::Doctor(Default::default()),
                RequestDeadline::after(Duration::ZERO),
            )
            .expect_err("an expired ingress deadline must prevent dispatcher work");

        assert!(error.is_daemon_unavailable());
        assert!(error.message().contains("before dispatch work started"));
    }

    struct ShutdownFinalizerDrainGuard;

    impl Drop for ShutdownFinalizerDrainGuard {
        fn drop(&mut self) {
            DaemonRequestDispatcher::drain_shutdown_finalizer_threads_for_test();
        }
    }

    #[test]
    #[serial_test::serial(env)]
    fn bounded_shutdown_step_returns_after_deadline() {
        let _drain_guard = ShutdownFinalizerDrainGuard;
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let blocker = Arc::clone(&release);
        let started = Instant::now();
        DaemonRequestDispatcher::run_bounded_shutdown_step(
            "blocking_test_step",
            Duration::from_millis(10),
            move || {
                let (released, wake) = &*blocker;
                let mut released = released.lock().expect("released");
                while !*released {
                    let wait = wake
                        .wait_timeout(released, Duration::from_secs(5))
                        .expect("released wait");
                    released = wait.0;
                    assert!(!wait.1.timed_out(), "released wait timed out");
                }
                Ok(())
            },
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "bounded shutdown step should return promptly after its deadline"
        );

        let (released, wake) = &*release;
        *released.lock().expect("released") = true;
        wake.notify_all();
    }

    #[test]
    #[serial_test::serial(env)]
    fn bounded_shutdown_step_does_not_exceed_retained_finalizer_cap() {
        let _drain_guard = ShutdownFinalizerDrainGuard;
        DaemonRequestDispatcher::drain_shutdown_finalizer_threads_for_test();

        let retained_release = Arc::new((Mutex::new(false), Condvar::new()));
        {
            let mut handles = SHUTDOWN_FINALIZER_THREADS
                .lock()
                .expect("shutdown finalizer thread registry lock");
            for _ in 0..MAX_SHUTDOWN_FINALIZER_THREADS {
                let retained_release = Arc::clone(&retained_release);
                handles.push(std::thread::spawn(move || {
                    let (released, wake) = &*retained_release;
                    let mut released = released.lock().expect("retained release");
                    while !*released {
                        let wait = wake
                            .wait_timeout(released, Duration::from_secs(5))
                            .expect("retained release wait");
                        released = wait.0;
                        assert!(!wait.1.timed_out(), "retained release wait timed out");
                    }
                }));
            }
        }

        let overflow_release = Arc::new((Mutex::new(false), Condvar::new()));
        let blocker = Arc::clone(&overflow_release);
        DaemonRequestDispatcher::run_bounded_shutdown_step(
            "blocking_cap_test_step",
            Duration::from_millis(10),
            move || {
                let (released, wake) = &*blocker;
                let mut released = released.lock().expect("overflow release");
                while !*released {
                    let wait = wake
                        .wait_timeout(released, Duration::from_secs(5))
                        .expect("overflow release wait");
                    released = wait.0;
                    assert!(!wait.1.timed_out(), "overflow release wait timed out");
                }
                Ok(())
            },
        );

        assert_eq!(
            SHUTDOWN_FINALIZER_THREADS
                .lock()
                .expect("shutdown finalizer thread registry lock")
                .len(),
            MAX_SHUTDOWN_FINALIZER_THREADS,
            "cap-exceeded path should not retain more than the documented shutdown finalizer thread budget"
        );

        let (released, wake) = &*overflow_release;
        *released.lock().expect("overflow release") = true;
        wake.notify_all();
        let (released, wake) = &*retained_release;
        *released.lock().expect("retained release") = true;
        wake.notify_all();
    }

    #[test]
    fn heartbeat_only_insert_evicts_oldest_member_when_cache_is_full() {
        use atm_core::protocol::{
            HeartbeatActivity, RuntimeMemberState, TeamMemberHeartbeatRequest,
        };
        use atm_core::types::{AgentName, IsoTimestamp, TeamName};
        use chrono::{Duration as ChronoDuration, Utc};

        let status_cache = super::RuntimeStatusCache::new();
        let team: TeamName = "test-team".parse().expect("team");
        let oldest_member: AgentName = "heartbeat-oldest".parse().expect("member");
        let trigger_member: AgentName = "heartbeat-trigger".parse().expect("member");
        let base = Utc::now();

        for index in 0..super::MAX_STATUS_CACHE_ENTRIES {
            let member_name: AgentName = if index == 0 {
                oldest_member.clone()
            } else {
                format!("heartbeat-{index}").parse().expect("member")
            };
            status_cache.record_heartbeat_for_test(
                &TeamMemberHeartbeatRequest {
                    team: team.clone(),
                    member: member_name,
                    pid: index as u32 + 1,
                    observed_at: IsoTimestamp::from_datetime(
                        base + ChronoDuration::seconds(index as i64),
                    ),
                    activity: HeartbeatActivity::Idle,
                },
                false,
            );
        }

        let response = status_cache.record_heartbeat_for_test(
            &TeamMemberHeartbeatRequest {
                team: team.clone(),
                member: trigger_member.clone(),
                pid: std::process::id(),
                observed_at: IsoTimestamp::from_datetime(base + ChronoDuration::hours(1)),
                activity: HeartbeatActivity::ActiveToolUse,
            },
            false,
        );
        assert_eq!(response.state, RuntimeMemberState::Active);

        assert_eq!(
            status_cache.member_count_for_test(),
            super::MAX_STATUS_CACHE_ENTRIES
        );
        assert_eq!(
            status_cache.member_state_for_test(&team, &oldest_member),
            None
        );
        assert_eq!(
            status_cache.member_state_for_test(&team, &trigger_member),
            Some(RuntimeMemberState::Active)
        );
    }
}
