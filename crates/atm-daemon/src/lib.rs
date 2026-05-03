mod client;
mod runtime_observability;

use std::fs;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use atm_core::dispatcher::{
    DaemonRequest, DaemonResponse, DispatchError, RequestDispatcher, RequestKind, RequestPayload,
};
use atm_core::doctor::{DoctorQuery, DoctorReport, DoctorRuntimeHealth, DoctorStatus, run_doctor};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::home;
use atm_core::observability::ObservabilityPort;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};

use crate::client::{
    REMOTE_SERVER_IO_TIMEOUT, SAME_HOST_SERVER_IO_TIMEOUT, dispatch_error_to_atm, read_frame,
    write_control_state, write_frame,
};
pub use crate::client::{
    ensure_daemon_running, request_doctor_json_with_autostart, request_remote,
};
use crate::runtime_observability::{
    DaemonObservability, emit_runtime_event, normalize_doctor_report_observability,
};

pub const SAME_HOST_REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
pub const REMOTE_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
pub const REMOTE_IO_TIMEOUT: Duration = Duration::from_secs(5);
pub const DEFAULT_REMOTE_RETRY_BUDGET: Duration = Duration::from_secs(30);
pub const AUTO_START_PUBLISH_TIMEOUT: Duration = Duration::from_secs(10);
pub const SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);
pub const SHUTDOWN_FORCE_TIMEOUT: Duration = Duration::from_secs(10);
pub const MAX_ACCEPTS: usize = 64;
pub const MAX_INFLIGHT_PER_CONNECTION: usize = 32;
pub(crate) const MAX_FRAME_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LocalEndpoint {
    #[cfg(unix)]
    UnixSocket(PathBuf),
    TcpLoopback(SocketAddr),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlState {
    pub pid: u32,
    pub local_endpoint: LocalEndpoint,
    pub remote_endpoint: SocketAddr,
}

#[derive(Debug, Clone)]
pub struct DaemonPaths {
    pub state_dir: PathBuf,
    pub singleton_path: PathBuf,
    pub control_path: PathBuf,
    #[cfg(unix)]
    pub local_socket_path: PathBuf,
}

impl DaemonPaths {
    pub fn from_home(home_dir: &Path) -> Self {
        let state_dir = home_dir.join(".atm-state").join("daemon");
        Self {
            singleton_path: state_dir.join("singleton.json"),
            control_path: state_dir.join("control.json"),
            #[cfg(unix)]
            local_socket_path: state_dir.join("local.sock"),
            state_dir,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DaemonConfig {
    pub home_dir: PathBuf,
    pub max_accepts: usize,
    pub max_inflight_per_connection: usize,
}

impl DaemonConfig {
    pub fn from_home(home_dir: PathBuf) -> Self {
        Self {
            home_dir,
            max_accepts: MAX_ACCEPTS,
            max_inflight_per_connection: MAX_INFLIGHT_PER_CONNECTION,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WireError {
    code: AtmErrorCode,
    message: String,
    recovery: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WireResponseEnvelope {
    response: Option<DaemonResponse>,
    error: Option<WireError>,
}

impl WireResponseEnvelope {
    fn success(response: DaemonResponse) -> Self {
        Self {
            response: Some(response),
            error: None,
        }
    }

    fn failure(error: AtmError) -> Self {
        Self {
            response: None,
            error: Some(WireError {
                code: error.code,
                message: error.message,
                recovery: error.recovery,
            }),
        }
    }
}

pub struct CoreDispatcher {
    home_dir: PathBuf,
    observability: Arc<dyn ObservabilityPort + Send + Sync>,
}

impl CoreDispatcher {
    pub fn new(home_dir: PathBuf, observability: Arc<dyn ObservabilityPort + Send + Sync>) -> Self {
        Self {
            home_dir,
            observability,
        }
    }
}

impl RequestDispatcher for CoreDispatcher {
    fn dispatch(&self, request: DaemonRequest) -> Result<DaemonResponse, DispatchError> {
        match request.payload {
            RequestPayload::Doctor(value) => {
                let query: DoctorQuery = serde_json::from_value(value)
                    .map_err(|error| DispatchError::PayloadDecode(error.to_string()))?;
                let report = thread::spawn({
                    let home_dir = self.home_dir.clone();
                    let observability = Arc::clone(&self.observability);
                    let team_name = request.team_name.clone();
                    move || {
                        let report = run_doctor(query, observability.as_ref())?;
                        let report =
                            normalize_doctor_report_observability(report, observability.as_ref());
                        Ok::<DoctorReport, AtmError>(attach_runtime_health(
                            report, &home_dir, &team_name,
                        ))
                    }
                })
                .join()
                .map_err(|_| DispatchError::Handler("doctor worker panicked".to_string()))?
                .map_err(|error| DispatchError::Handler(error.to_string()))?;
                let payload_json = serde_json::to_string(&report)
                    .map_err(|error| DispatchError::ResponseEncode(error.to_string()))?;
                emit_runtime_event(
                    self.observability.as_ref(),
                    &request.team_name,
                    &request.agent_name,
                    "doctor_request",
                    "ok",
                    None,
                );
                Ok(DaemonResponse {
                    kind: RequestKind::Doctor,
                    payload_json,
                })
            }
            RequestPayload::Heartbeat(_) => {
                emit_runtime_event(
                    self.observability.as_ref(),
                    &request.team_name,
                    &request.agent_name,
                    "heartbeat_request",
                    "ok",
                    None,
                );
                Ok(DaemonResponse {
                    kind: RequestKind::Heartbeat,
                    payload_json: "{\"ok\":true}".to_string(),
                })
            }
            other => Err(DispatchError::Unsupported(other.kind())),
            // TODO(phase-q): replace the hardcoded request-kind match with registered handlers once the daemon command surface grows beyond the current Q.4 set.
        }
    }
}

pub struct TestSocketClient<'a> {
    dispatcher: &'a dyn RequestDispatcher,
}

impl<'a> TestSocketClient<'a> {
    pub fn new(dispatcher: &'a dyn RequestDispatcher) -> Self {
        Self { dispatcher }
    }

    pub fn request(&self, request: DaemonRequest) -> Result<DaemonResponse, AtmError> {
        self.dispatcher
            .dispatch(request)
            .map_err(dispatch_error_to_atm)
    }
}

pub struct DaemonHandle {
    stop: Arc<AtomicBool>,
    inflight: Arc<AtomicUsize>,
    local_thread: Option<JoinHandle<()>>,
    remote_thread: Option<JoinHandle<()>>,
    worker_threads: Arc<Mutex<Vec<JoinHandle<()>>>>,
    singleton: SingletonGuard,
    home_dir: PathBuf,
    control_path: PathBuf,
    #[cfg(unix)]
    local_socket_path: PathBuf,
    local_endpoint: LocalEndpoint,
    remote_endpoint: SocketAddr,
}

impl DaemonHandle {
    pub fn local_endpoint(&self) -> &LocalEndpoint {
        &self.local_endpoint
    }

    pub fn remote_endpoint(&self) -> SocketAddr {
        self.remote_endpoint
    }

    pub fn shutdown(mut self) -> Result<(), AtmError> {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.local_thread.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.remote_thread.take() {
            let _ = handle.join();
        }

        wait_for_inflight_zero(&self.inflight, SHUTDOWN_DRAIN_TIMEOUT);
        join_worker_threads(&self.worker_threads)?;
        wait_for_inflight_zero(&self.inflight, SHUTDOWN_FORCE_TIMEOUT);
        join_worker_threads(&self.worker_threads)?;
        checkpoint_runtime_wal(&self.home_dir)?;

        let _ = fs::remove_file(&self.control_path);
        #[cfg(unix)]
        let _ = fs::remove_file(&self.local_socket_path);
        self.singleton.release()?;
        Ok(())
    }
}

pub fn start_runtime(
    config: DaemonConfig,
    dispatcher: Arc<dyn RequestDispatcher>,
) -> Result<DaemonHandle, AtmError> {
    let paths = DaemonPaths::from_home(&config.home_dir);
    fs::create_dir_all(&paths.state_dir).map_err(|error| {
        AtmError::daemon_start_failed(format!(
            "failed to create daemon state directory {}: {error}",
            paths.state_dir.display()
        ))
        .with_source(error)
    })?;

    let singleton = SingletonGuard::acquire(&paths.singleton_path)?;
    let stop = Arc::new(AtomicBool::new(false));
    let inflight = Arc::new(AtomicUsize::new(0));
    let worker_threads = Arc::new(Mutex::new(Vec::new()));

    #[cfg(unix)]
    if paths.local_socket_path.exists() {
        fs::remove_file(&paths.local_socket_path).map_err(|error| {
            AtmError::daemon_start_failed(format!(
                "failed to remove stale local socket {}: {error}",
                paths.local_socket_path.display()
            ))
            .with_source(error)
        })?;
    }

    #[cfg(unix)]
    let local_listener = UnixListener::bind(&paths.local_socket_path).map_err(|error| {
        AtmError::daemon_start_failed(format!(
            "failed to bind local daemon socket {}: {error}",
            paths.local_socket_path.display()
        ))
        .with_source(error)
    })?;
    #[cfg(unix)]
    local_listener.set_nonblocking(true).map_err(|error| {
        AtmError::daemon_start_failed("failed to set local socket nonblocking").with_source(error)
    })?;

    #[cfg(unix)]
    let local_endpoint = LocalEndpoint::UnixSocket(paths.local_socket_path.clone());

    #[cfg(not(unix))]
    let (local_listener, local_endpoint) = bind_loopback_listener()?;

    #[cfg(not(unix))]
    local_listener.set_nonblocking(true).map_err(|error| {
        AtmError::daemon_start_failed("failed to set loopback listener nonblocking")
            .with_source(error)
    })?;

    // TODO(phase-q): replace plain TCP loopback/remote transport with TLS before cross-host daemon traffic is enabled.
    let remote_listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|error| {
        AtmError::daemon_start_failed(format!(
            "failed to bind remote daemon TCP listener: {error}"
        ))
        .with_source(error)
    })?;
    remote_listener.set_nonblocking(true).map_err(|error| {
        AtmError::daemon_start_failed("failed to set remote TCP listener nonblocking")
            .with_source(error)
    })?;
    let remote_endpoint = remote_listener.local_addr().map_err(|error| {
        AtmError::daemon_start_failed("failed to inspect remote daemon address").with_source(error)
    })?;

    write_control_state(
        &paths.control_path,
        &ControlState {
            pid: std::process::id(),
            local_endpoint: local_endpoint.clone(),
            remote_endpoint,
        },
    )?;

    let local_thread = {
        let stop = Arc::clone(&stop);
        let inflight = Arc::clone(&inflight);
        let worker_threads = Arc::clone(&worker_threads);
        let dispatcher = Arc::clone(&dispatcher);
        let max_inflight = config.max_inflight_per_connection;
        #[cfg(unix)]
        let listener = local_listener;
        #[cfg(not(unix))]
        let listener = local_listener;
        Some(thread::spawn(move || {
            accept_local_loop(
                listener,
                stop,
                inflight,
                worker_threads,
                dispatcher,
                max_inflight,
            )
        }))
    };

    let remote_thread = {
        let stop = Arc::clone(&stop);
        let inflight = Arc::clone(&inflight);
        let worker_threads = Arc::clone(&worker_threads);
        let dispatcher = Arc::clone(&dispatcher);
        let max_inflight = config.max_inflight_per_connection;
        Some(thread::spawn(move || {
            accept_tcp_loop(
                remote_listener,
                stop,
                inflight,
                worker_threads,
                dispatcher,
                max_inflight,
            )
        }))
    };

    Ok(DaemonHandle {
        stop,
        inflight,
        local_thread,
        remote_thread,
        worker_threads,
        singleton,
        home_dir: config.home_dir,
        control_path: paths.control_path,
        #[cfg(unix)]
        local_socket_path: paths.local_socket_path,
        local_endpoint,
        remote_endpoint,
    })
}

#[cfg(unix)]
fn accept_local_loop(
    listener: UnixListener,
    stop: Arc<AtomicBool>,
    inflight: Arc<AtomicUsize>,
    worker_threads: Arc<Mutex<Vec<JoinHandle<()>>>>,
    dispatcher: Arc<dyn RequestDispatcher>,
    max_inflight: usize,
) {
    accept_unix_loop(
        listener,
        stop,
        inflight,
        worker_threads,
        dispatcher,
        max_inflight,
    );
}

#[cfg(unix)]
fn accept_unix_loop(
    listener: UnixListener,
    stop: Arc<AtomicBool>,
    inflight: Arc<AtomicUsize>,
    worker_threads: Arc<Mutex<Vec<JoinHandle<()>>>>,
    dispatcher: Arc<dyn RequestDispatcher>,
    max_inflight: usize,
) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                spawn_local_connection(
                    stream,
                    Arc::clone(&inflight),
                    Arc::clone(&worker_threads),
                    Arc::clone(&dispatcher),
                    max_inflight,
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(25));
            }
            Err(_) => thread::sleep(Duration::from_millis(25)),
        }
    }
}

#[cfg(not(unix))]
fn accept_local_loop(
    listener: TcpListener,
    stop: Arc<AtomicBool>,
    inflight: Arc<AtomicUsize>,
    worker_threads: Arc<Mutex<Vec<JoinHandle<()>>>>,
    dispatcher: Arc<dyn RequestDispatcher>,
    max_inflight: usize,
) {
    accept_tcp_loop(
        listener,
        stop,
        inflight,
        worker_threads,
        dispatcher,
        max_inflight,
    );
}

fn accept_tcp_loop(
    listener: TcpListener,
    stop: Arc<AtomicBool>,
    inflight: Arc<AtomicUsize>,
    worker_threads: Arc<Mutex<Vec<JoinHandle<()>>>>,
    dispatcher: Arc<dyn RequestDispatcher>,
    max_inflight: usize,
) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                spawn_tcp_connection(
                    stream,
                    Arc::clone(&inflight),
                    Arc::clone(&worker_threads),
                    Arc::clone(&dispatcher),
                    max_inflight,
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(25));
            }
            Err(_) => thread::sleep(Duration::from_millis(25)),
        }
    }
}

#[cfg(unix)]
fn spawn_local_connection(
    mut stream: UnixStream,
    inflight: Arc<AtomicUsize>,
    worker_threads: Arc<Mutex<Vec<JoinHandle<()>>>>,
    dispatcher: Arc<dyn RequestDispatcher>,
    max_inflight: usize,
) {
    if inflight.fetch_add(1, Ordering::SeqCst) >= max_inflight {
        inflight.fetch_sub(1, Ordering::SeqCst);
        let _ = write_frame(
            &mut stream,
            &WireResponseEnvelope::failure(AtmError::daemon_unavailable(
                "local daemon inflight capacity exceeded",
            )),
        );
        return;
    }
    let _ = stream.set_read_timeout(Some(SAME_HOST_SERVER_IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(SAME_HOST_SERVER_IO_TIMEOUT));
    let handle = thread::spawn(move || {
        let envelope = match read_frame::<DaemonRequest, _>(&mut stream) {
            Ok(request) => match dispatcher.dispatch(request) {
                Ok(response) => WireResponseEnvelope::success(response),
                Err(error) => WireResponseEnvelope::failure(dispatch_error_to_atm(error)),
            },
            Err(error) => WireResponseEnvelope::failure(error),
        };
        let _ = write_frame(&mut stream, &envelope);
        inflight.fetch_sub(1, Ordering::SeqCst);
    });
    if let Ok(mut handles) = worker_threads.lock() {
        handles.push(handle);
    }
}

fn spawn_tcp_connection(
    mut stream: TcpStream,
    inflight: Arc<AtomicUsize>,
    worker_threads: Arc<Mutex<Vec<JoinHandle<()>>>>,
    dispatcher: Arc<dyn RequestDispatcher>,
    max_inflight: usize,
) {
    if inflight.fetch_add(1, Ordering::SeqCst) >= max_inflight {
        inflight.fetch_sub(1, Ordering::SeqCst);
        let _ = write_frame(
            &mut stream,
            &WireResponseEnvelope::failure(AtmError::daemon_unavailable(
                "daemon inflight capacity exceeded",
            )),
        );
        return;
    }
    let _ = stream.set_read_timeout(Some(REMOTE_SERVER_IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(REMOTE_SERVER_IO_TIMEOUT));
    let handle = thread::spawn(move || {
        let envelope = match read_frame::<DaemonRequest, _>(&mut stream) {
            Ok(request) => match dispatcher.dispatch(request) {
                Ok(response) => WireResponseEnvelope::success(response),
                Err(error) => WireResponseEnvelope::failure(dispatch_error_to_atm(error)),
            },
            Err(error) => WireResponseEnvelope::failure(error),
        };
        let _ = write_frame(&mut stream, &envelope);
        inflight.fetch_sub(1, Ordering::SeqCst);
    });
    if let Ok(mut handles) = worker_threads.lock() {
        handles.push(handle);
    }
}

#[cfg(not(unix))]
fn bind_loopback_listener() -> Result<(TcpListener, LocalEndpoint), AtmError> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|error| {
        AtmError::daemon_start_failed(format!("failed to bind local loopback listener: {error}"))
            .with_source(error)
    })?;
    let addr = listener.local_addr().map_err(|error| {
        AtmError::daemon_start_failed("failed to inspect loopback address").with_source(error)
    })?;
    Ok((listener, LocalEndpoint::TcpLoopback(addr)))
}

struct SingletonGuard {
    path: PathBuf,
}

impl SingletonGuard {
    fn acquire(path: &Path) -> Result<Self, AtmError> {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
        {
            Ok(mut file) => {
                let payload = serde_json::json!({ "pid": std::process::id() });
                serde_json::to_writer(&mut file, &payload).map_err(|error| {
                    AtmError::daemon_start_failed("failed to serialize singleton state")
                        .with_source(error)
                })?;
                Ok(Self {
                    path: path.to_path_buf(),
                })
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let raw = fs::read(path).map_err(|read_error| {
                    AtmError::daemon_already_running(format!(
                        "daemon singleton exists at {} and could not be inspected: {read_error}",
                        path.display()
                    ))
                    .with_source(read_error)
                })?;
                let pid = serde_json::from_slice::<Value>(&raw)
                    .ok()
                    .and_then(|value| value.get("pid").and_then(Value::as_u64))
                    .map(|pid| pid as u32);
                if pid.is_some_and(process_is_alive) {
                    return Err(AtmError::daemon_already_running(format!(
                        "another ATM daemon already owns {}",
                        path.display()
                    )));
                }
                fs::remove_file(path).map_err(|remove_error| {
                    AtmError::daemon_start_failed(format!(
                        "failed to remove stale daemon singleton {}: {remove_error}",
                        path.display()
                    ))
                    .with_source(remove_error)
                })?;
                Self::acquire(path)
            }
            Err(error) => Err(AtmError::daemon_start_failed(format!(
                "failed to create daemon singleton {}: {error}",
                path.display()
            ))
            .with_source(error)),
        }
    }

    fn release(&self) -> Result<(), AtmError> {
        fs::remove_file(&self.path).map_err(|error| {
            AtmError::daemon_start_failed(format!(
                "failed to release daemon singleton {}: {error}",
                self.path.display()
            ))
            .with_source(error)
        })
    }
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    let pid: libc::pid_t = match pid.try_into() {
        Ok(pid) => pid,
        Err(_) => return false,
    };
    let result = unsafe { libc::kill(pid, 0) };
    if result == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(windows)]
fn process_is_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return false;
    }
    let mut exit_code = 0u32;
    let ok = unsafe { GetExitCodeProcess(handle, &mut exit_code) };
    unsafe { CloseHandle(handle) };
    ok != 0 && exit_code == STILL_ACTIVE as u32
}

pub fn run_foreground() -> Result<(), AtmError> {
    let home_dir = home::atm_home()?;
    let dispatcher = Arc::new(CoreDispatcher::new(
        home_dir.clone(),
        Arc::new(DaemonObservability::new(&home_dir)),
    ));
    let stop = Arc::new(AtomicBool::new(false));
    let reload = Arc::new(AtomicBool::new(false));
    register_signal_handlers(Arc::clone(&stop), Arc::clone(&reload))?;
    let handle = start_runtime(DaemonConfig::from_home(home_dir), dispatcher)?;
    while !stop.load(Ordering::SeqCst) {
        let _ = reload.swap(false, Ordering::SeqCst);
        thread::sleep(Duration::from_millis(100));
    }
    handle.shutdown()
}

fn register_signal_handlers(
    stop: Arc<AtomicBool>,
    reload: Arc<AtomicBool>,
) -> Result<(), AtmError> {
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&stop)).map_err(
        |error| {
            AtmError::daemon_start_failed("failed to install SIGINT handler").with_source(error)
        },
    )?;
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&stop)).map_err(
        |error| {
            AtmError::daemon_start_failed("failed to install SIGTERM handler").with_source(error)
        },
    )?;
    #[cfg(unix)]
    signal_hook::flag::register(signal_hook::consts::SIGHUP, reload).map_err(|error| {
        AtmError::daemon_start_failed("failed to install SIGHUP handler").with_source(error)
    })?;
    Ok(())
}

fn wait_for_inflight_zero(inflight: &AtomicUsize, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while inflight.load(Ordering::SeqCst) > 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(25));
    }
}

fn join_worker_threads(worker_threads: &Arc<Mutex<Vec<JoinHandle<()>>>>) -> Result<(), AtmError> {
    let handles = {
        let mut handles = worker_threads.lock().map_err(|_| {
            AtmError::daemon_start_failed("worker thread registry lock poisoned during shutdown")
        })?;
        std::mem::take(&mut *handles)
    };
    for handle in handles {
        let _ = handle.join();
    }
    Ok(())
}

fn attach_runtime_health(
    mut report: DoctorReport,
    home_dir: &Path,
    team_name: &atm_core::types::TeamName,
) -> DoctorReport {
    let sqlite_path = home::mail_db_path_from_home(home_dir, team_name).ok();
    report.runtime = Some(DoctorRuntimeHealth {
        singleton_state: DoctorStatus::Healthy,
        singleton_detail: "daemon singleton is owned by the active runtime".to_string(),
        status_cache_state: DoctorStatus::Warning,
        status_cache_detail: "live status-cache health is not yet separately reported in Q.4"
            .to_string(),
        sqlite_runtime_state: if sqlite_path.as_ref().is_some_and(|path| path.exists()) {
            DoctorStatus::Healthy
        } else {
            DoctorStatus::Warning
        },
        sqlite_runtime_detail: sqlite_path
            .map(|path| format!("runtime sees SQLite path {}", path.display()))
            .unwrap_or_else(|| {
                "runtime could not resolve a SQLite path for the active team".to_string()
            }),
    });
    report
}

fn checkpoint_runtime_wal(home_dir: &Path) -> Result<(), AtmError> {
    let teams_root = home_dir.join(".claude").join("teams");
    let Ok(entries) = fs::read_dir(&teams_root) else {
        return Ok(());
    };
    for entry in entries.filter_map(Result::ok) {
        let db_path = entry.path().join(".atm-state").join("mail.sqlite3");
        if !db_path.exists() {
            continue;
        }
        let connection = rusqlite::Connection::open(&db_path).map_err(|error| {
            AtmError::daemon_start_failed(format!(
                "failed to open SQLite store for WAL checkpoint at {}",
                db_path.display()
            ))
            .with_source(error)
        })?;
        connection
            .pragma_update(None, "wal_checkpoint", "TRUNCATE")
            .map_err(|error| {
                AtmError::daemon_start_failed(format!(
                    "failed to checkpoint SQLite WAL at {}",
                    db_path.display()
                ))
                .with_source(error)
            })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::request_local;
    use atm_core::observability::NullObservability;
    use serial_test::serial;
    use tempfile::TempDir;

    #[derive(Default)]
    struct FakeDispatcher {
        responses: std::sync::Mutex<Vec<DaemonResponse>>,
        requests: std::sync::Mutex<Vec<DaemonRequest>>,
    }

    impl FakeDispatcher {
        fn queue_response(&self, response: DaemonResponse) {
            self.responses
                .lock()
                .expect("responses lock")
                .push(response);
        }

        fn request_count(&self) -> usize {
            self.requests.lock().expect("requests lock").len()
        }
    }

    impl RequestDispatcher for FakeDispatcher {
        fn dispatch(&self, request: DaemonRequest) -> Result<DaemonResponse, DispatchError> {
            self.requests.lock().expect("requests lock").push(request);
            self.responses
                .lock()
                .expect("responses lock")
                .pop()
                .ok_or_else(|| DispatchError::Unsupported(RequestKind::Heartbeat))
        }
    }

    #[test]
    fn test_socket_client_uses_dispatcher_contract() {
        let dispatcher = FakeDispatcher::default();
        dispatcher.queue_response(DaemonResponse {
            kind: RequestKind::Doctor,
            payload_json: "{\"summary\":{\"status\":\"healthy\"}}".to_string(),
        });
        let client = TestSocketClient::new(&dispatcher);
        let response = client
            .request(DaemonRequest {
                team_name: "atm-dev".parse().expect("team"),
                agent_name: "arch-ctm".parse().expect("agent"),
                payload: RequestPayload::Doctor(serde_json::json!({"team_override":"atm-dev"})),
            })
            .expect("response");
        assert_eq!(response.kind, RequestKind::Doctor);
        assert_eq!(dispatcher.request_count(), 1);
    }

    #[test]
    #[serial]
    fn second_daemon_startup_fails_deterministically() {
        let tempdir = TempDir::new().expect("tempdir");
        let home_dir = tempdir.path().to_path_buf();
        let first = start_runtime(
            DaemonConfig::from_home(home_dir.clone()),
            Arc::new(CoreDispatcher::new(
                home_dir.clone(),
                Arc::new(NullObservability),
            )),
        )
        .expect("first daemon");
        let error = match start_runtime(
            DaemonConfig::from_home(home_dir),
            Arc::new(CoreDispatcher::new(
                tempdir.path().to_path_buf(),
                Arc::new(NullObservability),
            )),
        ) {
            Ok(handle) => panic!(
                "second daemon should fail, got endpoint {:?}",
                handle.local_endpoint()
            ),
            Err(error) => error,
        };
        assert_eq!(error.code, AtmErrorCode::DaemonAlreadyRunning);
        first.shutdown().expect("shutdown");
    }

    #[test]
    #[serial]
    fn stale_singleton_cleanup_allows_one_live_start_only() {
        let tempdir = TempDir::new().expect("tempdir");
        let paths = DaemonPaths::from_home(tempdir.path());
        fs::create_dir_all(&paths.state_dir).expect("state dir");
        fs::write(&paths.singleton_path, br#"{"pid":999999}"#).expect("stale singleton");
        let handle = start_runtime(
            DaemonConfig::from_home(tempdir.path().to_path_buf()),
            Arc::new(CoreDispatcher::new(
                tempdir.path().to_path_buf(),
                Arc::new(NullObservability),
            )),
        )
        .expect("daemon with stale singleton");
        let error = match start_runtime(
            DaemonConfig::from_home(tempdir.path().to_path_buf()),
            Arc::new(CoreDispatcher::new(
                tempdir.path().to_path_buf(),
                Arc::new(NullObservability),
            )),
        ) {
            Ok(handle) => panic!(
                "second daemon should still fail, got endpoint {:?}",
                handle.local_endpoint()
            ),
            Err(error) => error,
        };
        assert_eq!(error.code, AtmErrorCode::DaemonAlreadyRunning);
        handle.shutdown().expect("shutdown");
    }

    #[test]
    #[serial]
    fn local_same_host_daemon_api_flow_works() {
        let tempdir = TempDir::new().expect("tempdir");
        let current_dir = tempdir.path().join("workspace");
        fs::create_dir_all(&current_dir).expect("workspace dir");
        fs::write(
            current_dir.join(".atm.toml"),
            "[atm]\ndefault_team = \"atm-dev\"\n",
        )
        .expect("atm toml");
        let handle = start_runtime(
            DaemonConfig::from_home(tempdir.path().to_path_buf()),
            Arc::new(CoreDispatcher::new(
                tempdir.path().to_path_buf(),
                Arc::new(NullObservability),
            )),
        )
        .expect("runtime");
        let response = request_local(
            tempdir.path(),
            &DaemonRequest {
                team_name: "atm-dev".parse().expect("team"),
                agent_name: "arch-ctm".parse().expect("agent"),
                payload: RequestPayload::Heartbeat(serde_json::json!({"pid": 42})),
            },
            SAME_HOST_REQUEST_TIMEOUT,
        )
        .expect("local response");
        assert_eq!(response.kind, RequestKind::Heartbeat);
        let doctor = request_local(
            tempdir.path(),
            &DaemonRequest {
                team_name: "atm-dev".parse().expect("team"),
                agent_name: "arch-ctm".parse().expect("agent"),
                payload: RequestPayload::Doctor(serde_json::json!({
                    "home_dir": tempdir.path(),
                    "current_dir": current_dir,
                    "team_override": "atm-dev"
                })),
            },
            SAME_HOST_REQUEST_TIMEOUT,
        )
        .expect("doctor response");
        assert_eq!(doctor.kind, RequestKind::Doctor);
        handle.shutdown().expect("shutdown");
    }

    #[test]
    fn bounded_remote_host_unreachable_behavior_is_typed() {
        let started = Instant::now();
        let error = request_remote(
            "127.0.0.1:9".parse().expect("discard addr"),
            &DaemonRequest {
                team_name: "atm-dev".parse().expect("team"),
                agent_name: "arch-ctm".parse().expect("agent"),
                payload: RequestPayload::Send(serde_json::json!({"message":"hello"})),
            },
            Duration::from_millis(250),
        )
        .expect_err("unreachable host");
        assert_eq!(error.code, AtmErrorCode::DaemonRemoteUnavailable);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn remote_acceptance_is_required_for_send_success() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("listener");
        listener
            .set_nonblocking(true)
            .expect("listener nonblocking");
        let address = listener.local_addr().expect("local addr");
        let dispatcher = Arc::new(FakeDispatcher::default());
        dispatcher.queue_response(DaemonResponse {
            kind: RequestKind::Send,
            payload_json: "{\"ok\":true}".to_string(),
        });
        let inflight = Arc::new(AtomicUsize::new(0));
        let worker_threads = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let worker = {
            let inflight = Arc::clone(&inflight);
            let worker_threads = Arc::clone(&worker_threads);
            let dispatcher = dispatcher.clone();
            let stop = Arc::clone(&stop);
            thread::spawn(move || {
                accept_tcp_loop(listener, stop, inflight, worker_threads, dispatcher, 8)
            })
        };
        let response = request_remote(
            address,
            &DaemonRequest {
                team_name: "atm-dev".parse().expect("team"),
                agent_name: "arch-ctm".parse().expect("agent"),
                payload: RequestPayload::Send(serde_json::json!({"message":"hello"})),
            },
            Duration::from_secs(1),
        )
        .expect("remote response");
        assert_eq!(response.kind, RequestKind::Send);
        stop.store(true, Ordering::SeqCst);
        let _ = worker.join();
    }
}
