use std::io::Write as _;
use std::net::SocketAddr;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use atm_core::{
    LocalServiceRuntime, RequestEnvelope, ResponseEnvelope,
    ack::ack_mail_with_runtime_and_post_send_emitter,
    boundary::{self, GraftNudgeTarget, PostSendHookEvent},
    clear::clear_mail_with_runtime,
    doctor::{
        self, CrossHostAllowedHostDoctorRow, CrossHostAllowlistDoctorReport, CrossHostDoctorReport,
        CrossHostInterfaceDoctorRow, DaemonRuntimeDoctorReport, DoctorExecutionContext,
        DoctorFinding, DoctorQuery, DoctorReport, DoctorSeverity, DoctorStatus, DoctorSummary,
    },
    error::{AtmError, AtmErrorKind},
    error_codes::AtmErrorCode,
    graft::{
        GraftPostSendRequest, GraftPostSendResponse, graft_receiver_socket_path_from_home,
        read_graft_post_send_message, write_graft_post_send_message,
    },
    list::list_mail,
    process::process_is_alive,
    protocol::{
        CompatibilityVerdict, ReleaseVersion, RuntimeLivenessState, RuntimeStatusSnapshot,
        SendRequestEnvelope, SendResponseEnvelope, TeamMemberHeartbeatRequest,
        TeamMemberHeartbeatResponse,
    },
    read::{peek_mail_with_runtime, read_mail_with_runtime},
    schema::canonical_home_dir,
    send::{PeerLoopbackHost, send_mail_with_runtime_and_post_send_emitter},
};
use interprocess::local_socket::Stream as LocalSocketStream;
use interprocess::local_socket::traits::Stream as _;

use crate::AtmHomeDir;
use crate::daemon_runtime_observability::{
    DaemonRuntimeObservability, DaemonSubsystem, SubsystemObservability,
};
use crate::peer_transport::PeerTransportRuntime;
use crate::post_send_emitter::DaemonPostSendHookEmitter;
#[cfg(test)]
pub(crate) use crate::runtime_status_cache::MAX_STATUS_CACHE_ENTRIES;
pub(crate) use crate::runtime_status_cache::RuntimeStatusCache;
use crate::runtime_status_cache::{build_runtime_status_cache_state, runtime_status_finding};
use atm_runtime::RuntimeAssembly;
use atm_storage::{AllowedHostStore, PeerInterfaceConfigStore, RosterStore};

#[path = "runtime_health/doctor_projection.rs"]
mod doctor_projection;
use doctor_projection::{
    finalize_doctor_findings, project_bound_endpoints, project_cross_host_allowlist_hosts,
    project_cross_host_findings, project_cross_host_interfaces,
};
const SHUTDOWN_WAL_CHECKPOINT_DEADLINE: Duration = Duration::from_secs(2);
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
            )
            .with_recovery(
                "Repair the roster row and restart the graft-backed recipient session before retrying if a fresh nudge is still required.",
            ));
        };
        let recipient_home_dir =
            canonical_home_dir(&member.metadata_json).ok_or_else(|| graft_recipient_unavailable_error(
                event,
                "recipient has no authoritative home_dir for graft post-send delivery",
            ).with_recovery(format!(
                "Repair the roster row with `atm teams update-member --team {} --member {} --home-dir <path>` and restart the graft-backed recipient session before retrying if a fresh nudge is still required.",
                target.recipient_team, target.recipient
            )))?;
        let endpoint_path = graft_receiver_socket_path_from_home(
            recipient_home_dir.as_path(),
            &target.recipient_team,
            &target.recipient,
        );
        deliver_post_send_to_graft_receiver(&endpoint_path, event)
    }
}

fn deliver_post_send_to_graft_receiver(
    endpoint_path: &std::path::Path,
    event: &PostSendHookEvent,
) -> Result<(), AtmError> {
    let mut stream = connect_graft_receiver(endpoint_path, event)?;
    apply_graft_post_send_deadlines(&stream, event)?;
    let request = GraftPostSendRequest {
        event: event.clone(),
    };
    write_graft_post_send_message(
        &mut stream,
        &request,
        "failed to write graft post-send request",
        "graft post-send request exceeded the bounded payload cap",
    )
    .map_err(|error| graft_transport_error(event, error))?;
    stream
        .flush()
        .map_err(|source| graft_transport_error(event, AtmError::daemon_unavailable(
            "failed to flush graft post-send request",
        )
        .with_recovery(
            "Restart the graft-backed recipient session before retrying if a fresh nudge is still required.",
        )
        .with_source(source)))?;
    match read_graft_post_send_message::<GraftPostSendResponse>(
        &mut stream,
        "failed to read graft post-send response",
        "graft post-send response exceeded the bounded payload cap",
    )
    .map_err(|error| graft_transport_error(event, error))?
    {
        GraftPostSendResponse::Delivered => Ok(()),
        GraftPostSendResponse::Error(error) => Err(error.into_atm_error()),
    }
}

fn connect_graft_receiver(
    endpoint_path: &std::path::Path,
    event: &PostSendHookEvent,
) -> Result<LocalSocketStream, AtmError> {
    let name = atm_core::protocol::daemon_local_ipc_name_from_path(endpoint_path)?;
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("graft-post-send-connect".to_string())
        .spawn(move || {
            let _ = result_tx.send(LocalSocketStream::connect(name));
        })
        .map_err(|source| {
            graft_recipient_unavailable_error(
                event,
                "failed to spawn bounded graft post-send connect helper",
            )
            .with_recovery(
                "Retry after the daemon can spawn one bounded same-host connect helper thread.",
            )
            .with_source(source)
        })?;
    match result_rx.recv_timeout(GRAFT_POST_SEND_CONNECT_DEADLINE) {
        Ok(Ok(stream)) => Ok(stream),
        Ok(Err(source)) => Err(
            graft_recipient_unavailable_error(event, "recipient has no active graft receiver path")
                .with_recovery(
                    "Start or reconnect the graft-backed recipient session before retrying if a fresh nudge is still required.",
                )
                .with_source(source),
        ),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(
            graft_recipient_unavailable_error(
                event,
                "timed out connecting to the graft receiver path",
            )
            .with_recovery(
                "Start or reconnect the graft-backed recipient session before retrying if a fresh nudge is still required.",
            ),
        ),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(
            graft_recipient_unavailable_error(
                event,
                "graft post-send connect helper disconnected unexpectedly",
            )
            .with_recovery(
                "Restart the daemon and the graft-backed recipient session before retrying if a fresh nudge is still required.",
            ),
        ),
    }
}

fn apply_graft_post_send_deadlines(
    stream: &LocalSocketStream,
    event: &PostSendHookEvent,
) -> Result<(), AtmError> {
    apply_graft_post_send_deadline(
        stream.set_recv_timeout(Some(GRAFT_POST_SEND_IO_DEADLINE)),
        event,
        "failed to apply graft post-send receive timeout",
    )?;
    apply_graft_post_send_deadline(
        stream.set_send_timeout(Some(GRAFT_POST_SEND_IO_DEADLINE)),
        event,
        "failed to apply graft post-send send timeout",
    )
}

fn apply_graft_post_send_deadline(
    result: std::io::Result<()>,
    event: &PostSendHookEvent,
    message: &'static str,
) -> Result<(), AtmError> {
    match result {
        Ok(()) => Ok(()),
        #[cfg(windows)]
        Err(source) if source.kind() == std::io::ErrorKind::Unsupported => Ok(()),
        Err(source) => Err(
            graft_recipient_unavailable_error(event, message)
                .with_recovery(
                    "Restart the graft-backed recipient session before retrying if a fresh nudge is still required.",
                )
                .with_source(source),
        ),
    }
}

fn graft_transport_error(event: &PostSendHookEvent, error: AtmError) -> AtmError {
    let mut graft_error = graft_recipient_unavailable_error(event, &error.message);
    for recovery in error.recovery {
        graft_error = graft_error.with_recovery(recovery);
    }
    graft_error
}

fn graft_recipient_unavailable_error(
    event: &PostSendHookEvent,
    message: impl Into<String>,
) -> AtmError {
    AtmError::new_with_code(
        AtmErrorCode::PostSendGraftUnavailable,
        AtmErrorKind::DaemonUnavailable,
        format!(
            "recipient {}@{} {}",
            event.recipient,
            event.recipient_team,
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
    peer_interface_config_store: Arc<dyn PeerInterfaceConfigStore + Send + Sync>,
    allowed_host_store: Arc<dyn AllowedHostStore + Send + Sync>,
    remote_replay_store: Option<Arc<dyn boundary::RemoteReplayStore + Send + Sync>>,
    storage_finalizer: Option<Arc<dyn boundary::RuntimeStorageFinalizer + Send + Sync>>,
    peer_transport_runtime: PeerTransportRuntime,
}

impl std::fmt::Debug for DaemonRequestDispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonRequestDispatcher")
            .field("home_dir", &self.home_dir.as_path())
            .field("status_cache", &self.status_cache)
            .field("service_runtime", &self.service_runtime)
            .field("doctor_ports", &self.doctor_ports)
            .field("roster_store_present", &self.roster_store.is_some())
            .field(
                "peer_interface_config_store",
                &"dyn PeerInterfaceConfigStore",
            )
            .field("allowed_host_store", &"dyn AllowedHostStore")
            .field(
                "remote_replay_store_present",
                &self.remote_replay_store.is_some(),
            )
            .field(
                "storage_finalizer_present",
                &self.storage_finalizer.is_some(),
            )
            .field("peer_transport_runtime", &self.peer_transport_runtime)
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
            .map_err(|source| {
                AtmError::daemon_unavailable(format!(
                    "failed to spawn daemon shutdown finalizer step `{label}`"
                ))
                .with_recovery(
                    "Restart atm-daemon; the bounded shutdown finalizer helper could not be created.",
                )
                .with_source(source)
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
                    "failed to spawn daemon shutdown finalizer join helper `{label}`"
                ))
                .with_recovery(
                    "Restart atm-daemon; the bounded shutdown finalizer join helper could not be created.",
                )
                .with_source(source)
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
        peer_transport_runtime: PeerTransportRuntime,
    ) -> Self {
        let runtime_health_observability =
            SubsystemObservability::new(DaemonSubsystem::RuntimeHealth, Arc::clone(&observability));
        let roster_store = runtime_assembly.shared_roster_store_arc();
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
            peer_interface_config_store: runtime_assembly.peer_interface_config_store,
            allowed_host_store: runtime_assembly.allowed_host_store,
            remote_replay_store: Some(runtime_assembly.remote_replay_store),
            storage_finalizer: Some(runtime_assembly.storage_finalizer),
            peer_transport_runtime,
        }
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

impl boundary::RequestDispatcher for DaemonRequestDispatcher {
    fn dispatch(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        let graft_post_send_port: Arc<dyn boundary::GraftPostSendPort + Send + Sync> =
            Arc::new(DaemonGraftPostSendPort::new(self.service_runtime.clone()));
        let post_send_emitter = DaemonPostSendHookEmitter::new(Arc::clone(&graft_post_send_port));
        match request {
            RequestEnvelope::Send(SendRequestEnvelope::Compose(request)) => {
                if request.peer_loopback_host.is_some() {
                    return self.dispatch_loopback_send(request);
                }
                let outcome = send_mail_with_runtime_and_post_send_emitter(
                    request,
                    self.observability.as_ref(),
                    &self.service_runtime,
                    &post_send_emitter,
                )?;
                Ok(ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)))
            }
            RequestEnvelope::Send(SendRequestEnvelope::Acknowledge(request)) => {
                Ok(ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(
                    ack_mail_with_runtime_and_post_send_emitter(
                        request,
                        self.observability.as_ref(),
                        &self.service_runtime,
                        &post_send_emitter,
                    )?,
                )))
            }
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
        }
    }
}

impl DaemonRequestDispatcher {
    fn dispatch_loopback_send(
        &self,
        mut request: atm_core::send::SendRequest,
    ) -> Result<ResponseEnvelope, AtmError> {
        let host = request
            .peer_loopback_host
            .take()
            .ok_or_else(|| AtmError::daemon_unavailable("loopback peer host is missing"))?;
        let endpoint = self.resolve_loopback_endpoint(&host)?;
        request.peer_loopback_delivery = true;
        self.peer_transport_runtime.send_to_endpoint(
            endpoint,
            RequestEnvelope::Send(SendRequestEnvelope::Compose(request)),
        )
    }

    fn resolve_loopback_endpoint(&self, host: &PeerLoopbackHost) -> Result<SocketAddr, AtmError> {
        let bound_addr = self
            .peer_transport_runtime
            .bound_addr()?
            .ok_or_else(|| {
                AtmError::daemon_unavailable(
                    "loopback peer delivery is unavailable because the daemon peer listener is not running",
                )
                .with_recovery(
                    "Add and enable a loopback daemon interface row with `atm daemon interfaces ...`, restart atm-daemon, and retry the loopback send after the peer listener is bound.",
                )
            })?;
        let host_addr = if host.as_str().eq_ignore_ascii_case("localhost") {
            "127.0.0.1".parse().expect("loopback localhost parses")
        } else {
            host.as_str().parse().map_err(|error| {
                AtmError::address_parse(format!(
                    "invalid loopback host `{}`: {error}",
                    host.as_str()
                ))
                .with_recovery(
                    "Use `loopback@localhost` or `loopback@<literal-ip>` before retrying the loopback send.",
                )
            })?
        };
        Ok(SocketAddr::new(host_addr, bound_addr.port()))
    }

    fn compatibility_verdict(
        &self,
        preflight: atm_core::protocol::CompatibilityPreflight,
    ) -> Result<CompatibilityVerdict, AtmError> {
        let daemon_release = ReleaseVersion::current();
        if preflight.wire_version == atm_core::protocol::ATM_FRAME_VERSION_V1
            && preflight.client_release == daemon_release
        {
            return Ok(CompatibilityVerdict::Compatible { daemon_release });
        }
        Ok(CompatibilityVerdict::Incompatible {
            client_release: preflight.client_release,
            daemon_release,
            code: AtmErrorCode::ClientDaemonVersionIncompatible,
        })
    }

    pub(crate) fn reload_runtime_view(&self) -> Result<(), AtmError> {
        let roster_store = self
            .roster_store
            .as_ref()
            .cloned()
            .ok_or_else(|| {
                self.runtime_health_observability.emit_or_warn(
                    "reload_unavailable",
                    "failed",
                    "daemon runtime reload is unavailable because the roster store is not assembled",
                );
                AtmError::daemon_unavailable(
                    "daemon runtime reload is unavailable because the roster store is not assembled",
                )
                .with_recovery(
                    "Restore the runtime-bound roster store and restart atm-daemon before retrying SIGHUP reload.",
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

    pub(crate) fn finalize_storage_shutdown(&self) {
        if let Some(storage_finalizer) = self.storage_finalizer.clone() {
            Self::run_bounded_shutdown_step(
                "sqlite_wal_checkpoint",
                SHUTDOWN_WAL_CHECKPOINT_DEADLINE,
                move || storage_finalizer.finalize_storage_shutdown(),
            );
        }
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
        let roster_store = self
            .roster_store
            .as_ref()
            .cloned()
            .ok_or_else(|| {
                self.runtime_health_observability.emit_or_warn(
                    "heartbeat_unavailable",
                    "failed",
                    "daemon heartbeats are unavailable because the roster store is not assembled",
                );
                AtmError::daemon_unavailable(
                    "daemon heartbeats are unavailable because the roster store is not assembled",
                )
                .with_recovery(
                    "Restore the runtime-bound roster store and restart atm-daemon before retrying heartbeat traffic.",
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
            )
            .with_recovery(
                "Stop the conflicting ATM process, confirm the stale PID is gone, then retry the heartbeat from the active runtime owner.",
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
        let daemon_runtime = DaemonRuntimeDoctorReport {
            findings: vec![daemon_observability_finding],
        };
        let mut report = doctor::run_doctor_with_runtime_ports(
            query,
            self.observability.as_ref(),
            &self.service_runtime,
            &self.doctor_ports,
            Some(daemon_runtime),
        )?;
        let (cross_host, cross_host_findings) = self.project_cross_host_report()?;
        report.cross_host = Some(cross_host);
        let runtime_status = match &report.member_roster {
            Some(roster) => self.status_cache.snapshot_for_members(
                roster
                    .members
                    .iter()
                    .map(|member| (roster.team.clone(), member.name.clone())),
            ),
            None => self.status_cache.snapshot(),
        };
        report.findings.extend(cross_host_findings.clone());
        let runtime_status_finding = runtime_status_finding(&runtime_status);
        report.findings.push(runtime_status_finding.clone());
        if let Some(daemon_runtime) = report.daemon_runtime.as_mut() {
            daemon_runtime.findings.extend(cross_host_findings);
            daemon_runtime.findings.push(runtime_status_finding);
        } else {
            report.daemon_runtime = Some(DaemonRuntimeDoctorReport {
                findings: vec![runtime_status_finding],
            });
        }
        finalize_doctor_findings(&mut report);
        report.runtime_status = Some(runtime_status);
        // `daemon_context` reports the daemon process's own launch-time
        // environment, which is frozen when the singleton starts and does NOT
        // track the requesting shell. It is deliberately distinct from
        // `client_context` (threaded through the request payload in
        // `DoctorQuery::caller_*`), which reflects the invoking CLI process.
        // Surfacing the daemon's launch-time identity is diagnostically useful:
        // it explains why an earlier release appeared to report a stale team or
        // identity for every caller (see issue #548).
        report.daemon_context = Some(DoctorExecutionContext {
            team: atm_core::caller_context::read_cli_team_from_env_or_warn(
                "atm_daemon::runtime_health::daemon_context",
            ),
            identity: atm_core::caller_context::read_cli_identity_from_env_or_warn(
                "atm_daemon::runtime_health::daemon_context",
            ),
            version: Some(ReleaseVersion::current()),
        });
        Ok(report)
    }

    fn project_cross_host_report(
        &self,
    ) -> Result<(CrossHostDoctorReport, Vec<DoctorFinding>), AtmError> {
        let interface_rows = self.peer_interface_config_store.list_interfaces()?;
        let host_rows = self.allowed_host_store.list_hosts()?;
        let live_bound_addr = self.peer_transport_runtime.bound_addr()?;
        let has_enabled_interface_rows = interface_rows.iter().any(|row| row.enabled);
        let legacy_fallback_active = !has_enabled_interface_rows && live_bound_addr.is_some();
        let interfaces = project_cross_host_interfaces(&interface_rows);
        let bound_endpoints = project_bound_endpoints(&interfaces, live_bound_addr);
        let allowlist_hosts = project_cross_host_allowlist_hosts(&host_rows);
        let enabled_allowlist_count = host_rows.iter().filter(|row| row.enabled).count();
        let findings = project_cross_host_findings(
            legacy_fallback_active,
            has_enabled_interface_rows,
            live_bound_addr,
            &interfaces,
            enabled_allowlist_count,
        );

        Ok((
            CrossHostDoctorReport {
                legacy_fallback_active,
                bound_endpoints,
                interfaces,
                allowlist: CrossHostAllowlistDoctorReport {
                    enforced: true,
                    empty: enabled_allowlist_count == 0,
                    hosts: allowlist_hosts,
                },
            },
            findings,
        ))
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
            )
            .with_recovery(
                "Restart atm-daemon or restore same-host ownership before retrying daemon status collection.",
            ));
        }
        Ok(snapshot)
    }
}

#[cfg(test)]
impl DaemonRequestDispatcher {
    pub(crate) fn new_for_test_with_peer_transport(
        home_dir: std::path::PathBuf,
        status_cache: RuntimeStatusCache,
        roster_db_path: std::path::PathBuf,
        peer_transport_runtime: crate::PeerTransportRuntime,
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
            crate::test_support::sqlite_runtime_assembly_for_test(&roster_db_path);
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
            peer_interface_config_store: runtime_assembly.peer_interface_config_store.clone(),
            allowed_host_store: runtime_assembly.allowed_host_store.clone(),
            remote_replay_store: Some(runtime_assembly.remote_replay_store.clone()),
            storage_finalizer: Some(runtime_assembly.storage_finalizer.clone()),
            peer_transport_runtime,
        }
    }

    pub(crate) fn new_for_test(
        home_dir: std::path::PathBuf,
        status_cache: RuntimeStatusCache,
        roster_db_path: std::path::PathBuf,
    ) -> Self {
        Self::new_for_test_with_peer_transport(
            home_dir,
            status_cache,
            roster_db_path,
            crate::PeerTransportRuntime::default(),
        )
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
