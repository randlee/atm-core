use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use atm_core::boundary::{self, AtmProtocol, RequestDispatcher};
use atm_core::error::AtmError;
use atm_core::protocol::{JsonAtmProtocolCodec, ProtocolErrorEnvelope, ResponseEnvelope};
use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{
    Listener as LocalSocketListener, ListenerOptions, Stream as LocalSocketStream,
};

use crate::host_ownership::{HostOwnershipAdapter, HostOwnershipGuard};
use crate::lifecycle_control::LifecycleControlSourceAdapter;

#[cfg(unix)]
use std::fs;
use std::thread;

const MAX_CONCURRENT_CONNECTIONS: usize = 64;
const REQUEST_DEADLINE: Duration = Duration::from_secs(3);

enum ServeEvent {
    Connection(LocalSocketStream),
    Reload,
    Terminate,
    AcceptError(AtmError),
}

#[derive(Debug, Default)]
pub(crate) struct ActiveConnectionRegistry {
    active_connections: AtomicUsize,
    active_dispatches: AtomicUsize,
    dispatch_handles: Mutex<Vec<JoinHandle<()>>>,
    drain_state: Mutex<()>,
    drain_wake: Condvar,
}

impl ActiveConnectionRegistry {
    pub(crate) fn register(self: &Arc<Self>) -> ActiveConnectionGuard {
        self.active_connections.fetch_add(1, Ordering::SeqCst);
        ActiveConnectionGuard {
            registry: Arc::clone(self),
        }
    }

    fn register_dispatch_work(self: &Arc<Self>) -> ActiveDispatchGuard {
        self.active_dispatches.fetch_add(1, Ordering::SeqCst);
        ActiveDispatchGuard {
            registry: Arc::clone(self),
        }
    }

    pub(crate) fn active_connections(&self) -> usize {
        self.active_connections.load(Ordering::SeqCst)
    }

    pub(crate) fn active_work_items(&self) -> usize {
        self.active_connections.load(Ordering::SeqCst)
            + self.active_dispatches.load(Ordering::SeqCst)
    }

    pub(crate) fn interrupt_all(&self) -> Result<(), AtmError> {
        self.drain_wake.notify_all();
        Ok(())
    }

    fn lock_dispatch_handles(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, Vec<JoinHandle<()>>>, AtmError> {
        self.dispatch_handles
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("active dispatch handle lock poisoned"))
    }

    fn join_tracked_dispatches(&self) -> Result<(), AtmError> {
        let handles = {
            let mut handles = self.dispatch_handles.lock().map_err(|_| {
                AtmError::daemon_unavailable("active dispatch handle lock poisoned")
            })?;
            std::mem::take(&mut *handles)
        };
        for handle in handles {
            handle
                .join()
                .map_err(|_| AtmError::daemon_unavailable("daemon dispatch thread panicked"))?;
        }
        Ok(())
    }

    pub(crate) fn wait_for_connection_change(&self, timeout: Duration) -> Result<(), AtmError> {
        let state = self
            .drain_state
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("active connection drain lock poisoned"))?;
        let (_state, wait_result) = self
            .drain_wake
            .wait_timeout(state, timeout)
            .map_err(|_| AtmError::daemon_unavailable("active connection drain lock poisoned"))?;
        if wait_result.timed_out() {
            return Ok(());
        }
        Ok(())
    }

    fn release_connection(&self) {
        self.active_connections.fetch_sub(1, Ordering::SeqCst);
        self.drain_wake.notify_all();
    }
}

pub(crate) struct ActiveConnectionGuard {
    registry: Arc<ActiveConnectionRegistry>,
}

struct ActiveDispatchGuard {
    registry: Arc<ActiveConnectionRegistry>,
}

impl Drop for ActiveConnectionGuard {
    fn drop(&mut self) {
        self.registry.release_connection();
    }
}

impl Drop for ActiveDispatchGuard {
    fn drop(&mut self) {
        self.registry
            .active_dispatches
            .fetch_sub(1, Ordering::SeqCst);
        self.registry.drain_wake.notify_all();
    }
}

pub(crate) struct PreparedRuntimeServer {
    _ownership: HostOwnershipGuard,
    endpoint_path: PathBuf,
    listener: LocalSocketListener,
    lifecycle_control: LifecycleControlSourceAdapter,
    registry: Arc<ActiveConnectionRegistry>,
    force_shutdown: Arc<AtomicBool>,
    codec: JsonAtmProtocolCodec,
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
        prepare_local_ipc_endpoint(&endpoint_path)?;
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
                .with_source(source)
            })?;
        tracing::info!(
            max_concurrent_connections = MAX_CONCURRENT_CONNECTIONS,
            max_daemon_frame_bytes = atm_core::protocol::MAX_DAEMON_FRAME_BYTES,
            request_deadline_ms = REQUEST_DEADLINE.as_millis() as u64,
            "daemon local IPC transport limits configured"
        );
        emit_ready_signal_if_requested()?;
        Ok(Self {
            _ownership: ownership,
            endpoint_path,
            listener,
            lifecycle_control,
            registry: Arc::new(ActiveConnectionRegistry::default()),
            force_shutdown: Arc::new(AtomicBool::new(false)),
            codec: JsonAtmProtocolCodec,
        })
    }

    pub(crate) fn serve_with_runtime_hooks<BeginShutdown, ReloadRuntimeView, FinalizeShutdown>(
        self,
        dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
        graceful_drain_deadline: Duration,
        force_cancel_deadline: Duration,
        begin_shutdown: BeginShutdown,
        reload_runtime_view: ReloadRuntimeView,
        finalize_shutdown: FinalizeShutdown,
    ) -> Result<(), AtmError>
    where
        BeginShutdown: Fn() -> Result<(), AtmError>,
        ReloadRuntimeView: Fn() -> Result<(), AtmError>,
        FinalizeShutdown: Fn(),
    {
        self.serve_with_deadlines_and_accept_probe(
            dispatcher,
            graceful_drain_deadline,
            force_cancel_deadline,
            begin_shutdown,
            reload_runtime_view,
            finalize_shutdown,
        )
    }

    fn serve_with_deadlines_and_accept_probe<BeginShutdown, ReloadRuntimeView, FinalizeShutdown>(
        self,
        dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
        graceful_drain_deadline: Duration,
        force_cancel_deadline: Duration,
        begin_shutdown: BeginShutdown,
        reload_runtime_view: ReloadRuntimeView,
        finalize_shutdown: FinalizeShutdown,
    ) -> Result<(), AtmError>
    where
        BeginShutdown: Fn() -> Result<(), AtmError>,
        ReloadRuntimeView: Fn() -> Result<(), AtmError>,
        FinalizeShutdown: Fn(),
    {
        let Self {
            _ownership,
            endpoint_path,
            listener,
            lifecycle_control,
            registry,
            force_shutdown,
            codec,
        } = self;
        thread::scope(|scope| -> Result<(), AtmError> {
            let mut serve_error = None;
            let (event_tx, event_rx) = std::sync::mpsc::channel();
            {
                let event_tx = event_tx.clone();
                let lifecycle_control = lifecycle_control.clone();
                let endpoint_path = endpoint_path.clone();
                scope.spawn(move || {
                    let mut observed_generation = match lifecycle_control.event_generation() {
                        Ok(generation) => generation,
                        Err(error) => {
                            let _ = event_tx.send(ServeEvent::AcceptError(error));
                            return;
                        }
                    };
                    loop {
                        if let Err(error) =
                            lifecycle_control.wait_for_state_change(&mut observed_generation)
                        {
                            let _ = event_tx.send(ServeEvent::AcceptError(error));
                            return;
                        }
                        if lifecycle_control.terminate_requested() {
                            let _ = wake_listener(&endpoint_path);
                            let _ = event_tx.send(ServeEvent::Terminate);
                            return;
                        }
                        if lifecycle_control.take_reload_requested() {
                            let _ = event_tx.send(ServeEvent::Reload);
                        }
                    }
                });
            }
            {
                let event_tx = event_tx.clone();
                let lifecycle_control = lifecycle_control.clone();
                scope.spawn(move || {
                    loop {
                        match listener.accept() {
                            Ok(stream) => {
                                if lifecycle_control.terminate_requested() {
                                    return;
                                }
                                if event_tx.send(ServeEvent::Connection(stream)).is_err() {
                                    return;
                                }
                            }
                            Err(source) => {
                                let error = AtmError::daemon_unavailable(
                                    "failed while accepting daemon local IPC connection",
                                )
                                .with_source(source);
                                let _ = event_tx.send(ServeEvent::AcceptError(error));
                                return;
                            }
                        }
                    }
                });
            }
            loop {
                match event_rx.recv().map_err(|source| {
                    AtmError::daemon_unavailable(
                        "daemon local IPC accept loop event channel disconnected",
                    )
                    .with_source(source)
                })? {
                    ServeEvent::Connection(mut stream) => {
                        if registry.active_connections() >= MAX_CONCURRENT_CONNECTIONS {
                            let response = ResponseEnvelope::Error(
                                ProtocolErrorEnvelope::from_error(
                                    &AtmError::daemon_unavailable(
                                        "daemon connection cap exceeded (max 64 concurrent accepts)",
                                    )
                                    .with_recovery(
                                        "Wait for in-flight ATM commands to complete before retrying, or reduce concurrent atm invocations.",
                                    ),
                                ),
                            );
                            let frame = codec.response_to_frame(1, response)?;
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
                        scope.spawn(move || {
                            let _active = active;
                            if let Err(error) = handle_connection(
                                stream,
                                dispatcher,
                                force_shutdown.as_ref(),
                                registry,
                                codec,
                            ) {
                                tracing::warn!(%error, "daemon local IPC connection handling failed");
                            }
                        });
                    }
                    ServeEvent::Reload => match reload_runtime_view() {
                        Ok(()) => tracing::info!(
                            "bounded lifecycle-control-triggered config/roster reload applied"
                        ),
                        Err(error) => tracing::warn!(
                            error_code = %error.code,
                            error_message = %error.message,
                            "bounded lifecycle-control-triggered config/roster reload rejected; last-known-good serving config retained"
                        ),
                    },
                    ServeEvent::Terminate => {
                        break;
                    }
                    ServeEvent::AcceptError(error) => {
                        serve_error = Some(error);
                        break;
                    }
                }
            }

            let mut shutdown_error = begin_shutdown().err();
            let shutdown_started = Instant::now();
            if let Err(error) = drain_active_connections_for_shutdown(
                registry.as_ref(),
                force_shutdown.as_ref(),
                graceful_drain_deadline,
                force_cancel_deadline,
                shutdown_started,
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
            finalize_shutdown();
            if let Some(serve_error) = serve_error {
                if let Some(shutdown_error) = shutdown_error {
                    tracing::warn!(
                        %shutdown_error,
                        %serve_error,
                        "daemon shutdown encountered an additional error after a serve error"
                    );
                }
                return Err(serve_error);
            }
            shutdown_error.map_or(Ok(()), Err)
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
fn prepare_local_ipc_endpoint(endpoint_path: &Path) -> Result<(), AtmError> {
    if let Some(parent) = endpoint_path.parent() {
        fs::create_dir_all(parent).map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to create daemon local IPC directory at {}",
                parent.display()
            ))
            .with_source(source)
        })?;
    }
    remove_stale_endpoint(endpoint_path)
}

#[cfg(not(unix))]
fn prepare_local_ipc_endpoint(_endpoint_path: &Path) -> Result<(), AtmError> {
    // TODO(S.2/ADR-007): Windows local IPC endpoint preparation belongs here once
    // the adapter owns the named-pipe endpoint lifecycle on non-Unix hosts.
    Ok(())
}

#[cfg(unix)]
fn remove_stale_endpoint(endpoint_path: &Path) -> Result<(), AtmError> {
    if !endpoint_path.exists() {
        return Ok(());
    }
    fs::remove_file(endpoint_path).map_err(|source| {
        AtmError::daemon_unavailable(format!(
            "failed to remove stale daemon local IPC endpoint at {}",
            endpoint_path.display()
        ))
        .with_source(source)
    })
}

fn wake_listener(endpoint_path: &Path) -> Result<(), AtmError> {
    let name = atm_core::protocol::daemon_local_ipc_name_from_path(endpoint_path)?;
    let mut stream = LocalSocketStream::connect(name).map_err(|source| {
        AtmError::daemon_unavailable(format!(
            "failed to wake daemon local IPC listener at {}",
            endpoint_path.display()
        ))
        .with_source(source)
    })?;
    stream
        .set_send_timeout(Some(REQUEST_DEADLINE))
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to apply daemon listener wake timeout")
                .with_source(source)
        })?;
    stream.flush().map_err(|source| {
        AtmError::daemon_unavailable("failed to flush daemon listener wake signal")
            .with_source(source)
    })?;
    Ok(())
}

fn emit_ready_signal_if_requested() -> Result<(), AtmError> {
    if std::env::var_os("ATM_DAEMON_READY_STDOUT").is_none() {
        return Ok(());
    }
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "ATM_DAEMON_READY").map_err(|source| {
        AtmError::daemon_unavailable("failed to emit daemon ready signal").with_source(source)
    })?;
    stdout.flush().map_err(|source| {
        AtmError::daemon_unavailable("failed to flush daemon ready signal").with_source(source)
    })?;
    Ok(())
}

fn drain_active_connections_for_shutdown(
    registry: &ActiveConnectionRegistry,
    force_shutdown: &AtomicBool,
    graceful_drain_deadline: Duration,
    force_cancel_deadline: Duration,
    shutdown_started: Instant,
) -> Result<(), AtmError> {
    tracing::info!(
        active_connections = registry.active_connections(),
        active_work_items = registry.active_work_items(),
        "daemon shutdown signal received; starting graceful drain"
    );
    let graceful_deadline = shutdown_started + graceful_drain_deadline;
    let force_cancel_deadline = shutdown_started + force_cancel_deadline;
    while registry.active_work_items() > 0 && Instant::now() < graceful_deadline {
        registry.wait_for_connection_change(
            graceful_deadline.saturating_duration_since(Instant::now()),
        )?;
    }
    if registry.active_work_items() > 0 {
        tracing::info!(
            active_work_items = registry.active_work_items(),
            "daemon graceful drain hit deadline; continuing toward forced cancel"
        );
        force_shutdown.store(true, Ordering::SeqCst);
        registry.interrupt_all()?;
    } else {
        tracing::info!("daemon graceful drain completed cleanly");
    }
    while registry.active_work_items() > 0 && Instant::now() < force_cancel_deadline {
        registry.wait_for_connection_change(
            force_cancel_deadline.saturating_duration_since(Instant::now()),
        )?;
    }
    registry.join_tracked_dispatches()?;
    let remaining_work_items = registry.active_work_items();
    if remaining_work_items > 0 {
        return Err(AtmError::daemon_unavailable(format!(
            "forced cancel deadline elapsed with {remaining_work_items} active daemon work item(s)"
        )));
    }
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
        return Ok(());
    }
    stream
        .set_recv_timeout(Some(REQUEST_DEADLINE))
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to apply daemon request read deadline")
                .with_source(source)
        })?;
    stream
        .set_send_timeout(Some(REQUEST_DEADLINE))
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to apply daemon response write deadline")
                .with_source(source)
        })?;

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
    let dispatch_registry = Arc::clone(&registry);
    let dispatch_handle = std::thread::spawn(move || {
        let _dispatch_work = dispatch_registry.register_dispatch_work();
        let _ = result_tx.send(dispatcher.dispatch(request));
    });
    match registry.lock_dispatch_handles() {
        Ok(mut handles) => handles.push(dispatch_handle),
        Err(error) => {
            let _ = dispatch_handle.join();
            return Err(error);
        }
    }
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
        AtmError::daemon_unavailable("failed to flush daemon response frame").with_source(source)
    })?;
    Ok(())
}
