use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use std::{fmt, thread};

use atm_core::caller_context::{CallerContext, CallerContextOverrides, resolve_cli_caller_context};
#[cfg(not(windows))]
use atm_core::protocol;
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};
use atm_storage::{AgentName, AtmError, AtmErrorCode, TeamName};
use fs2::FileExt;
#[cfg(not(windows))]
use interprocess::local_socket::Stream as LocalSocketStream;
#[cfg(not(windows))]
use interprocess::local_socket::traits::Stream as _;
#[cfg(windows)]
use serde_json;
#[cfg(windows)]
use std::net::TcpStream;
#[cfg(windows)]
use std::net::TcpStream as LocalSocketStream;
use std::sync::Mutex;

pub use atm_core::protocol::{CompatibilityPreflight, CompatibilityVerdict, ReleaseVersion};

mod compatibility;

pub use compatibility::{Connection, Unverified, VersionVerified, verify_connection_compatibility};

pub const AUTO_START_PUBLISH_TIMEOUT: Duration = Duration::from_secs(10);
pub const HOST_RUNTIME_LAUNCH_LOCK_FILE: &str = "launch.lock";
const LOCAL_IPC_CONNECT_DEADLINE: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapConnectOutcome {
    Connected,
    NotFound,
    Timeout,
    Failed,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapLaunchGateOutcome {
    Launched,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapAutoStartOutcome {
    AutoStarted,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct BootstrapTraceReport {
    pub daemon_connect: BootstrapConnectOutcome,
    pub daemon_launch_gate: BootstrapLaunchGateOutcome,
    pub daemon_auto_start: BootstrapAutoStartOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connect_detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_gate_detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_start_detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapCommandEvent {
    pub command: &'static str,
    pub action: &'static str,
    pub outcome: &'static str,
    pub team: TeamName,
    pub agent: AgentName,
    pub error_code: Option<AtmErrorCode>,
    pub error_message: Option<String>,
}

#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalIpcDeadlineSupport {
    Applied,
    Unsupported,
}

#[derive(Debug, Clone)]
pub struct DaemonLocalIpcEndpoint(PathBuf);

impl DaemonLocalIpcEndpoint {
    pub fn new(path: PathBuf) -> Result<Self, AtmError> {
        validate_daemon_path("daemon local IPC endpoint", &path)?;
        Ok(Self(path))
    }

    pub fn display(&self) -> std::path::Display<'_> {
        self.0.display()
    }
}

impl AsRef<Path> for DaemonLocalIpcEndpoint {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct DaemonBinaryPath(PathBuf);

impl DaemonBinaryPath {
    pub fn new(path: PathBuf) -> Result<Self, AtmError> {
        validate_daemon_path("daemon binary path", &path)?;
        Ok(Self(path))
    }

    pub fn display(&self) -> std::path::Display<'_> {
        self.0.display()
    }
}

impl AsRef<Path> for DaemonBinaryPath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

fn validate_daemon_path(label: &str, path: &Path) -> Result<(), AtmError> {
    if path.as_os_str().is_empty() {
        return Err(AtmError::validation(format!("{label} must not be empty")));
    }
    if path.to_str().is_none() {
        return Err(AtmError::validation(format!(
            "{label} must be valid UTF-8 at the ATM boundary"
        )));
    }
    Ok(())
}

/// # Errors
///
/// Returns [`AtmError`] when the canonical same-host daemon socket path cannot
/// be resolved into a local IPC endpoint.
pub fn resolve_daemon_local_ipc_endpoint() -> Result<DaemonLocalIpcEndpoint, AtmError> {
    #[cfg(windows)]
    {
        return DaemonLocalIpcEndpoint::new(atm_core::local_http::local_http_record_path(
            &atm_core::home::atm_home()?,
        ));
    }
    #[cfg(not(windows))]
    DaemonLocalIpcEndpoint::new(protocol::daemon_socket_path()?)
}

/// # Errors
///
/// Returns [`AtmError`] when the canonical same-host daemon socket path for
/// `home_dir` cannot be resolved into a local IPC endpoint.
pub fn resolve_daemon_local_ipc_endpoint_from_home(
    home_dir: &Path,
) -> Result<DaemonLocalIpcEndpoint, AtmError> {
    #[cfg(windows)]
    {
        return DaemonLocalIpcEndpoint::new(atm_core::local_http::local_http_record_path(home_dir));
    }
    #[cfg(not(windows))]
    DaemonLocalIpcEndpoint::new(protocol::daemon_socket_path_from_home(home_dir))
}

/// # Errors
///
/// Returns [`AtmError`] when the current host executable path cannot be
/// resolved into the sibling `atm-daemon` binary path.
///
/// `ATM_DAEMON_BIN` is fully trusted process-owner input and intentionally
/// bypasses additional path validation.
pub fn resolve_daemon_bin(current_host_label: &str) -> Result<DaemonBinaryPath, AtmError> {
    if let Some(path) = std::env::var_os("ATM_DAEMON_BIN").filter(|value| !value.is_empty()) {
        return DaemonBinaryPath::new(PathBuf::from(path));
    }
    let current = std::env::current_exe().map_err(|source| {
        AtmError::daemon_unavailable_with_cause(
            format!("failed to resolve the current {current_host_label} executable path"),
            source,
        )
    })?;
    DaemonBinaryPath::new(
        current.with_file_name(format!("atm-daemon{}", std::env::consts::EXE_SUFFIX)),
    )
}

pub fn parse_bootstrap_caller_context() -> Result<CallerContext, AtmError> {
    resolve_cli_caller_context(CallerContextOverrides::default())
}

pub fn parse_bootstrap_agent() -> Result<AgentName, AtmError> {
    Ok(parse_bootstrap_caller_context()?.caller_identity)
}

pub fn parse_bootstrap_team() -> Result<TeamName, AtmError> {
    Ok(parse_bootstrap_caller_context()?.caller_team)
}

#[derive(Debug)]
pub struct DaemonSupervisor {
    endpoint: DaemonLocalIpcEndpoint,
    daemon_bin: DaemonBinaryPath,
}

pub struct BootstrapTraceability<'a> {
    command: &'static str,
    emit_event: &'a (dyn Fn(BootstrapCommandEvent) -> Result<(), AtmError> + Send + Sync),
    team: TeamName,
    agent: AgentName,
    // Mutex required: BootstrapTraceability must be Sync; RefCell would be
    // unsound once callers share the helper across bootstrap retries.
    state: Mutex<BootstrapTraceState>,
}

impl<'a> BootstrapTraceability<'a> {
    pub fn new(
        command: &'static str,
        emit_event: &'a (dyn Fn(BootstrapCommandEvent) -> Result<(), AtmError> + Send + Sync),
        team: TeamName,
        agent: AgentName,
    ) -> Self {
        Self {
            command,
            emit_event,
            team,
            agent,
            state: Mutex::new(BootstrapTraceState::default()),
        }
    }

    fn emit(&self, action: &'static str, outcome: &'static str, error: Option<&AtmError>) {
        self.record(action, outcome, error);
        let event = BootstrapCommandEvent {
            command: self.command,
            action,
            outcome,
            team: self.team.clone(),
            agent: self.agent.clone(),
            error_code: error.map(|error| error.code()),
            error_message: error.map(ToString::to_string),
        };
        if let Err(emit_error) = (self.emit_event)(event) {
            tracing::warn!(
                command = self.command,
                action,
                outcome,
                error = ?emit_error,
                "emit failed"
            );
        }
    }

    pub fn snapshot(&self) -> BootstrapTraceReport {
        let mut state = self
            .state
            .lock()
            .expect("bootstrap trace state lock poisoned");
        state.finalize()
    }

    fn record(&self, action: &'static str, outcome: &'static str, error: Option<&AtmError>) {
        let mut state = self
            .state
            .lock()
            .expect("bootstrap trace state lock poisoned");
        match action {
            "daemon_connect" => match outcome {
                "connected" => {
                    state.connect = Some(BootstrapConnectOutcome::Connected);
                    state.connect_detail = None;
                    if state.saw_spawn_requested {
                        state.auto_start = Some(BootstrapAutoStartOutcome::AutoStarted);
                        state.auto_start_detail = None;
                    }
                }
                "initial_miss" | "retry_attempt" | "pending" => {
                    if !matches!(state.connect, Some(BootstrapConnectOutcome::Connected)) {
                        state.connect = Some(BootstrapConnectOutcome::NotFound);
                    }
                    if let Some(error) = error {
                        state.connect_detail = Some(format_bootstrap_error_detail(error));
                    }
                }
                "error" => {
                    state.connect = Some(BootstrapConnectOutcome::Failed);
                    state.connect_detail = error.map(format_bootstrap_error_detail);
                }
                _ => {}
            },
            "daemon_launch_gate" => match outcome {
                "acquired" => {
                    state.launch_gate = Some(BootstrapLaunchGateOutcome::Launched);
                    state.launch_gate_detail = None;
                }
                "contended" => {
                    if state.launch_gate.is_none() {
                        state.launch_gate = Some(BootstrapLaunchGateOutcome::Skipped);
                    }
                }
                "timeout_exhausted" => {
                    state.launch_gate = Some(BootstrapLaunchGateOutcome::Failed);
                    state.launch_gate_detail = error.map(format_bootstrap_error_detail);
                    state.connect = Some(BootstrapConnectOutcome::Timeout);
                    state.connect_detail = error.map(format_bootstrap_error_detail);
                }
                "error" => {
                    state.launch_gate = Some(BootstrapLaunchGateOutcome::Failed);
                    state.launch_gate_detail = error.map(format_bootstrap_error_detail);
                }
                _ => {}
            },
            "daemon_auto_start" => match outcome {
                "spawn_requested" => {
                    state.saw_spawn_requested = true;
                }
                "error" | "timeout_exhausted" => {
                    state.auto_start = Some(BootstrapAutoStartOutcome::Failed);
                    state.auto_start_detail = error.map(format_bootstrap_error_detail);
                    if outcome == "timeout_exhausted" {
                        state.connect = Some(BootstrapConnectOutcome::Timeout);
                        state.connect_detail = error.map(format_bootstrap_error_detail);
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
}

#[derive(Debug, Default)]
struct BootstrapTraceState {
    connect: Option<BootstrapConnectOutcome>,
    launch_gate: Option<BootstrapLaunchGateOutcome>,
    auto_start: Option<BootstrapAutoStartOutcome>,
    connect_detail: Option<String>,
    launch_gate_detail: Option<String>,
    auto_start_detail: Option<String>,
    saw_spawn_requested: bool,
}

impl BootstrapTraceState {
    fn finalize(&mut self) -> BootstrapTraceReport {
        BootstrapTraceReport {
            daemon_connect: self.connect.unwrap_or(BootstrapConnectOutcome::NotFound),
            daemon_launch_gate: self
                .launch_gate
                .unwrap_or(BootstrapLaunchGateOutcome::Skipped),
            daemon_auto_start: self
                .auto_start
                .unwrap_or(BootstrapAutoStartOutcome::Skipped),
            connect_detail: self.connect_detail.clone(),
            launch_gate_detail: self.launch_gate_detail.clone(),
            auto_start_detail: self.auto_start_detail.clone(),
        }
    }
}

fn format_bootstrap_error_detail(error: &AtmError) -> String {
    error.message().to_owned()
}

pub fn try_connect(endpoint: &DaemonLocalIpcEndpoint) -> Result<LocalSocketStream, AtmError> {
    #[cfg(windows)]
    {
        return try_connect_local_http_record(endpoint.as_ref());
    }
    #[cfg(not(windows))]
    {
        let ipc_name = protocol::daemon_local_ipc_name_from_path(endpoint.as_ref())?;
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        thread::Builder::new()
        .name("daemon-local-ipc-connect".to_string())
        .spawn(move || {
            if result_tx.send(LocalSocketStream::connect(ipc_name)).is_err() {
                tracing::debug!(
                    "daemon local IPC connect worker dropped its result because the caller timed out first"
                );
            }
        })
        .map_err(|source| {
            AtmError::daemon_unavailable_with_cause(
                "failed to spawn bounded daemon local IPC connect worker",
                source,
            )
        })?;
        match result_rx.recv_timeout(LOCAL_IPC_CONNECT_DEADLINE) {
            Ok(Ok(stream)) => Ok(stream),
            Ok(Err(source)) => Err(AtmError::daemon_unavailable_with_cause(
                format!(
                    "failed to connect to daemon local IPC endpoint at {}",
                    endpoint.display()
                ),
                source,
            )),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(AtmError::daemon_unavailable(format!(
                "timed out connecting to daemon local IPC endpoint at {}",
                endpoint.display()
            ))),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(AtmError::daemon_unavailable(format!(
                    "daemon local IPC connect worker disconnected unexpectedly for {}",
                    endpoint.display()
                )))
            }
        }
    }
}

#[cfg(windows)]
fn try_connect_local_http_record(record_path: &Path) -> Result<LocalSocketStream, AtmError> {
    let record = load_local_http_record(record_path)?;
    record.capability()?;
    let endpoint = record
        .ipv4_loopback
        .or(record.ipv6_loopback)
        .ok_or_else(|| {
            AtmError::daemon_unavailable("local HTTP endpoint record has no loopback endpoint")
        })?;
    TcpStream::connect_timeout(&endpoint, LOCAL_IPC_CONNECT_DEADLINE).map_err(|source| {
        AtmError::daemon_unavailable(format!(
            "failed to connect to daemon local HTTP endpoint {endpoint}: {source}"
        ))
    })
}

/// Exchange one canonical request through HTTP over the daemon's UDS endpoint.
///
/// This is the retained production local-client path. The request is encoded
/// once as JSON HTTP and is decoded by the daemon before it reaches
/// [`atm_core::ApiRouter`].
pub fn exchange_request(
    endpoint: &DaemonLocalIpcEndpoint,
    request: &RequestEnvelope,
    request_deadline: Duration,
) -> Result<ResponseEnvelope, AtmError> {
    let mut stream = try_connect(endpoint)?;
    let _send_deadline_support = apply_local_ipc_deadline(
        set_stream_write_timeout(&stream, Some(request_deadline)),
        "failed to configure daemon local IPC write timeout",
    )?;
    let recv_deadline_support = apply_local_ipc_deadline(
        set_stream_read_timeout(&stream, Some(request_deadline)),
        "failed to configure daemon local IPC read timeout",
    )?;
    #[cfg(windows)]
    write_local_http_request(&mut stream, request, endpoint.as_ref())?;
    #[cfg(not(windows))]
    atm_core::api::write_http_request(&mut stream, request)?;
    read_http_response_with_deadline(stream, request, request_deadline, recv_deadline_support)
}

#[cfg(windows)]
fn write_local_http_request(
    writer: &mut TcpStream,
    request: &RequestEnvelope,
    record_path: &Path,
) -> Result<(), AtmError> {
    let record = load_local_http_record(record_path)?;
    let capability = record.capability()?.to_base64url();
    atm_core::api::write_http_request_with_headers(
        writer,
        request,
        &[(
            atm_core::local_http::LOCAL_CAPABILITY_HEADER,
            capability.as_str(),
        )],
    )
}

#[cfg(windows)]
fn load_local_http_record(
    record_path: &Path,
) -> Result<atm_core::local_http::LocalHttpEndpointRecord, AtmError> {
    let contents = fs::read(record_path).map_err(|source| {
        AtmError::daemon_unavailable(format!(
            "failed to read local HTTP endpoint record {}: {source}",
            record_path.display()
        ))
    })?;
    let record: atm_core::local_http::LocalHttpEndpointRecord = serde_json::from_slice(&contents)
        .map_err(|source| {
        AtmError::daemon_unavailable(format!(
            "failed to parse local HTTP endpoint record {}: {source}",
            record_path.display()
        ))
    })?;
    record.capability()?;
    let owner_instance_id =
        atm_core::local_http::owner_instance_id_for_local_http_record(record_path)?;
    if record.daemon_instance_id != owner_instance_id {
        return Err(AtmError::daemon_unavailable(
            "local HTTP endpoint record belongs to a different daemon instance",
        ));
    }
    Ok(record)
}

#[cfg(not(windows))]
fn set_stream_write_timeout(
    stream: &LocalSocketStream,
    timeout: Option<Duration>,
) -> std::io::Result<()> {
    stream.set_send_timeout(timeout)
}

#[cfg(windows)]
fn set_stream_write_timeout(
    stream: &LocalSocketStream,
    timeout: Option<Duration>,
) -> std::io::Result<()> {
    stream.set_write_timeout(timeout)
}

#[cfg(not(windows))]
fn set_stream_read_timeout(
    stream: &LocalSocketStream,
    timeout: Option<Duration>,
) -> std::io::Result<()> {
    stream.set_recv_timeout(timeout)
}

#[cfg(windows)]
fn set_stream_read_timeout(
    stream: &LocalSocketStream,
    timeout: Option<Duration>,
) -> std::io::Result<()> {
    stream.set_read_timeout(timeout)
}

fn read_http_response_with_deadline(
    mut stream: LocalSocketStream,
    request: &RequestEnvelope,
    _request_deadline: Duration,
    _recv_deadline_support: LocalIpcDeadlineSupport,
) -> Result<ResponseEnvelope, AtmError> {
    #[cfg(windows)]
    if _recv_deadline_support == LocalIpcDeadlineSupport::Unsupported {
        return read_http_response_with_helper(stream, request.clone(), _request_deadline);
    }
    atm_core::api::read_http_response(&mut stream, request)
}

#[cfg(windows)]
fn read_http_response_with_helper(
    mut stream: LocalSocketStream,
    request: RequestEnvelope,
    request_deadline: Duration,
) -> Result<ResponseEnvelope, AtmError> {
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("local-ipc-http-response-read-helper".to_string())
        .spawn(move || {
            let result = atm_core::api::read_http_response(&mut stream, &request);
            if result_tx.send(result).is_err() {
                tracing::debug!("daemon HTTP response reader timed out before helper completion");
            }
        })
        .map_err(|source| {
            AtmError::daemon_unavailable_with_cause(
                "failed to spawn daemon HTTP response read helper",
                source,
            )
        })?;
    result_rx
        .recv_timeout(request_deadline)
        .map_err(|error| match error {
            mpsc::RecvTimeoutError::Timeout => {
                AtmError::daemon_unavailable("timed out reading daemon HTTP response")
            }
            mpsc::RecvTimeoutError::Disconnected => AtmError::daemon_unavailable(
                "daemon HTTP response read helper disconnected unexpectedly",
            ),
        })?
}

fn apply_local_ipc_deadline(
    result: std::io::Result<()>,
    message: &'static str,
) -> Result<LocalIpcDeadlineSupport, AtmError> {
    match result {
        Ok(()) => Ok(LocalIpcDeadlineSupport::Applied),
        #[cfg(windows)]
        Err(source) if source.kind() == std::io::ErrorKind::Unsupported => {
            Ok(LocalIpcDeadlineSupport::Unsupported)
        }
        Err(source) => Err(AtmError::daemon_unavailable_with_cause(message, source)),
    }
}

pub fn unexpected_response(command: &str, response: impl fmt::Debug) -> AtmError {
    AtmError::validation(format!(
        "transport returned an unexpected response for `{command}`: {response:?}"
    ))
}

impl DaemonSupervisor {
    pub fn new(endpoint: DaemonLocalIpcEndpoint, daemon_bin: DaemonBinaryPath) -> Self {
        Self {
            endpoint,
            daemon_bin,
        }
    }

    pub fn ensure_daemon_available<F>(&self, try_connect: F) -> Result<(), AtmError>
    where
        F: FnMut() -> Result<(), AtmError>,
    {
        self.ensure_daemon_available_with_timeout_impl(
            try_connect,
            AUTO_START_PUBLISH_TIMEOUT,
            Duration::from_millis(25),
            None,
        )
    }

    pub fn ensure_daemon_available_with_traceability<F>(
        &self,
        traceability: &BootstrapTraceability<'_>,
        try_connect: F,
    ) -> Result<(), AtmError>
    where
        F: FnMut() -> Result<(), AtmError>,
    {
        self.ensure_daemon_available_with_timeout_impl(
            try_connect,
            AUTO_START_PUBLISH_TIMEOUT,
            Duration::from_millis(25),
            Some(traceability),
        )
    }

    pub fn ensure_daemon_available_with_timeout<F>(
        &self,
        try_connect: F,
        publish_timeout: Duration,
        poll_interval: Duration,
    ) -> Result<(), AtmError>
    where
        F: FnMut() -> Result<(), AtmError>,
    {
        self.ensure_daemon_available_with_timeout_impl(
            try_connect,
            publish_timeout,
            poll_interval,
            None,
        )
    }

    pub fn ensure_daemon_available_with_timeout_and_traceability<F>(
        &self,
        try_connect: F,
        publish_timeout: Duration,
        poll_interval: Duration,
        traceability: &BootstrapTraceability<'_>,
    ) -> Result<(), AtmError>
    where
        F: FnMut() -> Result<(), AtmError>,
    {
        self.ensure_daemon_available_with_timeout_impl(
            try_connect,
            publish_timeout,
            poll_interval,
            Some(traceability),
        )
    }

    fn ensure_daemon_available_with_timeout_impl<F>(
        &self,
        try_connect: F,
        publish_timeout: Duration,
        poll_interval: Duration,
        traceability: Option<&BootstrapTraceability<'_>>,
    ) -> Result<(), AtmError>
    where
        F: FnMut() -> Result<(), AtmError>,
    {
        self.ensure_daemon_available_with_lock_path_impl(
            try_connect,
            publish_timeout,
            poll_interval,
            atm_core::home::current_host_runtime_scope()?.launch_lock,
            traceability,
        )
    }

    pub fn ensure_daemon_available_with_lock_path<F>(
        &self,
        try_connect: F,
        publish_timeout: Duration,
        poll_interval: Duration,
        launch_lock_path: PathBuf,
    ) -> Result<(), AtmError>
    where
        F: FnMut() -> Result<(), AtmError>,
    {
        self.ensure_daemon_available_with_lock_path_impl(
            try_connect,
            publish_timeout,
            poll_interval,
            launch_lock_path,
            None,
        )
    }

    fn ensure_daemon_available_with_lock_path_impl<F>(
        &self,
        mut try_connect: F,
        publish_timeout: Duration,
        poll_interval: Duration,
        launch_lock_path: PathBuf,
        traceability: Option<&BootstrapTraceability<'_>>,
    ) -> Result<(), AtmError>
    where
        F: FnMut() -> Result<(), AtmError>,
    {
        if self.try_connect_with_traceability(&mut try_connect, traceability, "initial_miss") {
            return Ok(());
        }
        let deadline = Instant::now() + publish_timeout;
        let mut gate_contention_reported = false;
        loop {
            self.emit_trace(traceability, "daemon_connect", "retry_attempt", None);
            if self.try_connect_with_traceability(&mut try_connect, traceability, "pending") {
                return Ok(());
            }
            let launch_gate = match LaunchGateGuard::try_acquire_at(launch_lock_path.clone()) {
                Ok(launch_gate) => launch_gate,
                Err(error) => {
                    self.emit_trace(traceability, "daemon_connect", "error", Some(&error));
                    return Err(error);
                }
            };
            if let Some(_guard) = launch_gate {
                self.emit_trace(traceability, "daemon_launch_gate", "acquired", None);
                if self.try_connect_with_traceability(&mut try_connect, traceability, "connected") {
                    return Ok(());
                }
                return self.spawn_and_wait_for_daemon(
                    &mut try_connect,
                    deadline,
                    publish_timeout,
                    poll_interval,
                    traceability,
                );
            }
            if !gate_contention_reported {
                self.emit_trace(traceability, "daemon_launch_gate", "contended", None);
                gate_contention_reported = true;
            }
            if Instant::now() >= deadline {
                let error = LaunchGateGuard::rejected_error(&self.endpoint);
                self.emit_trace(
                    traceability,
                    "daemon_launch_gate",
                    "timeout_exhausted",
                    Some(&error),
                );
                return Err(error);
            }
            thread::sleep(poll_interval);
        }
    }

    fn try_connect_with_traceability<F>(
        &self,
        try_connect: &mut F,
        traceability: Option<&BootstrapTraceability<'_>>,
        miss_stage: &'static str,
    ) -> bool
    where
        F: FnMut() -> Result<(), AtmError>,
    {
        match try_connect() {
            Ok(()) => {
                self.emit_trace(traceability, "daemon_connect", "connected", None);
                true
            }
            Err(error) => {
                self.emit_trace(traceability, "daemon_connect", miss_stage, Some(&error));
                false
            }
        }
    }

    fn spawn_and_wait_for_daemon<F>(
        &self,
        try_connect: &mut F,
        deadline: Instant,
        publish_timeout: Duration,
        poll_interval: Duration,
        traceability: Option<&BootstrapTraceability<'_>>,
    ) -> Result<(), AtmError>
    where
        F: FnMut() -> Result<(), AtmError>,
    {
        self.emit_trace(traceability, "daemon_auto_start", "spawn_requested", None);
        if let Err(error) = self.spawn_daemon() {
            self.emit_trace(traceability, "daemon_auto_start", "error", Some(&error));
            return Err(error);
        }
        self.emit_trace(
            traceability,
            "daemon_auto_start",
            "publish_wait_started",
            None,
        );
        self.wait_for_published_daemon(
            try_connect,
            deadline,
            publish_timeout,
            poll_interval,
            traceability,
        )
    }

    fn wait_for_published_daemon<F>(
        &self,
        try_connect: &mut F,
        deadline: Instant,
        publish_timeout: Duration,
        poll_interval: Duration,
        traceability: Option<&BootstrapTraceability<'_>>,
    ) -> Result<(), AtmError>
    where
        F: FnMut() -> Result<(), AtmError>,
    {
        let halfway_deadline = Instant::now() + (publish_timeout / 2);
        let mut halfway_reported = false;
        while Instant::now() < deadline {
            if self.try_connect_with_traceability(try_connect, traceability, "pending") {
                return Ok(());
            }
            if !halfway_reported && Instant::now() >= halfway_deadline {
                tracing::warn!(
                    endpoint = %self.endpoint.display(),
                    publish_timeout_ms = publish_timeout.as_millis(),
                    "daemon auto-start is still waiting for the same-host IPC endpoint halfway through the publish budget"
                );
                halfway_reported = true;
            }
            self.emit_trace(
                traceability,
                "daemon_auto_start",
                "publish_wait_continuing",
                None,
            );
            thread::sleep(poll_interval);
        }
        let error = AtmError::daemon_auto_start_failed(format!(
            "failed to connect to daemon local IPC endpoint at {} after auto-start",
            self.endpoint.display()
        ));
        self.emit_trace(
            traceability,
            "daemon_auto_start",
            "timeout_exhausted",
            Some(&error),
        );
        Err(error)
    }

    fn emit_trace(
        &self,
        traceability: Option<&BootstrapTraceability<'_>>,
        action: &'static str,
        outcome: &'static str,
        error: Option<&AtmError>,
    ) {
        if let Some(traceability) = traceability {
            traceability.emit(action, outcome, error);
        }
    }

    fn spawn_daemon(&self) -> Result<(), AtmError> {
        if !self.daemon_bin.as_ref().is_file() {
            return Err(AtmError::daemon_unavailable(format!(
                "daemon binary is missing at {}",
                self.daemon_bin.display()
            )));
        }

        let mut command = Command::new(self.daemon_bin.as_ref());
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command.spawn().map_err(|source| {
            AtmError::daemon_auto_start_failed(format!(
                "failed to spawn daemon binary at {}: {source}",
                self.daemon_bin.display()
            ))
        })?;
        Ok(())
    }
}

pub struct LaunchGateGuard {
    file: File,
}

impl fmt::Debug for LaunchGateGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LaunchGateGuard").finish_non_exhaustive()
    }
}

impl LaunchGateGuard {
    pub fn rejected_error(endpoint: &DaemonLocalIpcEndpoint) -> AtmError {
        AtmError::daemon_launch_gate_rejected(format!(
            "daemon launch gate remained owned while connecting to {}",
            endpoint.display()
        ))
    }

    pub fn try_acquire_at(lock_path: PathBuf) -> Result<Option<Self>, AtmError> {
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent).map_err(|source| {
                AtmError::daemon_unavailable_with_cause(
                    format!(
                        "failed to create daemon launch lock directory at {}",
                        parent.display()
                    ),
                    source,
                )
            })?;
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|source| {
                AtmError::daemon_unavailable_with_cause(
                    format!(
                        "failed to open daemon launch gate at {}",
                        lock_path.display()
                    ),
                    source,
                )
            })?;

        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(Self { file })),
            Err(error) if is_launch_gate_contention_error(&error) => Ok(None),
            Err(source) => Err(AtmError::daemon_unavailable_with_cause(
                format!(
                    "failed to acquire daemon launch gate at {}",
                    lock_path.display()
                ),
                source,
            )),
        }
    }
}

impl Drop for LaunchGateGuard {
    fn drop(&mut self) {
        if let Err(error) = self.file.unlock() {
            eprintln!("warning: failed to release daemon launch gate: {error}");
        }
    }
}

pub fn is_launch_gate_contention_error(error: &std::io::Error) -> bool {
    if error.kind() == std::io::ErrorKind::WouldBlock {
        return true;
    }

    #[cfg(windows)]
    {
        matches!(error.raw_os_error(), Some(32 | 33))
    }

    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use atm_storage::{AtmError, AtmErrorCode};
    use tempfile::TempDir;

    use super::{
        BootstrapAutoStartOutcome, BootstrapCommandEvent, BootstrapConnectOutcome,
        BootstrapLaunchGateOutcome, BootstrapTraceReport, BootstrapTraceability, DaemonBinaryPath,
        DaemonLocalIpcEndpoint, DaemonSupervisor, HOST_RUNTIME_LAUNCH_LOCK_FILE, LaunchGateGuard,
        apply_local_ipc_deadline,
    };

    #[derive(Debug, Default)]
    struct RecordingEvents {
        events: Mutex<Vec<BootstrapCommandEvent>>,
    }

    impl RecordingEvents {
        fn emit(&self, event: BootstrapCommandEvent) -> Result<(), AtmError> {
            self.events.lock().expect("events lock").push(event);
            Ok(())
        }

        fn events(&self) -> Vec<BootstrapCommandEvent> {
            self.events.lock().expect("events lock").clone()
        }
    }

    fn supervisor(tempdir: &TempDir) -> DaemonSupervisor {
        DaemonSupervisor::new(
            DaemonLocalIpcEndpoint::new(tempdir.path().join("daemon.sock")).expect("endpoint"),
            DaemonBinaryPath::new(tempdir.path().join("atm-daemon")).expect("daemon path"),
        )
    }

    fn launch_lock_path(tempdir: &TempDir) -> PathBuf {
        tempdir.path().join(HOST_RUNTIME_LAUNCH_LOCK_FILE)
    }

    #[test]
    fn bootstrap_traceability_preserves_explicit_identity() {
        let events = RecordingEvents::default();
        let emit = |event| events.emit(event);
        let traceability = BootstrapTraceability::new(
            "send",
            &emit,
            "trace-team".parse().expect("team"),
            "trace-agent".parse().expect("agent"),
        );

        assert_eq!(traceability.team.as_str(), "trace-team");
        assert_eq!(traceability.agent.as_str(), "trace-agent");
    }

    #[test]
    fn traceability_emits_pending_and_connected_for_retry_success() {
        let tempdir = TempDir::new().expect("tempdir");
        let supervisor = supervisor(&tempdir);
        let events = RecordingEvents::default();
        let attempts = Arc::new(AtomicUsize::new(0));
        let try_connect_attempts = Arc::clone(&attempts);
        let emit = |event| events.emit(event);
        let traceability = BootstrapTraceability::new(
            "send",
            &emit,
            "trace-team".parse().expect("team"),
            "trace-agent".parse().expect("agent"),
        );

        supervisor
            .ensure_daemon_available_with_lock_path_impl(
                move || {
                    if try_connect_attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                        Err(AtmError::daemon_unavailable("not ready"))
                    } else {
                        Ok(())
                    }
                },
                Duration::from_millis(5),
                Duration::from_millis(1),
                launch_lock_path(&tempdir),
                Some(&traceability),
            )
            .expect("daemon available");

        let recorded = events.events();
        assert_eq!(recorded.len(), 3);
        assert_eq!(recorded[0].command, "send");
        assert_eq!(recorded[0].action, "daemon_connect");
        assert_eq!(recorded[0].outcome, "initial_miss");
        assert_eq!(recorded[1].action, "daemon_connect");
        assert_eq!(recorded[1].outcome, "retry_attempt");
        assert_eq!(recorded[2].action, "daemon_connect");
        assert_eq!(recorded[2].outcome, "connected");
        assert_eq!(recorded[2].team.as_str(), "trace-team");
        assert_eq!(recorded[2].agent.as_str(), "trace-agent");
        assert_eq!(
            traceability.snapshot(),
            BootstrapTraceReport {
                daemon_connect: BootstrapConnectOutcome::Connected,
                daemon_launch_gate: BootstrapLaunchGateOutcome::Skipped,
                daemon_auto_start: BootstrapAutoStartOutcome::Skipped,
                connect_detail: None,
                launch_gate_detail: None,
                auto_start_detail: None,
            }
        );
    }

    #[test]
    fn traceability_emits_spawn_failure_error() {
        let tempdir = TempDir::new().expect("tempdir");
        let supervisor = supervisor(&tempdir);
        let events = RecordingEvents::default();
        let emit = |event| events.emit(event);
        let traceability = BootstrapTraceability::new(
            "doctor",
            &emit,
            "trace-team".parse().expect("team"),
            "trace-agent".parse().expect("agent"),
        );

        let error = supervisor
            .ensure_daemon_available_with_lock_path_impl(
                || Err(AtmError::daemon_unavailable("not ready")),
                Duration::from_millis(5),
                Duration::from_millis(1),
                launch_lock_path(&tempdir),
                Some(&traceability),
            )
            .expect_err("spawn failure");

        assert_eq!(error.code(), AtmErrorCode::DaemonUnavailable);
        let recorded = events.events();
        assert!(recorded.iter().any(|event| {
            event.action == "daemon_auto_start" && event.outcome == "spawn_requested"
        }));
        let error_event = recorded
            .iter()
            .find(|event| event.action == "daemon_auto_start" && event.outcome == "error")
            .expect("error event");
        assert_eq!(error_event.command, "doctor");
        assert_eq!(
            error_event.error_code,
            Some(AtmErrorCode::DaemonUnavailable)
        );
        assert!(
            error_event
                .error_message
                .as_deref()
                .expect("error message")
                .contains("daemon binary is missing")
        );
        assert_eq!(
            traceability.snapshot().daemon_auto_start,
            BootstrapAutoStartOutcome::Failed
        );
        assert!(
            traceability
                .snapshot()
                .auto_start_detail
                .as_deref()
                .expect("auto-start detail")
                .contains("atm-daemon binary is installed")
        );
    }

    #[test]
    fn launch_gate_rejected_error_uses_daemon_launch_gate_code() {
        let tempdir = TempDir::new().expect("tempdir");
        let endpoint =
            DaemonLocalIpcEndpoint::new(tempdir.path().join("daemon.sock")).expect("endpoint");

        let error = LaunchGateGuard::rejected_error(&endpoint);
        assert_eq!(error.code(), AtmErrorCode::DaemonLaunchGateRejected);
    }

    #[test]
    fn local_ipc_deadline_handles_unsupported_timeout_per_platform_contract() {
        let result = apply_local_ipc_deadline(
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "local socket backend does not support I/O timeouts",
            )),
            "failed to configure daemon local IPC write timeout",
        );

        #[cfg(windows)]
        assert!(result.is_ok());

        #[cfg(not(windows))]
        {
            let error = result.expect_err(
                "non-Windows local IPC transports should keep unsupported deadline setup as an error",
            );
            assert_eq!(error.code(), AtmErrorCode::DaemonUnavailable);
            assert!(
                error
                    .message()
                    .contains("failed to configure daemon local IPC write timeout")
            );
        }
    }

    #[test]
    fn local_ipc_deadline_preserves_non_unsupported_errors() {
        let result = apply_local_ipc_deadline(
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "synthetic local IPC failure",
            )),
            "failed to configure daemon local IPC write timeout",
        )
        .expect_err("non-unsupported timeout errors should remain failures");

        assert_eq!(result.code(), AtmErrorCode::DaemonUnavailable);
        assert!(
            result
                .message()
                .contains("failed to configure daemon local IPC write timeout")
        );
    }
}
