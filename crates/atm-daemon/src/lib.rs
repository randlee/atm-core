mod client;
mod runtime_observability;
mod shutdown;
mod singleton;

use std::fs;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use atm_core::clear::{ClearQuery, clear_mail_via_store};
use atm_core::dispatcher::{
    DaemonRequest, DaemonResponse, DispatchError, RequestDispatcher, RequestKind, RequestPayload,
};
use atm_core::doctor::{DoctorQuery, run_doctor};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::home;
use atm_core::inbox_ingress::default_inbox_ingress;
use atm_core::observability::{CommandEvent, ObservabilityPort};
use atm_core::read::{ReadQuery, read_mail_via_store};
use atm_core::store::StoreError;
use atm_rusqlite::{RusqliteStore, checkpoint_runtime_wal as checkpoint_runtime_wal_via_store};
use serde::{Deserialize, Serialize};

#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};

use crate::client::{
    REMOTE_SERVER_IO_TIMEOUT, SAME_HOST_SERVER_IO_TIMEOUT, dispatch_error_to_atm, read_frame,
    write_control_state, write_frame,
};
pub use crate::client::{
    ensure_daemon_running, request_clear_with_autostart, request_doctor_json_with_autostart,
    request_read_with_autostart, request_remote,
};
pub use crate::runtime_observability::DaemonObservability;
use crate::runtime_observability::normalize_doctor_report_observability;
use crate::shutdown::{
    attach_runtime_health, join_accept_thread, join_worker_threads, wait_for_inflight_zero_until,
};
use crate::singleton::SingletonGuard;
#[cfg(not(unix))]
use crate::singleton::bind_loopback_listener;

pub const SAME_HOST_REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
pub const REMOTE_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
pub const REMOTE_IO_TIMEOUT: Duration = Duration::from_secs(5);
pub const DEFAULT_REMOTE_RETRY_BUDGET: Duration = Duration::from_secs(30);
pub const AUTO_START_PUBLISH_TIMEOUT: Duration = Duration::from_secs(10);
pub const DOCTOR_HANDLER_TIMEOUT: Duration = Duration::from_secs(3);
pub const SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);
pub const SHUTDOWN_FORCE_TIMEOUT: Duration = Duration::from_secs(10);
pub const ACCEPT_THREAD_JOIN_TIMEOUT: Duration = Duration::from_secs(2);
pub const WORKER_THREAD_JOIN_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(windows)]
const WINDOWS_STOP_POLL_INTERVAL: Duration = Duration::from_millis(100);
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
    worker_threads: Option<Arc<Mutex<Vec<JoinHandle<()>>>>>,
}

impl CoreDispatcher {
    pub fn new(home_dir: PathBuf, observability: Arc<dyn ObservabilityPort + Send + Sync>) -> Self {
        Self {
            home_dir,
            observability,
            worker_threads: None,
        }
    }

    pub fn with_worker_threads(mut self, worker_threads: Arc<Mutex<Vec<JoinHandle<()>>>>) -> Self {
        self.worker_threads = Some(worker_threads);
        self
    }

    fn register_worker_thread(&self, handle: JoinHandle<()>) -> Result<(), DispatchError> {
        if let Some(worker_threads) = &self.worker_threads {
            match worker_threads.lock() {
                Ok(mut handles) => handles.push(handle),
                Err(error) => {
                    let _ = handle.join();
                    return Err(DispatchError::Handler(format!(
                        "worker thread registry lock poisoned while tracking doctor worker: {error}"
                    )));
                }
            }
        }
        Ok(())
    }
}

struct InflightGuard(Arc<AtomicUsize>);

impl Drop for InflightGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn join_doctor_worker_until(
    handle: JoinHandle<()>,
    deadline: Instant,
) -> Result<Option<JoinHandle<()>>, DispatchError> {
    while !handle.is_finished() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(25));
    }
    if !handle.is_finished() {
        return Ok(Some(handle));
    }
    handle
        .join()
        .map_err(|payload| {
            DispatchError::Handler(format!(
                "doctor worker panicked: {}",
                shutdown::thread_panic_message(payload)
            ))
        })
        .map(|_| None)
}

impl RequestDispatcher for CoreDispatcher {
    fn dispatch(&self, request: DaemonRequest) -> Result<DaemonResponse, DispatchError> {
        match request.payload {
            RequestPayload::Send(_) => Err(DispatchError::Unsupported(RequestKind::Send)),
            RequestPayload::Ack(_) => Err(DispatchError::Unsupported(RequestKind::Ack)),
            RequestPayload::Read(value) => {
                let query: ReadQuery = serde_json::from_value(value)
                    .map_err(|error| DispatchError::PayloadDecode(error.to_string()))?;
                let store = RusqliteStore::open_for_team_home(&query.home_dir, &request.team_name)
                    .map_err(DispatchError::Store)?;
                let ingress = default_inbox_ingress();
                let outcome =
                    read_mail_via_store(query, &store, &ingress, self.observability.as_ref())
                        .map_err(dispatch_atm_error)?;
                let payload_json = serde_json::to_string(&outcome)
                    .map_err(|error| DispatchError::ResponseEncode(error.to_string()))?;
                Ok(DaemonResponse {
                    kind: RequestKind::Read,
                    payload_json,
                })
            }
            RequestPayload::Clear(value) => {
                let query: ClearQuery = serde_json::from_value(value)
                    .map_err(|error| DispatchError::PayloadDecode(error.to_string()))?;
                let store = RusqliteStore::open_for_team_home(&query.home_dir, &request.team_name)
                    .map_err(DispatchError::Store)?;
                let ingress = default_inbox_ingress();
                let outcome =
                    clear_mail_via_store(query, &store, &ingress, self.observability.as_ref())
                        .map_err(dispatch_atm_error)?;
                let payload_json = serde_json::to_string(&outcome)
                    .map_err(|error| DispatchError::ResponseEncode(error.to_string()))?;
                Ok(DaemonResponse {
                    kind: RequestKind::Clear,
                    payload_json,
                })
            }
            RequestPayload::Doctor(value) => {
                let query: DoctorQuery = serde_json::from_value(value)
                    .map_err(|error| DispatchError::PayloadDecode(error.to_string()))?;
                let (tx, rx) = mpsc::sync_channel(1);
                let cancelled = Arc::new(AtomicBool::new(false));
                let handle = thread::spawn({
                    let cancelled = Arc::clone(&cancelled);
                    let home_dir = self.home_dir.clone();
                    let observability = Arc::clone(&self.observability);
                    let team_name = request.team_name.clone();
                    move || {
                        let result = run_doctor(query, observability.as_ref()).map(|report| {
                            let report = normalize_doctor_report_observability(
                                report,
                                observability.as_ref(),
                            );
                            attach_runtime_health(report, &home_dir, &team_name)
                        });
                        if !cancelled.load(Ordering::SeqCst) {
                            let _ = tx.send(result);
                        }
                    }
                });
                let report = match rx.recv_timeout(DOCTOR_HANDLER_TIMEOUT) {
                    Ok(result) => {
                        if let Some(handle) = join_doctor_worker_until(
                            handle,
                            Instant::now() + WORKER_THREAD_JOIN_TIMEOUT,
                        )? {
                            self.register_worker_thread(handle)?;
                        }
                        result.map_err(|error| DispatchError::Handler(error.to_string()))?
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        cancelled.store(true, Ordering::SeqCst);
                        if let Some(handle) = join_doctor_worker_until(
                            handle,
                            Instant::now() + WORKER_THREAD_JOIN_TIMEOUT,
                        )? {
                            self.register_worker_thread(handle)?;
                        }
                        return Err(DispatchError::Handler(format!(
                            "doctor worker exceeded the {:?} handler budget",
                            DOCTOR_HANDLER_TIMEOUT
                        )));
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        cancelled.store(true, Ordering::SeqCst);
                        if let Some(handle) = join_doctor_worker_until(
                            handle,
                            Instant::now() + WORKER_THREAD_JOIN_TIMEOUT,
                        )? {
                            self.register_worker_thread(handle)?;
                        }
                        return Err(DispatchError::Handler(
                            "doctor worker exited before sending a result".to_string(),
                        ));
                    }
                };
                let payload_json = serde_json::to_string(&report)
                    .map_err(|error| DispatchError::ResponseEncode(error.to_string()))?;
                let _ = self.observability.emit(CommandEvent {
                    command: "atm-daemon",
                    action: "doctor_request",
                    outcome: "ok",
                    team: request.team_name.clone(),
                    agent: request.agent_name.clone(),
                    // INVARIANT: the daemon emits runtime-owned observability
                    // records with a fixed sender identity so queries can
                    // distinguish daemon events from user commands.
                    sender: "atm-daemon".parse().expect("daemon sender is valid"),
                    message_id: None,
                    requires_ack: false,
                    dry_run: false,
                    task_id: None,
                    error_code: None,
                    error_message: None,
                });
                Ok(DaemonResponse {
                    kind: RequestKind::Doctor,
                    payload_json,
                })
            }
            RequestPayload::Heartbeat(_) => {
                let _ = self.observability.emit(CommandEvent {
                    command: "atm-daemon",
                    action: "heartbeat_request",
                    outcome: "ok",
                    team: request.team_name.clone(),
                    agent: request.agent_name.clone(),
                    // INVARIANT: the daemon emits runtime-owned observability
                    // records with a fixed sender identity so queries can
                    // distinguish daemon events from user commands.
                    sender: "atm-daemon".parse().expect("daemon sender is valid"),
                    message_id: None,
                    requires_ack: false,
                    dry_run: false,
                    task_id: None,
                    error_code: None,
                    error_message: None,
                });
                Ok(DaemonResponse {
                    kind: RequestKind::Heartbeat,
                    payload_json: "{\"ok\":true}".to_string(),
                })
            } // TODO(phase-q §21.6.1): replace this hardcoded request-kind dispatch with an
              // injectable handler registry. That registry remains deferred scope while Q.4 only
              // exposes a small fixed set of request handlers through the thin daemon runtime.
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
    // Runtime worker threads are registered here so shutdown can join both
    // accepted connection workers and any detached request helpers exactly once.
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
        let shutdown_start = Instant::now();
        let shutdown_deadline = shutdown_start + SHUTDOWN_FORCE_TIMEOUT;
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.local_thread.take() {
            join_accept_thread(handle, "local accept loop")?;
        }
        if let Some(handle) = self.remote_thread.take() {
            join_accept_thread(handle, "remote accept loop")?;
        }

        let drain_deadline = (shutdown_start + SHUTDOWN_DRAIN_TIMEOUT).min(shutdown_deadline);
        // INVARIANT: SQLite dispatch workers share one serialized store
        // connection and may spend up to the configured busy_timeout inside a
        // single request. The shutdown path therefore gives them one bounded
        // drain window first, then a second bounded total-deadline window
        // before checkpointing WAL, so busy retries cannot race the final
        // checkpoint with a still-live writer thread.
        wait_for_inflight_zero_until(&self.inflight, drain_deadline);
        wait_for_inflight_zero_until(&self.inflight, shutdown_deadline);
        join_worker_threads(&self.worker_threads, shutdown_deadline)?;
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
    worker_threads: Arc<Mutex<Vec<JoinHandle<()>>>>,
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

    // TODO(Q.5): enable Windows TLS support before cross-host daemon traffic
    // is used beyond trusted local/QA scenarios.
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
            Err(error) => {
                eprintln!("daemon local accept loop error: {error}");
                thread::sleep(Duration::from_millis(25));
            }
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
    // TODO(phase-q §21.4): authenticate remote TCP daemon requests before this
    // transport is used beyond trusted local/QA scenarios.
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
            Err(error) => {
                eprintln!("daemon TCP accept loop error: {error}");
                thread::sleep(Duration::from_millis(25));
            }
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
        let _inflight_guard = InflightGuard(Arc::clone(&inflight));
        let envelope = match read_frame::<DaemonRequest, _>(&mut stream) {
            Ok(request) => match dispatcher.dispatch(request) {
                Ok(response) => WireResponseEnvelope::success(response),
                Err(error) => WireResponseEnvelope::failure(dispatch_error_to_atm(error)),
            },
            Err(error) => WireResponseEnvelope::failure(error),
        };
        let _ = write_frame(&mut stream, &envelope);
    });
    match worker_threads.lock() {
        Ok(mut handles) => handles.push(handle),
        Err(error) => {
            eprintln!(
                "daemon worker-thread registry lock is poisoned; detaching local connection worker: {error}"
            );
        }
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
        let _inflight_guard = InflightGuard(Arc::clone(&inflight));
        let envelope = match read_frame::<DaemonRequest, _>(&mut stream) {
            Ok(request) => match dispatcher.dispatch(request) {
                Ok(response) => WireResponseEnvelope::success(response),
                Err(error) => WireResponseEnvelope::failure(dispatch_error_to_atm(error)),
            },
            Err(error) => WireResponseEnvelope::failure(error),
        };
        let _ = write_frame(&mut stream, &envelope);
    });
    match worker_threads.lock() {
        Ok(mut handles) => handles.push(handle),
        Err(error) => {
            eprintln!(
                "daemon worker-thread registry lock is poisoned; detaching tcp connection worker: {error}"
            );
        }
    }
}

fn dispatch_atm_error(mut error: AtmError) -> DispatchError {
    if let Some(source) = error.source.take()
        && let Ok(store_error) = source.downcast::<StoreError>()
    {
        return DispatchError::Store(*store_error);
    }
    DispatchError::Handler(error.to_string())
}

pub fn run_foreground() -> Result<(), AtmError> {
    let home_dir = home::atm_home()?;
    let worker_threads = Arc::new(Mutex::new(Vec::new()));
    let dispatcher = Arc::new(
        CoreDispatcher::new(
            home_dir.clone(),
            Arc::new(DaemonObservability::new(&home_dir)),
        )
        .with_worker_threads(Arc::clone(&worker_threads)),
    );
    let stop = Arc::new(AtomicBool::new(false));
    let reload = Arc::new(AtomicBool::new(false));
    register_signal_handlers(Arc::clone(&stop), Arc::clone(&reload))?;
    let handle = start_runtime(
        DaemonConfig::from_home(home_dir),
        Arc::clone(&worker_threads),
        dispatcher,
    )?;
    #[cfg(windows)]
    let stop_marker = DaemonPaths::from_home(&handle.home_dir)
        .state_dir
        .join("stop.request");
    while !stop.load(Ordering::SeqCst) {
        #[cfg(windows)]
        if stop_marker.exists() {
            let _ = fs::remove_file(&stop_marker);
            stop.store(true, Ordering::SeqCst);
            continue;
        }
        let _ = reload.swap(false, Ordering::SeqCst);
        #[cfg(windows)]
        thread::sleep(WINDOWS_STOP_POLL_INTERVAL);
        #[cfg(not(windows))]
        thread::sleep(Duration::from_millis(100));
    }
    handle.shutdown()
}

/// Register the daemon's three-signal contract before listeners accept
/// traffic: `SIGINT` and `SIGTERM` trigger shutdown, and `SIGHUP` triggers a
/// bounded reload without dropping singleton ownership.
fn register_signal_handlers(
    stop: Arc<AtomicBool>,
    reload: Arc<AtomicBool>,
) -> Result<(), AtmError> {
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&stop)).map_err(
        |error| {
            AtmError::daemon_start_failed("failed to install SIGINT handler").with_source(error)
        },
    )?;
    #[cfg(not(windows))]
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&stop)).map_err(
        |error| {
            AtmError::daemon_start_failed("failed to install SIGTERM handler").with_source(error)
        },
    )?;
    #[cfg(unix)]
    signal_hook::flag::register(signal_hook::consts::SIGHUP, reload).map_err(|error| {
        AtmError::daemon_start_failed("failed to install SIGHUP handler").with_source(error)
    })?;
    #[cfg(windows)]
    // `signal-hook` does not wire Windows service or detached-process control
    // events through `SetConsoleCtrlHandler`, so the daemon cannot promise
    // SIGINT/SIGHUP-style shutdown semantics on headless Windows runtimes.
    // Tests and callers therefore use explicit shutdown paths instead.
    let _ = (stop, reload);
    Ok(())
}

fn checkpoint_runtime_wal(home_dir: &Path) -> Result<(), AtmError> {
    checkpoint_runtime_wal_via_store(home_dir).map_err(|error| {
        AtmError::daemon_start_failed("failed to checkpoint SQLite WAL during shutdown")
            .with_source(error)
    })
}

#[cfg(test)]
mod tests;
