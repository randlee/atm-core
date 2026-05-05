#![allow(dead_code)]

use std::fmt;
use std::path::PathBuf;
#[cfg(unix)]
use std::process::{Command, Stdio};
use std::sync::Arc;
#[cfg(unix)]
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::MetadataExt,
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

#[derive(Debug)]
struct LocalSocketClientTransport {
    socket_path: PathBuf,
    daemon_bin: PathBuf,
}

impl LocalSocketClientTransport {
    fn new(socket_path: PathBuf, daemon_bin: PathBuf) -> Self {
        Self {
            socket_path,
            daemon_bin,
        }
    }

    fn ensure_daemon_available(&self) -> Result<(), AtmError> {
        #[cfg(not(unix))]
        {
            Err(AtmError::daemon_unavailable(
                "ATM thin-client transport requires a Unix platform",
            ))
        }

        #[cfg(unix)]
        {
            if self.try_connect().is_ok() {
                return Ok(());
            }
            let published_socket = fs::metadata(&self.socket_path)
                .ok()
                .map(|metadata| (metadata.dev(), metadata.ino()));
            self.spawn_daemon()?;
            self.wait_for_socket_publish(published_socket)?;
            thread::sleep(Duration::from_millis(25));
            if self.try_connect().is_ok() {
                return Ok(());
            }
            Err(AtmError::daemon_unavailable(format!(
                "failed to connect to daemon socket at {} after auto-start",
                self.socket_path.display()
            )))
        }
    }

    #[cfg(unix)]
    fn try_connect(&self) -> Result<UnixStream, AtmError> {
        UnixStream::connect(&self.socket_path).map_err(|source| {
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
    fn spawn_daemon(&self) -> Result<(), AtmError> {
        if !self.daemon_bin.is_file() {
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

        let mut command = Command::new(&self.daemon_bin);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .env("ATM_DAEMON_SOCKET", &self.socket_path);
        command.spawn().map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to spawn daemon binary at {}",
                self.daemon_bin.display()
            ))
            .with_source(source)
        })?;
        Ok(())
    }

    #[cfg(unix)]
    fn wait_for_socket_publish(
        &self,
        published_socket: Option<(u64, u64)>,
    ) -> Result<(), AtmError> {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(metadata) = fs::metadata(&self.socket_path) {
                let observed_socket = (metadata.dev(), metadata.ino());
                if published_socket != Some(observed_socket) {
                    return Ok(());
                }
            }
            thread::sleep(Duration::from_millis(25));
        }
        Err(AtmError::daemon_unavailable(format!(
            "daemon socket was not published at {} after auto-start",
            self.socket_path.display()
        )))
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
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).map_err(|source| {
            AtmError::daemon_unavailable("failed to read daemon response frame").with_source(source)
        })?;
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
        let socket_path = atm_core::protocol::daemon_socket_path()?;
        let daemon_bin = resolve_daemon_bin()?;
        let transport = Arc::new(LocalSocketClientTransport::new(socket_path, daemon_bin));
        transport.ensure_daemon_available()?;
        Ok(Self::from_transport(transport, observability))
    }
}

fn resolve_daemon_bin() -> Result<PathBuf, AtmError> {
    if let Some(path) = std::env::var_os("ATM_DAEMON_BIN").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    let current = std::env::current_exe().map_err(|source| {
        AtmError::daemon_unavailable("failed to resolve the current atm executable path")
            .with_source(source)
    })?;
    Ok(current.with_file_name(format!("atm-daemon{}", std::env::consts::EXE_SUFFIX)))
}

fn unexpected_response(command: &str, response: ResponseEnvelope) -> AtmError {
    AtmError::validation(format!(
        "transport returned an unexpected response for `{command}`: {response:?}"
    ))
}
