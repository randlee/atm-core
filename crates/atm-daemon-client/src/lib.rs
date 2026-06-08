use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use std::{fmt, thread};

use atm_core::doctor::{
    BootstrapAutoStartOutcome, BootstrapConnectOutcome, BootstrapLaunchGateOutcome,
    BootstrapTraceReport,
};
use atm_core::error::AtmError;
use atm_core::observability::{CommandEvent, ObservabilityPort, action_name, outcome_label};
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};
use atm_core::types::{AgentName, TeamName};
use fs2::FileExt;
use interprocess::local_socket::Stream as LocalSocketStream;
use interprocess::local_socket::traits::Stream as _;
use std::sync::Mutex;

pub const AUTO_START_PUBLISH_TIMEOUT: Duration = Duration::from_secs(10);
pub const HOST_RUNTIME_LAUNCH_LOCK_FILE: &str = "launch.lock";
const LOCAL_IPC_CONNECT_DEADLINE: Duration = Duration::from_millis(250);
#[cfg(windows)]
const LOCAL_IPC_READ_HELPER_POLL_INTERVAL: Duration = Duration::from_millis(100);

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
        return Err(AtmError::validation(format!("{label} must not be empty")).with_recovery(
            "Set ATM_DAEMON_SOCKET to a non-empty UTF-8 daemon local IPC endpoint before invoking the same-host ATM daemon client.",
        ));
    }
    if path.to_str().is_none() {
        return Err(AtmError::validation(format!(
            "{label} must be valid UTF-8 at the ATM boundary"
        ))
        .with_recovery(
            "Set ATM_DAEMON_SOCKET to a non-empty UTF-8 daemon local IPC endpoint before invoking the same-host ATM daemon client.",
        ));
    }
    Ok(())
}

pub fn parse_bootstrap_agent() -> Result<AgentName, AtmError> {
    std::env::var("ATM_IDENTITY")
        .unwrap_or_else(|_| "unknown".to_string())
        .parse()
        .map_err(|error: AtmError| {
            error.with_recovery("Check ATM_IDENTITY and ATM_TEAM env vars are set")
        })
}

pub fn parse_bootstrap_team() -> Result<TeamName, AtmError> {
    std::env::var("ATM_TEAM")
        .unwrap_or_else(|_| "unknown".to_string())
        .parse()
        .map_err(|error: AtmError| {
            error.with_recovery("Check ATM_IDENTITY and ATM_TEAM env vars are set")
        })
}

#[derive(Debug)]
pub struct DaemonSupervisor {
    endpoint: DaemonLocalIpcEndpoint,
    daemon_bin: DaemonBinaryPath,
}

pub struct BootstrapTraceability<'a> {
    command: &'static str,
    observability: &'a (dyn ObservabilityPort + Send + Sync),
    team: TeamName,
    agent: AgentName,
    // Mutex required: BootstrapTraceability must be Sync (holds
    // &'a dyn ObservabilityPort + Send + Sync); RefCell would be unsound.
    state: Mutex<BootstrapTraceState>,
}

impl<'a> BootstrapTraceability<'a> {
    pub fn new(
        command: &'static str,
        observability: &'a (dyn ObservabilityPort + Send + Sync),
        team: TeamName,
        agent: AgentName,
    ) -> Self {
        Self {
            command,
            observability,
            team,
            agent,
            state: Mutex::new(BootstrapTraceState::default()),
        }
    }

    fn emit(&self, action: &'static str, outcome: &'static str, error: Option<&AtmError>) {
        self.record(action, outcome, error);
        let event = CommandEvent {
            command: self.command,
            action: action_name(action),
            outcome: outcome_label(outcome),
            team: self.team.clone(),
            agent: self.agent.clone(),
            sender: self.agent.clone(),
            message_id: None,
            requires_ack: false,
            dry_run: false,
            task_id: None,
            error_code: error.map(|error| error.code),
            error_message: error.map(ToString::to_string),
        };
        if let Err(emit_error) = self.observability.emit(event) {
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
    match error.primary_recovery() {
        Some(recovery) => format!("{} Recovery: {}", error.message, recovery),
        None => error.message.clone(),
    }
}

pub fn try_connect(endpoint: &DaemonLocalIpcEndpoint) -> Result<LocalSocketStream, AtmError> {
    let ipc_name = atm_core::protocol::daemon_local_ipc_name_from_path(endpoint.as_ref())?;
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
            AtmError::daemon_unavailable("failed to spawn bounded daemon local IPC connect worker")
                .with_recovery(
                    "Retry the request after the local runtime can create the same-host daemon connect helper thread again.",
                )
                .with_source(source)
        })?;
    match result_rx.recv_timeout(LOCAL_IPC_CONNECT_DEADLINE) {
        Ok(Ok(stream)) => Ok(stream),
        Ok(Err(source)) => Err(AtmError::daemon_unavailable(format!(
            "failed to connect to daemon local IPC endpoint at {}",
            endpoint.display()
        ))
        .with_source(source)),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(AtmError::daemon_unavailable(format!(
            "timed out connecting to daemon local IPC endpoint at {}",
            endpoint.display()
        ))
        .with_recovery(
            "Retry the request after atm-daemon reaches serving state. If the same-host connect path remains stuck, inspect daemon startup and local IPC health before retrying again.",
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(AtmError::daemon_unavailable(format!(
            "daemon local IPC connect worker disconnected unexpectedly for {}",
            endpoint.display()
        ))
        .with_recovery(
            "Retry the request after the same-host daemon connect helper can be created again.",
        )),
    }
}

/// This function performs blocking IPC I/O. Callers in async contexts must
/// wrap this in `tokio::task::spawn_blocking`.
pub fn exchange(
    endpoint: &DaemonLocalIpcEndpoint,
    request: RequestEnvelope,
    request_deadline: Duration,
) -> Result<ResponseEnvelope, AtmError> {
    let mut stream = try_connect(endpoint)?;
    let _send_deadline_support = apply_local_ipc_deadline(
        stream.set_send_timeout(Some(request_deadline)),
        "failed to configure daemon local IPC write timeout",
    )?;
    let recv_deadline_support = apply_local_ipc_deadline(
        stream.set_recv_timeout(Some(request_deadline)),
        "failed to configure daemon local IPC read timeout",
    )?;
    let request_id = atm_core::protocol::next_request_id();
    let frame = atm_core::protocol::request_to_frame_payload(request_id, request)?;
    atm_core::protocol::write_frame(&mut stream, &frame, "failed to write daemon request frame")?;
    stream.flush().map_err(|source| {
        AtmError::daemon_unavailable("failed to flush daemon request frame").with_source(source)
    })?;
    let response_frame =
        read_response_frame_with_deadline(stream, request_deadline, recv_deadline_support)?;
    let (response_id, response) = atm_core::protocol::response_from_frame_payload(response_frame)?;
    if response_id != request_id {
        return Err(AtmError::daemon_unavailable(format!(
            "daemon response request_id {} did not match request_id {}",
            response_id, request_id
        ))
        .with_recovery(
            "Align the ATM client and daemon builds so both sides use the same local IPC protocol contract before retrying.",
        ));
    }
    Ok(response)
}

fn read_response_frame_with_deadline(
    mut stream: LocalSocketStream,
    _request_deadline: Duration,
    _recv_deadline_support: LocalIpcDeadlineSupport,
) -> Result<atm_core::protocol::FramePayload, AtmError> {
    #[cfg(windows)]
    if _recv_deadline_support == LocalIpcDeadlineSupport::Unsupported {
        return read_response_frame_with_helper(stream, _request_deadline);
    }

    read_response_frame(&mut stream)
}

#[cfg(windows)]
fn read_response_frame_with_helper(
    mut stream: LocalSocketStream,
    request_deadline: Duration,
) -> Result<atm_core::protocol::FramePayload, AtmError> {
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("local-ipc-response-read-helper".to_string())
        .spawn(move || {
            let result = read_response_frame(&mut stream);
            if result_tx.send(result).is_err() {
                tracing::debug!(
                    "daemon local IPC response-read helper dropped its result because the caller timed out first"
                );
            }
        })
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to spawn daemon local IPC read helper")
                .with_recovery(
                    "Retry the request after the same-host daemon read helper can be created again.",
                )
                .with_source(source)
        })?;

    let started = Instant::now();
    loop {
        let remaining = request_deadline.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(AtmError::daemon_unavailable(
                "timed out reading daemon response frame",
            )
            .with_recovery(
                "Retry the request after atm-daemon reaches serving state. If the same-host read path remains stuck, inspect daemon and local IPC health before retrying again.",
            ));
        }
        let poll = std::cmp::min(remaining, LOCAL_IPC_READ_HELPER_POLL_INTERVAL);
        match result_rx.recv_timeout(poll) {
            Ok(result) => return result,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(AtmError::daemon_unavailable(
                    "daemon local IPC read helper disconnected unexpectedly",
                )
                .with_recovery(
                    "Retry the request after the same-host daemon read helper can be created again.",
                ));
            }
        }
    }
}

fn read_response_frame(
    stream: &mut LocalSocketStream,
) -> Result<atm_core::protocol::FramePayload, AtmError> {
    atm_core::protocol::read_frame(
        stream,
        "failed to read daemon response frame",
        "daemon response frame exceeded the maximum supported size",
    )?
    .ok_or_else(|| {
        AtmError::daemon_unavailable(
            "daemon closed the local IPC connection before returning a response frame",
        )
        .with_recovery(
            "Retry the request after atm-daemon reaches serving state and inspect daemon logs if the problem persists.",
        )
    })
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
        Err(source) => Err(AtmError::daemon_unavailable(message).with_source(source)),
    }
}

pub fn unexpected_response(command: &str, response: ResponseEnvelope) -> AtmError {
    AtmError::validation(format!(
        "transport returned an unexpected response for `{command}`: {response:?}"
    ))
    .with_recovery(
        "Retry the request once. If the mismatch persists, inspect daemon/client version alignment and retained daemon logs before retrying again.",
    )
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
            atm_core::home::host_runtime_lock_path(HOST_RUNTIME_LAUNCH_LOCK_FILE)?,
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
        if self.try_connect_with_traceability(&mut try_connect, traceability, "initial_miss")? {
            return Ok(());
        }
        let deadline = Instant::now() + publish_timeout;
        let mut gate_contention_reported = false;
        loop {
            self.emit_trace(traceability, "daemon_connect", "retry_attempt", None);
            if self.try_connect_with_traceability(&mut try_connect, traceability, "pending")? {
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
                if self.try_connect_with_traceability(
                    &mut try_connect,
                    traceability,
                    "connected",
                )? {
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
    ) -> Result<bool, AtmError>
    where
        F: FnMut() -> Result<(), AtmError>,
    {
        match try_connect() {
            Ok(()) => {
                self.emit_trace(traceability, "daemon_connect", "connected", None);
                Ok(true)
            }
            Err(error) => {
                self.emit_trace(traceability, "daemon_connect", miss_stage, Some(&error));
                Ok(false)
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
            if self.try_connect_with_traceability(try_connect, traceability, "pending")? {
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
        ))
        .with_recovery(
            "Inspect atm-daemon startup logs, confirm the daemon publishes its local IPC endpoint, and retry only after the same-host socket becomes reachable.",
        );
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
            return Err(
                AtmError::daemon_unavailable(format!(
                    "daemon binary is missing at {}",
                    self.daemon_bin.display()
                ))
                .with_recovery(
                    "Build or install atm-daemon, or set ATM_DAEMON_BIN to the correct executable before retrying.",
                ),
            );
        }

        let mut command = Command::new(self.daemon_bin.as_ref());
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .env("ATM_DAEMON_SOCKET", self.endpoint.as_ref());
        command.spawn().map_err(|source| {
            AtmError::daemon_auto_start_failed(format!(
                "failed to spawn daemon binary at {}",
                self.daemon_bin.display()
            ))
            .with_recovery(
                "Confirm ATM_DAEMON_BIN points to an executable atm-daemon binary and retry after fixing the daemon launch environment.",
            )
            .with_source(source)
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
        .with_recovery(
            "Wait for the in-flight daemon launch to finish, then retry the same-host connection. If the launch gate stays owned, inspect the launch-lock owner and clear stale launch state before retrying.",
        )
    }

    pub fn try_acquire_at(lock_path: PathBuf) -> Result<Option<Self>, AtmError> {
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent).map_err(|source| {
                AtmError::daemon_unavailable(format!(
                    "failed to create daemon launch lock directory at {}",
                    parent.display()
                ))
                .with_recovery(
                    "Create or grant write access to the daemon launch-lock directory before retrying daemon auto-start.",
                )
                .with_source(source)
            })?;
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|source| {
                AtmError::daemon_unavailable(format!(
                    "failed to open daemon launch gate at {}",
                    lock_path.display()
                ))
                .with_recovery(
                    "Confirm the daemon launch-lock path is writable and not blocked by another process before retrying daemon auto-start.",
                )
                .with_source(source)
            })?;

        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(Self { file })),
            Err(error) if is_launch_gate_contention_error(&error) => Ok(None),
            Err(source) => Err(AtmError::daemon_unavailable(format!(
                "failed to acquire daemon launch gate at {}",
                lock_path.display()
            ))
            .with_recovery(
                "Inspect the daemon launch-lock owner and repair stale lock state before retrying daemon auto-start.",
            )
            .with_source(source)),
        }
    }
}

impl Drop for LaunchGateGuard {
    fn drop(&mut self) {
        let _ = self.file.unlock();
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

    use atm_core::doctor::{
        BootstrapAutoStartOutcome, BootstrapConnectOutcome, BootstrapLaunchGateOutcome,
        BootstrapTraceReport,
    };
    use atm_core::error::{AtmError, AtmErrorCode};
    use atm_core::observability::{
        AtmLogQuery, AtmLogSnapshot, AtmObservabilityHealth, AtmObservabilityHealthState,
        CommandEvent, LogTailSession, ObservabilityPort,
    };
    use tempfile::TempDir;

    use super::{
        BootstrapTraceability, DaemonBinaryPath, DaemonLocalIpcEndpoint, DaemonSupervisor,
        HOST_RUNTIME_LAUNCH_LOCK_FILE, LaunchGateGuard, apply_local_ipc_deadline,
    };

    #[derive(Debug, Default)]
    struct RecordingObservability {
        events: Mutex<Vec<CommandEvent>>,
    }

    impl RecordingObservability {
        fn events(&self) -> Vec<CommandEvent> {
            self.events.lock().expect("events lock").clone()
        }
    }

    impl atm_core::boundary::sealed::Sealed for RecordingObservability {}

    impl ObservabilityPort for RecordingObservability {
        fn emit(&self, event: CommandEvent) -> Result<(), AtmError> {
            self.events.lock().expect("events lock").push(event);
            Ok(())
        }

        fn query(&self, _req: AtmLogQuery) -> Result<AtmLogSnapshot, AtmError> {
            Ok(AtmLogSnapshot::default())
        }

        fn follow(&self, _req: AtmLogQuery) -> Result<LogTailSession, AtmError> {
            Ok(LogTailSession::empty())
        }

        fn health(&self) -> Result<AtmObservabilityHealth, AtmError> {
            Ok(AtmObservabilityHealth {
                active_log_path: None,
                logging_state: AtmObservabilityHealthState::Healthy,
                query_state: Some(AtmObservabilityHealthState::Healthy),
                maintenance: None,
                diagnostic: None,
                detail: None,
            })
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
        let observability = RecordingObservability::default();
        let traceability = BootstrapTraceability::new(
            "send",
            &observability,
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
        let observability = RecordingObservability::default();
        let attempts = Arc::new(AtomicUsize::new(0));
        let try_connect_attempts = Arc::clone(&attempts);
        let traceability = BootstrapTraceability::new(
            "send",
            &observability,
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

        let events = observability.events();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].command, "send");
        assert_eq!(events[0].action.as_str(), "daemon_connect");
        assert_eq!(events[0].outcome.as_str(), "initial_miss");
        assert_eq!(events[1].action.as_str(), "daemon_connect");
        assert_eq!(events[1].outcome.as_str(), "retry_attempt");
        assert_eq!(events[2].action.as_str(), "daemon_connect");
        assert_eq!(events[2].outcome.as_str(), "connected");
        assert_eq!(events[2].team.as_str(), "trace-team");
        assert_eq!(events[2].agent.as_str(), "trace-agent");
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
        let observability = RecordingObservability::default();
        let traceability = BootstrapTraceability::new(
            "doctor",
            &observability,
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

        assert_eq!(error.code, AtmErrorCode::DaemonUnavailable);
        let events = observability.events();
        assert!(events.iter().any(|event| {
            event.action.as_str() == "daemon_auto_start"
                && event.outcome.as_str() == "spawn_requested"
        }));
        let error_event = events
            .iter()
            .find(|event| {
                event.action.as_str() == "daemon_auto_start" && event.outcome.as_str() == "error"
            })
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
        assert_eq!(error.code, AtmErrorCode::DaemonLaunchGateRejected);
    }

    #[test]
    fn local_ipc_deadline_handles_unsupported_timeout_per_platform_contract() {
        let result = apply_local_ipc_deadline(
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "named pipes do not support I/O timeouts",
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
            assert_eq!(error.code, AtmErrorCode::DaemonUnavailable);
            assert!(
                error
                    .message
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

        assert_eq!(result.code, AtmErrorCode::DaemonUnavailable);
        assert!(
            result
                .message
                .contains("failed to configure daemon local IPC write timeout")
        );
    }
}
