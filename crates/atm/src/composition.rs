#![allow(dead_code)]

use std::fmt;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::{Command, Stdio};
use std::sync::Arc;
#[cfg(unix)]
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::net::UnixStream,
    thread,
    time::{Duration, Instant},
};

use atm_core::ack::{AckOutcome, AckRequest};
use atm_core::boundary;
use atm_core::boundary::ClientTransport;
use atm_core::clear::{ClearOutcome, ClearQuery};
use atm_core::doctor::{DoctorQuery, DoctorReport};
use atm_core::error::AtmError;
use atm_core::observability::{CommandEvent, ObservabilityPort};
use atm_core::protocol::{
    RequestEnvelope, ResponseEnvelope, SendRequestEnvelope, SendResponseEnvelope,
};
use atm_core::read::{ReadOutcome, ReadQuery};
use atm_core::send::{SendOutcome, SendRequest};
#[cfg(unix)]
use fs2::FileExt;

use crate::observability::CliObservability;

#[derive(Debug, Default)]
pub(crate) struct SendCommandEntryPoint;

impl SendCommandEntryPoint {
    pub(crate) const fn new() -> Self {
        Self
    }
}

#[derive(Debug, Default)]
pub(crate) struct ReceiveCommandEntryPoint;

impl ReceiveCommandEntryPoint {
    pub(crate) const fn new() -> Self {
        Self
    }
}

#[derive(Debug, Clone)]
struct DaemonSocketPath(PathBuf);

impl DaemonSocketPath {
    fn new(path: PathBuf) -> Result<Self, AtmError> {
        validate_daemon_path("daemon socket path", &path)?;
        Ok(Self(path))
    }

    #[cfg(unix)]
    fn launch_gate_path(&self) -> PathBuf {
        self.0.with_extension("launch.lock")
    }

    fn display(&self) -> std::path::Display<'_> {
        self.0.display()
    }
}

impl AsRef<Path> for DaemonSocketPath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

#[derive(Debug, Clone)]
struct DaemonBinaryPath(PathBuf);

impl DaemonBinaryPath {
    fn new(path: PathBuf) -> Result<Self, AtmError> {
        validate_daemon_path("daemon binary path", &path)?;
        Ok(Self(path))
    }

    fn display(&self) -> std::path::Display<'_> {
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

#[derive(Debug)]
struct LocalSocketClientTransport {
    socket_path: DaemonSocketPath,
}

impl LocalSocketClientTransport {
    fn new(socket_path: DaemonSocketPath) -> Self {
        Self { socket_path }
    }

    #[cfg(unix)]
    fn try_connect(&self) -> Result<UnixStream, AtmError> {
        UnixStream::connect(self.socket_path.as_ref()).map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to connect to daemon socket at {}",
                self.socket_path.display()
            ))
            .with_source(source)
        })
    }

    #[cfg(not(unix))]
    fn try_connect(&self) -> Result<(), AtmError> {
        Err(AtmError::daemon_unavailable(
            "ATM thin-client transport requires a Unix platform",
        ))
    }

    #[cfg(unix)]
    fn exchange(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        let mut stream = self.try_connect()?;
        let encoded = serde_json::to_vec(&request).map_err(AtmError::from)?;
        stream.write_all(&encoded).map_err(|source| {
            AtmError::daemon_unavailable("failed to write daemon request frame").with_source(source)
        })?;
        stream
            .shutdown(std::net::Shutdown::Write)
            .map_err(|source| {
                AtmError::daemon_unavailable("failed to finalize daemon request frame")
                    .with_source(source)
            })?;
        let bytes = read_bounded_stream(
            &mut stream,
            "failed to read daemon response frame",
            "daemon response frame exceeded the maximum supported size",
        )?;
        serde_json::from_slice(&bytes).map_err(AtmError::from)
    }

    #[cfg(not(unix))]
    fn exchange(&self, _request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        Err(AtmError::daemon_unavailable(
            "ATM thin-client transport requires a Unix platform",
        ))
    }
}

impl boundary::sealed::Sealed for LocalSocketClientTransport {}

impl ClientTransport for LocalSocketClientTransport {
    fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        self.exchange(request)
    }
}

#[derive(Debug)]
struct DaemonSupervisor {
    socket_path: DaemonSocketPath,
    daemon_bin: DaemonBinaryPath,
}

impl DaemonSupervisor {
    fn new(socket_path: DaemonSocketPath, daemon_bin: DaemonBinaryPath) -> Self {
        Self {
            socket_path,
            daemon_bin,
        }
    }

    fn ensure_daemon_available(
        &self,
        _transport: &LocalSocketClientTransport,
    ) -> Result<(), AtmError> {
        #[cfg(not(unix))]
        {
            Err(AtmError::daemon_unavailable(
                "ATM thin-client transport requires a Unix platform",
            ))
        }

        #[cfg(unix)]
        {
            if _transport.try_connect().is_ok() {
                return Ok(());
            }
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if _transport.try_connect().is_ok() {
                    return Ok(());
                }
                if let Some(_guard) = LaunchGateGuard::try_acquire(&self.socket_path)? {
                    if _transport.try_connect().is_ok() {
                        return Ok(());
                    }
                    self.spawn_daemon()?;
                    while Instant::now() < deadline {
                        if _transport.try_connect().is_ok() {
                            return Ok(());
                        }
                        thread::sleep(Duration::from_millis(25));
                    }
                    break;
                }
                if Instant::now() >= deadline {
                    break;
                }
                thread::sleep(Duration::from_millis(25));
            }
            Err(AtmError::daemon_unavailable(format!(
                "failed to connect to daemon socket at {} after auto-start",
                self.socket_path.display()
            )))
        }
    }

    #[cfg(unix)]
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
            .env("ATM_DAEMON_SOCKET", self.socket_path.as_ref());
        command.spawn().map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to spawn daemon binary at {}",
                self.daemon_bin.display()
            ))
            .with_source(source)
        })?;
        Ok(())
    }
}

#[cfg(unix)]
#[derive(Debug)]
struct LaunchGateGuard {
    path: PathBuf,
    file: File,
}

#[cfg(unix)]
impl LaunchGateGuard {
    fn try_acquire(socket_path: &DaemonSocketPath) -> Result<Option<Self>, AtmError> {
        let lock_path = socket_path.launch_gate_path();
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent).map_err(|source| {
                AtmError::daemon_unavailable(format!(
                    "failed to create daemon launch lock directory at {}",
                    parent.display()
                ))
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
                .with_source(source)
            })?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(Self {
                path: lock_path,
                file,
            })),
            Err(source) if source.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(source) => Err(AtmError::daemon_unavailable(format!(
                "failed to acquire daemon launch gate at {}",
                lock_path.display()
            ))
            .with_source(source)),
        }
    }
}

#[cfg(unix)]
impl Drop for LaunchGateGuard {
    fn drop(&mut self) {
        let _ = self.file.unlock();
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(unix)]
fn read_bounded_stream(
    stream: &mut impl Read,
    read_error: &'static str,
    oversize_error: &'static str,
) -> Result<Vec<u8>, AtmError> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let read = stream
            .read(&mut chunk)
            .map_err(|source| AtmError::daemon_unavailable(read_error).with_source(source))?;
        if read == 0 {
            return Ok(bytes);
        }
        if bytes.len().saturating_add(read) > atm_core::protocol::MAX_DAEMON_FRAME_BYTES {
            return Err(AtmError::daemon_unavailable(oversize_error).with_recovery(
                "Reduce the daemon request/response payload size before retrying the ATM command.",
            ));
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
}

pub(crate) struct CliComposition<'a> {
    transport: Arc<dyn ClientTransport + Send + Sync + 'a>,
    observability_port: &'a (dyn ObservabilityPort + Send + Sync),
    send_command: SendCommandEntryPoint,
    receive_command: ReceiveCommandEntryPoint,
}

impl fmt::Debug for CliComposition<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CliComposition")
            .field("transport", &"dyn ClientTransport")
            .field("observability_port", &"dyn ObservabilityPort")
            .field("send_command", &self.send_command)
            .field("receive_command", &self.receive_command)
            .finish()
    }
}

impl<'a> CliComposition<'a> {
    pub(crate) fn from_transport(
        transport: Arc<dyn ClientTransport + Send + Sync + 'a>,
        observability_port: &'a (dyn ObservabilityPort + Send + Sync),
    ) -> Self {
        Self {
            transport,
            observability_port,
            send_command: SendCommandEntryPoint::new(),
            receive_command: ReceiveCommandEntryPoint::new(),
        }
    }

    pub(crate) fn transport(&self) -> &(dyn ClientTransport + Send + Sync + 'a) {
        self.transport.as_ref()
    }

    pub(crate) fn send_request(
        &self,
        request: RequestEnvelope,
    ) -> Result<ResponseEnvelope, AtmError> {
        match self.transport.send(request)? {
            ResponseEnvelope::Error(error) => Err(error.into_atm_error()),
            response => Ok(response),
        }
    }

    pub(crate) fn observability_port(&self) -> &(dyn ObservabilityPort + Send + Sync) {
        self.observability_port
    }

    pub(crate) fn send_command(&self) -> &SendCommandEntryPoint {
        &self.send_command
    }

    pub(crate) fn receive_command(&self) -> &ReceiveCommandEntryPoint {
        &self.receive_command
    }

    pub(crate) fn send(&self, request: SendRequest) -> Result<SendOutcome, AtmError> {
        match self.send_request(RequestEnvelope::Send(SendRequestEnvelope::Compose(request)))? {
            ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => {
                let _ = self.observability_port.emit(CommandEvent {
                    command: "send",
                    action: "send",
                    outcome: if outcome.dry_run { "dry_run" } else { "sent" },
                    team: outcome.team.clone(),
                    agent: outcome.agent.clone(),
                    sender: outcome.sender.clone(),
                    message_id: Some(outcome.message_id),
                    requires_ack: outcome.requires_ack,
                    dry_run: outcome.dry_run,
                    task_id: outcome.task_id.clone(),
                    error_code: None,
                    error_message: None,
                });
                Ok(outcome)
            }
            other => Err(unexpected_response("send", other)),
        }
    }

    pub(crate) fn ack(&self, request: AckRequest) -> Result<AckOutcome, AtmError> {
        match self.send_request(RequestEnvelope::Send(SendRequestEnvelope::Acknowledge(
            request,
        )))? {
            ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(outcome)) => {
                let _ = self.observability_port.emit(CommandEvent {
                    command: "ack",
                    action: "ack",
                    outcome: "ok",
                    team: outcome.team.clone(),
                    agent: outcome.agent.clone(),
                    sender: outcome.agent.clone(),
                    message_id: Some(outcome.message_id),
                    requires_ack: false,
                    dry_run: false,
                    task_id: outcome.task_id.clone(),
                    error_code: None,
                    error_message: None,
                });
                Ok(outcome)
            }
            other => Err(unexpected_response("ack", other)),
        }
    }

    pub(crate) fn receive(&self, query: ReadQuery) -> Result<ReadOutcome, AtmError> {
        match self.send_request(RequestEnvelope::Receive(query))? {
            ResponseEnvelope::Receive(outcome) => {
                let _ = self.observability_port.emit(CommandEvent {
                    command: "read",
                    action: "read",
                    outcome: "ok",
                    team: outcome.team.clone(),
                    agent: outcome.agent.clone(),
                    sender: outcome.agent.clone(),
                    message_id: None,
                    requires_ack: false,
                    dry_run: false,
                    task_id: None,
                    error_code: None,
                    error_message: None,
                });
                Ok(outcome)
            }
            other => Err(unexpected_response("receive", other)),
        }
    }

    pub(crate) fn clear(&self, query: ClearQuery) -> Result<ClearOutcome, AtmError> {
        match self.send_request(RequestEnvelope::Clear(query))? {
            ResponseEnvelope::Clear(outcome) => {
                let _ = self.observability_port.emit(CommandEvent {
                    command: "clear",
                    action: "clear",
                    outcome: "ok",
                    team: outcome.team.clone(),
                    agent: outcome.agent.clone(),
                    sender: outcome.agent.clone(),
                    message_id: None,
                    requires_ack: false,
                    dry_run: false,
                    task_id: None,
                    error_code: None,
                    error_message: None,
                });
                Ok(outcome)
            }
            other => Err(unexpected_response("clear", other)),
        }
    }

    pub(crate) fn doctor(&self, query: DoctorQuery) -> Result<DoctorReport, AtmError> {
        match self.send_request(RequestEnvelope::Doctor(query))? {
            ResponseEnvelope::Doctor(report) => Ok(report),
            other => Err(unexpected_response("doctor", other)),
        }
    }

    pub(crate) fn bootstrap(observability: &'a CliObservability) -> Result<Self, AtmError> {
        let socket_path = resolve_daemon_socket_path()?;
        let daemon_bin = resolve_daemon_bin()?;
        let transport = Arc::new(LocalSocketClientTransport::new(socket_path.clone()));
        let supervisor = DaemonSupervisor::new(socket_path, daemon_bin);
        supervisor.ensure_daemon_available(transport.as_ref())?;
        Ok(Self::from_transport(transport, observability))
    }
}

fn resolve_daemon_socket_path() -> Result<DaemonSocketPath, AtmError> {
    DaemonSocketPath::new(atm_core::protocol::daemon_socket_path()?)
}

fn resolve_daemon_bin() -> Result<DaemonBinaryPath, AtmError> {
    if let Some(path) = std::env::var_os("ATM_DAEMON_BIN").filter(|value| !value.is_empty()) {
        return DaemonBinaryPath::new(PathBuf::from(path));
    }
    let current = std::env::current_exe().map_err(|source| {
        AtmError::daemon_unavailable("failed to resolve the current atm executable path")
            .with_source(source)
    })?;
    DaemonBinaryPath::new(
        current.with_file_name(format!("atm-daemon{}", std::env::consts::EXE_SUFFIX)),
    )
}

fn unexpected_response(command: &str, response: ResponseEnvelope) -> AtmError {
    AtmError::validation(format!(
        "transport returned an unexpected response for `{command}`: {response:?}"
    ))
}
