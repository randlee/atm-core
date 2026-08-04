use std::fs::{self, File, OpenOptions};
#[cfg(unix)]
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use std::{fmt, thread};

use atm_core::caller_context::{CallerContext, CallerContextOverrides, resolve_cli_caller_context};
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};
use atm_storage::{AgentName, AtmError, AtmErrorCode, TeamName};
use fs2::FileExt;
#[cfg(unix)]
use interprocess::local_socket::Stream as LocalSocketStream;
#[cfg(unix)]
use interprocess::local_socket::prelude::*;
use std::net::TcpStream;
use std::sync::Mutex;

pub use atm_core::doctor::{
    BootstrapAutoStartOutcome, BootstrapConnectOutcome, BootstrapLaunchGateOutcome,
    BootstrapTraceReport,
};
pub use atm_core::protocol::{CompatibilityPreflight, CompatibilityVerdict, ReleaseVersion};

mod compatibility;
mod http_exchange;
mod local_transport;

pub use compatibility::{Connection, Unverified, VersionVerified, verify_connection_compatibility};
use http_exchange::{
    apply_local_ipc_deadline, load_local_http_record, read_http_response_with_deadline,
    set_stream_read_timeout, set_stream_write_timeout, write_local_http_request,
};
pub use local_transport::{LocalDaemonTransport, local_daemon_transport};

/// Upper bound for waiting on a daemon just spawned by the CLI to publish its
/// local HTTP record and accept its first connection.
pub const AUTO_START_PUBLISH_TIMEOUT: Duration = Duration::from_secs(30);
pub const HOST_RUNTIME_LAUNCH_LOCK_FILE: &str = "launch.lock";
const LOCAL_IPC_CONNECT_DEADLINE: Duration = Duration::from_millis(250);
// The request deadline governs daemon work. Keep a narrow response-only grace
// so a daemon that reaches that deadline can serialize its typed terminal
// result instead of racing the client's identical socket-read timeout.
const LOCAL_IPC_RESPONSE_GRACE: Duration = Duration::from_millis(250);
const AUTO_START_MAX_POLL_INTERVAL: Duration = Duration::from_millis(250);

const DAEMON_STRIPPED_ENVIRONMENT: [&str; 3] = ["ATM_TEAM", "ATM_IDENTITY", "ATM_ENVIRONMENT"];

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

impl DaemonLocalIpcEndpoint {
    #[cfg(unix)]
    fn unix_socket_path(&self) -> Result<PathBuf, AtmError> {
        self.0.parent().map_or_else(
            || {
                Err(AtmError::daemon_unavailable(
                    "daemon local endpoint record has no runtime directory",
                ))
            },
            |runtime_dir| Ok(runtime_dir.join(atm_core::home::HOST_RUNTIME_SOCKET_FILE)),
        )
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

/// Remove caller-scoped environment from a daemon child before it is exec'd.
///
/// The invoking CLI/graft process resolves caller context into typed request
/// data.  The long-lived daemon must never inherit those ambient values, and
/// this helper deliberately does not inspect or mutate the parent environment.
fn sanitize_daemon_child_environment(command: &mut Command) {
    for variable in DAEMON_STRIPPED_ENVIRONMENT {
        command.env_remove(variable);
    }
}

#[cfg(windows)]
fn reject_socket_override() -> Result<(), AtmError> {
    if std::env::var_os("ATM_DAEMON_SOCKET").is_some_and(|value| !value.is_empty()) {
        return Err(AtmError::socket_override_forbidden(
            "ATM_DAEMON_SOCKET cannot override the host singleton endpoint",
        ));
    }
    Ok(())
}

/// # Errors
///
/// Returns [`AtmError`] when the canonical same-host daemon HTTP record cannot
/// be resolved into a local IPC endpoint.
pub fn resolve_daemon_local_ipc_endpoint() -> Result<DaemonLocalIpcEndpoint, AtmError> {
    DaemonLocalIpcEndpoint::new(host_local_http_record_path()?)
}

/// # Errors
///
/// Returns [`AtmError`] when the canonical host-scoped daemon HTTP record
/// cannot be resolved into a local IPC endpoint.
///
/// The parameter is retained for source compatibility with callers that
/// previously supplied an ATM configuration home. Runtime endpoint discovery
/// is intentionally independent of `ATM_HOME`.
pub fn resolve_daemon_local_ipc_endpoint_from_home(
    _home_dir: &Path,
) -> Result<DaemonLocalIpcEndpoint, AtmError> {
    resolve_daemon_local_ipc_endpoint()
}

fn host_local_http_record_path() -> Result<PathBuf, AtmError> {
    #[cfg(windows)]
    reject_socket_override()?;

    let runtime_scope = atm_core::home::current_host_runtime_scope()?;
    Ok(runtime_scope
        .runtime_root
        .as_ref()
        .join(atm_core::local_http::LOCAL_HTTP_RECORD_FILENAME))
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

/// A connected same-host daemon stream.
///
/// Unix UDS and loopback TCP carry the same HTTP request/response contract;
/// the variant only records the explicitly selected transport.
pub enum LocalDaemonConnection {
    #[cfg(unix)]
    UnixDomainSocket(LocalSocketStream),
    TcpLoopback(TcpStream),
}

pub fn try_connect(endpoint: &DaemonLocalIpcEndpoint) -> Result<LocalDaemonConnection, AtmError> {
    try_connect_with_transport(endpoint, local_daemon_transport()?)
}

#[cfg(unix)]
fn try_connect_unix_socket(
    endpoint: &DaemonLocalIpcEndpoint,
) -> Result<LocalSocketStream, AtmError> {
    let socket_path = endpoint.unix_socket_path()?;
    let name = atm_core::protocol::daemon_local_ipc_name_from_path(&socket_path)?;
    LocalSocketStream::connect(name).map_err(|source| {
        AtmError::daemon_unavailable_with_cause(
            format!(
                "failed to connect to daemon UDS endpoint {}: {source}",
                socket_path.display(),
            ),
            source,
        )
    })
}

fn try_connect_local_http_record(record_path: &Path) -> Result<TcpStream, AtmError> {
    let record = load_local_http_record(record_path)?;
    record.capability()?;
    let endpoint = record
        .ipv4_loopback
        .or(record.ipv6_loopback)
        .ok_or_else(|| {
            AtmError::daemon_unavailable("local HTTP endpoint record has no loopback endpoint")
        })?;
    TcpStream::connect_timeout(&endpoint, LOCAL_IPC_CONNECT_DEADLINE).map_err(|source| {
        AtmError::daemon_unavailable_with_cause(
            format!("failed to connect to daemon local HTTP endpoint {endpoint}: {source}"),
            source,
        )
    })
}

/// Exchange one canonical request through the selected same-host HTTP transport.
///
/// Unix defaults to the daemon-owned UDS. TCP is used only when explicitly
/// requested with `ATM_LOCAL_TRANSPORT=tcp`; Windows always uses that path.
pub fn exchange_request(
    endpoint: &DaemonLocalIpcEndpoint,
    request: &RequestEnvelope,
    request_deadline: Duration,
) -> Result<ResponseEnvelope, AtmError> {
    exchange_request_with_transport(
        endpoint,
        request,
        request_deadline,
        local_daemon_transport()?,
    )
}

fn exchange_request_with_transport(
    endpoint: &DaemonLocalIpcEndpoint,
    request: &RequestEnvelope,
    request_deadline: Duration,
    transport: LocalDaemonTransport,
) -> Result<ResponseEnvelope, AtmError> {
    match try_connect_with_transport(endpoint, transport)? {
        #[cfg(unix)]
        LocalDaemonConnection::UnixDomainSocket(stream) => {
            exchange_uds_request(stream, request, request_deadline)
        }
        LocalDaemonConnection::TcpLoopback(stream) => {
            exchange_tcp_request(stream, endpoint, request, request_deadline)
        }
    }
}

fn try_connect_with_transport(
    endpoint: &DaemonLocalIpcEndpoint,
    transport: LocalDaemonTransport,
) -> Result<LocalDaemonConnection, AtmError> {
    match transport {
        #[cfg(unix)]
        LocalDaemonTransport::UnixDomainSocket => {
            try_connect_unix_socket(endpoint).map(LocalDaemonConnection::UnixDomainSocket)
        }
        LocalDaemonTransport::TcpLoopback => {
            try_connect_local_http_record(endpoint.as_ref()).map(LocalDaemonConnection::TcpLoopback)
        }
    }
}

fn exchange_tcp_request(
    mut stream: TcpStream,
    endpoint: &DaemonLocalIpcEndpoint,
    request: &RequestEnvelope,
    request_deadline: Duration,
) -> Result<ResponseEnvelope, AtmError> {
    let response_deadline = request_deadline.saturating_add(LOCAL_IPC_RESPONSE_GRACE);
    let _send_deadline_support = apply_local_ipc_deadline(
        set_stream_write_timeout(&stream, Some(request_deadline)),
        "failed to configure daemon local IPC write timeout",
    )?;
    let recv_deadline_support = apply_local_ipc_deadline(
        set_stream_read_timeout(&stream, Some(response_deadline)),
        "failed to configure daemon local IPC read timeout",
    )?;
    write_local_http_request(&mut stream, request, endpoint.as_ref())?;
    read_http_response_with_deadline(stream, request, response_deadline, recv_deadline_support)
}

#[cfg(unix)]
fn exchange_uds_request(
    mut stream: LocalSocketStream,
    request: &RequestEnvelope,
    request_deadline: Duration,
) -> Result<ResponseEnvelope, AtmError> {
    let response_deadline = request_deadline.saturating_add(LOCAL_IPC_RESPONSE_GRACE);
    // UDS timeout support is platform/backend-dependent. Match loopback TCP:
    // preserve real setup failures, but continue when the backend explicitly
    // reports that socket timeouts are unsupported.
    let _send_deadline_support = apply_local_ipc_deadline(
        stream.set_send_timeout(Some(request_deadline)),
        "failed to configure daemon UDS write timeout",
    )?;
    let _recv_deadline_support = apply_local_ipc_deadline(
        stream.set_recv_timeout(Some(response_deadline)),
        "failed to configure daemon UDS read timeout",
    )?;
    atm_core::api::write_http_request(&mut stream, request)?;
    stream.flush().map_err(|source| {
        AtmError::daemon_unavailable_with_cause("failed to flush daemon UDS request", source)
    })?;
    atm_core::api::read_http_response_with_frame_reader(
        &mut atm_core::api::HttpFrameReader::new(),
        &mut stream,
        request,
    )
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
        let child = match self.spawn_daemon() {
            Ok(child) => child,
            Err(error) => {
                self.emit_trace(traceability, "daemon_auto_start", "error", Some(&error));
                return Err(error);
            }
        };
        let mut cleanup = FailedAutoStartChild::new(child);
        self.emit_trace(
            traceability,
            "daemon_auto_start",
            "publish_wait_started",
            None,
        );
        let result = self.wait_for_published_daemon(
            try_connect,
            deadline,
            publish_timeout,
            poll_interval,
            traceability,
        );
        if result.is_ok() {
            cleanup.disarm();
        }
        result
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
        let mut current_poll_interval = poll_interval;
        let mut last_connect_error = None;
        while Instant::now() < deadline {
            match try_connect() {
                Ok(()) => {
                    self.emit_trace(traceability, "daemon_connect", "connected", None);
                    return Ok(());
                }
                Err(error) => {
                    self.emit_trace(traceability, "daemon_connect", "pending", Some(&error));
                    last_connect_error = Some(error);
                }
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
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            thread::sleep(current_poll_interval.min(remaining));
            current_poll_interval = next_auto_start_poll_interval(current_poll_interval);
        }
        let detail = last_connect_error
            .as_ref()
            .map_or_else(String::new, |error| {
                format!("; last local IPC failure: {error}")
            });
        let error = AtmError::daemon_auto_start_failed(format!(
            "failed to connect to daemon local IPC endpoint at {} after auto-start{detail}",
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

    fn spawn_daemon(&self) -> Result<Child, AtmError> {
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
        sanitize_daemon_child_environment(&mut command);
        command.spawn().map_err(|source| {
            AtmError::daemon_auto_start_failed(format!(
                "failed to spawn daemon binary at {}: {source}",
                self.daemon_bin.display()
            ))
        })
    }
}

/// A daemon started by this invocation must not outlive a failed publish wait.
/// Existing daemons are never represented by this child handle, so this cleanup
/// cannot affect the host-wide singleton owned by another CLI invocation.
fn reap_failed_auto_start(child: &mut Child) {
    match child.try_wait() {
        Ok(Some(_)) => return,
        Ok(None) => {}
        Err(error) => {
            tracing::warn!(error = %error, "failed to inspect daemon child after auto-start timeout");
            return;
        }
    }
    if let Err(error) = child.kill() {
        tracing::warn!(error = %error, "failed to terminate daemon child after auto-start timeout");
        return;
    }
    if let Err(error) = child.wait() {
        tracing::warn!(error = %error, "failed to reap daemon child after auto-start timeout");
    }
}

/// Reaps only the daemon process spawned by this CLI invocation unless its
/// publication succeeds.  Keeping this guard live across the wait also covers
/// an unexpected unwind in tracing or connection polling.
struct FailedAutoStartChild(Option<Child>);

impl FailedAutoStartChild {
    fn new(child: Child) -> Self {
        Self(Some(child))
    }

    fn disarm(&mut self) {
        self.0.take();
    }
}

impl Drop for FailedAutoStartChild {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            reap_failed_auto_start(child);
        }
    }
}

fn next_auto_start_poll_interval(current: Duration) -> Duration {
    current.saturating_mul(2).min(AUTO_START_MAX_POLL_INTERVAL)
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
    use std::ffi::{OsStr, OsString};
    use std::fs;
    use std::io::{self, Read};
    use std::net::{Ipv4Addr, TcpListener};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
    use std::time::{Duration, Instant};

    use atm_storage::{AtmError, AtmErrorCode};
    use tempfile::TempDir;

    #[cfg(unix)]
    use interprocess::local_socket::ListenerOptions;
    #[cfg(unix)]
    use interprocess::local_socket::prelude::*;

    use super::{
        AUTO_START_PUBLISH_TIMEOUT, BootstrapAutoStartOutcome, BootstrapCommandEvent,
        BootstrapConnectOutcome, BootstrapLaunchGateOutcome, BootstrapTraceReport,
        BootstrapTraceability, DAEMON_STRIPPED_ENVIRONMENT, DaemonBinaryPath,
        DaemonLocalIpcEndpoint, DaemonSupervisor, HOST_RUNTIME_LAUNCH_LOCK_FILE, LaunchGateGuard,
        LocalDaemonTransport, LocalIpcDeadlineSupport, apply_local_ipc_deadline,
        exchange_request_with_transport, next_auto_start_poll_interval,
        read_http_response_with_deadline, resolve_daemon_local_ipc_endpoint,
        resolve_daemon_local_ipc_endpoint_from_home, sanitize_daemon_child_environment,
    };
    #[cfg(unix)]
    use super::{FailedAutoStartChild, reap_failed_auto_start, try_connect_with_transport};

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
            DaemonLocalIpcEndpoint::new(
                tempdir
                    .path()
                    .join(atm_core::home::HOST_RUNTIME_SOCKET_FILE),
            )
            .expect("endpoint"),
            DaemonBinaryPath::new(tempdir.path().join("atm-daemon")).expect("daemon path"),
        )
    }

    fn launch_lock_path(tempdir: &TempDir) -> PathBuf {
        tempdir.path().join(HOST_RUNTIME_LAUNCH_LOCK_FILE)
    }

    fn configured_environment(command: &super::Command, key: &str) -> Option<Option<OsString>> {
        command
            .get_envs()
            .find(|(name, _)| *name == OsStr::new(key))
            .map(|(_, value)| value.map(OsStr::to_os_string))
    }

    struct ProcessEnvironmentGuard {
        originals: Vec<(&'static str, Option<OsString>)>,
        _lock: MutexGuard<'static, ()>,
    }

    impl ProcessEnvironmentGuard {
        fn set(changes: [(&'static str, Option<&str>); 5]) -> Self {
            static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
            let lock = LOCK
                .get_or_init(|| Mutex::new(()))
                .lock()
                .expect("environment lock");
            let originals = changes
                .into_iter()
                .map(|(key, value)| {
                    let original = std::env::var_os(key);
                    // SAFETY: the test holds the process-local environment lock
                    // for the complete period during which child inheritance is
                    // observed, and restores every value on drop.
                    unsafe {
                        match value {
                            Some(value) => std::env::set_var(key, value),
                            None => std::env::remove_var(key),
                        }
                    }
                    (key, original)
                })
                .collect();
            Self {
                originals,
                _lock: lock,
            }
        }
    }

    impl Drop for ProcessEnvironmentGuard {
        fn drop(&mut self) {
            for (key, value) in self.originals.iter().rev() {
                // SAFETY: restoration is serialized by the guard's process-local
                // environment lock and mirrors the guarded mutations above.
                unsafe {
                    match value {
                        Some(value) => std::env::set_var(key, value),
                        None => std::env::remove_var(key),
                    }
                }
            }
        }
    }

    #[test]
    fn daemon_child_environment_removes_caller_context_and_preserves_unrelated_values() {
        let mut command = super::Command::new("daemon-fixture");
        command
            .env("ATM_TEAM", "hostile-team")
            .env("ATM_IDENTITY", "hostile-identity")
            .env("ATM_ENVIRONMENT", "hostile-environment")
            .env("ATM_AK7_UNRELATED", "preserve-me");

        sanitize_daemon_child_environment(&mut command);

        for variable in DAEMON_STRIPPED_ENVIRONMENT {
            assert_eq!(
                configured_environment(&command, variable),
                Some(None),
                "{variable} must be explicitly removed from the child command"
            );
        }
        assert_eq!(
            configured_environment(&command, "ATM_AK7_UNRELATED"),
            Some(Some(OsString::from("preserve-me")))
        );
    }

    #[cfg(unix)]
    #[test]
    fn shared_auto_start_child_cannot_observe_caller_context_environment() {
        use std::os::unix::fs::PermissionsExt;

        let tempdir = TempDir::new().expect("tempdir");
        let output_path = tempdir.path().join("child-environment.txt");
        let script_path = tempdir.path().join("atm-daemon-fixture");
        fs::write(
            &script_path,
            r##"#!/bin/sh
set -eu
{
  printf 'ATM_TEAM=%s\n' "${ATM_TEAM-<unset>}"
  printf 'ATM_IDENTITY=%s\n' "${ATM_IDENTITY-<unset>}"
  printf 'ATM_ENVIRONMENT=%s\n' "${ATM_ENVIRONMENT-<unset>}"
  printf 'ATM_AK7_UNRELATED=%s\n' "${ATM_AK7_UNRELATED-<unset>}"
} > "$ATM_AK7_ENV_OUTPUT"
"##,
        )
        .expect("write daemon child fixture");
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700))
            .expect("make daemon child fixture executable");

        let output = output_path.to_str().expect("UTF-8 fixture output path");
        let _env = ProcessEnvironmentGuard::set([
            ("ATM_TEAM", Some("hostile-team")),
            ("ATM_IDENTITY", Some("hostile-identity")),
            ("ATM_ENVIRONMENT", Some("hostile-environment")),
            ("ATM_AK7_UNRELATED", Some("preserve-me")),
            ("ATM_AK7_ENV_OUTPUT", Some(output)),
        ]);
        let daemon = DaemonSupervisor::new(
            DaemonLocalIpcEndpoint::new(tempdir.path().join("unused.sock")).expect("endpoint"),
            DaemonBinaryPath::new(script_path).expect("daemon fixture path"),
        );

        let mut child = daemon.spawn_daemon().expect("spawn daemon child fixture");
        assert!(
            child
                .wait()
                .expect("wait for daemon child fixture")
                .success()
        );
        let observed = fs::read_to_string(output_path).expect("read child environment");
        assert!(observed.contains("ATM_TEAM=<unset>"), "{observed}");
        assert!(observed.contains("ATM_IDENTITY=<unset>"), "{observed}");
        assert!(observed.contains("ATM_ENVIRONMENT=<unset>"), "{observed}");
        assert!(
            observed.contains("ATM_AK7_UNRELATED=preserve-me"),
            "{observed}"
        );
    }

    #[test]
    fn local_ipc_endpoint_is_host_scoped_not_atm_home_scoped() {
        let supplied_atm_home = TempDir::new().expect("temp ATM home");
        let expected = atm_core::home::current_host_runtime_scope()
            .expect("host runtime scope")
            .runtime_root
            .as_ref()
            .join(atm_core::local_http::LOCAL_HTTP_RECORD_FILENAME);

        let canonical = resolve_daemon_local_ipc_endpoint().expect("canonical endpoint");
        let compatibility_endpoint =
            resolve_daemon_local_ipc_endpoint_from_home(supplied_atm_home.path())
                .expect("compatibility endpoint");

        assert_eq!(canonical.as_ref(), expected);
        assert_eq!(compatibility_endpoint.as_ref(), expected);
    }

    #[test]
    fn side_effecting_deadline_response_has_time_to_cross_local_http() {
        let tempdir = TempDir::new().expect("temp runtime");
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind local HTTP");
        let endpoint = listener.local_addr().expect("local HTTP address");
        let capability = atm_core::local_http::LocalCapability::generate().expect("capability");
        let record_path = tempdir
            .path()
            .join(atm_core::local_http::LOCAL_HTTP_RECORD_FILENAME);
        let record = atm_core::local_http::LocalHttpEndpointRecord::active(
            "01ARZ3NDEKTSV4RRFFQ69G5FAV"
                .parse()
                .expect("daemon instance id"),
            Some(endpoint),
            None,
            &capability,
        );
        let instance_id = record.daemon_instance_id;
        std::fs::write(
            &record_path,
            serde_json::to_vec(&record).expect("serialize endpoint record"),
        )
        .expect("write endpoint record");
        std::fs::write(
            tempdir
                .path()
                .join(atm_core::home::HOST_RUNTIME_OWNER_LOCK_FILE),
            format!("1:test:{instance_id}"),
        )
        .expect("write owner record");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept local HTTP request");
            let request = atm_core::api::read_http_request(&mut stream)
                .expect("read HTTP request")
                .expect("request");
            let (_wait_tx, wait_rx) = std::sync::mpsc::sync_channel::<()>(1);
            assert!(matches!(
                wait_rx.recv_timeout(Duration::from_millis(40)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ));
            atm_core::api::write_http_response(
                &mut stream,
                &atm_core::ResponseEnvelope::Error(AtmError::remote_delivery_unconfirmed(
                    "peer delivery deadline elapsed",
                )),
            )
            .expect("write terminal response");
            drop(request);
        });

        let request = atm_core::RequestEnvelope::Doctor(atm_core::doctor::DoctorQuery::default());
        let response = exchange_request_with_transport(
            &DaemonLocalIpcEndpoint::new(record_path).expect("endpoint path"),
            &request,
            Duration::from_millis(30),
            LocalDaemonTransport::TcpLoopback,
        )
        .expect("typed terminal response must outlive the work deadline");

        assert!(matches!(
            response,
            atm_core::ResponseEnvelope::Error(error)
                if error.code() == AtmErrorCode::RemoteDeliveryUnconfirmed
        ));
        server.join().expect("server join");
    }

    #[test]
    fn unsupported_response_timeout_cancels_and_joins_the_reader_helper() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind local HTTP");
        let address = listener.local_addr().expect("local HTTP address");
        let (closed_tx, closed_rx) = std::sync::mpsc::sync_channel(1);
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept local HTTP connection");
            let mut byte = [0_u8; 1];
            assert_eq!(stream.read(&mut byte).expect("read client close"), 0);
            closed_tx.send(()).expect("report client close");
        });
        let request = atm_core::RequestEnvelope::Doctor(atm_core::doctor::DoctorQuery::default());
        let stream = std::net::TcpStream::connect(address).expect("connect local HTTP");

        let error = read_http_response_with_deadline(
            stream,
            &request,
            Duration::from_millis(20),
            LocalIpcDeadlineSupport::Unsupported,
        )
        .expect_err("deadline must cancel the response-reader helper");

        assert!(
            error
                .message()
                .contains("timed out reading daemon HTTP response")
        );
        closed_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("peer observes cancellation after helper join");
        server.join().expect("server join");
    }

    #[cfg(unix)]
    #[test]
    fn uds_and_tcp_modes_share_the_http_response_contract() {
        let tempdir = TempDir::new().expect("temp runtime");
        let socket_path = tempdir
            .path()
            .join(atm_core::home::HOST_RUNTIME_SOCKET_FILE);
        let socket_name = atm_core::protocol::daemon_local_ipc_name_from_path(&socket_path)
            .expect("UDS name")
            .into_owned();
        let listener = ListenerOptions::new()
            .name(socket_name)
            .create_sync()
            .expect("bind UDS listener");
        let server = std::thread::spawn(move || {
            let mut stream = listener.accept().expect("accept UDS request");
            let request = atm_core::api::read_http_request(&mut stream)
                .expect("read request")
                .expect("request");
            assert_eq!(request.path, "/v1/atm/doctor");
            atm_core::api::write_http_response(
                &mut stream,
                &atm_core::ResponseEnvelope::Error(AtmError::daemon_unavailable(
                    "same dispatcher response",
                )),
            )
            .expect("write response");
        });

        let request = atm_core::RequestEnvelope::Doctor(atm_core::doctor::DoctorQuery::default());
        let response = exchange_request_with_transport(
            &DaemonLocalIpcEndpoint::new(
                tempdir
                    .path()
                    .join(atm_core::local_http::LOCAL_HTTP_RECORD_FILENAME),
            )
            .expect("endpoint"),
            &request,
            Duration::from_secs(1),
            LocalDaemonTransport::UnixDomainSocket,
        )
        .expect("UDS HTTP response");

        assert!(
            matches!(response, atm_core::ResponseEnvelope::Error(error) if error.is_daemon_unavailable())
        );
        server.join().expect("server join");
    }

    #[cfg(unix)]
    #[test]
    fn uds_connect_failure_keeps_the_socket_cause_in_the_displayed_error() {
        let tempdir = TempDir::new().expect("temp runtime");
        let socket_path = tempdir
            .path()
            .join(atm_core::home::HOST_RUNTIME_SOCKET_FILE);
        let result = try_connect_with_transport(
            &DaemonLocalIpcEndpoint::new(
                tempdir
                    .path()
                    .join(atm_core::local_http::LOCAL_HTTP_RECORD_FILENAME),
            )
            .expect("endpoint"),
            LocalDaemonTransport::UnixDomainSocket,
        );
        let Err(error) = result else {
            panic!("no UDS listener exists");
        };
        assert!(
            error.message().contains(&socket_path.display().to_string()),
            "the displayed error identifies the failed UDS endpoint: {error:?}"
        );
        assert!(
            error.cause().is_some_and(|cause| !cause.is_empty()),
            "the underlying UDS connection failure remains structured"
        );
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
        let endpoint = DaemonLocalIpcEndpoint::new(
            tempdir
                .path()
                .join(atm_core::home::HOST_RUNTIME_SOCKET_FILE),
        )
        .expect("endpoint");

        let error = LaunchGateGuard::rejected_error(&endpoint);
        assert_eq!(error.code(), AtmErrorCode::DaemonLaunchGateRejected);
    }

    #[test]
    fn local_ipc_deadline_handles_unavailable_timeout_per_platform_contract() {
        for timeout_error in [
            io::ErrorKind::Unsupported,
            // macOS AF_UNIX returns EINVAL for unsupported SO_SNDTIMEO and
            // SO_RCVTIMEO configuration.
            io::ErrorKind::InvalidInput,
        ] {
            let result = apply_local_ipc_deadline(
                Err(io::Error::new(
                    timeout_error,
                    "local socket backend does not support I/O timeouts",
                )),
                "failed to configure daemon local IPC write timeout",
            );

            assert_eq!(
                result.expect("unavailable timeout support is tolerated"),
                LocalIpcDeadlineSupport::Unsupported
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

    #[test]
    fn auto_start_publish_budget_covers_cold_daemon_bootstrap() {
        assert_eq!(AUTO_START_PUBLISH_TIMEOUT, Duration::from_secs(30));
    }

    #[test]
    fn auto_start_poll_backoff_is_bounded() {
        assert_eq!(
            next_auto_start_poll_interval(Duration::from_millis(25)),
            Duration::from_millis(50)
        );
        assert_eq!(
            next_auto_start_poll_interval(Duration::from_millis(200)),
            Duration::from_millis(250)
        );
        assert_eq!(
            next_auto_start_poll_interval(Duration::from_millis(250)),
            Duration::from_millis(250)
        );
    }

    #[test]
    fn publish_wait_retries_until_the_daemon_record_is_connectable() {
        let tempdir = TempDir::new().expect("tempdir");
        let supervisor = supervisor(&tempdir);
        let attempts = AtomicUsize::new(0);

        supervisor
            .wait_for_published_daemon(
                &mut || {
                    if attempts.fetch_add(1, Ordering::SeqCst) < 2 {
                        Err(AtmError::daemon_unavailable("record not ready"))
                    } else {
                        Ok(())
                    }
                },
                Instant::now() + Duration::from_millis(50),
                Duration::from_millis(1),
                Duration::from_millis(1),
                None,
            )
            .expect("daemon record becomes connectable before the bounded deadline");

        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn publish_wait_retains_the_last_local_ipc_failure_in_its_timeout_error() {
        let tempdir = TempDir::new().expect("tempdir");
        let supervisor = supervisor(&tempdir);
        let error = supervisor
            .wait_for_published_daemon(
                &mut || Err(AtmError::daemon_unavailable("fixture connection refused")),
                Instant::now() + Duration::from_millis(2),
                Duration::from_millis(2),
                Duration::from_millis(1),
                None,
            )
            .expect_err("unreachable daemon must exhaust the publish wait");

        assert!(
            error.message().contains("fixture connection refused"),
            "the timeout must retain the last local IPC diagnostic"
        );
    }

    #[cfg(unix)]
    #[test]
    fn failed_auto_start_reaps_the_daemon_child() {
        let mut child = super::Command::new("sleep")
            .arg("60")
            .spawn()
            .expect("spawn daemon child fixture");

        reap_failed_auto_start(&mut child);

        assert!(
            child.try_wait().expect("inspect reaped child").is_some(),
            "failed auto-start must reap its child"
        );
    }

    #[cfg(unix)]
    #[test]
    fn auto_start_guard_reaps_the_daemon_child_on_unwind_scope_exit() {
        let child = super::Command::new("sleep")
            .arg("60")
            .spawn()
            .expect("spawn daemon child fixture");
        let pid = child.id().to_string();

        drop(FailedAutoStartChild::new(child));

        assert!(
            !super::Command::new("kill")
                .args(["-0", &pid])
                .output()
                .expect("check child process")
                .status
                .success(),
            "dropping the failed-auto-start guard must reap its child"
        );
    }
}
