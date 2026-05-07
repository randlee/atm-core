#![forbid(unsafe_code)]
//! Skeleton crate for Phase R daemon runtime work.

pub(crate) mod composition;

#[cfg(unix)]
use std::collections::HashMap;
use std::error::Error as StdError;
use std::fmt;
#[cfg(unix)]
use std::fs::{self, File, OpenOptions};
#[cfg(unix)]
use std::io::Write;
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(unix)]
use std::sync::Mutex;
#[cfg(unix)]
use std::sync::OnceLock;
#[cfg(unix)]
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
#[cfg(unix)]
use std::thread;
#[cfg(unix)]
use std::time::{Duration, Instant};

#[cfg(unix)]
use fs2::FileExt;
#[cfg(unix)]
use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGTERM};
#[cfg(unix)]
use signal_hook::flag;

use crate::composition::{RuntimeLifecycle, RuntimeLifecycleState};
#[cfg(unix)]
use atm_core::protocol::RequestEnvelope as ProtocolRequestEnvelope;
use atm_core::{
    RequestEnvelope, ResponseEnvelope,
    ack::ack_mail,
    boundary::{
        self, ConfigIngress, ConfigLoadRequest, ConfigLoadResponse, ConfigTeamLoadRequest,
        ConfigTeamLoadResponse, InboxExport, InboxExportRecordRequest, InboxExportRecordResponse,
        InboxExportReexportMessageRequest, InboxExportReexportMessageResponse, InboxIngress,
        InboxIngressDiagnosticsRequest, InboxIngressDiagnosticsResponse,
        InboxIngressIdentityFingerprintRequest, InboxIngressIdentityFingerprintResponse,
        InboxIngressImportRequest, InboxIngressImportResponse, NotificationEvent, ReconcileRequest,
        ReconcileResult, RequestDispatcher, RuntimeStatusSnapshot, WatchEventBatch,
        WatchSubscriptionRequest,
    },
    clear::clear_mail,
    doctor::run_doctor,
    error::AtmError,
    observability::{
        AtmLogQuery, AtmLogSnapshot, AtmObservabilityHealth, AtmObservabilityHealthState,
        CommandEvent, LogTailSession, ObservabilityPort,
    },
    protocol::{SendRequestEnvelope, SendResponseEnvelope},
    read::read_mail,
    send::send_mail,
};
#[cfg(unix)]
const MAX_CONCURRENT_CONNECTIONS: usize = 64;
#[cfg(unix)]
const REQUEST_DEADLINE: Duration = Duration::from_secs(3);
#[cfg(unix)]
const SHUTDOWN_POLL_INTERVAL: Duration = Duration::from_millis(25);
#[cfg(unix)]
const GRACEFUL_DRAIN_DEADLINE: Duration = Duration::from_secs(5);
#[cfg(unix)]
const FORCE_CANCEL_DEADLINE: Duration = Duration::from_secs(10);
#[cfg(unix)]
const HOST_RUNTIME_OWNER_LOCK_FILE: &str = "owner.lock";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DaemonBoundaryStubError {
    PeerClientTransport,
}

impl fmt::Display for DaemonBoundaryStubError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PeerClientTransport => {
                f.write_str("daemon peer client transport scaffold is not wired")
            }
        }
    }
}

impl StdError for DaemonBoundaryStubError {}

fn daemon_boundary_stub_error(message: &'static str, source: DaemonBoundaryStubError) -> AtmError {
    AtmError::config(message)
        .with_recovery("Complete the Phase R daemon boundary wiring before invoking this path.")
        .with_source(source)
}

#[cfg(unix)]
fn host_runtime_lock_path(file_name: &str) -> Result<PathBuf, AtmError> {
    Ok(host_runtime_lock_path_from_home(
        &atm_core::home::host_runtime_dir()?,
        file_name,
    ))
}

#[cfg(unix)]
fn host_runtime_lock_path_from_home(home_dir: &std::path::Path, file_name: &str) -> PathBuf {
    home_dir.join(file_name)
}

#[cfg(unix)]
fn write_owner_record(lock_file: &mut File) -> Result<(), AtmError> {
    lock_file.set_len(0).map_err(|source| {
        AtmError::daemon_unavailable("failed to reset daemon singleton lock metadata")
            .with_source(source)
    })?;
    writeln!(lock_file, "{}", std::process::id()).map_err(|source| {
        AtmError::daemon_unavailable("failed to write daemon singleton lock metadata")
            .with_source(source)
    })?;
    lock_file.sync_all().map_err(|source| {
        AtmError::daemon_unavailable("failed to sync daemon singleton lock metadata")
            .with_source(source)
    })?;
    Ok(())
}

#[cfg(unix)]
fn recorded_owner_pid(lock_file: &File) -> Result<Option<u32>, AtmError> {
    use std::io::{Read, Seek, SeekFrom};

    let mut clone = lock_file.try_clone().map_err(|source| {
        AtmError::daemon_unavailable("failed to clone daemon ownership record handle")
            .with_source(source)
    })?;
    clone.seek(SeekFrom::Start(0)).map_err(|source| {
        AtmError::daemon_unavailable("failed to seek daemon ownership record").with_source(source)
    })?;
    let mut record = String::new();
    clone.read_to_string(&mut record).map_err(|source| {
        AtmError::daemon_unavailable("failed to read daemon ownership record").with_source(source)
    })?;
    let trimmed = record.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let pid = trimmed
        .split_once(':')
        .map(|(pid, _)| pid)
        .unwrap_or(trimmed)
        .parse::<u32>()
        .ok();
    Ok(pid)
}

#[cfg(unix)]
#[derive(Debug, Default)]
struct ActiveConnectionRegistry {
    next_id: AtomicUsize,
    active_connections: AtomicUsize,
    streams: Mutex<HashMap<usize, UnixStream>>,
}

#[cfg(unix)]
impl ActiveConnectionRegistry {
    fn register(self: &Arc<Self>, stream: &UnixStream) -> Result<ActiveConnectionGuard, AtmError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let cloned = stream.try_clone().map_err(|source| {
            AtmError::daemon_unavailable("failed to clone active daemon connection handle")
                .with_source(source)
        })?;
        self.streams
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("active connection registry lock poisoned"))?
            .insert(id, cloned);
        self.active_connections.fetch_add(1, Ordering::SeqCst);
        Ok(ActiveConnectionGuard {
            id,
            registry: Arc::clone(self),
        })
    }

    fn active_connections(&self) -> usize {
        self.active_connections.load(Ordering::SeqCst)
    }

    fn interrupt_all(&self) -> Result<(), AtmError> {
        let mut streams = self.streams.lock().map_err(|_| {
            AtmError::daemon_unavailable("active connection registry lock poisoned")
        })?;
        for stream in streams.values_mut() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
        Ok(())
    }

    fn remove(&self, id: usize) {
        if let Ok(mut streams) = self.streams.lock() {
            streams.remove(&id);
        }
        self.active_connections.fetch_sub(1, Ordering::SeqCst);
    }
}
fn load_workspace_config_direct(
    request: ConfigLoadRequest,
) -> Result<ConfigLoadResponse, AtmError> {
    atm_core::boundary_support::load_workspace_config(request)
}

fn load_team_config_direct(
    request: ConfigTeamLoadRequest,
) -> Result<ConfigTeamLoadResponse, AtmError> {
    atm_core::boundary_support::load_team_config(request)
}

fn import_inbox_source_direct(
    request: InboxIngressImportRequest,
) -> Result<InboxIngressImportResponse, AtmError> {
    atm_core::boundary_support::import_inbox_source(request)
}

fn compute_identity_fingerprint_direct(
    request: InboxIngressIdentityFingerprintRequest,
) -> Result<InboxIngressIdentityFingerprintResponse, AtmError> {
    atm_core::boundary_support::compute_identity_fingerprint(request)
}

fn report_inbox_diagnostics_direct(
    request: InboxIngressDiagnosticsRequest,
) -> Result<InboxIngressDiagnosticsResponse, AtmError> {
    atm_core::boundary_support::report_inbox_diagnostics(request)
}

fn export_source_files_direct(
    request: InboxExportRecordRequest,
) -> Result<InboxExportRecordResponse, AtmError> {
    atm_core::boundary_support::export_source_files(request)
}

fn reexport_messages_direct(
    request: InboxExportReexportMessageRequest,
) -> Result<InboxExportReexportMessageResponse, AtmError> {
    atm_core::boundary_support::reexport_messages(request)
}

#[derive(Debug, Clone)]
struct DaemonObservability {
    home_dir: PathBuf,
}

impl DaemonObservability {
    fn new(home_dir: PathBuf) -> Self {
        Self { home_dir }
    }
}

impl boundary::sealed::Sealed for DaemonObservability {}

impl ObservabilityPort for DaemonObservability {
    fn emit(&self, _event: CommandEvent) -> Result<(), AtmError> {
        Ok(())
    }

    fn query(&self, _req: AtmLogQuery) -> Result<AtmLogSnapshot, AtmError> {
        Ok(AtmLogSnapshot::default())
    }

    fn follow(&self, _req: AtmLogQuery) -> Result<LogTailSession, AtmError> {
        Ok(LogTailSession::empty())
    }

    fn health(&self) -> Result<AtmObservabilityHealth, AtmError> {
        let active_log_path = self
            .home_dir
            .join(".local")
            .join("share")
            .join("logs")
            .join("atm.log.jsonl");
        let fault = std::env::var("ATM_OBSERVABILITY_RETAINED_SINK_FAULT")
            .ok()
            .map(|value| value.trim().to_ascii_lowercase());
        let logging_state = match fault.as_deref() {
            Some("degraded") => AtmObservabilityHealthState::Degraded,
            Some("unavailable") => AtmObservabilityHealthState::Unavailable,
            _ => AtmObservabilityHealthState::Healthy,
        };
        Ok(AtmObservabilityHealth {
            active_log_path: Some(active_log_path),
            logging_state,
            query_state: Some(AtmObservabilityHealthState::Healthy),
            detail: None,
        })
    }
}

#[cfg(unix)]
#[derive(Debug)]
struct DaemonShutdownSignals {
    terminate: Arc<AtomicBool>,
    reload: Arc<AtomicBool>,
}

#[cfg(unix)]
#[derive(Debug)]
struct SharedDaemonShutdownSignals {
    terminate: Arc<AtomicBool>,
    reload: Arc<AtomicBool>,
}

#[cfg(unix)]
impl DaemonShutdownSignals {
    fn install() -> Result<Self, AtmError> {
        static SIGNALS: OnceLock<SharedDaemonShutdownSignals> = OnceLock::new();
        static INSTALL_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        // OnceLock owns global registration; Mutex serializes the read-check-write window.
        let _guard = INSTALL_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("daemon signal install lock poisoned"))?;
        if SIGNALS.get().is_none() {
            let terminate = Arc::new(AtomicBool::new(false));
            let reload = Arc::new(AtomicBool::new(false));
            for signal in [SIGINT, SIGTERM] {
                flag::register(signal, Arc::clone(&terminate)).map_err(|source| {
                    AtmError::daemon_unavailable("failed to install daemon shutdown signal handler")
                        .with_source(source)
                })?;
            }
            flag::register(SIGHUP, Arc::clone(&reload)).map_err(|source| {
                AtmError::daemon_unavailable("failed to install daemon reload signal handler")
                    .with_source(source)
            })?;
            let _ = SIGNALS.set(SharedDaemonShutdownSignals { terminate, reload });
        }
        let shared = SIGNALS.get().ok_or_else(|| {
            AtmError::daemon_unavailable("daemon shutdown signals were not initialized")
        })?;
        shared.terminate.store(false, Ordering::SeqCst);
        shared.reload.store(false, Ordering::SeqCst);
        Ok(Self {
            terminate: Arc::clone(&shared.terminate),
            reload: Arc::clone(&shared.reload),
        })
    }
}

#[cfg(unix)]
#[derive(Debug)]
struct SingletonGuard {
    socket_path: PathBuf,
    lock_path: PathBuf,
    lock_file: File,
}

#[cfg(unix)]
impl SingletonGuard {
    fn acquire(socket_path: &std::path::Path) -> Result<Self, AtmError> {
        let lock_path = host_runtime_lock_path(HOST_RUNTIME_OWNER_LOCK_FILE)?;
        Self::acquire_at(socket_path, lock_path)
    }

    fn acquire_at(socket_path: &std::path::Path, lock_path: PathBuf) -> Result<Self, AtmError> {
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent).map_err(|source| {
                AtmError::daemon_unavailable(format!(
                    "failed to create daemon lock directory at {}",
                    parent.display()
                ))
                .with_source(source)
            })?;
        }

        let mut lock_file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|source| {
                AtmError::daemon_unavailable(format!(
                    "failed to open daemon singleton lock at {}",
                    lock_path.display()
                ))
                .with_source(source)
            })?;
        match lock_file.try_lock_exclusive() {
            Ok(()) => {}
            Err(source) if source.kind() == std::io::ErrorKind::WouldBlock => {
                if let Some(pid) = recorded_owner_pid(&lock_file)?
                    && !atm_core::process::process_is_alive(pid)
                {
                    return Err(AtmError::daemon_stale_owner_recovery_failed(format!(
                        "daemon owner record at {} points to non-live pid {}",
                        lock_path.display(),
                        pid
                    )));
                }
                return Err(AtmError::daemon_serving_state_rejected(format!(
                    "a live ATM daemon already owns {}",
                    lock_path.display()
                ))
                .with_source(source));
            }
            Err(source) => {
                return Err(AtmError::daemon_unavailable(format!(
                    "failed to acquire daemon singleton lock at {}",
                    lock_path.display()
                ))
                .with_source(source));
            }
        }
        write_owner_record(&mut lock_file)?;
        remove_stale_socket(socket_path)?;
        Ok(Self {
            socket_path: socket_path.to_path_buf(),
            lock_path,
            lock_file,
        })
    }
}

#[cfg(unix)]
impl Drop for SingletonGuard {
    fn drop(&mut self) {
        let _ = self.lock_file.unlock();
        let _ = fs::remove_file(&self.socket_path);
        let _ = fs::remove_file(&self.lock_path);
    }
}

#[cfg(unix)]
struct ActiveConnectionGuard {
    id: usize,
    registry: Arc<ActiveConnectionRegistry>,
}

#[cfg(unix)]
impl Drop for ActiveConnectionGuard {
    fn drop(&mut self) {
        self.registry.remove(self.id);
    }
}

#[cfg(unix)]
fn remove_stale_socket(socket_path: &std::path::Path) -> Result<(), AtmError> {
    if !socket_path.exists() {
        return Ok(());
    }
    fs::remove_file(socket_path).map_err(|source| {
        AtmError::daemon_unavailable(format!(
            "failed to remove stale daemon socket at {}",
            socket_path.display()
        ))
        .with_source(source)
    })
}

#[cfg(unix)]
fn handle_connection(
    stream: &mut UnixStream,
    dispatcher: &dyn RequestDispatcher,
    force_shutdown: &AtomicBool,
) -> Result<(), AtmError> {
    if force_shutdown.load(Ordering::SeqCst) {
        return Ok(());
    }
    stream
        .set_read_timeout(Some(REQUEST_DEADLINE))
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to apply daemon request read deadline")
                .with_source(source)
        })?;
    stream
        .set_write_timeout(Some(REQUEST_DEADLINE))
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to apply daemon response write deadline")
                .with_source(source)
        })?;

    let bytes = atm_core::protocol::read_bounded_stream(
        stream,
        "failed to read daemon request frame",
        "daemon request frame exceeded the maximum supported size",
    )?;
    if bytes.is_empty() {
        return Ok(());
    }
    let request: ProtocolRequestEnvelope =
        serde_json::from_slice(&bytes).map_err(AtmError::from)?;
    let started = Instant::now();
    // TODO(phase-R): enforce a max 32 inflight requests per connection once framed multiplexing lands.
    let response = match dispatcher.dispatch(request) {
        Ok(response) if started.elapsed() <= REQUEST_DEADLINE => response,
        Ok(_) => ResponseEnvelope::Error(atm_core::protocol::ProtocolErrorEnvelope::from_error(
            &AtmError::daemon_unavailable(
                "daemon request exceeded the 3s runtime deadline after the handler completed; the operation may have succeeded",
            )
            .with_recovery(
                "Check the destination mailbox or service-side effects before retrying this ATM command.",
            ),
        )),
        Err(error) => ResponseEnvelope::Error(
            atm_core::protocol::ProtocolErrorEnvelope::from_error(&error),
        ),
    };
    let encoded = serde_json::to_vec(&response).map_err(AtmError::from)?;
    stream.write_all(&encoded).map_err(|source| {
        AtmError::daemon_unavailable("failed to write daemon response frame").with_source(source)
    })?;
    stream.flush().map_err(|source| {
        AtmError::daemon_unavailable("failed to flush daemon response frame").with_source(source)
    })?;
    Ok(())
}

/// Placeholder runtime transport for the daemon server boundary.
#[derive(Debug, Default)]
pub(crate) struct LocalSocketServerTransport;

impl LocalSocketServerTransport {
    pub(crate) const fn new() -> Self {
        Self
    }

    #[cfg(unix)]
    pub(crate) fn start_runtime(
        &self,
        dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
        lifecycle: Arc<RuntimeLifecycle>,
    ) -> Result<(), AtmError> {
        let socket_path = atm_core::protocol::daemon_socket_path()?;
        self.start_runtime_at_socket_path(socket_path, dispatcher, lifecycle)
    }

    #[cfg(unix)]
    pub(crate) fn start_runtime_at_socket_path(
        &self,
        socket_path: PathBuf,
        dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
        lifecycle: Arc<RuntimeLifecycle>,
    ) -> Result<(), AtmError> {
        let signals = DaemonShutdownSignals::install()?;
        let _singleton = SingletonGuard::acquire(&socket_path)?;
        if let Some(parent) = socket_path.parent() {
            fs::create_dir_all(parent).map_err(|source| {
                AtmError::daemon_unavailable(format!(
                    "failed to create daemon socket directory at {}",
                    parent.display()
                ))
                .with_source(source)
            })?;
        }
        let listener = UnixListener::bind(&socket_path).map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to bind daemon socket at {}",
                socket_path.display()
            ))
            .with_source(source)
        })?;
        listener.set_nonblocking(true).map_err(|source| {
            AtmError::daemon_unavailable("failed to configure daemon socket listener")
                .with_source(source)
        })?;
        lifecycle.transition(RuntimeLifecycleState::Running)?;
        let registry = Arc::new(ActiveConnectionRegistry::default());
        let force_shutdown = Arc::new(AtomicBool::new(false));
        let result = thread::scope(|scope| -> Result<(), AtmError> {
            loop {
                if signals.reload.swap(false, Ordering::SeqCst) {
                    tracing::info!(
                        "TODO(phase-R): bounded SIGHUP-triggered config/roster reload is not wired yet"
                    );
                }
                if signals.terminate.load(Ordering::SeqCst) {
                    break;
                }

                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if registry.active_connections() >= MAX_CONCURRENT_CONNECTIONS {
                            let response = ResponseEnvelope::Error(
                                atm_core::protocol::ProtocolErrorEnvelope::from_error(
                                    &AtmError::daemon_unavailable(
                                        "daemon connection cap exceeded (max 64 concurrent accepts)",
                                    )
                                    .with_recovery(
                                        "Wait for in-flight ATM commands to complete before retrying, or reduce concurrent atm invocations.",
                                    ),
                                ),
                            );
                            let encoded = serde_json::to_vec(&response).map_err(AtmError::from)?;
                            let _ = stream.set_read_timeout(Some(REQUEST_DEADLINE));
                            let _ = stream.set_write_timeout(Some(REQUEST_DEADLINE));
                            let _ = stream.write_all(&encoded);
                            let _ = stream.flush();
                            continue;
                        }

                        let _active = registry.register(&stream)?;
                        let dispatcher = Arc::clone(&dispatcher);
                        let force_shutdown = Arc::clone(&force_shutdown);
                        scope.spawn(move || {
                            let _active = _active;
                            if let Err(error) = handle_connection(
                                &mut stream,
                                dispatcher.as_ref(),
                                force_shutdown.as_ref(),
                            ) {
                                tracing::warn!(%error, "daemon connection handling failed");
                            }
                        });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(SHUTDOWN_POLL_INTERVAL);
                    }
                    Err(source) => {
                        return Err(AtmError::daemon_unavailable(
                            "failed while accepting daemon connection",
                        )
                        .with_source(source));
                    }
                }
            }

            lifecycle.transition(RuntimeLifecycleState::Draining)?;
            let shutdown_started = Instant::now();
            tracing::info!(
                active_connections = registry.active_connections(),
                "daemon shutdown signal received; starting graceful drain"
            );
            let graceful_deadline = shutdown_started + GRACEFUL_DRAIN_DEADLINE;
            let force_cancel_deadline = shutdown_started + FORCE_CANCEL_DEADLINE;
            while registry.active_connections() > 0 && Instant::now() < graceful_deadline {
                thread::sleep(SHUTDOWN_POLL_INTERVAL);
            }
            let remaining_after_graceful = registry.active_connections();
            if remaining_after_graceful == 0 {
                tracing::info!("daemon graceful drain completed cleanly");
            } else {
                tracing::info!(
                    active_connections = remaining_after_graceful,
                    "daemon graceful drain hit deadline; continuing toward forced cancel"
                );
            }
            if remaining_after_graceful > 0 {
                force_shutdown.store(true, Ordering::SeqCst);
                registry.interrupt_all()?;
            }
            while registry.active_connections() > 0 && Instant::now() < force_cancel_deadline {
                thread::sleep(SHUTDOWN_POLL_INTERVAL);
            }
            let remaining_connections = registry.active_connections();
            if remaining_connections > 0 {
                return Err(AtmError::daemon_unavailable(format!(
                    "forced cancel deadline elapsed with {remaining_connections} active daemon connections"
                )));
            }
            lifecycle.transition(RuntimeLifecycleState::Stopped)?;
            Ok(())
        });
        if result.is_err() {
            let _ = lifecycle.force_stopped();
        }
        result
    }
}

impl boundary::sealed::Sealed for LocalSocketServerTransport {}

impl boundary::ServerTransport for LocalSocketServerTransport {
    #[cfg(unix)]
    fn serve(&self, dispatcher: Arc<dyn RequestDispatcher + Send + Sync>) -> Result<(), AtmError> {
        let lifecycle = Arc::new(RuntimeLifecycle::new());
        lifecycle.transition(RuntimeLifecycleState::Starting)?;
        self.start_runtime(dispatcher, lifecycle)
    }

    #[cfg(not(unix))]
    fn serve(&self, _dispatcher: Arc<dyn RequestDispatcher + Send + Sync>) -> Result<(), AtmError> {
        Err(AtmError::daemon_unavailable(
            "atm-daemon socket transport requires a Unix platform",
        ))
    }
}

/// Placeholder runtime dispatcher for daemon-owned protocol routing.
#[derive(Debug, Clone)]
struct DaemonRequestDispatcher {
    observability: DaemonObservability,
}

impl DaemonRequestDispatcher {
    fn new(home_dir: PathBuf) -> Self {
        Self {
            observability: DaemonObservability::new(home_dir),
        }
    }
}

impl boundary::sealed::Sealed for DaemonRequestDispatcher {}

impl boundary::RequestDispatcher for DaemonRequestDispatcher {
    fn dispatch(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        match request {
            RequestEnvelope::Send(SendRequestEnvelope::Compose(request)) => {
                Ok(ResponseEnvelope::Send(SendResponseEnvelope::Sent(
                    send_mail(request, &self.observability)?,
                )))
            }
            RequestEnvelope::Send(SendRequestEnvelope::Acknowledge(request)) => {
                Ok(ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(
                    ack_mail(request, &self.observability)?,
                )))
            }
            RequestEnvelope::Receive(query) => Ok(ResponseEnvelope::Receive(read_mail(
                query,
                &self.observability,
            )?)),
            RequestEnvelope::Clear(query) => Ok(ResponseEnvelope::Clear(clear_mail(
                query,
                &self.observability,
            )?)),
            RequestEnvelope::Doctor(query) => Ok(ResponseEnvelope::Doctor(run_doctor(
                query,
                &self.observability,
            )?)),
        }
    }
}

/// Placeholder runtime sink for daemon-emitted notifications.
#[derive(Debug, Default)]
pub(crate) struct DaemonNotificationSink;

impl DaemonNotificationSink {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for DaemonNotificationSink {}

impl boundary::NotificationSink for DaemonNotificationSink {
    fn deliver(&self, event: NotificationEvent) -> Result<(), AtmError> {
        atm_core::boundary_support::deliver_notification(event)
    }
}

/// Placeholder runtime client transport for peer-to-peer daemon delivery.
#[derive(Debug, Default)]
struct PeerClientTransport;

impl PeerClientTransport {
    const fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for PeerClientTransport {}

impl boundary::ClientTransport for PeerClientTransport {
    fn send(&self, _request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        Err(daemon_boundary_stub_error(
            "daemon peer client transport stub is not implemented yet",
            DaemonBoundaryStubError::PeerClientTransport,
        ))
    }
}

/// Placeholder runtime source for daemon status snapshots.
#[derive(Debug, Default)]
pub(crate) struct DaemonStatusSource;

impl DaemonStatusSource {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for DaemonStatusSource {}

impl boundary::StatusSource for DaemonStatusSource {
    fn snapshot(&self) -> Result<RuntimeStatusSnapshot, AtmError> {
        atm_core::boundary_support::snapshot_status()
    }
}

/// Placeholder runtime source for daemon watch events.
#[derive(Debug, Default)]
pub(crate) struct FileWatchEventSource;

impl FileWatchEventSource {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for FileWatchEventSource {}

impl boundary::WatchEventSource for FileWatchEventSource {
    fn poll(&self, request: WatchSubscriptionRequest) -> Result<WatchEventBatch, AtmError> {
        atm_core::boundary_support::poll_watch(request)
    }
}

/// Placeholder runtime coordinator for daemon reconcile work.
#[derive(Debug, Default)]
pub(crate) struct DaemonReconcileCoordinator;

impl DaemonReconcileCoordinator {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for DaemonReconcileCoordinator {}

impl boundary::ReconcileCoordinator for DaemonReconcileCoordinator {
    fn reconcile(&self, request: ReconcileRequest) -> Result<ReconcileResult, AtmError> {
        atm_core::boundary_support::reconcile(request)
    }
}

/// Placeholder runtime config ingress for daemon-owned config loading.
#[derive(Debug, Default)]
pub(crate) struct DaemonConfigIngress;

impl DaemonConfigIngress {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for DaemonConfigIngress {}

impl ConfigIngress for DaemonConfigIngress {
    fn load_config(&self, request: ConfigLoadRequest) -> Result<ConfigLoadResponse, AtmError> {
        load_workspace_config_direct(request)
    }

    fn load_team_config(
        &self,
        request: ConfigTeamLoadRequest,
    ) -> Result<ConfigTeamLoadResponse, AtmError> {
        load_team_config_direct(request)
    }
}

/// Placeholder runtime inbox ingress for daemon-owned import workflows.
#[derive(Debug, Default)]
pub(crate) struct DaemonInboxIngress;

impl DaemonInboxIngress {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for DaemonInboxIngress {}

impl InboxIngress for DaemonInboxIngress {
    fn import_inbox_source(
        &self,
        request: InboxIngressImportRequest,
    ) -> Result<InboxIngressImportResponse, AtmError> {
        import_inbox_source_direct(request)
    }

    fn compute_identity_fingerprint(
        &self,
        request: InboxIngressIdentityFingerprintRequest,
    ) -> Result<InboxIngressIdentityFingerprintResponse, AtmError> {
        compute_identity_fingerprint_direct(request)
    }

    fn report_diagnostics(
        &self,
        request: InboxIngressDiagnosticsRequest,
    ) -> Result<InboxIngressDiagnosticsResponse, AtmError> {
        report_inbox_diagnostics_direct(request)
    }
}

/// Placeholder runtime inbox export for daemon-owned export workflows.
#[derive(Debug, Default)]
pub(crate) struct DaemonInboxExport;

impl DaemonInboxExport {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for DaemonInboxExport {}

impl InboxExport for DaemonInboxExport {
    fn export_record(
        &self,
        request: InboxExportRecordRequest,
    ) -> Result<InboxExportRecordResponse, AtmError> {
        export_source_files_direct(request)
    }

    fn reexport_message(
        &self,
        request: InboxExportReexportMessageRequest,
    ) -> Result<InboxExportReexportMessageResponse, AtmError> {
        reexport_messages_direct(request)
    }
}

/// Run the daemon entrypoint with the currently assembled runtime composition.
///
/// # Errors
///
/// Returns [`AtmError`] when the daemon transport cannot start or serve.
pub fn run_daemon() -> Result<(), AtmError> {
    composition::compose_runtime()?.start()
}

#[cfg(test)]
mod tests {
    use super::{
        ActiveConnectionRegistry, DaemonShutdownSignals, HOST_RUNTIME_OWNER_LOCK_FILE,
        SingletonGuard, host_runtime_lock_path_from_home,
    };
    use atm_core::error_codes::AtmErrorCode;
    use std::fs::OpenOptions;
    use std::io::{Read, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::sync::{Arc, mpsc};
    use std::time::Duration;
    use tempfile::TempDir;

    #[test]
    fn daemon_shutdown_signals_install_is_repeatable() {
        let first = DaemonShutdownSignals::install().expect("first install");
        let second = DaemonShutdownSignals::install().expect("second install");

        assert!(!first.terminate.load(std::sync::atomic::Ordering::SeqCst));
        assert!(!second.reload.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn singleton_guard_is_host_wide_across_different_socket_paths() {
        let tempdir = TempDir::new().expect("tempdir");
        let runtime_dir = atm_core::home::host_runtime_dir_from_home(tempdir.path());

        let first_socket = tempdir.path().join("one.sock");
        let second_socket = tempdir.path().join("other").join("two.sock");
        let first = SingletonGuard::acquire_at(
            &first_socket,
            host_runtime_lock_path_from_home(&runtime_dir, HOST_RUNTIME_OWNER_LOCK_FILE),
        )
        .expect("first singleton");
        let error = SingletonGuard::acquire_at(
            &second_socket,
            host_runtime_lock_path_from_home(&runtime_dir, HOST_RUNTIME_OWNER_LOCK_FILE),
        )
        .expect_err("second singleton");

        assert_eq!(error.code, AtmErrorCode::DaemonServingStateRejected);
        drop(first);
    }

    #[test]
    fn singleton_guard_reports_stale_owner_record_failure() {
        let tempdir = TempDir::new().expect("tempdir");
        let runtime_dir = atm_core::home::host_runtime_dir_from_home(tempdir.path());
        let lock_path =
            host_runtime_lock_path_from_home(&runtime_dir, HOST_RUNTIME_OWNER_LOCK_FILE);
        if let Some(parent) = lock_path.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .expect("open lock file");
        fs2::FileExt::try_lock_exclusive(&file).expect("lock file");
        writeln!(&mut file, "999999").expect("write owner");
        file.sync_all().expect("sync owner");

        let error = SingletonGuard::acquire_at(&tempdir.path().join("atm.sock"), lock_path)
            .expect_err("stale");
        assert_eq!(error.code, AtmErrorCode::DaemonStaleOwnerRecoveryFailed);
    }

    #[test]
    fn blocked_connection_is_interrupted_on_force_cancel() {
        let tempdir = TempDir::new().expect("tempdir");
        let registry = Arc::new(ActiveConnectionRegistry::default());
        let socket_path = tempdir.path().join("daemon-test.sock");
        let listener = UnixListener::bind(&socket_path).expect("bind listener");
        let client = UnixStream::connect(&socket_path).expect("connect client");
        let (mut server, _) = listener.accept().expect("accept server");
        let _guard = registry.register(&server).expect("register");
        let (done_tx, done_rx) = mpsc::channel();

        std::thread::spawn(move || {
            let mut byte = [0u8; 1];
            let result = server.read(&mut byte).map(|_| ());
            done_tx.send(result).expect("send result");
        });

        registry.interrupt_all().expect("interrupt all");
        let result = done_rx
            .recv_timeout(Duration::from_millis(250))
            .expect("connection finished");
        drop(client);
        assert!(result.is_ok(), "connection result: {result:?}");
    }
}
