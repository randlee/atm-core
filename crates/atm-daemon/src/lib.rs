#![forbid(unsafe_code)]
//! Skeleton crate for Phase R daemon runtime work.

mod boundary_adapters;
pub(crate) mod composition;
mod direct_boundaries;
mod runtime_health;

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

#[cfg(unix)]
use atm_core::protocol::RequestEnvelope as ProtocolRequestEnvelope;
use atm_core::{
    RequestEnvelope, ResponseEnvelope,
    boundary::{self, RequestDispatcher},
    error::AtmError,
};
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
    AtmError::daemon_unavailable(message)
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
        self.active_connections.fetch_sub(1, Ordering::SeqCst);
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

                        let active = registry.register(&stream)?;
                        let dispatcher = Arc::clone(&dispatcher);
                        let force_shutdown = Arc::clone(&force_shutdown);
                        scope.spawn(move || {
                            let _active = active;
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
            if let Some(error) = serve_error {
                return Err(error);
            }
            Ok(())
        })
    }
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

/// Run the daemon entrypoint with the currently assembled runtime composition.
///
/// # Errors
///
/// Returns [`AtmError`] when the daemon transport cannot start or serve.
pub fn run_daemon() -> Result<(), AtmError> {
    composition::compose_runtime()?.start()
}

#[cfg(all(test, unix))]
mod tests {
    use super::runtime_health::{DaemonRequestDispatcher, RuntimeStatusCache};
    use super::{
        ActiveConnectionRegistry, DaemonShutdownSignals, HOST_RUNTIME_OWNER_LOCK_FILE,
        SingletonGuard, host_runtime_lock_path_from_home,
    };
    use atm_core::boundary::RequestDispatcher;
    use atm_core::error_codes::AtmErrorCode;
    use atm_core::protocol::{
        HeartbeatActivity, RequestEnvelope, ResponseEnvelope, RuntimeLivenessState,
        RuntimeReadinessState, TeamMemberHeartbeatRequest,
    };
    use atm_core::schema::{AgentMember, TeamConfig};
    use atm_core::types::{AgentName, IsoTimestamp, TeamName};
    use atm_rusqlite::assemble_boundary;
    use std::fs::OpenOptions;
    use std::io::{Read, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::sync::{Arc, mpsc};
    use std::time::Duration;
    use tempfile::TempDir;

    #[test]
    fn daemon_shutdown_signals_install_is_repeatable() {
        let first = DaemonShutdownSignals::install().expect("first install");
        first
            .terminate
            .store(true, std::sync::atomic::Ordering::SeqCst);
        first
            .reload
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let second = DaemonShutdownSignals::install().expect("second install");

        assert!(second.terminate.load(std::sync::atomic::Ordering::SeqCst));
        assert!(second.reload.load(std::sync::atomic::Ordering::SeqCst));
        second
            .terminate
            .store(false, std::sync::atomic::Ordering::SeqCst);
        second
            .reload
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }

    #[test]
    fn daemon_host_runtime_lock_path_ignores_atm_home() {
        let tempdir = TempDir::new().expect("tempdir");
        let user_home = tempdir.path().join("user-home");
        let atm_home = tempdir.path().join("workspace").join(".atm-home");
        let runtime_dir = atm_core::home::host_runtime_dir_from_home(&user_home);
        let path = host_runtime_lock_path_from_home(&runtime_dir, HOST_RUNTIME_OWNER_LOCK_FILE);

        assert_eq!(
            path,
            user_home.join(".atm").join("daemon").join("owner.lock")
        );
        assert!(
            !path.starts_with(&atm_home),
            "daemon singleton lock must remain OS-home scoped"
        );
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
    fn singleton_guard_recovers_stale_owner_once_lock_is_released() {
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

        let (release_tx, release_rx) = mpsc::channel();
        std::thread::spawn(move || {
            release_rx.recv().expect("release signal");
            drop(file);
        });

        let release_tx_clone = release_tx.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            release_tx_clone.send(()).expect("release lock");
        });

        let guard = SingletonGuard::acquire_at(&tempdir.path().join("atm.sock"), lock_path)
            .expect("stale owner recovery should succeed");
        drop(guard);
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

    fn install_test_roster(db_path: &std::path::Path, members: &[&str]) {
        let assembly = assemble_boundary(db_path).expect("sqlite boundary");
        assembly
            .roster_store()
            .replace_roster(atm_core::boundary::RosterStoreReplaceRosterRequest {
                team: "test-team".parse().expect("team"),
                roster: TeamConfig {
                    members: members
                        .iter()
                        .map(|name| AgentMember::with_name((*name).parse().expect("member")))
                        .collect(),
                    ..Default::default()
                },
                source: Some("daemon-heartbeat-test".to_string()),
            })
            .expect("replace roster");
    }

    fn write_team_config(home_dir: &std::path::Path, members: &[&str]) {
        let team_dir = home_dir.join(".claude").join("teams").join("test-team");
        std::fs::create_dir_all(team_dir.join("inboxes")).expect("inboxes dir");
        let config = TeamConfig {
            members: members
                .iter()
                .map(|name| AgentMember::with_name((*name).parse().expect("member")))
                .collect(),
            ..Default::default()
        };
        std::fs::write(
            team_dir.join("config.json"),
            serde_json::to_vec(&config).expect("team config"),
        )
        .expect("write team config");
    }

    #[test]
    fn heartbeat_updates_status_cache_and_doctor_projection() {
        let tempdir = TempDir::new().expect("tempdir");
        let atm_home = tempdir.path().join("atm-home");
        std::fs::create_dir_all(&atm_home).expect("atm home dir");
        let db_path = tempdir.path().join("mail.db");

        install_test_roster(&db_path, &["team-lead", "qa-a"]);
        write_team_config(&atm_home, &["team-lead", "qa-a"]);

        let status_cache = RuntimeStatusCache::new();
        let dispatcher =
            DaemonRequestDispatcher::new_for_test(atm_home.clone(), status_cache.clone(), db_path);
        let team: TeamName = "test-team".parse().expect("team");
        let member: AgentName = "team-lead".parse().expect("member");

        let response = dispatcher
            .dispatch(RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
                team: team.clone(),
                member: member.clone(),
                pid: std::process::id(),
                observed_at: IsoTimestamp::now(),
                activity: HeartbeatActivity::ActiveToolUse,
            }))
            .expect("heartbeat response");
        match response {
            ResponseEnvelope::Heartbeat(response) => {
                assert_eq!(response.team, team);
                assert_eq!(response.member, member);
                assert_eq!(
                    response.state,
                    atm_core::protocol::RuntimeMemberState::Active
                );
            }
            other => panic!("expected heartbeat response, got {other:?}"),
        }

        let snapshot = status_cache.snapshot().expect("snapshot");
        assert_eq!(snapshot.liveness, RuntimeLivenessState::Running);
        assert_eq!(snapshot.readiness, RuntimeReadinessState::Ready);
        assert_eq!(snapshot.member_counts.active_members, 1);

        let doctor = dispatcher
            .dispatch(RequestEnvelope::Doctor(atm_core::doctor::DoctorQuery {
                home_dir: atm_home.clone(),
                current_dir: atm_home.clone(),
                team_override: Some(team.clone()),
            }))
            .expect("doctor response");
        match doctor {
            ResponseEnvelope::Doctor(report) => {
                let runtime_status = report.runtime_status.expect("runtime status");
                assert_eq!(runtime_status.member_counts.active_members, 1);
                assert_eq!(runtime_status.member_counts.unknown_members, 1);
            }
            other => panic!("expected doctor response, got {other:?}"),
        }
    }

    #[test]
    fn heartbeat_rejects_live_pid_conflict() {
        let tempdir = TempDir::new().expect("tempdir");
        let atm_home = tempdir.path().join("atm-home");
        std::fs::create_dir_all(&atm_home).expect("atm home dir");
        let db_path = tempdir.path().join("mail.db");

        install_test_roster(&db_path, &["team-lead"]);

        let dispatcher =
            DaemonRequestDispatcher::new_for_test(atm_home, RuntimeStatusCache::new(), db_path);
        let team: TeamName = "test-team".parse().expect("team");
        let member: AgentName = "team-lead".parse().expect("member");

        dispatcher
            .dispatch(RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
                team: team.clone(),
                member: member.clone(),
                pid: std::process::id(),
                observed_at: IsoTimestamp::now(),
                activity: HeartbeatActivity::ActiveToolUse,
            }))
            .expect("initial heartbeat");

        let error = dispatcher
            .dispatch(RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
                team,
                member,
                pid: std::process::id().saturating_add(1),
                observed_at: IsoTimestamp::now(),
                activity: HeartbeatActivity::Idle,
            }))
            .expect_err("live pid conflict");

        assert_eq!(error.code, AtmErrorCode::IdentityConflict);
        assert_eq!(
            error.message,
            "ATM_IDENTITY_CONFLICT: stop and report to user immediately"
        );
    }

    #[test]
    fn heartbeat_accepts_pid_takeover_when_previous_pid_is_dead() {
        let tempdir = TempDir::new().expect("tempdir");
        let atm_home = tempdir.path().join("atm-home");
        std::fs::create_dir_all(&atm_home).expect("atm home dir");
        let db_path = tempdir.path().join("mail.db");

        install_test_roster(&db_path, &["team-lead"]);

        let dispatcher =
            DaemonRequestDispatcher::new_for_test(atm_home, RuntimeStatusCache::new(), db_path);
        let team: TeamName = "test-team".parse().expect("team");
        let member: AgentName = "team-lead".parse().expect("member");

        dispatcher
            .dispatch(RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
                team: team.clone(),
                member: member.clone(),
                pid: 999_999,
                observed_at: IsoTimestamp::now(),
                activity: HeartbeatActivity::Idle,
            }))
            .expect("initial dead-pid heartbeat");

        let response = dispatcher
            .dispatch(RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
                team,
                member,
                pid: std::process::id(),
                observed_at: IsoTimestamp::now(),
                activity: HeartbeatActivity::ActiveToolUse,
            }))
            .expect("takeover heartbeat");

        match response {
            ResponseEnvelope::Heartbeat(response) => {
                assert!(response.pid_changed);
                assert_eq!(response.pid, std::process::id());
                assert_eq!(
                    response.state,
                    atm_core::protocol::RuntimeMemberState::Active
                );
            }
            other => panic!("expected heartbeat response, got {other:?}"),
        }
    }
}
