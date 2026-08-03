use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use atm_core::api::ApiRouter;
use atm_core::error::AtmError;
use atm_core::protocol::ResponseEnvelope;
use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{
    Listener as LocalSocketListener, ListenerNonblockingMode, ListenerOptions,
    Stream as LocalSocketStream,
};

#[cfg(test)]
use crate::DaemonSubsystem;
use crate::SubsystemObservability;
use crate::active_connection_registry::ActiveConnectionRegistry;
use crate::daemon_worker_join::{
    CompletionTrackedJoinHandle, JoinTimeoutPolicy, LOCAL_WORKER_JOIN_DEADLINE, join_with_timeout,
};
use crate::host_ownership::{HostOwnershipAdapter, HostOwnershipGuard};
use crate::lifecycle_control::LifecycleControlSourceAdapter;
use crate::local_admission::{BOUNDED_ADMISSION_RETRY_INTERVAL, send_with_bounded_admission};
use crate::local_ipc_connection::drain_active_connections_for_shutdown;
#[cfg(unix)]
use crate::local_tcp_transport::LocalTcpLoopbackServer;
use crate::shutdown_beacon::ShutdownBeacon;

mod accept_loop;
pub(crate) mod shutdown;

use std::thread;

#[cfg(unix)]
use crate::local_ipc_transport::shutdown::remove_stale_endpoint;
#[cfg(test)]
pub(crate) use crate::request_worker::DISPATCH_PANIC_RECOVERED_MESSAGE;
#[cfg(test)]
pub(crate) use crate::request_worker::install_injected_accept_error_for_test;
use crate::request_worker::{DispatchWorkerPool, handle_connection};
use accept_loop::{handle_shutdown_probe, take_accept_error};
use shutdown::{
    emit_ready_signal_if_requested, finalize_serve_loop, finish_serve_shutdown,
    prepare_local_ipc_endpoint, record_serve_error, record_shutdown_signal,
    write_shutdown_response,
};

// Same-host ATM traffic is unary request/response, so this cap only needs to
// comfortably exceed realistic single-host caller fan-out while still bounding
// per-connection worker threads and shutdown drain pressure.
pub(crate) const MAX_CONCURRENT_CONNECTIONS: usize = 64;
const REQUEST_DEADLINE: Duration = Duration::from_secs(3);
// Polling a nonblocking listener keeps shutdown portable across Unix UDS and
// Windows named pipes without opening a dummy connection that is not HTTP.
const LOCAL_IPC_ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(1);
// A bounded queue absorbs normal concurrent accept bursts. Only an actually
// saturated queue retries so lifecycle checks stay prompt without imposing a
// scheduler delay on every otherwise-admissible connection.
const CONNECTION_ADMISSION_QUEUE: usize = MAX_CONCURRENT_CONNECTIONS;
const LOCAL_IPC_WORKER_JOIN_POLICY: JoinTimeoutPolicy = JoinTimeoutPolicy {
    subsystem: "local_ipc_transport",
    worker_kind: "local IPC connection worker",
    panic_message: "local IPC connection worker panicked during shutdown",
    timeout_message: "local IPC connection worker exceeded the shutdown join deadline",
};
#[cfg(unix)]
type TcpLoopbackWorker<'scope> = (
    Arc<AtomicBool>,
    std::thread::ScopedJoinHandle<'scope, Result<(), AtmError>>,
);
pub(crate) const CONNECTION_WORKER_PANIC_RECOVERED_MESSAGE: &str =
    "daemon local IPC connection worker panicked; transport thread recovered";
#[derive(Debug, Default)]
struct ServeLoopSignals {
    // Mutex-backed slot: AtmError is non-Copy so cannot be stored atomically without unsafe;
    // single-error semantics do not benefit from a channel because the scoped waiter thread
    // cannot outlive the owner.
    accept_error: Mutex<Option<AtmError>>,
}

impl ServeLoopSignals {
    fn record_accept_error(&self, error: AtmError) -> Result<(), AtmError> {
        let mut slot = self.accept_error.lock().map_err(|_| {
            AtmError::daemon_unavailable("serve-loop accept-error state lock poisoned")
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
        let _ = self.preparation;
        #[cfg(unix)]
        {
            remove_stale_endpoint(self.endpoint_path())?;
        }
        Ok(())
    }
}

impl Drop for SocketEndpointGuard {
    fn drop(&mut self) {
        if let Err(error) = self.unpublish() {
            tracing::warn!(
                subsystem = "local_ipc_transport",
                action = "endpoint_drop_cleanup",
                outcome = "failed",
                %error,
                "daemon local IPC endpoint cleanup failed during drop"
            );
        }
    }
}

pub(crate) struct PreparedRuntimeServer {
    host_ownership_guard: HostOwnershipGuard,
    listener: LocalSocketListener,
    #[cfg(unix)]
    tcp_loopback: LocalTcpLoopbackServer,
    #[cfg(test)]
    accept_error_inject: Option<std::sync::mpsc::SyncSender<()>>,
    lifecycle_control: LifecycleControlSourceAdapter,
    registry: Arc<ActiveConnectionRegistry>,
    force_shutdown: Arc<AtomicBool>,
    observability: SubsystemObservability,
    // The endpoint guard must drop after the listener and connection registry have been torn
    // down so same-host endpoint unpublication never races a still-live serving resource.
    endpoint_guard: Option<SocketEndpointGuard>,
}

pub(crate) struct RuntimeServeHooks<BeginShutdown, ReloadRuntimeView, PublishReady> {
    pub(crate) endpoint_guard: SocketEndpointGuard,
    pub(crate) graceful_drain_deadline: Duration,
    pub(crate) force_cancel_deadline: Duration,
    pub(crate) begin_shutdown: BeginShutdown,
    pub(crate) reload_runtime_view: ReloadRuntimeView,
    pub(crate) publish_ready: PublishReady,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalIpcEndpointPreparation {
    FilesystemEndpointPrepared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShutdownResponseOutcome {
    NoFrame,
    RejectedRequest,
}

enum AcceptLoopOutcome {
    Continue,
    Break(Option<AtmError>),
    Dispatch(LocalSocketStream),
}

struct AcceptLoopContext<'a> {
    listener: &'a LocalSocketListener,
    lifecycle_control: &'a LifecycleControlSourceAdapter,
    observability: &'a SubsystemObservability,
    connection_workers: &'a ConnectionWorkerPool,
    signals: &'a ServeLoopSignals,
    shutdown_beacon: &'a ShutdownBeacon,
    #[cfg(test)]
    accept_error_inject: &'a mut Option<std::sync::mpsc::SyncSender<()>>,
}

/// Fixed same-host connection workers with one bounded admission queue.
///
/// The queue absorbs a short accept burst without serializing every handoff;
/// when it is full, the listener retries with lifecycle checks and the OS
/// listener backlog supplies further backpressure.
struct ConnectionWorkerPool {
    sender: std::sync::mpsc::SyncSender<LocalSocketStream>,
    workers: Mutex<Vec<CompletionTrackedJoinHandle<()>>>,
    #[cfg(test)]
    saturated_admission_signal: Mutex<Option<std::sync::mpsc::SyncSender<()>>>,
    #[cfg(test)]
    shutdown_join_signal: Mutex<Option<std::sync::mpsc::SyncSender<()>>>,
}

impl ConnectionWorkerPool {
    fn start(
        force_shutdown: Arc<AtomicBool>,
        registry: Arc<ActiveConnectionRegistry>,
        observability: SubsystemObservability,
        dispatch_workers: Arc<DispatchWorkerPool>,
    ) -> Result<Self, AtmError> {
        let (sender, receiver) = std::sync::mpsc::sync_channel(CONNECTION_ADMISSION_QUEUE);
        let receiver = Arc::new(Mutex::new(receiver));
        let mut workers = Vec::with_capacity(MAX_CONCURRENT_CONNECTIONS);
        for worker_index in 0..MAX_CONCURRENT_CONNECTIONS {
            let receiver = Arc::clone(&receiver);
            let force_shutdown = Arc::clone(&force_shutdown);
            let registry = Arc::clone(&registry);
            let observability = observability.clone();
            let dispatch_workers = Arc::clone(&dispatch_workers);
            let (completion_tx, completion_rx) = std::sync::mpsc::sync_channel(1);
            let worker = std::thread::Builder::new()
                .name(format!("local-ipc-connection-{worker_index}"))
                .spawn(move || {
                    let _completion_tx = completion_tx;
                    loop {
                        let stream = match receiver.lock() {
                            Ok(receiver) => receiver.recv(),
                            Err(_) => return,
                        };
                        let Ok(stream) = stream else {
                            return;
                        };
                        let _active = registry.register();
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            handle_connection(
                                stream,
                                force_shutdown.as_ref(),
                                dispatch_workers.as_ref(),
                                &observability,
                            )
                        }));
                        match result {
                            Ok(Ok(())) => {}
                            Ok(Err(error)) => tracing::warn!(
                                subsystem = "local_ipc_transport",
                                action = "connection_worker",
                                outcome = "classified_failure",
                                %error,
                                "daemon local IPC connection handling failed"
                            ),
                            Err(_) => observability.emit_or_warn(
                                "connection_worker",
                                "panic",
                                CONNECTION_WORKER_PANIC_RECOVERED_MESSAGE,
                            ),
                        }
                    }
                })
                .map_err(|_| {
                    AtmError::daemon_unavailable("failed to start local IPC connection worker")
                })?;
            workers.push(CompletionTrackedJoinHandle {
                completion_rx,
                join_handle: worker,
            });
        }
        Ok(Self {
            sender,
            workers: Mutex::new(workers),
            #[cfg(test)]
            saturated_admission_signal: Mutex::new(None),
            #[cfg(test)]
            shutdown_join_signal: Mutex::new(None),
        })
    }

    fn dispatch(
        &self,
        stream: LocalSocketStream,
        lifecycle_control: &LifecycleControlSourceAdapter,
        shutdown_beacon: &ShutdownBeacon,
    ) -> Result<(), AtmError> {
        Self::ensure_admission_open(lifecycle_control, shutdown_beacon)?;
        send_with_bounded_admission(
            &self.sender,
            stream,
            || {
                #[cfg(test)]
                self.signal_saturated_admission_for_test();
                Self::ensure_admission_open(lifecycle_control, shutdown_beacon)?;
                Ok(BOUNDED_ADMISSION_RETRY_INTERVAL)
            },
            "local IPC connection workers stopped accepting work",
        )
    }

    fn ensure_admission_open(
        lifecycle_control: &LifecycleControlSourceAdapter,
        shutdown_beacon: &ShutdownBeacon,
    ) -> Result<(), AtmError> {
        if lifecycle_control.terminate_requested() || shutdown_beacon.is_tripped() {
            return Err(AtmError::daemon_unavailable(
                "daemon local IPC connection admission stopped during shutdown",
            ));
        }
        Ok(())
    }

    #[cfg(test)]
    fn install_saturated_admission_signal_for_test(&self, signal: std::sync::mpsc::SyncSender<()>) {
        *self
            .saturated_admission_signal
            .lock()
            .expect("lock saturated-admission test signal") = Some(signal);
    }

    #[cfg(test)]
    fn signal_saturated_admission_for_test(&self) {
        if let Some(signal) = self
            .saturated_admission_signal
            .lock()
            .expect("lock saturated-admission test signal")
            .take()
        {
            let _ = signal.send(());
        }
    }

    #[cfg(test)]
    fn install_shutdown_join_signal_for_test(&self, signal: std::sync::mpsc::SyncSender<()>) {
        *self
            .shutdown_join_signal
            .lock()
            .expect("lock connection-worker shutdown-join test signal") = Some(signal);
    }

    #[cfg(test)]
    fn signal_shutdown_join_for_test(signal_slot: &Mutex<Option<std::sync::mpsc::SyncSender<()>>>) {
        if let Some(signal) = signal_slot
            .lock()
            .expect("lock connection-worker shutdown-join test signal")
            .take()
        {
            let _ = signal.send(());
        }
    }

    fn shutdown(self) -> Result<(), AtmError> {
        let Self {
            sender,
            workers,
            #[cfg(test)]
            shutdown_join_signal,
            ..
        } = self;
        drop(sender);
        // The sender must be gone before joining: otherwise idle workers can
        // remain in `recv` and turn ordinary shutdown into a deadlock.
        #[cfg(test)]
        Self::signal_shutdown_join_for_test(&shutdown_join_signal);
        for worker in workers.into_inner().map_err(|_| {
            AtmError::daemon_unavailable("local IPC connection worker lock poisoned")
        })? {
            join_with_timeout(
                worker,
                LOCAL_WORKER_JOIN_DEADLINE,
                LOCAL_IPC_WORKER_JOIN_POLICY,
            )?;
        }
        Ok(())
    }
}

struct ServeShutdownContext<'a> {
    endpoint_guard: SocketEndpointGuard,
    graceful_drain_deadline: Duration,
    force_cancel_deadline: Duration,
    registry: &'a Arc<ActiveConnectionRegistry>,
    force_shutdown: &'a AtomicBool,
    lifecycle_control: &'a LifecycleControlSourceAdapter,
}

struct ServeRuntimeScopeContext<'a, BeginShutdown, ReloadRuntimeView, PublishReady> {
    listener: &'a LocalSocketListener,
    dispatcher: Arc<dyn ApiRouter + Send + Sync>,
    #[cfg(test)]
    accept_error_inject: &'a mut Option<std::sync::mpsc::SyncSender<()>>,
    lifecycle_control: LifecycleControlSourceAdapter,
    registry: Arc<ActiveConnectionRegistry>,
    force_shutdown: Arc<AtomicBool>,
    observability: SubsystemObservability,
    endpoint_guard: SocketEndpointGuard,
    #[cfg(unix)]
    tcp_loopback: LocalTcpLoopbackServer,
    graceful_drain_deadline: Duration,
    force_cancel_deadline: Duration,
    begin_shutdown: BeginShutdown,
    reload_runtime_view: ReloadRuntimeView,
    publish_ready: PublishReady,
}

impl std::fmt::Debug for PreparedRuntimeServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedRuntimeServer")
            .field("registry", &self.registry)
            .finish_non_exhaustive()
    }
}

impl PreparedRuntimeServer {
    #[cfg(test)]
    pub(crate) fn install_accept_error_injection_for_test(
        &mut self,
        signal: std::sync::mpsc::SyncSender<()>,
    ) {
        self.accept_error_inject = Some(signal);
    }

    fn bind_with_observability(
        endpoint_path: PathBuf,
        observability: SubsystemObservability,
        host_ownership_observability: SubsystemObservability,
        lifecycle_observability: SubsystemObservability,
    ) -> Result<Self, AtmError> {
        // Install lifecycle control before singleton ownership so daemon startup never
        // claims the host-wide owner lock unless shutdown/reload hooks are ready too.
        // Reversing this order can transiently block a healthy daemon restart behind an
        // instance that failed before it could service lifecycle-control requests.
        let lifecycle_control =
            LifecycleControlSourceAdapter::install_with_observability(lifecycle_observability)?;
        let ownership =
            HostOwnershipAdapter::new_with_observability(host_ownership_observability).acquire()?;
        Self::bind_after_install(endpoint_path, observability, lifecycle_control, ownership)
    }

    #[cfg(test)]
    fn bind_with_observability_and_home_for_test(
        endpoint_path: PathBuf,
        host_home_dir: &std::path::Path,
        observability: SubsystemObservability,
        host_ownership_observability: SubsystemObservability,
        lifecycle_observability: SubsystemObservability,
    ) -> Result<Self, AtmError> {
        let lifecycle_control =
            LifecycleControlSourceAdapter::install_with_observability(lifecycle_observability)?;
        let ownership = HostOwnershipAdapter::new_with_observability(host_ownership_observability)
            .acquire_at_home_for_test(host_home_dir)?;
        Self::bind_after_install(endpoint_path, observability, lifecycle_control, ownership)
    }

    fn bind_after_install(
        endpoint_path: PathBuf,
        observability: SubsystemObservability,
        lifecycle_control: LifecycleControlSourceAdapter,
        ownership: HostOwnershipGuard,
    ) -> Result<Self, AtmError> {
        let endpoint_preparation = prepare_local_ipc_endpoint(&endpoint_path)?;
        #[cfg(unix)]
        let tcp_loopback = LocalTcpLoopbackServer::bind_in_runtime_dir(
            endpoint_path.parent().ok_or_else(|| {
                AtmError::daemon_unavailable("daemon local IPC endpoint has no runtime directory")
            })?,
            ownership.instance_id(),
        )?;
        let listener = ListenerOptions::new()
            .name(atm_core::protocol::daemon_local_ipc_name_from_path(
                &endpoint_path,
            )?)
            .nonblocking(ListenerNonblockingMode::Accept)
            .create_sync()
            .map_err(|_source| {
                AtmError::daemon_unavailable(format!(
                    "failed to bind daemon local IPC endpoint at {}",
                    endpoint_path.display()
                ))
            })?;
        tracing::info!(
            max_concurrent_connections = MAX_CONCURRENT_CONNECTIONS,
            max_http_request_body_bytes = atm_core::MAX_HTTP_REQUEST_BODY_BYTES,
            request_deadline_ms = REQUEST_DEADLINE.as_millis() as u64,
            endpoint_preparation = ?endpoint_preparation,
            "daemon local IPC transport limits configured"
        );
        observability.emit_or_warn(
            "bind_listener",
            "ok",
            "daemon local IPC transport prepared the runtime listener",
        );
        emit_ready_signal_if_requested()?;
        Ok(Self {
            host_ownership_guard: ownership,
            endpoint_guard: Some(SocketEndpointGuard::new(
                endpoint_path,
                endpoint_preparation,
            )),
            #[cfg(unix)]
            tcp_loopback,
            listener,
            #[cfg(test)]
            accept_error_inject: None,
            lifecycle_control,
            registry: Arc::new(ActiveConnectionRegistry::default()),
            force_shutdown: Arc::new(AtomicBool::new(false)),
            observability,
        })
    }

    pub(crate) fn serve_with_runtime_hooks<BeginShutdown, ReloadRuntimeView, PublishReady>(
        self,
        dispatcher: Arc<dyn ApiRouter + Send + Sync>,
        hooks: RuntimeServeHooks<BeginShutdown, ReloadRuntimeView, PublishReady>,
    ) -> Result<(), AtmError>
    where
        BeginShutdown: Fn() -> Result<(), AtmError>,
        ReloadRuntimeView: Fn() -> Result<(), AtmError> + Send,
        PublishReady: Fn() -> Result<(), AtmError>,
    {
        self.serve_with_deadlines_and_accept_probe(dispatcher, hooks)
    }

    fn serve_with_deadlines_and_accept_probe<BeginShutdown, ReloadRuntimeView, PublishReady>(
        self,
        dispatcher: Arc<dyn ApiRouter + Send + Sync>,
        hooks: RuntimeServeHooks<BeginShutdown, ReloadRuntimeView, PublishReady>,
    ) -> Result<(), AtmError>
    where
        BeginShutdown: Fn() -> Result<(), AtmError>,
        ReloadRuntimeView: Fn() -> Result<(), AtmError> + Send,
        PublishReady: Fn() -> Result<(), AtmError>,
    {
        let RuntimeServeHooks {
            endpoint_guard,
            graceful_drain_deadline,
            force_cancel_deadline,
            begin_shutdown,
            reload_runtime_view,
            publish_ready,
        } = hooks;
        let Self {
            host_ownership_guard: _host_ownership_guard,
            endpoint_guard: _endpoint_guard,
            listener,
            #[cfg(test)]
            mut accept_error_inject,
            lifecycle_control,
            registry,
            force_shutdown,
            observability,
            #[cfg(unix)]
            tcp_loopback,
        } = self;
        let serve_context = ServeRuntimeScopeContext {
            listener: &listener,
            dispatcher,
            #[cfg(test)]
            accept_error_inject: &mut accept_error_inject,
            lifecycle_control,
            registry,
            force_shutdown,
            observability,
            endpoint_guard,
            #[cfg(unix)]
            tcp_loopback,
            graceful_drain_deadline,
            force_cancel_deadline,
            begin_shutdown,
            reload_runtime_view,
            publish_ready,
        };
        thread::scope(|scope| serve_runtime_scope(scope, serve_context))
    }

    pub(crate) fn take_endpoint_guard(&mut self) -> Result<SocketEndpointGuard, AtmError> {
        self.endpoint_guard.take().ok_or_else(|| {
            AtmError::daemon_unavailable(
                "daemon local IPC endpoint guard was missing during runtime handoff",
            )
        })
    }
}

fn serve_runtime_scope<'scope, BeginShutdown, ReloadRuntimeView, PublishReady>(
    scope: &'scope thread::Scope<'scope, '_>,
    context: ServeRuntimeScopeContext<'_, BeginShutdown, ReloadRuntimeView, PublishReady>,
) -> Result<(), AtmError>
where
    BeginShutdown: Fn() -> Result<(), AtmError>,
    ReloadRuntimeView: Fn() -> Result<(), AtmError> + Send + 'scope,
    PublishReady: Fn() -> Result<(), AtmError>,
{
    let ServeRuntimeScopeContext {
        listener,
        dispatcher,
        #[cfg(test)]
        accept_error_inject,
        lifecycle_control,
        registry,
        force_shutdown,
        observability,
        endpoint_guard,
        #[cfg(unix)]
        tcp_loopback,
        graceful_drain_deadline,
        force_cancel_deadline,
        begin_shutdown,
        reload_runtime_view,
        publish_ready,
    } = context;
    let (shutdown_beacon, signals) = new_serve_loop_state();
    let lifecycle_waiter = spawn_runtime_lifecycle_waiter(
        scope,
        &signals,
        &shutdown_beacon,
        &lifecycle_control,
        reload_runtime_view,
    )?;
    publish_ready()?;
    let pools = start_worker_pools(&dispatcher, &registry, &force_shutdown, &observability)?;
    #[cfg(unix)]
    let (stop, tcp) = start_tcp(
        scope,
        tcp_loopback,
        (&pools, &observability, &lifecycle_control),
    )?;
    let mut accept_context = build_accept_context(
        listener,
        &lifecycle_control,
        &observability,
        &pools.connection_workers,
        signals.as_ref(),
        shutdown_beacon.as_ref(),
        #[cfg(test)]
        accept_error_inject,
    );
    let serve_error = capture_serve_error(scope, &mut accept_context);
    #[cfg(unix)]
    stop_unix_tcp_accept_loop(&stop);
    let shutdown_error = finalize_runtime_scope(
        &begin_shutdown,
        endpoint_guard,
        graceful_drain_deadline,
        force_cancel_deadline,
        &registry,
        force_shutdown.as_ref(),
        &lifecycle_control,
        lifecycle_waiter,
    );
    #[cfg(unix)]
    let worker_shutdown_error =
        shutdown_runtime_worker_pools(pools).or(finish_tcp_loopback_server(tcp)?);
    #[cfg(not(unix))]
    let worker_shutdown_error = shutdown_runtime_worker_pools(pools);
    let shutdown_error = shutdown_error.or(worker_shutdown_error);
    finish_serve_shutdown(serve_error, shutdown_error)
}

#[cfg(unix)]
fn stop_unix_tcp_accept_loop(stop: &AtomicBool) {
    stop.store(true, Ordering::SeqCst);
}

fn new_serve_loop_state() -> (Arc<ShutdownBeacon>, Arc<ServeLoopSignals>) {
    (
        Arc::new(ShutdownBeacon::default()),
        Arc::new(ServeLoopSignals::default()),
    )
}

#[cfg(unix)]
fn start_tcp<'scope>(
    scope: &'scope thread::Scope<'scope, '_>,
    tcp_loopback: LocalTcpLoopbackServer,
    runtime: (
        &RuntimeWorkerPools,
        &SubsystemObservability,
        &LifecycleControlSourceAdapter,
    ),
) -> Result<TcpLoopbackWorker<'scope>, AtmError> {
    let (worker_pools, observability, lifecycle_control) = runtime;
    start_tcp_loopback_server(
        scope,
        tcp_loopback,
        Arc::clone(&worker_pools.dispatch_workers),
        observability.clone(),
        lifecycle_control.clone(),
    )
}

struct RuntimeWorkerPools {
    connection_workers: ConnectionWorkerPool,
    dispatch_workers: Arc<DispatchWorkerPool>,
}

fn start_worker_pools(
    dispatcher: &Arc<dyn ApiRouter + Send + Sync>,
    registry: &Arc<ActiveConnectionRegistry>,
    force_shutdown: &Arc<AtomicBool>,
    observability: &SubsystemObservability,
) -> Result<RuntimeWorkerPools, AtmError> {
    let dispatch_workers = DispatchWorkerPool::start(
        Arc::clone(dispatcher),
        Arc::clone(registry),
        observability.clone(),
        MAX_CONCURRENT_CONNECTIONS,
    )?;
    let connection_workers = ConnectionWorkerPool::start(
        Arc::clone(force_shutdown),
        Arc::clone(registry),
        observability.clone(),
        Arc::clone(&dispatch_workers),
    )?;
    Ok(RuntimeWorkerPools {
        connection_workers,
        dispatch_workers,
    })
}

fn shutdown_runtime_worker_pools(worker_pools: RuntimeWorkerPools) -> Option<AtmError> {
    let connection_error = worker_pools.connection_workers.shutdown().err();
    let dispatch_error = worker_pools.dispatch_workers.shutdown().err();
    connection_error.or(dispatch_error)
}

#[allow(clippy::too_many_arguments)]
fn finalize_runtime_scope<BeginShutdown>(
    begin_shutdown: &BeginShutdown,
    endpoint_guard: SocketEndpointGuard,
    graceful_drain_deadline: Duration,
    force_cancel_deadline: Duration,
    registry: &Arc<ActiveConnectionRegistry>,
    force_shutdown: &AtomicBool,
    lifecycle_control: &LifecycleControlSourceAdapter,
    lifecycle_waiter: std::thread::ScopedJoinHandle<'_, ()>,
) -> Option<AtmError>
where
    BeginShutdown: Fn() -> Result<(), AtmError>,
{
    finalize_serve_loop(
        begin_shutdown,
        ServeShutdownContext {
            endpoint_guard,
            graceful_drain_deadline,
            force_cancel_deadline,
            registry,
            force_shutdown,
            lifecycle_control,
        },
        lifecycle_waiter,
    )
}

fn spawn_runtime_lifecycle_waiter<'scope, ReloadRuntimeView>(
    scope: &'scope thread::Scope<'scope, '_>,
    signals: &Arc<ServeLoopSignals>,
    shutdown_beacon: &Arc<ShutdownBeacon>,
    lifecycle: &LifecycleControlSourceAdapter,
    reload_runtime_view: ReloadRuntimeView,
) -> Result<std::thread::ScopedJoinHandle<'scope, ()>, AtmError>
where
    ReloadRuntimeView: Fn() -> Result<(), AtmError> + Send + 'scope,
{
    spawn_lifecycle_waiter(
        scope,
        Arc::clone(signals),
        Arc::clone(shutdown_beacon),
        lifecycle.clone(),
        reload_runtime_view,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_accept_context<'a>(
    listener: &'a LocalSocketListener,
    lifecycle_control: &'a LifecycleControlSourceAdapter,
    observability: &'a SubsystemObservability,
    connection_workers: &'a ConnectionWorkerPool,
    signals: &'a ServeLoopSignals,
    shutdown_beacon: &'a ShutdownBeacon,
    #[cfg(test)] accept_error_inject: &'a mut Option<std::sync::mpsc::SyncSender<()>>,
) -> AcceptLoopContext<'a> {
    AcceptLoopContext {
        listener,
        lifecycle_control,
        observability,
        connection_workers,
        signals,
        shutdown_beacon,
        #[cfg(test)]
        accept_error_inject,
    }
}

#[cfg(unix)]
fn start_tcp_loopback_server<'scope>(
    scope: &'scope thread::Scope<'scope, '_>,
    server: LocalTcpLoopbackServer,
    dispatch_workers: Arc<DispatchWorkerPool>,
    observability: SubsystemObservability,
    lifecycle: LifecycleControlSourceAdapter,
) -> Result<TcpLoopbackWorker<'scope>, AtmError> {
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop);
    let worker = thread::Builder::new()
        .name("local-loopback-tcp-http".to_string())
        .spawn_scoped(scope, move || {
            server.serve_until_terminated(dispatch_workers, observability, &lifecycle, worker_stop)
        })
        .map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to start local loopback TCP HTTP listener: {source}"
            ))
        })?;
    Ok((stop, worker))
}

#[cfg(unix)]
fn finish_tcp_loopback_server(
    server: std::thread::ScopedJoinHandle<'_, Result<(), AtmError>>,
) -> Result<Option<AtmError>, AtmError> {
    server
        .join()
        .map_err(|_| AtmError::daemon_unavailable("local loopback TCP HTTP listener panicked"))
        .map(Result::err)
}

fn capture_serve_error<'scope>(
    scope: &'scope thread::Scope<'scope, '_>,
    accept_context: &mut AcceptLoopContext<'_>,
) -> Option<AtmError> {
    match run_accept_loop(scope, accept_context) {
        Ok(serve_error) => serve_error,
        Err(error) => Some(error),
    }
}

fn spawn_lifecycle_waiter<'scope, 'env, ReloadRuntimeView>(
    scope: &'scope thread::Scope<'scope, 'env>,
    signals: Arc<ServeLoopSignals>,
    shutdown_beacon: Arc<ShutdownBeacon>,
    lifecycle_control: LifecycleControlSourceAdapter,
    reload_runtime_view: ReloadRuntimeView,
) -> Result<std::thread::ScopedJoinHandle<'scope, ()>, AtmError>
where
    ReloadRuntimeView: Fn() -> Result<(), AtmError> + Send + 'scope,
{
    thread::Builder::new()
        .name("local-ipc-lifecycle-waiter".to_string())
        .spawn_scoped(scope, move || {
            let mut observed_generation = match lifecycle_control.event_generation() {
                Ok(generation) => generation,
                Err(error) => {
                    record_shutdown_signal(&lifecycle_control, shutdown_beacon.as_ref());
                    let _ = signals.record_accept_error(error);
                    return;
                }
            };
            loop {
                if shutdown_beacon.is_tripped() {
                    return;
                }
                if let Err(error) = lifecycle_control.wait_for_state_change(&mut observed_generation) {
                    record_shutdown_signal(&lifecycle_control, shutdown_beacon.as_ref());
                    let _ = signals.record_accept_error(error);
                    return;
                }
                if shutdown_beacon.is_tripped() {
                    return;
                }
                if lifecycle_control.terminate_requested() {
                    record_shutdown_signal(&lifecycle_control, shutdown_beacon.as_ref());
                    return;
                }
                if lifecycle_control.take_reload_requested() {
                    match reload_runtime_view() {
                        Ok(()) => tracing::info!(
                            subsystem = "local_ipc_transport",
                            action = "reload_runtime_view",
                            "lifecycle-control-triggered config/roster reload applied outside the accept loop"
                        ),
                        Err(error) => tracing::warn!(
                            subsystem = "local_ipc_transport",
                            action = "reload_runtime_view",
                            outcome = "rejected",
                            error_code = %error.code(),
                            error_message = %error.message(),
                            "lifecycle-control-triggered config/roster reload rejected; last-known-good serving config retained"
                        ),
                    }
                }
            }
        })
        .map_err(|_source| {
            AtmError::daemon_unavailable("failed to spawn local IPC lifecycle waiter")


        })
}

fn run_accept_loop<'scope>(
    scope: &'scope thread::Scope<'scope, '_>,
    context: &mut AcceptLoopContext<'_>,
) -> Result<Option<AtmError>, AtmError> {
    loop {
        match prepare_accept_iteration(context)? {
            AcceptLoopOutcome::Continue => continue,
            AcceptLoopOutcome::Break(error) => return Ok(error),
            AcceptLoopOutcome::Dispatch(stream) => {
                match handle_accepted_stream(scope, stream, context)? {
                    AcceptLoopOutcome::Continue => continue,
                    AcceptLoopOutcome::Break(error) => return Ok(error),
                    AcceptLoopOutcome::Dispatch(_) => unreachable!("dispatch stream consumed"),
                }
            }
        }
    }
}

fn prepare_accept_iteration(
    context: &mut AcceptLoopContext<'_>,
) -> Result<AcceptLoopOutcome, AtmError> {
    // Completed dispatch handles are reaped by their connection worker after
    // its response is written. Reaping the entire shared handle list before
    // every accepted connection made a burst of unary admissions repeatedly
    // scan and lock the same list (quadratic under load) without improving
    // correctness or shutdown ownership.
    if let Some(error) = take_accept_error(
        context.signals,
        context.lifecycle_control,
        context.shutdown_beacon,
    )? {
        return Ok(AcceptLoopOutcome::Break(Some(error)));
    }
    if context.shutdown_beacon.is_tripped() {
        return Ok(AcceptLoopOutcome::Break(None));
    }
    #[cfg(test)]
    if let Some(sender) = context.accept_error_inject.take() {
        let _ = sender.send(());
        context.observability.emit_or_warn(
            "accept_loop",
            "failed",
            "injected daemon local IPC accept error for test",
        );
        return Ok(AcceptLoopOutcome::Break(Some(record_serve_error(
            context.lifecycle_control,
            context.shutdown_beacon,
            AtmError::daemon_unavailable("injected daemon local IPC accept error for test"),
        ))));
    }
    match context.listener.accept() {
        Ok(stream) => Ok(AcceptLoopOutcome::Dispatch(stream)),
        Err(source) if source.kind() == ErrorKind::WouldBlock => {
            std::thread::sleep(LOCAL_IPC_ACCEPT_POLL_INTERVAL);
            Ok(AcceptLoopOutcome::Continue)
        }
        Err(_source) => {
            context.observability.emit_or_warn(
                "accept_loop",
                "failed",
                "daemon local IPC listener stopped accepting connections unexpectedly",
            );
            Ok(AcceptLoopOutcome::Break(Some(record_serve_error(
                context.lifecycle_control,
                context.shutdown_beacon,
                AtmError::daemon_unavailable("failed while accepting daemon local IPC connection"),
            ))))
        }
    }
}

fn handle_accepted_stream<'scope>(
    scope: &'scope thread::Scope<'scope, '_>,
    mut stream: LocalSocketStream,
    context: &AcceptLoopContext<'_>,
) -> Result<AcceptLoopOutcome, AtmError> {
    if let Some(error) = take_accept_error(
        context.signals,
        context.lifecycle_control,
        context.shutdown_beacon,
    )? {
        return Ok(AcceptLoopOutcome::Break(Some(error)));
    }
    if context.lifecycle_control.terminate_requested() || context.shutdown_beacon.is_tripped() {
        return handle_shutdown_probe(
            &mut stream,
            context.lifecycle_control,
            context.shutdown_beacon,
        );
    }
    let _ = scope;
    context.connection_workers.dispatch(
        stream,
        context.lifecycle_control,
        context.shutdown_beacon,
    )?;
    Ok(AcceptLoopOutcome::Continue)
}

#[derive(Debug, Clone)]
pub(crate) struct LocalIpcServerTransportAdapter {
    observability: SubsystemObservability,
    host_ownership_observability: SubsystemObservability,
    lifecycle_observability: SubsystemObservability,
}

impl LocalIpcServerTransportAdapter {
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::new_with_observability(
            SubsystemObservability::disabled(DaemonSubsystem::LocalIpcTransport),
            SubsystemObservability::disabled(DaemonSubsystem::HostOwnership),
            SubsystemObservability::disabled(DaemonSubsystem::LifecycleControl),
        )
    }

    pub(crate) fn new_with_observability(
        observability: SubsystemObservability,
        host_ownership_observability: SubsystemObservability,
        lifecycle_observability: SubsystemObservability,
    ) -> Self {
        Self {
            observability,
            host_ownership_observability,
            lifecycle_observability,
        }
    }

    pub(crate) fn prepare_runtime(&self) -> Result<PreparedRuntimeServer, AtmError> {
        // Runtime endpoints and their local-HTTP capability record are
        // host-singleton artifacts. `ATM_HOME` selects request/config state;
        // it must not publish a second endpoint for another workspace.
        self.prepare_runtime_at_socket_path(atm_core::protocol::daemon_socket_path()?)
    }

    pub(crate) fn prepare_runtime_at_socket_path(
        &self,
        endpoint_path: PathBuf,
    ) -> Result<PreparedRuntimeServer, AtmError> {
        PreparedRuntimeServer::bind_with_observability(
            endpoint_path,
            self.observability.clone(),
            self.host_ownership_observability.clone(),
            self.lifecycle_observability.clone(),
        )
    }

    #[cfg(test)]
    pub(crate) fn prepare_runtime_at_socket_path_for_home(
        &self,
        endpoint_path: PathBuf,
        host_home_dir: &std::path::Path,
    ) -> Result<PreparedRuntimeServer, AtmError> {
        PreparedRuntimeServer::bind_with_observability_and_home_for_test(
            endpoint_path,
            host_home_dir,
            self.observability.clone(),
            self.host_ownership_observability.clone(),
            self.lifecycle_observability.clone(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::active_connection_registry::TrackedDispatchHandle;
    #[cfg(unix)]
    use atm_core::api::{
        ApiRequest, ApiResponse, ApiRouter, AuthenticatedIngress, RequestDeadline,
    };
    #[cfg(unix)]
    use atm_core::doctor::DoctorQuery;
    #[cfg(unix)]
    use std::sync::Condvar;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    use tempfile::TempDir;

    #[cfg(unix)]
    struct BlockingDoctorDispatcher {
        started: mpsc::Sender<()>,
        release: Arc<(Mutex<bool>, Condvar)>,
    }

    #[cfg(unix)]
    impl atm_core::boundary::sealed::Sealed for BlockingDoctorDispatcher {}

    #[cfg(unix)]
    impl ApiRouter for BlockingDoctorDispatcher {
        fn route(
            &self,
            _request: ApiRequest,
            _ingress: AuthenticatedIngress,
            _deadline: RequestDeadline,
        ) -> Result<ApiResponse, AtmError> {
            self.started
                .send(())
                .map_err(|_| AtmError::daemon_unavailable("test lost dispatch start signal"))?;
            let (released, wake) = self.release.as_ref();
            let mut released = released
                .lock()
                .map_err(|_| AtmError::daemon_unavailable("test release lock poisoned"))?;
            while !*released {
                released = wake
                    .wait(released)
                    .map_err(|_| AtmError::daemon_unavailable("test release wait poisoned"))?;
            }
            Ok(ApiResponse::new(ResponseEnvelope::Error(
                AtmError::daemon_unavailable("test dispatch released"),
            )))
        }
    }

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

    #[test]
    fn local_ipc_accept_is_nonblocking_for_shutdown_polling() {
        let tempdir = TempDir::new().expect("tempdir");
        let endpoint = tempdir.path().join("nonblocking.sock");
        let name =
            atm_core::protocol::daemon_local_ipc_name_from_path(&endpoint).expect("socket name");
        let listener = ListenerOptions::new()
            .name(name)
            .nonblocking(ListenerNonblockingMode::Accept)
            .create_sync()
            .expect("bind nonblocking listener");

        let started = Instant::now();
        let error = listener
            .accept()
            .expect_err("an idle listener must not block waiting for a dummy wake connection");

        assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "nonblocking accept took too long: {:?}",
            started.elapsed()
        );
    }

    #[cfg(unix)]
    #[test]
    fn saturated_dispatch_shutdown_unblocks_the_unix_connection_handoff() {
        let tempdir = TempDir::new().expect("tempdir");
        let socket_path = tempdir.path().join("saturated-dispatch.sock");
        let socket_name = atm_core::protocol::daemon_local_ipc_name_from_path(&socket_path)
            .expect("socket name")
            .into_owned();
        let listener = ListenerOptions::new()
            .name(socket_name.clone())
            .create_sync()
            .expect("bind test listener");
        let lifecycle = LifecycleControlSourceAdapter::new_for_test();
        let shutdown_beacon = ShutdownBeacon::default();
        let force_shutdown = Arc::new(AtomicBool::new(false));
        let registry = Arc::new(ActiveConnectionRegistry::default());
        let (started_tx, started_rx) = mpsc::channel();
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let dispatch_workers = DispatchWorkerPool::start(
            Arc::new(BlockingDoctorDispatcher {
                started: started_tx,
                release: Arc::clone(&release),
            }),
            Arc::clone(&registry),
            SubsystemObservability::disabled(DaemonSubsystem::LocalIpcTransport),
            MAX_CONCURRENT_CONNECTIONS,
        )
        .expect("start saturated dispatch workers");
        let connection_workers = ConnectionWorkerPool::start(
            Arc::clone(&force_shutdown),
            registry,
            SubsystemObservability::disabled(DaemonSubsystem::LocalIpcTransport),
            Arc::clone(&dispatch_workers),
        )
        .expect("start connection workers");
        let request = atm_core::protocol::RequestEnvelope::Doctor(DoctorQuery::default());
        let mut clients = Vec::with_capacity(MAX_CONCURRENT_CONNECTIONS);

        for _ in 0..MAX_CONCURRENT_CONNECTIONS {
            let client = LocalSocketStream::connect(socket_name.clone()).expect("connect client");
            let server = listener.accept().expect("accept client");
            connection_workers
                .dispatch(server, &lifecycle, &shutdown_beacon)
                .expect("admit connection worker");
            clients.push(client);
        }
        for client in &mut clients {
            atm_core::api::write_http_request(client, &request).expect("write doctor request");
            client.flush().expect("flush doctor request");
        }
        for _ in 0..MAX_CONCURRENT_CONNECTIONS {
            started_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("every dispatch worker is occupied");
        }

        let mut queued_clients = Vec::with_capacity(CONNECTION_ADMISSION_QUEUE);
        for _ in 0..CONNECTION_ADMISSION_QUEUE {
            let client =
                LocalSocketStream::connect(socket_name.clone()).expect("connect queued client");
            let server = listener.accept().expect("accept queued client");
            connection_workers
                .dispatch(server, &lifecycle, &shutdown_beacon)
                .expect("queue bounded connection handoff");
            queued_clients.push(client);
        }

        let extra_client =
            LocalSocketStream::connect(socket_name.clone()).expect("connect overflow");
        let extra_server = listener.accept().expect("accept overflow");
        let (handoff_tx, handoff_rx) = mpsc::sync_channel(1);
        let (saturated_tx, saturated_rx) = mpsc::sync_channel(1);
        connection_workers.install_saturated_admission_signal_for_test(saturated_tx);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let _ = handoff_tx.send(connection_workers.dispatch(
                    extra_server,
                    &lifecycle,
                    &shutdown_beacon,
                ));
            });
            saturated_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("overflow handoff reached the bounded admission queue");
            lifecycle.set_terminate_for_test(true);
            let error = handoff_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("shutdown must unblock a saturated connection handoff")
                .expect_err("saturated handoff must stop rather than admit new work");
            assert!(
                error
                    .message()
                    .contains("admission stopped during shutdown")
            );
        });

        drop(extra_client);
        drop(queued_clients);
        drop(clients);
        let (connection_join_tx, connection_join_rx) = mpsc::sync_channel(1);
        let (dispatch_join_tx, dispatch_join_rx) = mpsc::sync_channel(1);
        connection_workers.install_shutdown_join_signal_for_test(connection_join_tx);
        dispatch_workers.install_shutdown_join_signal_for_test(dispatch_join_tx);
        let (connection_shutdown_tx, connection_shutdown_rx) = mpsc::sync_channel(1);
        let (dispatch_shutdown_tx, dispatch_shutdown_rx) = mpsc::sync_channel(1);
        let dispatch_workers_for_shutdown = Arc::clone(&dispatch_workers);
        std::thread::scope(|scope| {
            scope.spawn(move || {
                let _ = connection_shutdown_tx.send(connection_workers.shutdown());
            });
            scope.spawn(move || {
                let _ = dispatch_shutdown_tx.send(dispatch_workers_for_shutdown.shutdown());
            });
            connection_join_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("connection shutdown reaches its worker join before dispatch release");
            dispatch_join_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("dispatch shutdown reaches its worker join before dispatch release");
            assert!(matches!(
                connection_shutdown_rx.try_recv(),
                Err(mpsc::TryRecvError::Empty)
            ));
            assert!(matches!(
                dispatch_shutdown_rx.try_recv(),
                Err(mpsc::TryRecvError::Empty)
            ));

            // At this point both shutdown paths have dropped their admission
            // senders and reached `worker.join()` while every worker is still
            // blocked inside the dispatcher. Releasing only now makes the
            // ordering regression detectable: the previous test released
            // before shutdown and would complete both receivers above.
            let (released, wake) = release.as_ref();
            *released.lock().expect("release lock") = true;
            wake.notify_all();
            connection_shutdown_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("connection shutdown result")
                .expect("connection workers stop after stalled shutdown admission");
            dispatch_shutdown_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("dispatch shutdown result")
                .expect("dispatch workers stop after stalled shutdown admission");
        });
    }

    #[cfg(windows)]
    #[test]
    fn prepare_local_ipc_endpoint_rejects_logical_parent_that_is_a_file() {
        let tempdir = TempDir::new().expect("tempdir");
        let parent_file = tempdir.path().join("not-a-dir");
        std::fs::write(&parent_file, "x").expect("parent file");
        let endpoint = parent_file.join("daemon.sock");

        let error = prepare_local_ipc_endpoint(&endpoint).expect_err("prepare endpoint");

        assert!(error.is_daemon_unavailable());
        assert!(
            error
                .to_string()
                .contains("failed to create daemon local IPC directory")
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
            LOCAL_WORKER_JOIN_DEADLINE,
        )
        .expect_err("bounded drain should fail once the forced-cancel deadline elapses");
        assert!(
            shutdown_started.elapsed() < Duration::from_secs(1),
            "forced cancel should bound shutdown even when tracked request work never completes"
        );
        assert!(force_shutdown.load(Ordering::SeqCst));
        assert!(
            error
                .message()
                .contains("tracked daemon dispatch worker exceeded the shutdown join deadline")
        );
        let _ = release_tx.send(());
        drop(active_connection);
    }

    #[test]
    fn drain_active_connections_for_shutdown_surfaces_dispatch_panic_as_fatal() {
        // The opportunistic accept-loop and per-connection reaps treat a panicked dispatch
        // worker as non-fatal (see `active_connection_registry::tests::
        // reap_finished_dispatches_logs_and_continues_after_panic`). The deliberate shutdown
        // drain must still surface a panic discovered while it is joining tracked dispatch
        // workers, since a wedged/panicked worker blocking graceful shutdown is legitimately
        // worth surfacing to the caller.
        let registry = Arc::new(ActiveConnectionRegistry::default());
        let dispatch_registry = Arc::clone(&registry);
        let (completion_tx, completion_rx) = mpsc::sync_channel(1);
        let join_handle = std::thread::spawn(move || {
            let _dispatch = dispatch_registry.register_dispatch_work();
            let _completion_tx = completion_tx;
            panic!("intentional dispatch worker panic for shutdown drain test");
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
        let error = drain_active_connections_for_shutdown(
            registry.as_ref(),
            &force_shutdown,
            Duration::from_secs(5),
            Duration::from_secs(5),
            Instant::now(),
            LOCAL_WORKER_JOIN_DEADLINE,
        )
        .expect_err("a dispatch worker panic discovered during the shutdown drain must be fatal");
        assert!(
            error.message().contains("daemon dispatch thread panicked"),
            "unexpected error: {error:?}"
        );
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
                .message()
                .contains("tracked daemon dispatch registry exceeded its bounded capacity"),
            "unexpected error: {error:?}"
        );

        let _ = release_first_tx.send(());
        registry
            .join_tracked_dispatches(Duration::from_secs(1))
            .expect("drain first tracked dispatch");
    }
}
