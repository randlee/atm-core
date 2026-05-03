use std::fs;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use atm_core::dispatcher::{
    DaemonRequest, DaemonResponse, DispatchError, RequestDispatcher, RequestKind, RequestPayload,
};
use atm_core::doctor::{DoctorQuery, DoctorReport, DoctorSeverity, DoctorStatus, run_doctor};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::home;
use atm_core::observability::{
    AtmLogQuery, AtmLogSnapshot, AtmObservabilityHealth, AtmObservabilityHealthState, CommandEvent,
    LogTailSession, ObservabilityPort,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};

pub const SAME_HOST_REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
pub const REMOTE_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
pub const REMOTE_IO_TIMEOUT: Duration = Duration::from_secs(5);
pub const DEFAULT_REMOTE_RETRY_BUDGET: Duration = Duration::from_secs(30);
pub const AUTO_START_PUBLISH_TIMEOUT: Duration = Duration::from_secs(10);
pub const SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);
pub const SHUTDOWN_FORCE_TIMEOUT: Duration = Duration::from_secs(10);
pub const MAX_ACCEPTS: usize = 64;
pub const MAX_INFLIGHT_PER_CONNECTION: usize = 32;

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
    observability: Arc<dyn ObservabilityPort + Send + Sync>,
}

impl CoreDispatcher {
    pub fn new(observability: Arc<dyn ObservabilityPort + Send + Sync>) -> Self {
        Self { observability }
    }
}

impl RequestDispatcher for CoreDispatcher {
    fn dispatch(&self, request: DaemonRequest) -> Result<DaemonResponse, DispatchError> {
        match request.payload {
            RequestPayload::Doctor(value) => {
                let query: DoctorQuery = serde_json::from_value(value)
                    .map_err(|error| DispatchError::InvalidPayload(error.to_string()))?;
                let observability = DaemonObservability::new(&query.home_dir);
                let report = run_doctor(query, &observability)
                    .map_err(|error| DispatchError::InvalidPayload(error.to_string()))?;
                let report = normalize_doctor_report_observability(report, &observability);
                let payload_json = serde_json::to_string(&report)
                    .map_err(|error| DispatchError::InvalidPayload(error.to_string()))?;
                emit_runtime_event(self.observability.as_ref(), "doctor_request", "ok", None);
                Ok(DaemonResponse {
                    kind: RequestKind::Doctor,
                    payload_json,
                })
            }
            RequestPayload::Heartbeat(_) => {
                emit_runtime_event(self.observability.as_ref(), "heartbeat_request", "ok", None);
                Ok(DaemonResponse {
                    kind: RequestKind::Heartbeat,
                    payload_json: "{\"ok\":true}".to_string(),
                })
            }
            other => Err(DispatchError::InvalidPayload(format!(
                "request kind {:?} is not implemented in the thin Q.4 daemon runtime",
                other.kind()
            ))),
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
    singleton: SingletonGuard,
    control_path: PathBuf,
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

        let drain_deadline = Instant::now() + SHUTDOWN_DRAIN_TIMEOUT;
        while self.inflight.load(Ordering::SeqCst) > 0 && Instant::now() < drain_deadline {
            thread::sleep(Duration::from_millis(25));
        }

        let force_deadline = Instant::now() + SHUTDOWN_FORCE_TIMEOUT;
        while self.inflight.load(Ordering::SeqCst) > 0 && Instant::now() < force_deadline {
            thread::sleep(Duration::from_millis(25));
        }

        let _ = fs::remove_file(&self.control_path);
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
        let dispatcher = Arc::clone(&dispatcher);
        let max_inflight = config.max_inflight_per_connection;
        #[cfg(unix)]
        let listener = local_listener;
        #[cfg(not(unix))]
        let listener = local_listener;
        Some(thread::spawn(move || {
            accept_local_loop(listener, stop, inflight, dispatcher, max_inflight)
        }))
    };

    let remote_thread = {
        let stop = Arc::clone(&stop);
        let inflight = Arc::clone(&inflight);
        let dispatcher = Arc::clone(&dispatcher);
        let max_inflight = config.max_inflight_per_connection;
        Some(thread::spawn(move || {
            accept_tcp_loop(remote_listener, stop, inflight, dispatcher, max_inflight)
        }))
    };

    Ok(DaemonHandle {
        stop,
        inflight,
        local_thread,
        remote_thread,
        singleton,
        control_path: paths.control_path,
        local_endpoint,
        remote_endpoint,
    })
}

pub fn request_doctor_json_with_autostart(query: DoctorQuery) -> Result<String, AtmError> {
    let request = DaemonRequest {
        team_name: query
            .team_override
            .clone()
            .or_else(|| {
                std::env::var("ATM_TEAM")
                    .ok()
                    .and_then(|value| value.parse().ok())
            })
            .unwrap_or_else(|| "atm-dev".parse().expect("static team name")),
        agent_name: std::env::var("ATM_IDENTITY")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(|| "unknown".parse().expect("static agent name")),
        payload: RequestPayload::Doctor(serde_json::to_value(&query).map_err(|error| {
            AtmError::daemon_protocol("failed to serialize doctor query").with_source(error)
        })?),
    };

    let home_dir = query.home_dir.clone();
    match request_local(&home_dir, &request, SAME_HOST_REQUEST_TIMEOUT) {
        Ok(response) => Ok(response.payload_json),
        Err(_) => {
            auto_start_daemon(&home_dir)?;
            let response = request_local(&home_dir, &request, SAME_HOST_REQUEST_TIMEOUT)?;
            Ok(response.payload_json)
        }
    }
}

pub fn ensure_daemon_running(home_dir: &Path) -> Result<(), AtmError> {
    let heartbeat_request = DaemonRequest {
        team_name: std::env::var("ATM_TEAM")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(|| "atm-dev".parse().expect("static team name")),
        agent_name: std::env::var("ATM_IDENTITY")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(|| "unknown".parse().expect("static agent name")),
        payload: RequestPayload::Heartbeat(serde_json::json!({
            "pid": std::process::id(),
        })),
    };
    match request_local(home_dir, &heartbeat_request, SAME_HOST_REQUEST_TIMEOUT) {
        Ok(_) => Ok(()),
        Err(_) => {
            auto_start_daemon(home_dir)?;
            request_local(home_dir, &heartbeat_request, SAME_HOST_REQUEST_TIMEOUT).map(|_| ())
        }
    }
}

pub fn request_local(
    home_dir: &Path,
    request: &DaemonRequest,
    timeout: Duration,
) -> Result<DaemonResponse, AtmError> {
    let state = read_control_state(home_dir)?;
    match state.local_endpoint {
        #[cfg(unix)]
        LocalEndpoint::UnixSocket(path) => {
            let mut stream = UnixStream::connect(&path).map_err(|error| {
                AtmError::daemon_unavailable(format!(
                    "failed to connect to local ATM daemon socket {}: {error}",
                    path.display()
                ))
                .with_source(error)
            })?;
            stream.set_read_timeout(Some(timeout)).map_err(|error| {
                AtmError::daemon_request_timeout("failed to set local daemon read timeout")
                    .with_source(error)
            })?;
            stream.set_write_timeout(Some(timeout)).map_err(|error| {
                AtmError::daemon_request_timeout("failed to set local daemon write timeout")
                    .with_source(error)
            })?;
            exchange_unix(&mut stream, request)
        }
        LocalEndpoint::TcpLoopback(addr) => {
            let mut stream = TcpStream::connect_timeout(&addr, timeout).map_err(|error| {
                AtmError::daemon_unavailable(format!(
                    "failed to connect to local ATM daemon loopback endpoint {addr}: {error}"
                ))
                .with_source(error)
            })?;
            stream.set_read_timeout(Some(timeout)).map_err(|error| {
                AtmError::daemon_request_timeout("failed to set local daemon read timeout")
                    .with_source(error)
            })?;
            stream.set_write_timeout(Some(timeout)).map_err(|error| {
                AtmError::daemon_request_timeout("failed to set local daemon write timeout")
                    .with_source(error)
            })?;
            exchange_tcp(&mut stream, request)
        }
    }
}

pub fn request_remote(
    address: SocketAddr,
    request: &DaemonRequest,
    retry_budget: Duration,
) -> Result<DaemonResponse, AtmError> {
    let deadline = Instant::now() + retry_budget;
    let mut last_error = None;
    while Instant::now() < deadline {
        match TcpStream::connect_timeout(&address, REMOTE_CONNECT_TIMEOUT) {
            Ok(mut stream) => {
                stream
                    .set_read_timeout(Some(REMOTE_IO_TIMEOUT))
                    .map_err(|error| {
                        AtmError::daemon_request_timeout("failed to set remote read timeout")
                            .with_source(error)
                    })?;
                stream
                    .set_write_timeout(Some(REMOTE_IO_TIMEOUT))
                    .map_err(|error| {
                        AtmError::daemon_request_timeout("failed to set remote write timeout")
                            .with_source(error)
                    })?;
                return exchange_tcp(&mut stream, request);
            }
            Err(error) => {
                last_error = Some(error);
                thread::sleep(Duration::from_millis(50));
            }
        }
    }

    Err(AtmError::daemon_remote_unavailable(format!(
        "remote daemon at {address} did not accept the request within {:?}",
        retry_budget
    ))
    .with_source(last_error.expect("remote retry captured at least one error")))
}

pub fn auto_start_daemon(home_dir: &Path) -> Result<(), AtmError> {
    let start_command = daemon_start_command();
    let mut command = Command::new(&start_command.program);
    command
        .args(&start_command.args)
        .env("ATM_HOME", home_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(current_dir) = &start_command.current_dir {
        command.current_dir(current_dir);
    }
    command.spawn().map_err(|error| {
        AtmError::daemon_start_failed(format!(
            "failed to start atm-daemon with {}: {error}",
            start_command.display()
        ))
        .with_source(error)
    })?;

    let wait_deadline = Instant::now() + AUTO_START_PUBLISH_TIMEOUT;
    while Instant::now() < wait_deadline {
        if read_control_state(home_dir).is_ok() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }

    Err(AtmError::daemon_start_failed(format!(
        "atm-daemon did not publish its control state within {:?}",
        AUTO_START_PUBLISH_TIMEOUT
    )))
}

fn default_daemon_binary() -> Option<PathBuf> {
    if let Ok(current) = std::env::current_exe()
        && let Some(dir) = current.parent()
    {
        let sibling = dir.join("atm-daemon");
        if sibling.exists() {
            return Some(sibling);
        }
        if let Some(parent) = dir.parent() {
            let sibling = parent.join("atm-daemon");
            if sibling.exists() {
                return Some(sibling);
            }
        }
    }
    None
}

struct StartCommand {
    program: PathBuf,
    args: Vec<String>,
    current_dir: Option<PathBuf>,
}

impl StartCommand {
    fn display(&self) -> String {
        let mut parts = vec![self.program.display().to_string()];
        parts.extend(self.args.iter().cloned());
        parts.join(" ")
    }
}

fn daemon_start_command() -> StartCommand {
    if let Some(program) = std::env::var_os("ATM_DAEMON_BIN").map(PathBuf::from) {
        return StartCommand {
            program,
            args: Vec::new(),
            current_dir: None,
        };
    }
    if let Some(program) = default_daemon_binary() {
        return StartCommand {
            program,
            args: Vec::new(),
            current_dir: None,
        };
    }
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf();
    StartCommand {
        program: PathBuf::from("cargo"),
        args: vec![
            "run".to_string(),
            "--quiet".to_string(),
            "-p".to_string(),
            "atm-daemon".to_string(),
            "--bin".to_string(),
            "atm-daemon".to_string(),
        ],
        current_dir: Some(workspace_root),
    }
}

fn dispatch_error_to_atm(error: DispatchError) -> AtmError {
    match error {
        DispatchError::Store(error) => AtmError::daemon_protocol(error.to_string()),
        DispatchError::InvalidPayload(message) => AtmError::daemon_protocol(message),
    }
}

fn read_control_state(home_dir: &Path) -> Result<ControlState, AtmError> {
    let paths = DaemonPaths::from_home(home_dir);
    let raw = fs::read(&paths.control_path).map_err(|error| {
        AtmError::daemon_unavailable(format!(
            "daemon control state is unavailable at {}: {error}",
            paths.control_path.display()
        ))
        .with_source(error)
    })?;
    serde_json::from_slice(&raw).map_err(|error| {
        AtmError::daemon_protocol("failed to parse daemon control state").with_source(error)
    })
}

fn write_control_state(path: &Path, state: &ControlState) -> Result<(), AtmError> {
    let bytes = serde_json::to_vec(state).map_err(|error| {
        AtmError::daemon_protocol("failed to serialize daemon control state").with_source(error)
    })?;
    fs::write(path, bytes).map_err(|error| {
        AtmError::daemon_start_failed(format!(
            "failed to write daemon control state {}: {error}",
            path.display()
        ))
        .with_source(error)
    })
}

#[cfg(unix)]
fn exchange_unix(
    stream: &mut UnixStream,
    request: &DaemonRequest,
) -> Result<DaemonResponse, AtmError> {
    write_frame(stream, request)?;
    let envelope: WireResponseEnvelope = read_frame(stream)?;
    response_from_envelope(envelope)
}

fn exchange_tcp(
    stream: &mut TcpStream,
    request: &DaemonRequest,
) -> Result<DaemonResponse, AtmError> {
    write_frame(stream, request)?;
    let envelope: WireResponseEnvelope = read_frame(stream)?;
    response_from_envelope(envelope)
}

fn response_from_envelope(envelope: WireResponseEnvelope) -> Result<DaemonResponse, AtmError> {
    if let Some(response) = envelope.response {
        return Ok(response);
    }
    let error = envelope
        .error
        .ok_or_else(|| AtmError::daemon_protocol("daemon response envelope was empty"))?;
    Err(wire_error_to_atm(error))
}

fn write_frame<T: Serialize, W: Write>(writer: &mut W, value: &T) -> Result<(), AtmError> {
    let mut writer = BufWriter::new(writer);
    serde_json::to_writer(&mut writer, value).map_err(|error| {
        AtmError::daemon_protocol("failed to serialize daemon frame").with_source(error)
    })?;
    writer.write_all(b"\n").map_err(|error| {
        AtmError::daemon_protocol("failed to write daemon frame delimiter").with_source(error)
    })?;
    writer.flush().map_err(|error| {
        AtmError::daemon_protocol("failed to flush daemon frame").with_source(error)
    })
}

fn read_frame<T: for<'de> Deserialize<'de>, R: std::io::Read>(
    reader: &mut R,
) -> Result<T, AtmError> {
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    let bytes = reader.read_line(&mut line).map_err(|error| {
        AtmError::daemon_protocol("failed to read daemon frame").with_source(error)
    })?;
    if bytes == 0 {
        return Err(AtmError::daemon_protocol(
            "daemon connection closed before a response frame was received",
        ));
    }
    serde_json::from_str(&line).map_err(|error| {
        AtmError::daemon_protocol("failed to decode daemon frame").with_source(error)
    })
}

#[cfg(unix)]
fn accept_local_loop(
    listener: UnixListener,
    stop: Arc<AtomicBool>,
    inflight: Arc<AtomicUsize>,
    dispatcher: Arc<dyn RequestDispatcher>,
    max_inflight: usize,
) {
    accept_unix_loop(listener, stop, inflight, dispatcher, max_inflight);
}

#[cfg(unix)]
fn accept_unix_loop(
    listener: UnixListener,
    stop: Arc<AtomicBool>,
    inflight: Arc<AtomicUsize>,
    dispatcher: Arc<dyn RequestDispatcher>,
    max_inflight: usize,
) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                spawn_local_connection(
                    stream,
                    Arc::clone(&inflight),
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
    dispatcher: Arc<dyn RequestDispatcher>,
    max_inflight: usize,
) {
    accept_tcp_loop(listener, stop, inflight, dispatcher, max_inflight);
}

fn accept_tcp_loop(
    listener: TcpListener,
    stop: Arc<AtomicBool>,
    inflight: Arc<AtomicUsize>,
    dispatcher: Arc<dyn RequestDispatcher>,
    max_inflight: usize,
) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                spawn_tcp_connection(
                    stream,
                    Arc::clone(&inflight),
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
    thread::spawn(move || {
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
}

fn spawn_tcp_connection(
    mut stream: TcpStream,
    inflight: Arc<AtomicUsize>,
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
    thread::spawn(move || {
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
    let dispatcher = Arc::new(CoreDispatcher::new(Arc::new(DaemonObservability::new(
        &home_dir,
    ))));
    let stop = Arc::new(AtomicBool::new(false));
    register_signal_handlers(Arc::clone(&stop))?;
    let handle = start_runtime(DaemonConfig::from_home(home_dir), dispatcher)?;
    while !stop.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(100));
    }
    handle.shutdown()
}

#[derive(Debug)]
struct DaemonObservability {
    active_log_path: PathBuf,
    fault_mode: Option<String>,
}

impl DaemonObservability {
    fn new(home_dir: &Path) -> Self {
        Self {
            active_log_path: home_dir
                .join(".local")
                .join("share")
                .join("logs")
                .join("atm.log.jsonl"),
            fault_mode: std::env::var("ATM_OBSERVABILITY_RETAINED_SINK_FAULT").ok(),
        }
    }
}

impl atm_core::observability::sealed::Sealed for DaemonObservability {}

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
        let (logging_state, detail) = match self.fault_mode.as_deref() {
            Some("degraded") => (AtmObservabilityHealthState::Degraded, None),
            Some("unavailable") => (AtmObservabilityHealthState::Unavailable, None),
            _ => (AtmObservabilityHealthState::Healthy, None),
        };
        Ok(AtmObservabilityHealth {
            active_log_path: Some(self.active_log_path.clone()),
            logging_state,
            query_state: Some(AtmObservabilityHealthState::Healthy),
            detail,
        })
    }
}

fn register_signal_handlers(stop: Arc<AtomicBool>) -> Result<(), AtmError> {
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
    signal_hook::flag::register(signal_hook::consts::SIGHUP, stop).map_err(|error| {
        AtmError::daemon_start_failed("failed to install SIGHUP handler").with_source(error)
    })?;
    Ok(())
}

pub fn emit_runtime_event(
    observability: &dyn ObservabilityPort,
    action: &'static str,
    outcome: &'static str,
    error: Option<&AtmError>,
) {
    let _ = observability.emit(CommandEvent {
        command: "atm-daemon",
        action,
        outcome,
        team: "atm-dev".parse().expect("static team name"),
        agent: "atm-daemon".parse().expect("static agent name"),
        sender: "atm-daemon".to_string(),
        message_id: None,
        requires_ack: false,
        dry_run: false,
        task_id: None,
        error_code: error.map(|error| error.code),
        error_message: error.map(ToString::to_string),
    });
}

fn wire_error_to_atm(error: WireError) -> AtmError {
    let recovery = error
        .recovery
        .unwrap_or_else(|| "Restart the daemon and retry the command.".to_string());
    match error.code {
        AtmErrorCode::DaemonUnavailable => {
            AtmError::daemon_unavailable(error.message).with_recovery(recovery)
        }
        AtmErrorCode::DaemonStartFailed => {
            AtmError::daemon_start_failed(error.message).with_recovery(recovery)
        }
        AtmErrorCode::DaemonAlreadyRunning => {
            AtmError::daemon_already_running(error.message).with_recovery(recovery)
        }
        AtmErrorCode::DaemonRequestTimeout => {
            AtmError::daemon_request_timeout(error.message).with_recovery(recovery)
        }
        AtmErrorCode::DaemonRemoteUnavailable => {
            AtmError::daemon_remote_unavailable(error.message).with_recovery(recovery)
        }
        _ => AtmError::daemon_protocol(error.message).with_recovery(recovery),
    }
}

fn normalize_doctor_report_observability(
    mut report: DoctorReport,
    observability: &dyn ObservabilityPort,
) -> DoctorReport {
    let (health, finding) = match observability.health() {
        Ok(health) => {
            let finding = atm_core::doctor::health::observability_finding(&health);
            (health, finding)
        }
        Err(error) => {
            let health = atm_core::doctor::health::unavailable_snapshot(error.to_string());
            let finding = atm_core::doctor::health::observability_finding_from_error(&error);
            (health, finding)
        }
    };

    report.findings.retain(|finding| {
        !matches!(
            finding.code,
            AtmErrorCode::ObservabilityHealthOk
                | AtmErrorCode::WarningObservabilityHealthDegraded
                | AtmErrorCode::ObservabilityHealthFailed
        )
    });
    report.findings.push(finding);
    report.recommendations = report
        .findings
        .iter()
        .filter_map(|finding| finding.remediation.clone())
        .collect();
    report.observability = health;
    refresh_doctor_summary(&mut report);
    report
}

fn refresh_doctor_summary(report: &mut DoctorReport) {
    let (info_count, warning_count, error_count) = report.findings.iter().fold(
        (0usize, 0usize, 0usize),
        |(info, warning, error), finding| match finding.severity {
            DoctorSeverity::Info => (info + 1, warning, error),
            DoctorSeverity::Warning => (info, warning + 1, error),
            DoctorSeverity::Error => (info, warning, error + 1),
        },
    );
    let status = if error_count > 0 {
        DoctorStatus::Error
    } else if warning_count > 0 {
        DoctorStatus::Warning
    } else {
        DoctorStatus::Healthy
    };
    let message = match status {
        DoctorStatus::Healthy => "ATM doctor completed with healthy findings only",
        DoctorStatus::Warning => "ATM doctor completed with warnings",
        DoctorStatus::Error => "ATM doctor found critical issues",
    };
    report.summary.status = status;
    report.summary.message = message.to_string();
    report.summary.info_count = info_count;
    report.summary.warning_count = warning_count;
    report.summary.error_count = error_count;
}

#[cfg(test)]
mod tests {
    use super::*;
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
                .ok_or_else(|| DispatchError::InvalidPayload("no queued response".to_string()))
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
            Arc::new(CoreDispatcher::new(Arc::new(NullObservability))),
        )
        .expect("first daemon");
        let error = match start_runtime(
            DaemonConfig::from_home(home_dir),
            Arc::new(CoreDispatcher::new(Arc::new(NullObservability))),
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
            Arc::new(CoreDispatcher::new(Arc::new(NullObservability))),
        )
        .expect("daemon with stale singleton");
        let error = match start_runtime(
            DaemonConfig::from_home(tempdir.path().to_path_buf()),
            Arc::new(CoreDispatcher::new(Arc::new(NullObservability))),
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
        let handle = start_runtime(
            DaemonConfig::from_home(tempdir.path().to_path_buf()),
            Arc::new(CoreDispatcher::new(Arc::new(NullObservability))),
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
        assert!(started.elapsed() < Duration::from_secs(2));
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
        let stop = Arc::new(AtomicBool::new(false));
        let worker = {
            let inflight = Arc::clone(&inflight);
            let dispatcher = dispatcher.clone();
            let stop = Arc::clone(&stop);
            thread::spawn(move || accept_tcp_loop(listener, stop, inflight, dispatcher, 8))
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
