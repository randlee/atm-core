use std::collections::HashMap;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use atm_core::{
    ApiRequest, ApiResponse, ApiRouter, AuthenticatedIngress, LocalServiceRuntime, RequestDeadline,
    RequestEnvelope, ResponseEnvelope,
    boundary::{self, GraftNudgeTarget, PostSendHookEvent},
    clear::clear_mail_with_runtime,
    doctor::{
        self, DaemonRuntimeDoctorReport, DoctorExecutionContext, DoctorFinding, DoctorQuery,
        DoctorReport, DoctorSeverity, DoctorStatus, DoctorSummary,
    },
    error::{AtmError, AtmErrorCode},
    graft::{
        GraftPostSendRequest, GraftPostSendResponse, deliver_graft_post_send,
        graft_receiver_record_path_from_home,
    },
    list::list_mail,
    process::process_is_alive,
    protocol::{
        CompatibilityVerdict, PeerSyncOutcome, PeerSyncRequest, ReleaseVersion,
        RuntimeLivenessState, RuntimeStatusSnapshot, SendResponseEnvelope,
        TeamMemberHeartbeatRequest, TeamMemberHeartbeatResponse,
    },
    read::{peek_mail_with_runtime, read_mail_with_runtime},
    schema::canonical_home_dir,
    send::{PreparedWrite, WriteOutcome, WriteRequest, prepare_write_with_runtime},
};

use crate::AtmHomeDir;
use crate::daemon_runtime_observability::{
    DaemonRuntimeObservability, DaemonSubsystem, SubsystemObservability,
};
use crate::https_transport::{HttpsMessageTransport, HttpsRequestDeadline};
use crate::post_send_emitter::DaemonPostSendHookEmitter;
#[cfg(test)]
pub(crate) use crate::runtime_status_cache::MAX_STATUS_CACHE_ENTRIES;
pub(crate) use crate::runtime_status_cache::RuntimeStatusCache;
use crate::runtime_status_cache::{build_runtime_status_cache_state, runtime_status_finding};
use atm_runtime::RuntimeAssembly;
use atm_storage::RosterStore;
use atm_storage::{OutboundMessageQuery, PeerConfigStore};
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

const GRAFT_POST_SEND_CONNECT_DEADLINE: Duration = Duration::from_millis(250);
const GRAFT_POST_SEND_IO_DEADLINE: Duration = Duration::from_secs(3);

#[derive(Debug, Clone)]
struct DaemonGraftPostSendPort {
    runtime: LocalServiceRuntime,
}

impl DaemonGraftPostSendPort {
    fn new(runtime: LocalServiceRuntime) -> Self {
        Self { runtime }
    }
}

impl boundary::sealed::Sealed for DaemonGraftPostSendPort {}

impl boundary::GraftPostSendPort for DaemonGraftPostSendPort {
    fn deliver_post_send(
        &self,
        event: &PostSendHookEvent,
        target: &GraftNudgeTarget,
    ) -> Result<(), AtmError> {
        let Some(member) = self
            .runtime
            .load_roster_member(&target.recipient_team, &target.recipient)?
        else {
            return Err(graft_recipient_unavailable_error(
                event,
                "recipient is missing from the authoritative ATM roster",
            ));
        };
        let recipient_home_dir = canonical_home_dir(&member.metadata_json).ok_or_else(|| {
            graft_recipient_unavailable_error(
                event,
                "recipient has no authoritative home_dir for graft post-send delivery",
            )
        })?;
        let record_path = graft_receiver_record_path_from_home(
            recipient_home_dir.as_path(),
            &target.recipient_team,
            &target.recipient,
        );
        deliver_post_send_to_graft_receiver(&record_path, event)
    }
}

fn deliver_post_send_to_graft_receiver(
    record_path: &std::path::Path,
    event: &PostSendHookEvent,
) -> Result<(), AtmError> {
    let request = GraftPostSendRequest {
        event: event.clone(),
    };
    match deliver_graft_post_send(
        record_path,
        &request,
        GRAFT_POST_SEND_CONNECT_DEADLINE,
        GRAFT_POST_SEND_IO_DEADLINE,
    )
    .map_err(|error| graft_transport_error(event, error))?
    {
        GraftPostSendResponse::Delivered => Ok(()),
        GraftPostSendResponse::Error(error) => Err(error),
    }
}

fn graft_transport_error(event: &PostSendHookEvent, error: AtmError) -> AtmError {
    graft_recipient_unavailable_error(event, error.detail())
}

fn graft_recipient_unavailable_error(
    event: &PostSendHookEvent,
    message: impl Into<String>,
) -> AtmError {
    AtmError::new(
        AtmErrorCode::PostSendGraftUnavailable,
        format!(
            "failed to deliver graft nudge to {}: {}",
            event.recipient,
            message.into()
        ),
    )
}

pub(crate) struct DaemonRequestDispatcher {
    // Invariant: this is the validated ATM_HOME root for the running daemon,
    // not an arbitrary workspace path.
    home_dir: AtmHomeDir,
    observability: Arc<dyn DaemonRuntimeObservability>,
    runtime_health_observability: SubsystemObservability,
    status_cache: RuntimeStatusCache,
    service_runtime: LocalServiceRuntime,
    doctor_ports: atm_core::doctor::RuntimeDoctorPorts,
    roster_store: Option<Arc<dyn RosterStore + Send + Sync>>,
    peer_config_store: Arc<dyn PeerConfigStore + Send + Sync>,
    outbound_message_query: Arc<dyn OutboundMessageQuery + Send + Sync>,
    https_transport: std::sync::Mutex<Option<Arc<dyn HttpsMessageTransport>>>,
    peer_sync_cooldown: std::sync::Mutex<HashMap<atm_core::types::HostName, std::time::Instant>>,
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
            .field("outbound_message_query", &"dyn OutboundMessageQuery")
            .field("https_transport", &"dyn HttpsMessageTransport")
            .finish()
    }
}

impl DaemonRequestDispatcher {
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
            .map_err(|_source| {
                AtmError::daemon_unavailable(format!(
                    "failed to spawn daemon shutdown finalizer step `{label}`"
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
            .map_err(|_source| {
                AtmError::daemon_unavailable(format!(
                    "failed to spawn daemon shutdown finalizer join helper `{label}`"
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
    ) -> Self {
        let runtime_health_observability =
            SubsystemObservability::new(DaemonSubsystem::RuntimeHealth, Arc::clone(&observability));
        let roster_store = runtime_assembly.shared_roster_store_arc();
        let peer_config_store = runtime_assembly.peer_config_store();
        let outbound_message_query = runtime_assembly.outbound_message_query();
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
        Self {
            home_dir,
            observability: Arc::clone(&observability),
            runtime_health_observability: runtime_health_observability.clone(),
            status_cache,
            service_runtime: runtime_assembly.service_runtime,
            doctor_ports: runtime_assembly.doctor_ports,
            roster_store: Some(roster_store),
            peer_config_store,
            outbound_message_query,
            https_transport: std::sync::Mutex::new(None),
            peer_sync_cooldown: std::sync::Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn install_https_transport(
        &self,
        transport: Arc<dyn HttpsMessageTransport>,
    ) -> Result<(), AtmError> {
        let mut slot = self
            .https_transport
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("HTTPS peer transport slot lock poisoned"))?;
        *slot = Some(transport);
        Ok(())
    }

    pub(crate) fn clear_https_transport(&self) -> Result<(), AtmError> {
        let mut slot = self
            .https_transport
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("HTTPS peer transport slot lock poisoned"))?;
        *slot = None;
        Ok(())
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

struct MessageRecord {
    prepared: PreparedWrite,
    outbound_request: WriteRequest,
}

trait MessageWriter: Send + Sync {
    fn write(&self, request: WriteRequest) -> Result<MessageRecord, AtmError>;
}

trait PostWriteRouter: Send + Sync {
    fn dispatch(&self, message: &mut MessageRecord) -> Result<(), AtmError>;
}

impl DaemonRequestDispatcher {
    pub(crate) fn dispatch(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        match request {
            RequestEnvelope::Write(request) => self.route_write(*request),
            request => self.dispatch_non_write(request),
        }
    }

    fn route_write(&self, request: WriteRequest) -> Result<ResponseEnvelope, AtmError> {
        let mut message = MessageWriter::write(self, request)?;
        if message.prepared.requires_post_write_route() {
            PostWriteRouter::dispatch(self, &mut message)?;
        }
        let outcome = message
            .prepared
            .finish(&self.service_runtime, self.observability.as_ref())?;
        Ok(match outcome {
            WriteOutcome::Sent(outcome) => {
                ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome))
            }
            WriteOutcome::Acknowledged(outcome) => {
                ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(outcome))
            }
        })
    }

    fn persist_local_write(&self, request: WriteRequest) -> Result<PreparedWrite, AtmError> {
        prepare_write_with_runtime(request, self.observability.as_ref(), &self.service_runtime)
    }

    fn dispatch_non_write(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        match request {
            RequestEnvelope::Heartbeat(request) => {
                Ok(ResponseEnvelope::Heartbeat(self.record_heartbeat(request)?))
            }
            RequestEnvelope::CompatibilityPreflight(preflight) => Ok(
                ResponseEnvelope::CompatibilityVerdict(self.compatibility_verdict(preflight)?),
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
            RequestEnvelope::PeerSync(request) => {
                Ok(ResponseEnvelope::PeerSync(self.sync_peer(request)?))
            }
            RequestEnvelope::Write(_) => unreachable!("writes are handled by route_write"),
        }
    }
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

impl PostWriteRouter for DaemonRequestDispatcher {
    fn dispatch(&self, message: &mut MessageRecord) -> Result<(), AtmError> {
        let Some(host) = message
            .outbound_request
            .to
            .as_ref()
            .and_then(|address| address.host.as_ref())
        else {
            let graft_post_send_port: Arc<dyn boundary::GraftPostSendPort + Send + Sync> =
                Arc::new(DaemonGraftPostSendPort::new(self.service_runtime.clone()));
            let post_send_emitter =
                DaemonPostSendHookEmitter::new(Arc::clone(&graft_post_send_port));
            message
                .prepared
                .emit_local_post_write(&self.service_runtime, &post_send_emitter);
            return Ok(());
        };
        let peer = self.peer_config_store.trusted_peer(host)?.ok_or_else(|| {
            AtmError::daemon_unavailable(format!("no trusted HTTPS peer is configured for {host}"))
        })?;
        let transport = self
            .https_transport
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("HTTPS peer transport slot lock poisoned"))?
            .clone()
            .ok_or_else(|| {
                AtmError::daemon_unavailable("HTTPS peer transport is not enabled in this daemon")
            })?;
        match transport.deliver(
            message.outbound_request.clone(),
            &peer,
            HttpsRequestDeadline::default(),
        ) {
            Ok(ResponseEnvelope::Error(error)) => Err(error),
            Ok(_) => self
                .reconcile_after_success(host, &peer, transport.as_ref(), true)
                .map(|_| ()),
            Err(error) => Err(error),
        }
    }
}

impl DaemonRequestDispatcher {
    fn reconcile_after_success(
        &self,
        peer_host: &atm_core::types::HostName,
        peer: &atm_storage::TrustedPeer,
        transport: &dyn HttpsMessageTransport,
        apply_cooldown: bool,
    ) -> Result<u16, AtmError> {
        let policy = self.peer_config_store.peer_sync_policy(peer_host)?;
        policy.validate()?;
        if policy.max_message_age.is_zero() {
            return Ok(0);
        }
        if apply_cooldown {
            let now = std::time::Instant::now();
            let mut cooldown = self
                .peer_sync_cooldown
                .lock()
                .map_err(|_| AtmError::daemon_unavailable("peer sync cooldown lock poisoned"))?;
            if cooldown
                .get(peer_host)
                .is_some_and(|last| now.duration_since(*last) < Duration::from_secs(60))
            {
                return Ok(0);
            }
            // This is only a short-lived rate limiter, never delivery state.
            // Evicting the oldest entry bounds memory without affecting message data.
            const MAX_PEER_SYNC_COOLDOWN_ENTRIES: usize = 256;
            if cooldown.len() >= MAX_PEER_SYNC_COOLDOWN_ENTRIES
                && !cooldown.contains_key(peer_host)
                && let Some(oldest) = cooldown
                    .iter()
                    .min_by_key(|(_, instant)| **instant)
                    .map(|(host, _)| host.clone())
            {
                cooldown.remove(&oldest);
            }
            cooldown.insert(peer_host.clone(), now);
        }
        let not_before = atm_core::types::IsoTimestamp::from_datetime(
            chrono::Utc::now()
                - chrono::Duration::from_std(policy.max_message_age).map_err(|_source| {
                    AtmError::validation("peer sync maximum message age is out of range")
                })?,
        );
        let writes = self.outbound_message_query.recent_outbound_for_peer(
            peer_host,
            not_before,
            policy.max_batch_messages,
        )?;
        let delivered = u16::try_from(writes.len()).map_err(|_| {
            AtmError::validation("peer sync selection exceeded its configured batch limit")
        })?;
        // The peer transport contract honors this deadline. Keeping the whole
        // pass bounded means shutdown never waits on an unbounded reconciliation
        // loop or creates an independent worker/state machine.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        for stored in writes {
            if std::time::Instant::now() >= deadline {
                return Err(AtmError::daemon_unavailable(
                    "peer reconciliation exceeded its bounded request deadline",
                ));
            }
            let request: WriteRequest =
                serde_json::from_str(&stored.request_json).map_err(|_source| {
                    AtmError::mailbox_read("stored immutable peer outbound write is invalid")
                })?;
            transport.deliver(request, peer, HttpsRequestDeadline::default())?;
        }
        Ok(delivered)
    }

    fn sync_peer(&self, request: PeerSyncRequest) -> Result<PeerSyncOutcome, AtmError> {
        let peer = self
            .peer_config_store
            .trusted_peer(&request.peer)?
            .ok_or_else(|| AtmError::peer_config_validation("unknown trusted peer"))?;
        let transport = self
            .https_transport
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("HTTPS peer transport slot lock poisoned"))?
            .clone()
            .ok_or_else(|| {
                AtmError::daemon_unavailable("HTTPS peer transport is not enabled in this daemon")
            })?;
        let delivered =
            self.reconcile_after_success(&request.peer, &peer, transport.as_ref(), false)?;
        Ok(PeerSyncOutcome {
            peer: request.peer,
            delivered,
        })
    }

    #[cfg(test)]
    pub(crate) fn reconcile_after_success_for_test(
        &self,
        peer_host: &atm_core::types::HostName,
        peer: &atm_storage::TrustedPeer,
        transport: &dyn HttpsMessageTransport,
    ) -> Result<(), AtmError> {
        self.reconcile_after_success(peer_host, peer, transport, true)
            .map(|_| ())
    }

    #[cfg(test)]
    pub(crate) fn seed_peer_sync_cooldown_for_test(
        &self,
        entries: impl IntoIterator<Item = (atm_core::types::HostName, std::time::Instant)>,
    ) {
        self.peer_sync_cooldown
            .lock()
            .expect("peer sync cooldown lock")
            .extend(entries);
    }

    #[cfg(test)]
    pub(crate) fn peer_sync_cooldown_for_test(
        &self,
    ) -> HashMap<atm_core::types::HostName, std::time::Instant> {
        self.peer_sync_cooldown
            .lock()
            .expect("peer sync cooldown lock")
            .clone()
    }
}

impl DaemonRequestDispatcher {
    fn compatibility_verdict(
        &self,
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
        // This is existing doctor-only launch context. Client context remains
        // request-scoped and is reported separately.
        report.daemon_context = Some(DoctorExecutionContext {
            team: atm_core::caller_context::read_cli_team_from_env_or_warn(
                "atm_daemon::runtime_health::daemon_context",
            ),
            identity: atm_core::caller_context::read_cli_identity_from_env_or_warn(
                "atm_daemon::runtime_health::daemon_context",
            ),
            version: Some(ReleaseVersion::current()),
            cli_schema_version: Some(atm_core::protocol::CLI_SCHEMA_VERSION),
            http_api_version: Some(atm_core::protocol::HttpApiVersion::current()),
        });
        finalize_doctor_report(&mut report);
        Ok(report)
    }
}

fn finalize_doctor_report(report: &mut DoctorReport) {
    report.recommendations = report
        .findings
        .iter()
        .filter_map(|finding| finding.remediation.clone())
        .collect();
    let status = doctor::health::status_from_findings(&report.findings);
    let (info_count, warning_count, error_count) = doctor_finding_counts(&report.findings);
    report.summary = DoctorSummary {
        status,
        message: doctor_summary_message(status).to_string(),
        info_count,
        warning_count,
        error_count,
    };
}

fn doctor_finding_counts(findings: &[DoctorFinding]) -> (usize, usize, usize) {
    findings.iter().fold(
        (0usize, 0usize, 0usize),
        |(info, warning, error), finding| match finding.severity {
            DoctorSeverity::Info => (info + 1, warning, error),
            DoctorSeverity::Warning => (info, warning + 1, error),
            DoctorSeverity::Error => (info, warning, error + 1),
        },
    )
}

fn doctor_summary_message(status: DoctorStatus) -> &'static str {
    match status {
        DoctorStatus::Healthy => "ATM doctor completed with healthy findings only",
        DoctorStatus::Warning => "ATM doctor completed with warnings",
        DoctorStatus::Error => "ATM doctor found critical issues",
    }
}

impl ApiRouter for DaemonRequestDispatcher {
    fn route(
        &self,
        request: ApiRequest,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        if deadline.expired() {
            return Err(AtmError::daemon_unavailable(
                "daemon API request exceeded its same-host deadline before routing",
            ));
        }
        let request = request.into_inner();
        if let RequestEnvelope::Write(write) = &request {
            match ingress {
                AuthenticatedIngress::Local
                    if write.origin_message_id.is_some() || write.origin_timestamp.is_some() =>
                {
                    return Err(AtmError::validation(
                        "local write requests must not supply origin message metadata",
                    ));
                }
                AuthenticatedIngress::Peer
                    if write.authenticated_source_host.is_none()
                        || write.origin_message_id.is_none()
                        || write.origin_timestamp.is_none() =>
                {
                    return Err(AtmError::validation(
                        "peer write requests require authenticated source provenance and immutable origin metadata",
                    ));
                }
                AuthenticatedIngress::Local | AuthenticatedIngress::Peer => {}
            }
        }
        self.dispatch(request).map(ApiResponse::new)
    }
}

fn daemon_observability_finding(
    health: &atm_core::observability::AtmObservabilityHealth,
) -> DoctorFinding {
    let path = health
        .active_log_path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "<unavailable>".to_string());
    let detail = health
        .detail
        .as_ref()
        .map(|detail| format!(" Detail: {detail}"))
        .unwrap_or_default();
    match health.logging_state {
        atm_core::observability::AtmObservabilityHealthState::Healthy => DoctorFinding {
            severity: DoctorSeverity::Info,
            code: atm_core::error_codes::AtmErrorCode::ObservabilityHealthOk,
            message: format!(
                "daemon retained observability sink is healthy at {path}; daemon query/follow remain deferred to the CLI-owned log surface.{detail}"
            ),
            remediation: None,
        },
        atm_core::observability::AtmObservabilityHealthState::Degraded => DoctorFinding {
            severity: DoctorSeverity::Warning,
            code: atm_core::error_codes::AtmErrorCode::WarningObservabilityHealthDegraded,
            message: format!(
                "daemon retained observability sink is degraded at {path}; daemon query/follow remain deferred to the CLI-owned log surface.{detail}"
            ),
            remediation: Some(
                "Inspect the daemon retained log path and sink errors, then re-run `atm doctor`."
                    .to_string(),
            ),
        },
        atm_core::observability::AtmObservabilityHealthState::Unavailable => DoctorFinding {
            severity: DoctorSeverity::Error,
            code: atm_core::error_codes::AtmErrorCode::ObservabilityHealthFailed,
            message: format!(
                "daemon retained observability sink is unavailable at {path}; daemon query/follow remain deferred to the CLI-owned log surface.{detail}"
            ),
            remediation: Some(
                "Restore the daemon retained-log path and confirm it is writable before re-running `atm doctor`."
                    .to_string(),
            ),
        },
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
        match build_runtime_status_cache_state(
            None,
            runtime_assembly.shared_roster_store_arc().as_ref(),
        ) {
            Ok(state) => status_cache.publish_state(state),
            Err(error) => {
                tracing::warn!(
                    subsystem = "runtime_health",
                    action = "sqlite_cache_hydration",
                    outcome = "degraded",
                    %error,
                    "failed to hydrate test runtime status cache from runtime-bound roster state"
                );
            }
        }
        let runtime_health_observability = crate::SubsystemObservability::new(
            crate::DaemonSubsystem::RuntimeHealth,
            std::sync::Arc::clone(&runtime_observability),
        );
        Self {
            home_dir: crate::AtmHomeDir::from_path_for_test(home_dir.clone()),
            observability: runtime_observability,
            runtime_health_observability,
            status_cache,
            service_runtime: runtime_assembly.service_runtime.clone(),
            doctor_ports: runtime_assembly.doctor_ports.clone(),
            roster_store: Some(runtime_assembly.shared_roster_store_arc()),
            peer_config_store: runtime_assembly.peer_config_store(),
            outbound_message_query: runtime_assembly.outbound_message_query(),
            https_transport: std::sync::Mutex::new(None),
            peer_sync_cooldown: std::sync::Mutex::new(HashMap::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DaemonRequestDispatcher, MAX_SHUTDOWN_FINALIZER_THREADS, SHUTDOWN_FINALIZER_THREADS,
    };
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

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
