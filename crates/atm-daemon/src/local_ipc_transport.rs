use std::io::Write;
use std::num::NonZeroU64;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use atm_core::boundary::{self, AtmProtocol, RequestDispatcher};
use atm_core::error::AtmError;
use atm_core::protocol::{JsonAtmProtocolCodec, ProtocolErrorEnvelope, ResponseEnvelope};
use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{
    Listener as LocalSocketListener, ListenerOptions, Stream as LocalSocketStream,
};

use crate::active_connection_registry::{ActiveConnectionRegistry, TrackedDispatchHandle};
use crate::host_ownership::{HostOwnershipAdapter, HostOwnershipGuard};
use crate::lifecycle_control::LifecycleControlSourceAdapter;
use crate::local_ipc_connection::drain_active_connections_for_shutdown;
use crate::local_ipc_wake::{schedule_delayed_listener_wake, wake_listener};
use crate::shutdown_beacon::ShutdownBeacon;

#[cfg(unix)]
use std::fs;
use std::thread;

const MAX_CONCURRENT_CONNECTIONS: usize = 64;
const REQUEST_DEADLINE: Duration = Duration::from_secs(3);
const TRACKED_DISPATCH_JOIN_DEADLINE: Duration = Duration::from_millis(250);
const TERMINATE_REJECTION_GRACE_DEADLINE: Duration = Duration::from_millis(100);
const TERMINATE_REJECTION_REQUEST_ID: u64 = NonZeroU64::MIN.get();

#[derive(Debug, Default)]
struct ServeLoopSignals {
    reload_requested: AtomicBool,
    // The serve loop needs to retain the full typed accept failure for later propagation, so a
    // mutex-backed slot is required here; an atomic flag alone could not carry the original
    // `AtmError` across the lifecycle/accept thread boundary.
    accept_error: Mutex<Option<AtmError>>,
}

impl ServeLoopSignals {
    fn request_reload(&self) {
        // SeqCst keeps the lifecycle waiter wakeup and the direct accept-loop
        // reload observation in one total order, so a just-issued reload cannot
        // be observed after a later accept-error/shutdown transition.
        self.reload_requested.store(true, Ordering::SeqCst);
    }

    fn take_reload(&self) -> bool {
        // SeqCst pairs with request_reload() so the direct serve loop never
        // reorders a consumed reload flag around accept-error/shutdown state.
        self.reload_requested.swap(false, Ordering::SeqCst)
    }

    fn record_accept_error(&self, error: AtmError) -> Result<(), AtmError> {
        let mut slot = self.accept_error.lock().map_err(|_| {
            AtmError::daemon_unavailable("serve-loop accept-error state lock poisoned")
                .with_recovery(
                    "Restart the daemon; the same-host serve loop can no longer preserve accept failures safely.",
                )
        })?;
        if slot.is_none() {
            *slot = Some(error);
        }
        Ok(())
    }

    fn take_accept_error(&self) -> Result<Option<AtmError>, AtmError> {
        self.accept_error
            .lock()
            .map_err(|_| {
                AtmError::daemon_unavailable("serve-loop accept-error state lock poisoned")
                    .with_recovery(
                        "Restart the daemon; the same-host serve loop can no longer preserve accept failures safely.",
                    )
            })
            .map(|mut slot| slot.take())
    }
}

#[derive(Debug)]
pub(crate) struct SocketEndpointGuard {
    endpoint_path: PathBuf,
    preparation: LocalIpcEndpointPreparation,
    // Endpoint cleanup may be requested explicitly during shutdown and again from Drop, so an
    // atomic one-shot bit prevents duplicate unpublish work without introducing a wider lock.
    unpublished: AtomicBool,
}

impl SocketEndpointGuard {
    fn new(endpoint_path: PathBuf, preparation: LocalIpcEndpointPreparation) -> Self {
        Self {
            endpoint_path,
            preparation,
            unpublished: AtomicBool::new(false),
        }
    }

    fn endpoint_path(&self) -> &Path {
        &self.endpoint_path
    }

    fn unpublish(&self) -> Result<(), AtmError> {
        if self.unpublished.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        match self.preparation {
            #[cfg(unix)]
            LocalIpcEndpointPreparation::FilesystemEndpointPrepared => {
                remove_stale_endpoint(self.endpoint_path())?;
            }
            #[cfg(not(unix))]
            LocalIpcEndpointPreparation::NonFilesystemEndpointPrepared => {
                // Windows same-host IPC publishes a named pipe rather than a filesystem
                // socket path; once the listener has dropped there is no extra path
                // artifact left for the guard to remove here.
            }
        }
        Ok(())
    }
}

impl Drop for SocketEndpointGuard {
    fn drop(&mut self) {
        if let Err(error) = self.unpublish() {
            tracing::warn!(%error, "daemon local IPC endpoint cleanup failed during drop");
        }
    }
}

pub(crate) struct PreparedRuntimeServer {
    host_ownership_guard: HostOwnershipGuard,
    endpoint_path: PathBuf,
    listener: LocalSocketListener,
    lifecycle_control: LifecycleControlSourceAdapter,
    registry: Arc<ActiveConnectionRegistry>,
    force_shutdown: Arc<AtomicBool>,
    codec: JsonAtmProtocolCodec,
    // The endpoint guard must drop after the listener and connection registry have been torn
    // down so same-host endpoint unpublication never races a still-live serving resource.
    endpoint_guard: Option<SocketEndpointGuard>,
}

pub(crate) struct RuntimeServeHooks<
    BeginShutdown,
    ReloadRuntimeView,
    FinalizeShutdown,
    PublishReady,
> {
    pub(crate) endpoint_guard: SocketEndpointGuard,
    pub(crate) graceful_drain_deadline: Duration,
    pub(crate) force_cancel_deadline: Duration,
    pub(crate) begin_shutdown: BeginShutdown,
    pub(crate) reload_runtime_view: ReloadRuntimeView,
    pub(crate) finalize_shutdown: FinalizeShutdown,
    pub(crate) publish_ready: PublishReady,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(unix)]
enum LocalIpcEndpointPreparation {
    FilesystemEndpointPrepared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(not(unix))]
enum LocalIpcEndpointPreparation {
    NonFilesystemEndpointPrepared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShutdownResponseOutcome {
    NoFrame,
    RejectedRequest,
}

impl std::fmt::Debug for PreparedRuntimeServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedRuntimeServer")
            .field("registry", &self.registry)
            .finish_non_exhaustive()
    }
}

impl PreparedRuntimeServer {
    fn bind(endpoint_path: PathBuf) -> Result<Self, AtmError> {
        // Install lifecycle control before singleton ownership so daemon startup never
        // claims the host-wide owner lock unless shutdown/reload hooks are ready too.
        // Reversing this order can transiently block a healthy daemon restart behind an
        // instance that failed before it could service lifecycle-control requests.
        let lifecycle_control = LifecycleControlSourceAdapter::install()?;
        let ownership = HostOwnershipAdapter::new().acquire()?;
        let endpoint_preparation = prepare_local_ipc_endpoint(&endpoint_path)?;
        let listener = ListenerOptions::new()
            .name(atm_core::protocol::daemon_local_ipc_name_from_path(
                &endpoint_path,
            )?)
            .create_sync()
            .map_err(|source| {
                AtmError::daemon_unavailable(format!(
                    "failed to bind daemon local IPC endpoint at {}",
                    endpoint_path.display()
                ))
                .with_recovery(
                    "Confirm the endpoint path is writable and no conflicting daemon still owns the same-host IPC address before restarting atm-daemon.",
                )
                .with_source(source)
            })?;
        tracing::info!(
            max_concurrent_connections = MAX_CONCURRENT_CONNECTIONS,
            max_daemon_frame_bytes = atm_core::protocol::MAX_DAEMON_FRAME_BYTES,
            request_deadline_ms = REQUEST_DEADLINE.as_millis() as u64,
            endpoint_preparation = ?endpoint_preparation,
            "daemon local IPC transport limits configured"
        );
        emit_ready_signal_if_requested()?;
        Ok(Self {
            host_ownership_guard: ownership,
            endpoint_path: endpoint_path.clone(),
            endpoint_guard: Some(SocketEndpointGuard::new(
                endpoint_path,
                endpoint_preparation,
            )),
            listener,
            lifecycle_control,
            registry: Arc::new(ActiveConnectionRegistry::default()),
            force_shutdown: Arc::new(AtomicBool::new(false)),
            codec: JsonAtmProtocolCodec,
        })
    }

    pub(crate) fn serve_with_runtime_hooks<
        BeginShutdown,
        ReloadRuntimeView,
        FinalizeShutdown,
        PublishReady,
    >(
        self,
        dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
        hooks: RuntimeServeHooks<BeginShutdown, ReloadRuntimeView, FinalizeShutdown, PublishReady>,
    ) -> Result<(), AtmError>
    where
        BeginShutdown: Fn() -> Result<(), AtmError>,
        ReloadRuntimeView: Fn() -> Result<(), AtmError>,
        FinalizeShutdown: Fn(),
        PublishReady: Fn() -> Result<(), AtmError>,
    {
        self.serve_with_deadlines_and_accept_probe(dispatcher, hooks)
    }

    fn serve_with_deadlines_and_accept_probe<
        BeginShutdown,
        ReloadRuntimeView,
        FinalizeShutdown,
        PublishReady,
    >(
        self,
        dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
        hooks: RuntimeServeHooks<BeginShutdown, ReloadRuntimeView, FinalizeShutdown, PublishReady>,
    ) -> Result<(), AtmError>
    where
        BeginShutdown: Fn() -> Result<(), AtmError>,
        ReloadRuntimeView: Fn() -> Result<(), AtmError>,
        FinalizeShutdown: Fn(),
        PublishReady: Fn() -> Result<(), AtmError>,
    {
        let RuntimeServeHooks {
            endpoint_guard,
            graceful_drain_deadline,
            force_cancel_deadline,
            begin_shutdown,
            reload_runtime_view,
            finalize_shutdown,
            publish_ready,
        } = hooks;
        let Self {
            host_ownership_guard: _host_ownership_guard,
            endpoint_path,
            endpoint_guard: _endpoint_guard,
            listener,
            lifecycle_control,
            registry,
            force_shutdown,
            codec,
        } = self;
        thread::scope(|scope| -> Result<(), AtmError> {
            // Each serving invocation owns its own shutdown beacon. The beacon must not survive a
            // later bind/restart cycle because a tripped beacon from an older listener would
            // incorrectly poison the next same-host endpoint publication.
            let shutdown_beacon = Arc::new(ShutdownBeacon::default());
            let signals = Arc::new(ServeLoopSignals::default());
            let mut serve_error = None;
            let mut terminate_probe_pending = false;
            let lifecycle_waiter = {
                let signals = Arc::clone(&signals);
                let shutdown_beacon = Arc::clone(&shutdown_beacon);
                let lifecycle_control = lifecycle_control.clone();
                let endpoint_path = endpoint_path.clone();
                thread::Builder::new()
                    .name("local-ipc-lifecycle-waiter".to_string())
                    .spawn_scoped(scope, move || {
                    let mut observed_generation = match lifecycle_control.event_generation() {
                        Ok(generation) => generation,
                        Err(error) => {
                            shutdown_beacon.trip();
                            let _ = lifecycle_control.notify_state_change();
                            let _ = signals.record_accept_error(error);
                            let _ = wake_listener(&endpoint_path);
                            return;
                        }
                    };
                    loop {
                        if shutdown_beacon.is_tripped() {
                            return;
                        }
                        if let Err(error) =
                            lifecycle_control.wait_for_state_change(&mut observed_generation)
                        {
                            shutdown_beacon.trip();
                            let _ = lifecycle_control.notify_state_change();
                            let _ = signals.record_accept_error(error);
                            let _ = wake_listener(&endpoint_path);
                            return;
                        }
                        if shutdown_beacon.is_tripped() {
                            return;
                        }
                        if lifecycle_control.terminate_requested() {
                            shutdown_beacon.trip();
                            let _ = lifecycle_control.notify_state_change();
                            let _ = wake_listener(&endpoint_path);
                            return;
                        }
                        if lifecycle_control.take_reload_requested() {
                            signals.request_reload();
                            if let Err(error) = wake_listener(&endpoint_path) {
                                shutdown_beacon.trip();
                                let _ = lifecycle_control.notify_state_change();
                                let _ = signals.record_accept_error(
                                    AtmError::daemon_lifecycle_wedge(
                                        "daemon lifecycle waiter could not wake the direct local IPC accept loop for reload handling",
                                    )
                                    .with_recovery(
                                        "Restart the daemon; lifecycle-control reload state could not interrupt the same-host listener cleanly.",
                                    )
                                    .with_source(error),
                                );
                                return;
                            }
                        }
                    }
                })
                .map_err(|source| {
                    AtmError::daemon_unavailable("failed to spawn local IPC lifecycle waiter")
                        .with_recovery(
                            "Restart the daemon after confirming the host can spawn the same-host lifecycle waiter thread.",
                        )
                        .with_source(source)
                })?
            };
            publish_ready()?;
            loop {
                // Direct accept-loop target: lifecycle wakeups interrupt accept(), then this loop
                // drains reload/error state inline so same-host serving stays in one place.
                // Completed request workers are reaped before every accept iteration so join
                // ownership stays with the direct serve loop instead of a side-channel worker.
                if let Err(error) = registry.reap_finished_dispatches() {
                    shutdown_beacon.trip();
                    let _ = lifecycle_control.notify_state_change();
                    serve_error = Some(error);
                    break;
                }
                match signals.take_accept_error() {
                    Ok(Some(error)) => {
                        shutdown_beacon.trip();
                        let _ = lifecycle_control.notify_state_change();
                        serve_error = Some(error);
                        break;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        shutdown_beacon.trip();
                        let _ = lifecycle_control.notify_state_change();
                        serve_error = Some(error);
                        break;
                    }
                }
                // Reload work stays serialized inside the direct accept loop so the listener never
                // races a partially-applied runtime view update.
                if signals.take_reload() {
                    match reload_runtime_view() {
                        Ok(()) => tracing::info!(
                            "bounded lifecycle-control-triggered config/roster reload applied"
                        ),
                        Err(error) => tracing::warn!(
                            error_code = %error.code,
                            error_message = %error.message,
                            "bounded lifecycle-control-triggered config/roster reload rejected; last-known-good serving config retained"
                        ),
                    }
                    continue;
                }
                #[cfg(test)]
                if let Some(error) = take_injected_accept_error_for_test() {
                    shutdown_beacon.trip();
                    let _ = lifecycle_control.notify_state_change();
                    serve_error = Some(error);
                    break;
                }

                let mut stream = match listener.accept() {
                    Ok(stream) => stream,
                    Err(source) => {
                        shutdown_beacon.trip();
                        let _ = lifecycle_control.notify_state_change();
                        serve_error = Some(
                            AtmError::daemon_unavailable(
                                "failed while accepting daemon local IPC connection",
                            )
                            .with_recovery(
                                "Restart the daemon; the local IPC listener stopped accepting connections unexpectedly.",
                            )
                            .with_source(source),
                        );
                        break;
                    }
                };

                match signals.take_accept_error() {
                    Ok(Some(error)) => {
                        shutdown_beacon.trip();
                        let _ = lifecycle_control.notify_state_change();
                        serve_error = Some(error);
                        break;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        shutdown_beacon.trip();
                        let _ = lifecycle_control.notify_state_change();
                        serve_error = Some(error);
                        break;
                    }
                }
                if signals.take_reload() {
                    drop(stream);
                    match reload_runtime_view() {
                        Ok(()) => tracing::info!(
                            "bounded lifecycle-control-triggered config/roster reload applied"
                        ),
                        Err(error) => tracing::warn!(
                            error_code = %error.code,
                            error_message = %error.message,
                            "bounded lifecycle-control-triggered config/roster reload rejected; last-known-good serving config retained"
                        ),
                    }
                    continue;
                }
                if lifecycle_control.terminate_requested() || shutdown_beacon.is_tripped() {
                    shutdown_beacon.trip();
                    let _ = lifecycle_control.notify_state_change();
                    match write_shutdown_response(&mut stream, &codec)? {
                        ShutdownResponseOutcome::RejectedRequest => break,
                        ShutdownResponseOutcome::NoFrame if terminate_probe_pending => break,
                        ShutdownResponseOutcome::NoFrame => {
                            terminate_probe_pending = true;
                            if let Err(error) = schedule_delayed_listener_wake(
                                endpoint_path.clone(),
                                TERMINATE_REJECTION_GRACE_DEADLINE,
                            ) {
                                tracing::warn!(
                                    %error,
                                    path = %endpoint_path.display(),
                                    "failed to schedule delayed listener wake during shutdown probe"
                                );
                                let _ = wake_listener(&endpoint_path);
                                break;
                            }
                            continue;
                        }
                    }
                }
                terminate_probe_pending = false;
                if registry.active_connections() >= MAX_CONCURRENT_CONNECTIONS {
                    let response = ResponseEnvelope::Error(ProtocolErrorEnvelope::from_error(
                        &AtmError::daemon_unavailable(
                            "daemon connection cap exceeded (max 64 concurrent accepts)",
                        )
                        .with_recovery(
                            "Wait for in-flight ATM commands to complete before retrying, or reduce concurrent atm invocations.",
                        ),
                    ));
                    let frame = codec.response_to_frame(
                        atm_core::protocol::RequestId::new(TERMINATE_REJECTION_REQUEST_ID)?,
                        response,
                    )?;
                    let _ = stream.set_recv_timeout(Some(REQUEST_DEADLINE));
                    let _ = stream.set_send_timeout(Some(REQUEST_DEADLINE));
                    let _ = atm_core::protocol::write_frame(
                        &mut stream,
                        &frame,
                        "failed to write daemon rejection response frame",
                    );
                    let _ = stream.flush();
                    continue;
                }

                let active = registry.register();
                let dispatcher = Arc::clone(&dispatcher);
                let force_shutdown = Arc::clone(&force_shutdown);
                let registry = Arc::clone(&registry);
                let codec = codec.clone();
                thread::Builder::new()
                    .name("local-ipc-connection-worker".to_string())
                    .spawn_scoped(scope, move || {
                    let _active = active;
                    let result = catch_unwind(AssertUnwindSafe(|| {
                        handle_connection(
                            stream,
                            dispatcher,
                            force_shutdown.as_ref(),
                            registry,
                            codec,
                        )
                    }));
                    match result {
                        Ok(Ok(())) => {}
                        Ok(Err(error)) => {
                            tracing::warn!(%error, "daemon local IPC connection handling failed");
                        }
                        Err(_) => {
                            tracing::warn!(
                                "daemon local IPC connection worker panicked; the transport thread recovered and continued shutdown accounting"
                            );
                        }
                    }
                })
                .map_err(|source| {
                    AtmError::daemon_unavailable("failed to spawn local IPC connection worker")
                        .with_recovery(
                            "Restart the daemon after confirming the host can spawn same-host connection workers.",
                        )
                        .with_source(source)
                })?;
            }

            // Every serve-loop exit path, including AcceptError and internal tracked-work failures,
            // must transition through begin_shutdown() before finalization so RuntimeComposition
            // observes Running -> Draining -> Stopped instead of a silent hard stop.
            let mut shutdown_error = begin_shutdown().err();
            let shutdown_started = Instant::now();
            if let Err(error) = drain_active_connections_for_shutdown(
                registry.as_ref(),
                force_shutdown.as_ref(),
                graceful_drain_deadline,
                force_cancel_deadline,
                shutdown_started,
                TRACKED_DISPATCH_JOIN_DEADLINE,
            ) {
                if let Some(existing) = shutdown_error.as_ref() {
                    tracing::warn!(
                        begin_shutdown_error = %existing,
                        drain_error = %error,
                        "daemon shutdown drain failed after an earlier shutdown-start error"
                    );
                } else {
                    shutdown_error = Some(error);
                }
            }
            let _ = lifecycle_control.notify_state_change();
            if let Err(error) = wake_listener(endpoint_guard.endpoint_path()) {
                tracing::debug!(%error, "daemon local IPC listener wake was unnecessary during shutdown");
            }
            if lifecycle_waiter.join().is_err() {
                let error = AtmError::daemon_lifecycle_wedge(
                    "daemon lifecycle waiter panicked during transport shutdown",
                )
                .with_recovery(
                    "Restart the daemon; the same-host lifecycle waiter crashed while the runtime was transitioning out of serving state.",
                );
                if let Some(existing) = shutdown_error.as_ref() {
                    tracing::warn!(
                        begin_shutdown_error = %existing,
                        lifecycle_waiter_error = %error,
                        "daemon lifecycle waiter failed after an earlier shutdown error"
                    );
                } else {
                    shutdown_error = Some(error);
                }
            }
            if let Err(error) = lifecycle_control.shutdown_worker_with_timeout() {
                if let Some(existing) = shutdown_error.as_ref() {
                    tracing::warn!(
                        begin_shutdown_error = %existing,
                        lifecycle_worker_error = %error,
                        "daemon lifecycle worker shutdown failed after an earlier shutdown-start error"
                    );
                } else {
                    shutdown_error = Some(error);
                }
            }
            if let Err(error) = endpoint_guard.unpublish() {
                if let Some(existing) = shutdown_error.as_ref() {
                    tracing::warn!(
                        begin_shutdown_error = %existing,
                        endpoint_cleanup_error = %error,
                        "daemon endpoint cleanup failed after an earlier shutdown-start error"
                    );
                } else {
                    shutdown_error = Some(error);
                }
            }
            finalize_shutdown();
            if let Some(serve_error) = serve_error {
                if let Some(shutdown_error) = shutdown_error {
                    tracing::warn!(
                        %shutdown_error,
                        %serve_error,
                        "daemon shutdown encountered an additional error after a serve error"
                    );
                } else {
                    tracing::warn!(
                        %serve_error,
                        "daemon serve loop exited with an error after shutdown finalization"
                    );
                }
                return Err(serve_error);
            }
            shutdown_error.map_or(Ok(()), Err)
        })
    }

    pub(crate) fn take_endpoint_guard(&mut self) -> Result<SocketEndpointGuard, AtmError> {
        self.endpoint_guard.take().ok_or_else(|| {
            AtmError::daemon_unavailable("daemon local IPC endpoint guard was missing during runtime handoff")
                .with_recovery(
                    "Restart the daemon; same-host endpoint cleanup ownership was lost before the runtime began serving.",
                )
        })
    }
}

#[derive(Debug, Default)]
pub(crate) struct LocalIpcServerTransportAdapter;

impl LocalIpcServerTransportAdapter {
    pub(crate) const fn new() -> Self {
        Self
    }

    pub(crate) fn prepare_runtime(&self) -> Result<PreparedRuntimeServer, AtmError> {
        let endpoint_path = atm_core::protocol::daemon_socket_path()?;
        self.prepare_runtime_at_socket_path(endpoint_path)
    }

    pub(crate) fn prepare_runtime_at_socket_path(
        &self,
        endpoint_path: PathBuf,
    ) -> Result<PreparedRuntimeServer, AtmError> {
        PreparedRuntimeServer::bind(endpoint_path)
    }
}

impl boundary::sealed::Sealed for LocalIpcServerTransportAdapter {}

impl boundary::ServerTransport for LocalIpcServerTransportAdapter {
    fn serve(&self, _dispatcher: Arc<dyn RequestDispatcher + Send + Sync>) -> Result<(), AtmError> {
        Err(AtmError::daemon_unavailable(
            "LocalIpcServerTransportAdapter::serve cannot bootstrap the daemon directly; use RuntimeComposition::start()",
        )
        .with_recovery(
            "Enter the daemon through RuntimeComposition::start() so lifecycle state, host ownership, and shutdown handling stay consistent.",
        ))
    }
}

#[cfg(unix)]
fn prepare_local_ipc_endpoint(
    endpoint_path: &Path,
) -> Result<LocalIpcEndpointPreparation, AtmError> {
    if let Some(parent) = endpoint_path.parent() {
        fs::create_dir_all(parent).map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to create daemon local IPC directory at {}",
                parent.display()
            ))
            .with_recovery(
                "Grant write access to the daemon socket parent directory or choose a writable ATM_HOME before retrying.",
            )
            .with_source(source)
        })?;
    }
    remove_stale_endpoint(endpoint_path)?;
    Ok(LocalIpcEndpointPreparation::FilesystemEndpointPrepared)
}

#[cfg(not(unix))]
fn prepare_local_ipc_endpoint(
    _endpoint_path: &Path,
) -> Result<LocalIpcEndpointPreparation, AtmError> {
    // Windows named-pipe-backed local IPC does not allocate a filesystem socket path, so
    // endpoint preparation is explicitly a non-filesystem step rather than a silent no-op.
    Ok(LocalIpcEndpointPreparation::NonFilesystemEndpointPrepared)
}

#[cfg(unix)]
fn remove_stale_endpoint(endpoint_path: &Path) -> Result<(), AtmError> {
    if !endpoint_path.exists() {
        return Ok(());
    }
    match fs::remove_file(endpoint_path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(AtmError::daemon_unavailable(format!(
            "failed to remove stale daemon local IPC endpoint at {}",
            endpoint_path.display()
        ))
        .with_recovery(
            "Stop the conflicting daemon or remove the stale same-host socket path before restarting atm-daemon.",
        )
        .with_source(source)),
    }
}

fn write_shutdown_response(
    stream: &mut LocalSocketStream,
    codec: &JsonAtmProtocolCodec,
) -> Result<ShutdownResponseOutcome, AtmError> {
    let _ = stream.set_recv_timeout(Some(REQUEST_DEADLINE));
    let _ = stream.set_send_timeout(Some(REQUEST_DEADLINE));
    let Some(frame) = atm_core::protocol::read_frame(
        stream,
        "failed to read daemon request frame during shutdown rejection",
        "daemon request frame exceeded the maximum supported size during shutdown rejection",
    )?
    else {
        return Ok(ShutdownResponseOutcome::NoFrame);
    };
    let Ok((request_id, _request)) = codec.request_from_frame(frame) else {
        return Ok(ShutdownResponseOutcome::NoFrame);
    };
    let response = ResponseEnvelope::Error(ProtocolErrorEnvelope::from_error(
        &AtmError::daemon_unavailable("daemon is shutting down and not accepting new requests")
            .with_recovery("Retry the ATM command after the daemon restarts."),
    ));
    let frame = codec.response_to_frame(request_id, response)?;
    atm_core::protocol::write_frame(
        stream,
        &frame,
        "failed to write daemon shutdown rejection response frame",
    )?;
    stream.flush().map_err(|source| {
        AtmError::daemon_unavailable("failed to flush daemon shutdown rejection response frame")
            .with_recovery(
                "Retry the ATM command after the daemon restarts; the shutdown rejection response could not be delivered cleanly.",
            )
            .with_source(source)
    })?;
    Ok(ShutdownResponseOutcome::RejectedRequest)
}

#[cfg(test)]
static INJECTED_ACCEPT_ERROR_FOR_TEST: std::sync::Mutex<Option<std::sync::mpsc::SyncSender<()>>> =
    std::sync::Mutex::new(None);

#[cfg(test)]
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) fn install_injected_accept_error_for_test(
    signal: std::sync::mpsc::SyncSender<()>,
) -> InjectAcceptErrorGuard {
    *INJECTED_ACCEPT_ERROR_FOR_TEST
        .lock()
        .expect("accept error injection lock") = Some(signal);
    InjectAcceptErrorGuard
}

#[cfg(test)]
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) struct InjectAcceptErrorGuard;

#[cfg(test)]
impl Drop for InjectAcceptErrorGuard {
    fn drop(&mut self) {
        *INJECTED_ACCEPT_ERROR_FOR_TEST
            .lock()
            .expect("accept error injection lock") = None;
    }
}

#[cfg(test)]
fn take_injected_accept_error_for_test() -> Option<AtmError> {
    let sender = INJECTED_ACCEPT_ERROR_FOR_TEST
        .lock()
        .expect("accept error injection lock")
        .take()?;
    let _ = sender.send(());
    Some(
        AtmError::daemon_unavailable("injected daemon local IPC accept error for test")
            .with_recovery("Test-only injected accept failure."),
    )
}

fn emit_ready_signal_if_requested() -> Result<(), AtmError> {
    if std::env::var_os("ATM_DAEMON_READY_STDOUT").is_none() {
        return Ok(());
    }
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "ATM_DAEMON_READY").map_err(|source| {
        AtmError::daemon_unavailable("failed to emit daemon ready signal")
            .with_recovery(
                "Restart the daemon after confirming the parent process still accepts the ready signal on stdout.",
            )
            .with_source(source)
    })?;
    stdout.flush().map_err(|source| {
        AtmError::daemon_unavailable("failed to flush daemon ready signal")
            .with_recovery(
                "Restart the daemon after confirming the parent process still accepts the ready signal on stdout.",
            )
            .with_source(source)
    })?;
    Ok(())
}

fn handle_connection(
    mut stream: LocalSocketStream,
    dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
    force_shutdown: &AtomicBool,
    registry: Arc<ActiveConnectionRegistry>,
    codec: JsonAtmProtocolCodec,
) -> Result<(), AtmError> {
    if force_shutdown.load(Ordering::SeqCst) {
        return write_shutdown_response(&mut stream, &codec).map(|_| ());
    }
    stream
        .set_recv_timeout(Some(REQUEST_DEADLINE))
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to apply daemon request read deadline")
                .with_recovery(
                    "Restart the daemon; the same-host request socket could not apply its bounded read deadline.",
                )
                .with_source(source)
        })?;
    stream
        .set_send_timeout(Some(REQUEST_DEADLINE))
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to apply daemon response write deadline")
                .with_recovery(
                    "Restart the daemon; the same-host request socket could not apply its bounded write deadline.",
                )
                .with_source(source)
        })?;

    // Phase S still enforces one request per accepted same-host connection even though the
    // request runtime owns its execution separately from this socket receive loop.
    let Some(frame) = atm_core::protocol::read_frame(
        &mut stream,
        "failed to read daemon request frame",
        "daemon request frame exceeded the maximum supported size",
    )?
    else {
        return Ok(());
    };
    tracing::debug!(
        max_daemon_frame_bytes = atm_core::protocol::MAX_DAEMON_FRAME_BYTES,
        "daemon request frame accepted under configured size cap"
    );
    let (request_id, request) = codec.request_from_frame(frame)?;

    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
    let (completion_tx, completion_rx) = std::sync::mpsc::sync_channel(1);
    let dispatch_registry = Arc::clone(&registry);
    let dispatch_handle = std::thread::Builder::new()
        .name("local-ipc-dispatch".to_string())
        .spawn(move || {
            let _dispatch_work = dispatch_registry.register_dispatch_work();
            let _ = result_tx.send(dispatcher.dispatch(request));
            let _ = completion_tx.send(());
        })
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to spawn daemon local IPC dispatch worker")
                .with_recovery(
                    "Retry the ATM command after atm-daemon restarts; the same-host request worker could not be created.",
                )
                .with_source(source)
        })?;
    // The tracked-dispatch registry owns the eventual join. Timeouts can let a request worker run
    // past this socket exchange, but the direct accept loop and bounded shutdown sweep still reap
    // the handle before runtime teardown completes.
    registry.push_dispatch_handle(
        TrackedDispatchHandle {
            completion_rx,
            join_handle: dispatch_handle,
        },
        MAX_CONCURRENT_CONNECTIONS,
    )?;
    let response = match result_rx.recv_timeout(REQUEST_DEADLINE) {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => ResponseEnvelope::Error(ProtocolErrorEnvelope::from_error(&error)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            tracing::warn!("daemon request dispatcher exceeded the runtime deadline");
            ResponseEnvelope::Error(ProtocolErrorEnvelope::from_error(
                &AtmError::daemon_unavailable(
                    "daemon request exceeded the 3s runtime deadline; the operation may still complete in the background",
                )
                .with_recovery(
                    "Check the destination mailbox or service-side effects before retrying this ATM command.",
                ),
            ))
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            ResponseEnvelope::Error(ProtocolErrorEnvelope::from_error(
                &AtmError::daemon_unavailable(
                    "daemon request dispatcher stopped before returning a response",
                )
                .with_recovery(
                    "Retry the ATM command after the daemon finishes recovering the request runtime.",
                ),
            ))
        }
    };
    let frame = codec.response_to_frame(request_id, response)?;
    atm_core::protocol::write_frame(&mut stream, &frame, "failed to write daemon response frame")?;
    stream.flush().map_err(|source| {
        AtmError::daemon_unavailable("failed to flush daemon response frame")
            .with_recovery(
                "Retry the ATM command after the daemon finishes recovering the same-host request runtime.",
            )
            .with_source(source)
    })?;
    registry.reap_finished_dispatches()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::lifecycle_control::LifecycleControlSourceAdapter;
    #[cfg(unix)]
    use crate::test_support::{
        DoctorOnlyDispatcher, LifecycleFlagResetGuard, connect_daemon_local_ipc_until_ready,
    };
    #[cfg(unix)]
    use atm_core::boundary::RequestDispatcher;
    #[cfg(unix)]
    use atm_core::doctor::DoctorQuery;
    #[cfg(unix)]
    use atm_core::error_codes::AtmErrorCode;
    #[cfg(unix)]
    use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};
    #[cfg(unix)]
    use atm_core::test_support::EnvGuard;
    #[cfg(unix)]
    use serial_test::serial;
    use std::sync::Arc;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    #[cfg(unix)]
    use tempfile::TempDir;

    #[cfg(unix)]
    #[derive(Debug, Default)]
    struct PanicDispatcher;

    #[cfg(unix)]
    impl atm_core::boundary::sealed::Sealed for PanicDispatcher {}

    #[cfg(unix)]
    impl RequestDispatcher for PanicDispatcher {
        fn dispatch(
            &self,
            request: RequestEnvelope,
        ) -> Result<ResponseEnvelope, atm_core::error::AtmError> {
            panic!("intentional dispatcher panic for test: {request:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn prepare_local_ipc_endpoint_reports_filesystem_preparation() {
        let tempdir = TempDir::new().expect("tempdir");
        let endpoint = tempdir.path().join("daemon.sock");

        let result = prepare_local_ipc_endpoint(&endpoint).expect("prepare endpoint");

        assert_eq!(
            result,
            LocalIpcEndpointPreparation::FilesystemEndpointPrepared
        );
        assert!(endpoint.parent().expect("parent").exists());
    }

    #[cfg(windows)]
    #[test]
    fn prepare_local_ipc_endpoint_reports_non_filesystem_preparation() {
        let endpoint = PathBuf::from(r"\\.\pipe\atm-test-daemon");

        let result = prepare_local_ipc_endpoint(&endpoint).expect("prepare endpoint");

        assert_eq!(
            result,
            LocalIpcEndpointPreparation::NonFilesystemEndpointPrepared
        );
    }

    #[cfg(unix)]
    #[test]
    #[serial]
    fn accept_error_without_lifecycle_signal_exits_within_one_second() {
        let tempdir = TempDir::new().expect("tempdir");
        let atm_home = tempdir.path().join("atm-home");
        std::fs::create_dir_all(&atm_home).expect("atm home dir");
        let _env = EnvGuard::set_many([
            ("ATM_HOME", Some(atm_home.to_str().expect("utf8 atm home"))),
            ("ATM_LOG_DIR", None),
            ("HOME", Some(tempdir.path().to_str().expect("utf8 home"))),
            ("USERPROFILE", None),
        ]);
        let socket_path = tempdir.path().join("daemon.sock");
        let mut runtime = PreparedRuntimeServer::bind(socket_path).expect("prepare runtime");
        let endpoint_guard = runtime.take_endpoint_guard().expect("take endpoint guard");
        let lifecycle = LifecycleControlSourceAdapter::install().expect("install lifecycle");
        let _reset = LifecycleFlagResetGuard::install(lifecycle);
        let dispatcher: Arc<dyn RequestDispatcher + Send + Sync> = Arc::new(DoctorOnlyDispatcher);
        let (serve_result_tx, serve_result_rx) = mpsc::channel();
        let (inject_tx, inject_rx) = mpsc::sync_channel(1);
        let _inject_guard = install_injected_accept_error_for_test(inject_tx);

        let join = std::thread::spawn(move || {
            // These shortened deadlines validate immediate shutdown-beacon wake correctness, not
            // the production SLO values documented for the real daemon runtime.
            let result = runtime.serve_with_runtime_hooks(
                dispatcher,
                RuntimeServeHooks {
                    endpoint_guard,
                    graceful_drain_deadline: Duration::from_millis(500),
                    force_cancel_deadline: Duration::from_secs(2),
                    begin_shutdown: || Ok(()),
                    reload_runtime_view: || Ok(()),
                    finalize_shutdown: || {},
                    publish_ready: || Ok(()),
                },
            );
            serve_result_tx.send(result).expect("send serve result");
        });

        inject_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("accept error should inject within 1s");
        let shutdown_started = Instant::now();
        let error = serve_result_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("serve result should arrive within 1s")
            .expect_err("serve should fail after injected accept error");
        assert!(
            shutdown_started.elapsed() <= Duration::from_secs(1),
            "lifecycle waiter should observe the shutdown beacon and exit within 1s"
        );
        assert!(error.message.contains("accept error") || error.message.contains("accepting"));
        join.join().expect("join serve thread");
    }

    #[cfg(unix)]
    #[test]
    #[serial]
    fn accept_after_terminate_returns_typed_shutdown_error() {
        let tempdir = TempDir::new().expect("tempdir");
        let atm_home = tempdir.path().join("atm-home");
        std::fs::create_dir_all(&atm_home).expect("atm home dir");
        let _env = EnvGuard::set_many([
            ("ATM_HOME", Some(atm_home.to_str().expect("utf8 atm home"))),
            ("ATM_LOG_DIR", None),
            ("HOME", Some(tempdir.path().to_str().expect("utf8 home"))),
            ("USERPROFILE", None),
        ]);
        let socket_path = tempdir.path().join("daemon.sock");
        let mut runtime =
            PreparedRuntimeServer::bind(socket_path.clone()).expect("prepare runtime");
        let endpoint_guard = runtime.take_endpoint_guard().expect("take endpoint guard");
        let lifecycle = LifecycleControlSourceAdapter::install().expect("install lifecycle");
        let _reset = LifecycleFlagResetGuard::install(lifecycle.clone());
        let dispatcher: Arc<dyn RequestDispatcher + Send + Sync> = Arc::new(DoctorOnlyDispatcher);
        let (serve_result_tx, serve_result_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);

        let join = std::thread::spawn(move || {
            // These shortened deadlines validate immediate shutdown-beacon wake correctness, not
            // the production SLO values documented for the real daemon runtime.
            let result = runtime.serve_with_runtime_hooks(
                dispatcher,
                RuntimeServeHooks {
                    endpoint_guard,
                    graceful_drain_deadline: Duration::from_millis(500),
                    force_cancel_deadline: Duration::from_secs(2),
                    begin_shutdown: || Ok(()),
                    reload_runtime_view: || Ok(()),
                    finalize_shutdown: || {},
                    publish_ready: move || {
                        ready_tx.send(()).map_err(|_| {
                            AtmError::daemon_unavailable(
                                "shutdown rejection test failed to observe the daemon ready signal",
                            )
                            .with_recovery(
                                "Restore the bounded ready-signal handshake before retrying the same-host daemon shutdown rejection test.",
                            )
                        })
                    },
                },
            );
            serve_result_tx.send(result).expect("send serve result");
        });

        lifecycle.terminate_flag().store(true, Ordering::SeqCst);
        let mut stream = connect_daemon_local_ipc_until_ready(&socket_path, ready_rx);
        stream
            .set_send_timeout(Some(Duration::from_secs(5)))
            .expect("set send timeout");
        stream
            .set_recv_timeout(Some(Duration::from_secs(5)))
            .expect("set recv timeout");
        let request = RequestEnvelope::Doctor(DoctorQuery {
            home_dir: tempdir.path().join("home"),
            current_dir: tempdir.path().join("cwd"),
            team_override: None,
        });
        let request_id = atm_core::protocol::next_request_id();
        let frame =
            atm_core::protocol::request_to_frame_payload(request_id, request).expect("frame");
        atm_core::protocol::write_frame(&mut stream, &frame, "write doctor frame").expect("write");
        std::io::Write::flush(&mut stream).expect("flush");
        let response_frame = atm_core::protocol::read_frame(
            &mut stream,
            "read shutdown response frame",
            "shutdown response frame too large",
        )
        .expect("read frame")
        .expect("response frame");
        let (response_id, response) =
            atm_core::protocol::response_from_frame_payload(response_frame)
                .expect("decode response");
        assert_eq!(response_id, request_id);
        match response {
            ResponseEnvelope::Error(error) => {
                assert_eq!(error.code, AtmErrorCode::DaemonUnavailable);
                assert!(error.message.contains("shutting down"));
            }
            other => panic!("unexpected shutdown response: {other:?}"),
        }

        serve_result_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("recv serve result")
            .expect("serve runtime result");
        join.join().expect("join serve thread");
    }

    #[cfg(unix)]
    #[test]
    #[serial]
    fn panic_in_dispatch_still_cleans_up_socket_path_on_shutdown() {
        let tempdir = TempDir::new().expect("tempdir");
        let socket_path = tempdir.path().join("daemon.sock");
        let mut runtime =
            PreparedRuntimeServer::bind(socket_path.clone()).expect("prepare runtime");
        let endpoint_guard = runtime.take_endpoint_guard().expect("take endpoint guard");
        let lifecycle = LifecycleControlSourceAdapter::install().expect("install lifecycle");
        let _reset = LifecycleFlagResetGuard::install(lifecycle.clone());
        let dispatcher: Arc<dyn RequestDispatcher + Send + Sync> = Arc::new(PanicDispatcher);
        let (serve_result_tx, serve_result_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);

        let join = std::thread::spawn(move || {
            let result = runtime.serve_with_runtime_hooks(
                dispatcher,
                RuntimeServeHooks {
                    endpoint_guard,
                    graceful_drain_deadline: Duration::from_millis(500),
                    force_cancel_deadline: Duration::from_secs(2),
                    begin_shutdown: || Ok(()),
                    reload_runtime_view: || Ok(()),
                    finalize_shutdown: || {},
                    publish_ready: move || {
                        ready_tx.send(()).map_err(|_| {
                            AtmError::daemon_unavailable(
                                "panic recovery test failed to observe the daemon ready signal",
                            )
                            .with_recovery(
                                "Restore the bounded ready-signal handshake before retrying the same-host panic recovery test.",
                            )
                        })
                    },
                },
            );
            serve_result_tx.send(result).expect("send serve result");
        });

        let mut stream = connect_daemon_local_ipc_until_ready(&socket_path, ready_rx);
        stream
            .set_send_timeout(Some(Duration::from_secs(5)))
            .expect("set send timeout");
        stream
            .set_recv_timeout(Some(Duration::from_secs(5)))
            .expect("set recv timeout");
        let request = RequestEnvelope::Doctor(DoctorQuery {
            home_dir: tempdir.path().join("home"),
            current_dir: tempdir.path().join("cwd"),
            team_override: None,
        });
        let request_id = atm_core::protocol::next_request_id();
        let frame =
            atm_core::protocol::request_to_frame_payload(request_id, request).expect("frame");
        atm_core::protocol::write_frame(&mut stream, &frame, "write doctor frame").expect("write");
        std::io::Write::flush(&mut stream).expect("flush");
        let response_frame = atm_core::protocol::read_frame(
            &mut stream,
            "read panic response",
            "panic frame too large",
        )
        .expect("read frame")
        .expect("response frame");
        let (response_id, response) =
            atm_core::protocol::response_from_frame_payload(response_frame)
                .expect("decode response");
        assert_eq!(response_id, request_id);
        assert!(matches!(response, ResponseEnvelope::Error(_)));

        lifecycle.set_terminate_for_test(true);
        serve_result_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("recv serve result")
            .expect("serve runtime result");
        join.join().expect("join serve thread");
        assert!(
            !socket_path.exists(),
            "socket endpoint should be removed during shutdown even after a dispatch panic"
        );
    }

    #[test]
    fn bounded_drain_returns_after_force_cancel_deadline() {
        let registry = Arc::new(ActiveConnectionRegistry::default());
        let active_connection = registry.register();
        let dispatch_registry = Arc::clone(&registry);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let (completion_tx, completion_rx) = mpsc::sync_channel(1);
        let join_handle = std::thread::spawn(move || {
            let _dispatch = dispatch_registry.register_dispatch_work();
            let _ = release_rx.recv();
            let _ = completion_tx.send(());
        });
        registry
            .push_dispatch_handle(
                TrackedDispatchHandle {
                    completion_rx,
                    join_handle,
                },
                1,
            )
            .expect("dispatch handle push");
        let force_shutdown = AtomicBool::new(false);
        let shutdown_started = Instant::now();
        let error = drain_active_connections_for_shutdown(
            registry.as_ref(),
            &force_shutdown,
            Duration::from_millis(10),
            Duration::from_millis(20),
            Instant::now(),
            TRACKED_DISPATCH_JOIN_DEADLINE,
        )
        .expect_err("bounded drain should fail once the forced-cancel deadline elapses");
        assert!(
            shutdown_started.elapsed() < Duration::from_secs(1),
            "forced cancel should bound shutdown even when tracked request work never completes"
        );
        assert!(force_shutdown.load(Ordering::SeqCst));
        assert!(
            error
                .message
                .contains("tracked daemon dispatch worker exceeded the shutdown join deadline")
        );
        let _ = release_tx.send(());
        drop(active_connection);
    }

    #[test]
    fn tracked_dispatch_handle_capacity_overflow_returns_lifecycle_wedge() {
        let registry = ActiveConnectionRegistry::default();

        let (release_first_tx, release_first_rx) = mpsc::sync_channel(1);
        let (completion_first_tx, completion_first_rx) = mpsc::sync_channel(1);
        let first_join = std::thread::spawn(move || {
            let _ = release_first_rx.recv();
            let _ = completion_first_tx.send(());
        });
        registry
            .push_dispatch_handle(
                TrackedDispatchHandle {
                    completion_rx: completion_first_rx,
                    join_handle: first_join,
                },
                1,
            )
            .expect("first handle should fit within capacity");

        let (completion_second_tx, completion_second_rx) = mpsc::sync_channel(1);
        let second_join = std::thread::spawn(move || {
            let _ = completion_second_tx.send(());
        });
        let error = registry
            .push_dispatch_handle(
                TrackedDispatchHandle {
                    completion_rx: completion_second_rx,
                    join_handle: second_join,
                },
                1,
            )
            .expect_err("second handle should be rejected once the bounded registry is full");
        assert!(
            error
                .message
                .contains("tracked daemon dispatch registry exceeded its bounded capacity"),
            "unexpected error: {error:?}"
        );

        let _ = release_first_tx.send(());
        registry
            .join_tracked_dispatches(Duration::from_secs(1))
            .expect("drain first tracked dispatch");
    }
}
