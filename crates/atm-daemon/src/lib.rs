#![forbid(unsafe_code)]
//! Skeleton crate for Phase R daemon runtime work.

mod boundary_adapters;
pub(crate) mod composition;
mod direct_boundaries;
mod peer_transport;
mod runtime_health;
mod shutdown_signals;

#[cfg(unix)]
use std::collections::HashMap;
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
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
#[cfg(unix)]
use std::thread;
#[cfg(unix)]
use std::time::{Duration, Instant};

use atm_rusqlite::SqliteBoundaryAssembly;
#[cfg(unix)]
use fs2::FileExt;

#[cfg(unix)]
use atm_core::ResponseEnvelope;
#[cfg(unix)]
use atm_core::protocol::RequestEnvelope as ProtocolRequestEnvelope;
use atm_core::{
    boundary::{self, RequestDispatcher},
    error::AtmError,
};
pub(crate) use atm_rusqlite::RemoteReplayStateRecord;
pub(crate) use peer_transport::{PeerTransportRuntime, RemoteReplayStore};
#[cfg(unix)]
use shutdown_signals::DaemonShutdownSignals;
#[cfg(unix)]
pub use shutdown_signals::request_shutdown_for_test;
#[cfg(unix)]
pub use shutdown_signals::reset_shutdown_signals_for_test;
#[cfg(unix)]
const MAX_CONCURRENT_CONNECTIONS: usize = 64;
#[cfg(unix)]
const REQUEST_DEADLINE: Duration = Duration::from_secs(3);
#[cfg(unix)]
const SHUTDOWN_POLL_INTERVAL: Duration = Duration::from_millis(25);
#[cfg(unix)]
const STALE_OWNER_RECOVERY_RETRY_ATTEMPTS: usize = 3;
#[cfg(unix)]
const GRACEFUL_DRAIN_DEADLINE: Duration = Duration::from_secs(5);
#[cfg(unix)]
const FORCE_CANCEL_DEADLINE: Duration = Duration::from_secs(10);
#[cfg(unix)]
const HOST_RUNTIME_OWNER_LOCK_FILE: &str = "owner.lock";

#[derive(Debug, Clone)]
struct SqliteRemoteReplayStore {
    assembly: Arc<SqliteBoundaryAssembly>,
}

impl SqliteRemoteReplayStore {
    fn from_path(db_path: PathBuf) -> Result<Self, AtmError> {
        Ok(Self {
            assembly: Arc::new(SqliteBoundaryAssembly::new(db_path)?),
        })
    }
}

impl RemoteReplayStore for SqliteRemoteReplayStore {
    fn enqueue(&self, record: RemoteReplayStateRecord) -> Result<(), AtmError> {
        self.assembly.record_remote_replay_state(record)
    }

    fn load_all(&self) -> Result<Vec<RemoteReplayStateRecord>, AtmError> {
        self.assembly.load_remote_replay_states()
    }

    fn delete(
        &self,
        team: &atm_core::types::TeamName,
        agent: &atm_core::types::AgentName,
        message_key: &atm_core::boundary::MessageKey,
    ) -> Result<(), AtmError> {
        self.assembly
            .delete_remote_replay_state(team, agent, message_key)
    }

    fn purge_expired(&self, now: atm_core::types::IsoTimestamp) -> Result<usize, AtmError> {
        self.assembly.purge_expired_remote_replay_states(now)
    }
}

pub(crate) fn sqlite_remote_replay_store_from_path(
    db_path: PathBuf,
) -> Result<Arc<dyn RemoteReplayStore>, AtmError> {
    Ok(Arc::new(SqliteRemoteReplayStore::from_path(db_path)?))
}

#[cfg(unix)]
fn host_runtime_lock_path(file_name: &str) -> Result<PathBuf, AtmError> {
    // Host runtime ownership is intentionally OS-home scoped. `ATM_HOME`
    // selects mailbox/config roots, but both `atm` and `atm-daemon` must
    // resolve the same `host_runtime_dir()` so one machine cannot fork
    // separate singleton or launch-lock roots per workspace.
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
    // Keep interruptible stream clones so graceful-drain escalation can break
    // blocked reads instead of waiting forever for peer cooperation.
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
        // Poisoned lock during remove: connection count still decremented to
        // prevent stale accounting.
        self.active_connections.fetch_sub(1, Ordering::SeqCst);
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
        let mut lock_file = open_singleton_lock(&lock_path)?;
        match lock_file.try_lock_exclusive() {
            Ok(()) => {}
            Err(source) if source.kind() == std::io::ErrorKind::WouldBlock => {
                let mut recovered = false;
                if let Some(pid) = recorded_owner_pid(&lock_file)?
                    && !atm_core::process::process_is_alive(pid)
                {
                    drop(lock_file);
                    lock_file = recover_stale_owner_lock(&lock_path, pid)?;
                    recovered = true;
                }
                if !recovered {
                    return Err(AtmError::daemon_serving_state_rejected(format!(
                        "a live ATM daemon already owns {}",
                        lock_path.display()
                    ))
                    .with_source(source));
                }
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
fn open_singleton_lock(lock_path: &std::path::Path) -> Result<File, AtmError> {
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent).map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to create daemon lock directory at {}",
                parent.display()
            ))
            .with_source(source)
        })?;
    }

    OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(lock_path)
        .map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to open daemon singleton lock at {}",
                lock_path.display()
            ))
            .with_source(source)
        })
}

#[cfg(unix)]
fn recover_stale_owner_lock(lock_path: &std::path::Path, stale_pid: u32) -> Result<File, AtmError> {
    for _ in 0..STALE_OWNER_RECOVERY_RETRY_ATTEMPTS {
        thread::sleep(SHUTDOWN_POLL_INTERVAL);
        let retry_file = open_singleton_lock(lock_path)?;
        match retry_file.try_lock_exclusive() {
            Ok(()) => return Ok(retry_file),
            Err(source) if source.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(source) => {
                return Err(AtmError::daemon_unavailable(format!(
                    "failed to retry daemon singleton recovery at {}",
                    lock_path.display()
                ))
                .with_source(source));
            }
        }
    }

    Err(AtmError::daemon_stale_owner_recovery_failed(format!(
        "daemon owner record at {} points to non-live pid {} and the singleton lock could not be safely recovered",
        lock_path.display(),
        stale_pid
    )))
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
struct PreparedRuntimeServer {
    _singleton: SingletonGuard,
    listener: UnixListener,
    signals: DaemonShutdownSignals,
    registry: Arc<ActiveConnectionRegistry>,
    force_shutdown: Arc<AtomicBool>,
}

#[cfg(unix)]
impl PreparedRuntimeServer {
    fn bind(socket_path: PathBuf) -> Result<Self, AtmError> {
        let signals = DaemonShutdownSignals::install()?;
        let singleton = SingletonGuard::acquire(&socket_path)?;
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
        Ok(Self {
            _singleton: singleton,
            listener,
            signals,
            registry: Arc::new(ActiveConnectionRegistry::default()),
            force_shutdown: Arc::new(AtomicBool::new(false)),
        })
    }

    fn serve(self, dispatcher: Arc<dyn RequestDispatcher + Send + Sync>) -> Result<(), AtmError> {
        self.serve_with_deadlines(dispatcher, GRACEFUL_DRAIN_DEADLINE, FORCE_CANCEL_DEADLINE)
    }

    fn serve_with_deadlines(
        self,
        dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
        graceful_drain_deadline: Duration,
        force_cancel_deadline: Duration,
    ) -> Result<(), AtmError> {
        self.serve_with_deadlines_and_accept_probe(
            dispatcher,
            graceful_drain_deadline,
            force_cancel_deadline,
            None,
        )
    }

    fn serve_with_deadlines_and_accept_probe(
        self,
        dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
        graceful_drain_deadline: Duration,
        force_cancel_deadline: Duration,
        accepted_probe: Option<std::sync::mpsc::Sender<()>>,
    ) -> Result<(), AtmError> {
        let Self {
            _singleton,
            listener,
            signals,
            registry,
            force_shutdown,
        } = self;
        thread::scope(|scope| -> Result<(), AtmError> {
            let mut serve_error = None;
            loop {
                if signals.reload.swap(false, Ordering::SeqCst) {
                    tracing::warn!(
                        deferred_sprint = "R.18",
                        deferred_doc = "docs/phase-R/sprint-R18.md",
                        "bounded SIGHUP-triggered config/roster reload is not wired yet"
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

                        let active = registry.register(&stream)?;
                        if let Some(accepted_probe) = accepted_probe.as_ref() {
                            let _ = accepted_probe.send(());
                        }
                        let dispatcher = Arc::clone(&dispatcher);
                        let force_shutdown = Arc::clone(&force_shutdown);
                        scope.spawn(move || {
                            let _active = active;
                            if let Err(error) =
                                handle_connection(&mut stream, dispatcher, force_shutdown.as_ref())
                            {
                                tracing::warn!(%error, "daemon connection handling failed");
                            }
                        });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(SHUTDOWN_POLL_INTERVAL);
                    }
                    Err(source) => {
                        serve_error = Some(
                            AtmError::daemon_unavailable(
                                "failed while accepting daemon connection",
                            )
                            .with_source(source),
                        );
                        break;
                    }
                }
            }

            let shutdown_started = Instant::now();
            drain_active_connections_for_shutdown(
                registry.as_ref(),
                force_shutdown.as_ref(),
                graceful_drain_deadline,
                force_cancel_deadline,
                shutdown_started,
            )?;
            if let Some(error) = serve_error {
                return Err(error);
            }
            Ok(())
        })
    }
}

#[cfg(unix)]
fn drain_active_connections_for_shutdown(
    registry: &ActiveConnectionRegistry,
    force_shutdown: &AtomicBool,
    graceful_drain_deadline: Duration,
    force_cancel_deadline: Duration,
    shutdown_started: Instant,
) -> Result<(), AtmError> {
    tracing::info!(
        active_connections = registry.active_connections(),
        "daemon shutdown signal received; starting graceful drain"
    );
    let graceful_deadline = shutdown_started + graceful_drain_deadline;
    let force_cancel_deadline = shutdown_started + force_cancel_deadline;
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
    Ok(())
}

#[cfg(not(unix))]
struct PreparedRuntimeServer;

#[cfg(not(unix))]
impl PreparedRuntimeServer {
    fn serve(self, _dispatcher: Arc<dyn RequestDispatcher + Send + Sync>) -> Result<(), AtmError> {
        Err(AtmError::daemon_unavailable(
            "atm-daemon socket transport requires a Unix platform",
        ))
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
    dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
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
    // TODO(phase-R): enforce a max 32 inflight requests per connection once framed multiplexing lands.
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = result_tx.send(dispatcher.dispatch(request));
    });
    let response = match result_rx.recv_timeout(REQUEST_DEADLINE) {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => {
            ResponseEnvelope::Error(atm_core::protocol::ProtocolErrorEnvelope::from_error(&error))
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            ResponseEnvelope::Error(atm_core::protocol::ProtocolErrorEnvelope::from_error(
            &AtmError::daemon_unavailable(
                "daemon request exceeded the 3s runtime deadline; the operation may still complete in the background",
            )
            .with_recovery(
                "Check the destination mailbox or service-side effects before retrying this ATM command.",
            ),
        ))
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            ResponseEnvelope::Error(atm_core::protocol::ProtocolErrorEnvelope::from_error(
                &AtmError::daemon_unavailable(
                    "daemon request dispatcher stopped before returning a response",
                )
                .with_recovery(
                    "Retry the ATM command after the daemon finishes recovering the request runtime.",
                ),
            ))
        }
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
    pub(crate) fn prepare_runtime(&self) -> Result<PreparedRuntimeServer, AtmError> {
        let socket_path = atm_core::protocol::daemon_socket_path()?;
        self.prepare_runtime_at_socket_path(socket_path)
    }

    #[cfg(not(unix))]
    pub(crate) fn prepare_runtime(&self) -> Result<PreparedRuntimeServer, AtmError> {
        Err(AtmError::daemon_unavailable(
            "atm-daemon socket transport requires a Unix platform",
        ))
    }

    #[cfg(unix)]
    pub(crate) fn prepare_runtime_at_socket_path(
        &self,
        socket_path: PathBuf,
    ) -> Result<PreparedRuntimeServer, AtmError> {
        PreparedRuntimeServer::bind(socket_path)
    }

    #[cfg(not(unix))]
    #[allow(dead_code)]
    pub(crate) fn prepare_runtime_at_socket_path(
        &self,
        _socket_path: PathBuf,
    ) -> Result<PreparedRuntimeServer, AtmError> {
        Err(AtmError::daemon_unavailable(
            "atm-daemon socket transport requires a Unix platform",
        ))
    }
}

impl boundary::sealed::Sealed for LocalSocketServerTransport {}

impl boundary::ServerTransport for LocalSocketServerTransport {
    #[cfg(unix)]
    fn serve(&self, _dispatcher: Arc<dyn RequestDispatcher + Send + Sync>) -> Result<(), AtmError> {
        Err(AtmError::daemon_unavailable(
            "LocalSocketServerTransport::serve cannot bootstrap the daemon directly; use RuntimeComposition::start()",
        )
        .with_recovery(
            "Enter the daemon through RuntimeComposition::start() so lifecycle state, singleton ownership, and shutdown handling stay consistent.",
        ))
    }

    #[cfg(not(unix))]
    fn serve(&self, _dispatcher: Arc<dyn RequestDispatcher + Send + Sync>) -> Result<(), AtmError> {
        Err(AtmError::daemon_unavailable(
            "atm-daemon socket transport requires a Unix platform",
        ))
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

#[cfg(all(test, unix))]
mod tests;
