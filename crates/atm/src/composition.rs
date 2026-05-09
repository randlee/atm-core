use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    thread,
    time::Instant,
};

use atm_core::ack::{AckOutcome, AckRequest};
use atm_core::boundary;
use atm_core::boundary::{AtmProtocol, ClientTransport};
use atm_core::clear::{ClearOutcome, ClearQuery};
use atm_core::doctor::{DoctorQuery, DoctorReport};
use atm_core::error::AtmError;
use atm_core::observability::{CommandEvent, ObservabilityPort};
use atm_core::protocol::{
    JsonAtmProtocolCodec, RequestEnvelope, ResponseEnvelope, SendRequestEnvelope,
    SendResponseEnvelope,
};
use atm_core::read::{ReadOutcome, ReadQuery};
use atm_core::send::{SendOutcome, SendRequest};
use fs2::FileExt;
use interprocess::local_socket::Stream as LocalSocketStream;
use interprocess::local_socket::traits::Stream as _;

use crate::observability::CliObservability;

const SAME_HOST_REQUEST_DEADLINE: std::time::Duration = std::time::Duration::from_secs(3);
const AUTO_START_PUBLISH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const HOST_RUNTIME_LAUNCH_LOCK_FILE: &str = "launch.lock";

#[allow(dead_code)]
#[derive(Debug, Default)]
pub(crate) struct SendCommandEntryPoint;

impl SendCommandEntryPoint {
    pub(crate) const fn new() -> Self {
        Self
    }
}

#[allow(dead_code)]
#[derive(Debug, Default)]
pub(crate) struct ReceiveCommandEntryPoint;

impl ReceiveCommandEntryPoint {
    pub(crate) const fn new() -> Self {
        Self
    }
}

#[derive(Debug, Clone)]
struct DaemonLocalIpcEndpoint(PathBuf);

impl DaemonLocalIpcEndpoint {
    fn new(path: PathBuf) -> Result<Self, AtmError> {
        validate_daemon_path("daemon local IPC endpoint", &path)?;
        Ok(Self(path))
    }

    fn display(&self) -> std::path::Display<'_> {
        self.0.display()
    }
}

impl AsRef<Path> for DaemonLocalIpcEndpoint {
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
        return Err(AtmError::validation(format!("{label} must not be empty")).with_recovery(
            "Set ATM_DAEMON_SOCKET to a non-empty UTF-8 daemon local IPC endpoint before invoking the daemon transport.",
        ));
    }
    if path.to_str().is_none() {
        return Err(AtmError::validation(format!(
            "{label} must be valid UTF-8 at the ATM boundary"
        ))
        .with_recovery(
            "Set ATM_DAEMON_SOCKET to a non-empty UTF-8 daemon local IPC endpoint before invoking the daemon transport.",
        ));
    }
    Ok(())
}

#[derive(Debug)]
struct LocalIpcClientTransportAdapter {
    endpoint: DaemonLocalIpcEndpoint,
    codec: JsonAtmProtocolCodec,
}

impl LocalIpcClientTransportAdapter {
    fn new(endpoint: DaemonLocalIpcEndpoint) -> Self {
        Self {
            endpoint,
            codec: JsonAtmProtocolCodec,
        }
    }

    fn try_connect(&self) -> Result<LocalSocketStream, AtmError> {
        LocalSocketStream::connect(atm_core::protocol::daemon_local_ipc_name_from_path(
            self.endpoint.as_ref(),
        )?)
        .map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to connect to daemon local IPC endpoint at {}",
                self.endpoint.display()
            ))
            .with_source(source)
        })
    }

    fn exchange(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        let mut stream = self.try_connect()?;
        stream
            .set_send_timeout(Some(SAME_HOST_REQUEST_DEADLINE))
            .map_err(|source| {
                AtmError::daemon_unavailable("failed to configure daemon local IPC write timeout")
                    .with_source(source)
            })?;
        stream
            .set_recv_timeout(Some(SAME_HOST_REQUEST_DEADLINE))
            .map_err(|source| {
                AtmError::daemon_unavailable("failed to configure daemon local IPC read timeout")
                    .with_source(source)
            })?;
        let request_id = atm_core::protocol::next_request_id();
        let frame = self.codec.request_to_frame(request_id, request)?;
        atm_core::protocol::write_frame(
            &mut stream,
            &frame,
            "failed to write daemon request frame",
        )?;
        stream.flush().map_err(|source| {
            AtmError::daemon_unavailable("failed to flush daemon request frame").with_source(source)
        })?;
        let response_frame = atm_core::protocol::read_frame(
            &mut stream,
            "failed to read daemon response frame",
            "daemon response frame exceeded the maximum supported size",
        )?
        .ok_or_else(|| {
            AtmError::daemon_unavailable(
                "daemon closed the local IPC connection before returning a response frame",
            )
            .with_recovery(
                "Retry the ATM command after the daemon reaches serving state and verify the daemon logs if the problem persists.",
            )
        })?;
        let (response_id, response) = self.codec.response_from_frame(response_frame)?;
        if response_id != request_id {
            return Err(AtmError::daemon_unavailable(format!(
                "daemon response request_id {} did not match request_id {}",
                response_id, request_id
            ))
            .with_recovery(
                "Align the CLI and daemon builds so both sides use the same ATM daemon protocol contract before retrying.",
            ));
        }
        Ok(response)
    }
}

impl boundary::sealed::Sealed for LocalIpcClientTransportAdapter {}

impl ClientTransport for LocalIpcClientTransportAdapter {
    fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        self.exchange(request)
    }
}

#[derive(Debug)]
struct DaemonSupervisor {
    endpoint: DaemonLocalIpcEndpoint,
    daemon_bin: DaemonBinaryPath,
}

impl DaemonSupervisor {
    fn new(endpoint: DaemonLocalIpcEndpoint, daemon_bin: DaemonBinaryPath) -> Self {
        Self {
            endpoint,
            daemon_bin,
        }
    }

    fn ensure_daemon_available(
        &self,
        transport: &LocalIpcClientTransportAdapter,
    ) -> Result<(), AtmError> {
        self.ensure_daemon_available_with_timeout(
            transport,
            AUTO_START_PUBLISH_TIMEOUT,
            Duration::from_millis(25),
        )
    }

    fn ensure_daemon_available_with_timeout(
        &self,
        transport: &LocalIpcClientTransportAdapter,
        publish_timeout: Duration,
        poll_interval: Duration,
    ) -> Result<(), AtmError> {
        self.ensure_daemon_available_with_lock_path(
            transport,
            publish_timeout,
            poll_interval,
            atm_core::home::host_runtime_lock_path(HOST_RUNTIME_LAUNCH_LOCK_FILE)?,
        )
    }

    fn ensure_daemon_available_with_lock_path(
        &self,
        transport: &LocalIpcClientTransportAdapter,
        publish_timeout: Duration,
        poll_interval: Duration,
        launch_lock_path: PathBuf,
    ) -> Result<(), AtmError> {
        if transport.try_connect().is_ok() {
            return Ok(());
        }
        let deadline = Instant::now() + publish_timeout;
        loop {
            if transport.try_connect().is_ok() {
                return Ok(());
            }
            if let Some(_guard) = LaunchGateGuard::try_acquire_at(launch_lock_path.clone())? {
                if transport.try_connect().is_ok() {
                    return Ok(());
                }
                self.spawn_daemon()?;
                while Instant::now() < deadline {
                    if transport.try_connect().is_ok() {
                        return Ok(());
                    }
                    thread::sleep(poll_interval);
                }
                return Err(AtmError::daemon_auto_start_failed(format!(
                    "failed to connect to daemon local IPC endpoint at {} after auto-start",
                    self.endpoint.display()
                )));
            }
            if Instant::now() >= deadline {
                return Err(LaunchGateGuard::rejected_error(&self.endpoint));
            }
            thread::sleep(poll_interval);
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
            .with_source(source)
        })?;
        Ok(())
    }
}

#[derive(Debug)]
struct LaunchGateGuard {
    file: File,
}

impl LaunchGateGuard {
    #[allow(dead_code)]
    fn try_acquire() -> Result<Option<Self>, AtmError> {
        let lock_path = atm_core::home::host_runtime_lock_path(HOST_RUNTIME_LAUNCH_LOCK_FILE)?;
        Self::try_acquire_at(lock_path)
    }

    fn rejected_error(endpoint: &DaemonLocalIpcEndpoint) -> AtmError {
        AtmError::daemon_launch_gate_rejected(format!(
            "daemon launch gate remained owned while connecting to {}",
            endpoint.display()
        ))
    }

    fn try_acquire_at(lock_path: PathBuf) -> Result<Option<Self>, AtmError> {
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
            Ok(()) => Ok(Some(Self { file })),
            Err(source) if source.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(source) => Err(AtmError::daemon_launch_gate_rejected(format!(
                "failed to acquire daemon launch gate at {}",
                lock_path.display()
            ))
            .with_source(source)),
        }
    }
}

impl Drop for LaunchGateGuard {
    fn drop(&mut self) {
        let _ = self.file.unlock();
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

    #[allow(dead_code)]
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

    #[allow(dead_code)]
    pub(crate) fn observability_port(&self) -> &(dyn ObservabilityPort + Send + Sync) {
        self.observability_port
    }

    #[allow(dead_code)]
    pub(crate) fn send_command(&self) -> &SendCommandEntryPoint {
        &self.send_command
    }

    #[allow(dead_code)]
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
        let endpoint = resolve_daemon_local_ipc_endpoint()?;
        let daemon_bin = resolve_daemon_bin()?;
        let transport = Arc::new(LocalIpcClientTransportAdapter::new(endpoint.clone()));
        let supervisor = DaemonSupervisor::new(endpoint, daemon_bin);
        supervisor.ensure_daemon_available(transport.as_ref())?;
        Ok(Self::from_transport(transport, observability))
    }
}

fn resolve_daemon_local_ipc_endpoint() -> Result<DaemonLocalIpcEndpoint, AtmError> {
    DaemonLocalIpcEndpoint::new(atm_core::protocol::daemon_socket_path()?)
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
    .with_recovery(
        "Retry the ATM command once. If the mismatch persists, inspect daemon/client version alignment and retained daemon logs before retrying again.",
    )
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;
    use std::time::Duration;

    use atm_core::ack::AckRequest;
    use atm_core::boundary::ClientTransport;
    use atm_core::clear::ClearQuery;
    use atm_core::doctor::{DoctorQuery, DoctorStatus};
    use atm_core::error::AtmError;
    use atm_core::protocol::{
        ProtocolErrorEnvelope, RequestEnvelope, ResponseEnvelope, SendRequestEnvelope,
    };
    use atm_core::read::ReadQuery;
    use atm_core::schema::{
        AgentMember, LegacyMessageId, MessageEnvelope, TeamConfig,
        hydrate_legacy_fields_from_metadata,
    };
    use atm_core::send::{SendMessageSource, SendRequest};
    use atm_core::test_support::{
        ROLE_TEAM_LEAD, TEST_LEAD, TEST_RECIPIENT, TEST_RECIPIENT_ADDRESS, TEST_SENDER, TEST_TEAM,
    };
    use atm_core::transport::testing::{
        FakeClientTransport, HealthyObservability, LoopbackClientTransport,
    };
    use atm_core::types::{AckActivationMode, ReadSelection};
    use chrono::Utc;
    use serde_json::Value;
    use tempfile::TempDir;

    use super::{
        CliComposition, DaemonBinaryPath, DaemonLocalIpcEndpoint, DaemonSupervisor,
        HOST_RUNTIME_LAUNCH_LOCK_FILE, LaunchGateGuard, LocalIpcClientTransportAdapter,
    };
    use crate::observability::CliObservability;

    struct LoopbackFixture {
        _tempdir: TempDir,
        home_dir: std::path::PathBuf,
        current_dir: std::path::PathBuf,
    }

    impl LoopbackFixture {
        fn new(recipient: &str) -> Self {
            let tempdir = tempfile::tempdir().expect("tempdir");
            let home_dir = tempdir.path().to_path_buf();
            let current_dir = tempdir.path().join("cwd");
            fs::create_dir_all(&current_dir).expect("cwd");
            let fixture = Self {
                _tempdir: tempdir,
                home_dir,
                current_dir,
            };
            fixture.write_team_config(recipient);
            fixture
        }

        fn team_dir(&self) -> std::path::PathBuf {
            self.home_dir.join(".claude").join("teams").join(TEST_TEAM)
        }

        fn inbox_path(&self, agent: &str) -> std::path::PathBuf {
            self.team_dir()
                .join("inboxes")
                .join(format!("{agent}.json"))
        }

        fn write_team_config(&self, recipient: &str) {
            let team_dir = self.team_dir();
            fs::create_dir_all(&team_dir).expect("team dir");
            fs::create_dir_all(team_dir.join("inboxes")).expect("inboxes dir");
            fs::create_dir_all(team_dir.join(".atm-state").join("workflow")).expect("workflow dir");
            let config = TeamConfig {
                members: vec![
                    AgentMember::with_name(TEST_SENDER.parse().expect("sender")),
                    AgentMember::with_name(recipient.parse().expect("recipient")),
                    AgentMember::with_name(TEST_LEAD.parse().expect("lead")),
                ],
                ..Default::default()
            };
            fs::write(
                team_dir.join("config.json"),
                serde_json::to_vec(&config).expect("team config"),
            )
            .expect("write team config");
        }

        fn write_inbox_values(&self, agent: &str, values: &[Value]) {
            let inbox_path = self.inbox_path(agent);
            if let Some(parent) = inbox_path.parent() {
                fs::create_dir_all(parent).expect("inbox dir");
            }
            fs::write(
                inbox_path,
                serde_json::to_string_pretty(values).expect("json array"),
            )
            .expect("write inbox");
        }

        fn inbox_contents(&self, agent: &str) -> Vec<MessageEnvelope> {
            let raw = fs::read_to_string(self.inbox_path(agent)).expect("inbox contents");
            let values: Vec<Value> = serde_json::from_str(&raw).expect("json array");
            values
                .into_iter()
                .map(|mut value| {
                    hydrate_legacy_fields_from_metadata(&mut value);
                    serde_json::from_value(value).expect("message envelope")
                })
                .collect()
        }

        fn write_inbox_messages(&self, agent: &str, messages: &[MessageEnvelope]) {
            let values = messages
                .iter()
                .map(|message| serde_json::to_value(message).expect("message value"))
                .collect::<Vec<_>>();
            self.write_inbox_values(agent, &values);
        }

        fn send_request(&self, body: &str) -> SendRequest {
            SendRequest::new(
                self.home_dir.clone(),
                self.current_dir.clone(),
                Some(TEST_SENDER),
                TEST_RECIPIENT_ADDRESS,
                Some(TEST_TEAM),
                SendMessageSource::Inline(body.to_string()),
                None,
                false,
                None,
                false,
            )
            .expect("send request")
        }

        fn send_request_with_flags(
            &self,
            body: &str,
            requires_ack: bool,
            task_id: Option<atm_core::types::TaskId>,
        ) -> SendRequest {
            SendRequest::new(
                self.home_dir.clone(),
                self.current_dir.clone(),
                Some(TEST_SENDER),
                TEST_RECIPIENT_ADDRESS,
                Some(TEST_TEAM),
                SendMessageSource::Inline(body.to_string()),
                None,
                requires_ack,
                task_id,
                false,
            )
            .expect("send request")
        }

        fn ack_request(&self, message_id: LegacyMessageId, reply_body: &str) -> AckRequest {
            AckRequest {
                home_dir: self.home_dir.clone(),
                current_dir: self.current_dir.clone(),
                actor_override: Some(TEST_SENDER.parse().expect("actor")),
                team_override: Some(TEST_TEAM.parse().expect("team")),
                message_id,
                reply_body: reply_body.to_string(),
            }
        }

        fn read_query(&self) -> ReadQuery {
            ReadQuery::new(
                self.home_dir.clone(),
                self.current_dir.clone(),
                Some(TEST_SENDER),
                Some(TEST_RECIPIENT_ADDRESS),
                Some(TEST_TEAM),
                ReadSelection::All,
                false,
                false,
                AckActivationMode::ReadOnly,
                None,
                None,
                None,
                None,
            )
            .expect("read query")
        }

        fn clear_query(&self) -> ClearQuery {
            ClearQuery {
                home_dir: self.home_dir.clone(),
                current_dir: self.current_dir.clone(),
                actor_override: Some(TEST_SENDER.parse().expect("actor")),
                target_address: Some(TEST_RECIPIENT_ADDRESS.parse().expect("recipient")),
                team_override: Some(TEST_TEAM.parse().expect("team")),
                older_than: None,
                idle_only: false,
                dry_run: false,
            }
        }

        fn doctor_query(&self) -> DoctorQuery {
            DoctorQuery {
                home_dir: self.home_dir.clone(),
                current_dir: self.current_dir.clone(),
                team_override: Some(TEST_TEAM.parse().expect("team")),
            }
        }

        fn message(&self, text: &str, read: bool) -> MessageEnvelope {
            MessageEnvelope {
                from: TEST_LEAD.parse().expect("lead"),
                text: text.to_string(),
                timestamp: Utc::now().into(),
                read,
                source_team: Some(TEST_TEAM.parse().expect("team")),
                summary: None,
                message_id: None,
                pending_ack_at: None,
                acknowledged_at: None,
                acknowledges_message_id: None,
                parent_message_id: None,
                thread_mode: None,
                stale_at: None,
                task_id: None,
                extra: serde_json::Map::new(),
            }
        }

        fn pending_ack_message(&self, text: &str) -> (LegacyMessageId, MessageEnvelope) {
            let message_id = LegacyMessageId::new();
            let mut message = self.message(text, true);
            message.message_id = Some(message_id);
            message.pending_ack_at = Some(Utc::now().into());
            (message_id, message)
        }
    }

    #[test]
    fn fake_transport_maps_protocol_error_envelope_to_atm_error() {
        let tempdir = TempDir::new().expect("tempdir");
        let observability = CliObservability::fallback();
        let transport = Arc::new(FakeClientTransport::new(|_| {
            Ok(ResponseEnvelope::Error(ProtocolErrorEnvelope::from_error(
                &AtmError::daemon_unavailable("synthetic daemon failure")
                    .with_recovery("retry after the daemon is reachable"),
            )))
        }));
        let composition = CliComposition::from_transport(transport, &observability);

        let error = composition
            .send_request(RequestEnvelope::Doctor(atm_core::doctor::DoctorQuery {
                home_dir: tempdir.path().join("home"),
                current_dir: tempdir.path().join("cwd"),
                team_override: None,
            }))
            .expect_err("protocol error");

        assert_eq!(
            error.code,
            atm_core::error_codes::AtmErrorCode::DaemonUnavailable
        );
        assert!(error.to_string().contains("synthetic daemon failure"));
        assert_eq!(
            error.recovery.as_deref(),
            Some("retry after the daemon is reachable")
        );
    }

    #[test]
    fn loopback_transport_send_persists_inbox_without_daemon() {
        let fixture = LoopbackFixture::new(TEST_RECIPIENT);
        let transport_observability = Arc::new(atm_core::observability::NullObservability);
        let composition_observability = CliObservability::fallback();
        let composition = CliComposition::from_transport(
            Arc::new(LoopbackClientTransport::new(transport_observability)),
            &composition_observability,
        );

        let outcome = composition
            .send(fixture.send_request("hello from loopback"))
            .expect("send outcome");

        assert_eq!(outcome.agent.as_str(), TEST_RECIPIENT);
        assert_eq!(outcome.sender.as_str(), TEST_SENDER);
        let inbox = fixture.inbox_contents(TEST_RECIPIENT);
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0].text, "hello from loopback");
        assert_eq!(inbox[0].from.as_str(), TEST_SENDER);
    }

    #[test]
    fn loopback_transport_missing_config_notice_retains_at_most_one_team_lead_message_under_concurrency()
     {
        let fixture = LoopbackFixture::new(TEST_RECIPIENT);
        fs::remove_file(fixture.team_dir().join("config.json")).expect("remove config");
        fixture.write_inbox_values(TEST_RECIPIENT, &[]);
        fixture.write_inbox_values(ROLE_TEAM_LEAD, &[]);

        let transport = Arc::new(LoopbackClientTransport::new(Arc::new(
            atm_core::observability::NullObservability,
        )));
        let (first, second) = std::thread::scope(|scope| {
            let first_request = fixture.send_request("first");
            let second_request = fixture.send_request("second");
            let first_transport = transport.clone();
            let second_transport = transport.clone();
            let first = scope.spawn(move || {
                first_transport.send(RequestEnvelope::Send(SendRequestEnvelope::Compose(
                    first_request,
                )))
            });
            let second = scope.spawn(move || {
                second_transport.send(RequestEnvelope::Send(SendRequestEnvelope::Compose(
                    second_request,
                )))
            });
            (
                first.join().expect("first transport result"),
                second.join().expect("second transport result"),
            )
        });

        assert!(first.is_ok(), "first response: {first:?}");
        assert!(second.is_ok(), "second response: {second:?}");
        let notices = fixture.inbox_contents(ROLE_TEAM_LEAD);
        assert!(
            notices.len() <= 1,
            "loopback missing-config fallback should retain at most one notice; got {}",
            notices.len()
        );
    }

    #[test]
    fn loopback_transport_send_preserves_ack_and_task_metadata_without_daemon() {
        let fixture = LoopbackFixture::new(TEST_RECIPIENT);
        let composition_observability = CliObservability::fallback();
        let composition = CliComposition::from_transport(
            Arc::new(LoopbackClientTransport::new(Arc::new(
                atm_core::observability::NullObservability,
            ))),
            &composition_observability,
        );

        let outcome = composition
            .send(fixture.send_request_with_flags(
                "needs acknowledgement",
                false,
                Some("TASK-314".parse().expect("task id")),
            ))
            .expect("send outcome");

        assert!(outcome.requires_ack);
        assert_eq!(
            outcome.task_id.as_ref().map(|value| value.as_str()),
            Some("TASK-314")
        );

        let inbox = fixture.inbox_contents(TEST_RECIPIENT);
        assert_eq!(inbox.len(), 1);
        assert_eq!(
            inbox[0].task_id.as_ref().map(|value| value.as_str()),
            Some("TASK-314")
        );
        assert!(inbox[0].pending_ack_at.is_some());
    }

    #[test]
    fn loopback_transport_read_surfaces_messages_without_daemon() {
        let fixture = LoopbackFixture::new(TEST_RECIPIENT);
        fixture.write_inbox_messages(TEST_RECIPIENT, &[fixture.message("read me", false)]);
        let composition_observability = CliObservability::fallback();
        let composition = CliComposition::from_transport(
            Arc::new(LoopbackClientTransport::new(Arc::new(
                atm_core::observability::NullObservability,
            ))),
            &composition_observability,
        );

        let outcome = composition
            .receive(fixture.read_query())
            .expect("read outcome");

        assert_eq!(outcome.agent.as_str(), TEST_RECIPIENT);
        assert_eq!(outcome.count, 1);
        assert_eq!(outcome.messages[0].envelope.text, "read me");
    }

    #[test]
    fn loopback_transport_clear_removes_read_messages_without_daemon() {
        let fixture = LoopbackFixture::new(TEST_RECIPIENT);
        fixture.write_inbox_messages(TEST_RECIPIENT, &[fixture.message("done", true)]);
        let composition_observability = CliObservability::fallback();
        let composition = CliComposition::from_transport(
            Arc::new(LoopbackClientTransport::new(Arc::new(
                atm_core::observability::NullObservability,
            ))),
            &composition_observability,
        );

        let outcome = composition
            .clear(fixture.clear_query())
            .expect("clear outcome");

        assert_eq!(outcome.removed_total, 1);
        assert_eq!(outcome.remaining_total, 0);
        assert!(fixture.inbox_contents(TEST_RECIPIENT).is_empty());
    }

    #[test]
    fn loopback_transport_doctor_reports_health_without_daemon() {
        let fixture = LoopbackFixture::new(TEST_RECIPIENT);
        let observability = Arc::new(HealthyObservability);
        let composition_observability = CliObservability::fallback();
        let composition = CliComposition::from_transport(
            Arc::new(LoopbackClientTransport::new(observability)),
            &composition_observability,
        );

        let report = composition
            .doctor(fixture.doctor_query())
            .expect("doctor report");

        assert_eq!(report.summary.status, DoctorStatus::Healthy);
        assert_eq!(report.summary.error_count, 0);
    }

    #[test]
    fn loopback_transport_ack_appends_reply_without_daemon() {
        let fixture = LoopbackFixture::new(TEST_RECIPIENT);
        let (message_id, pending_ack) = fixture.pending_ack_message("please ack");
        fixture.write_inbox_messages(TEST_SENDER, &[pending_ack]);
        let composition_observability = CliObservability::fallback();
        let composition = CliComposition::from_transport(
            Arc::new(LoopbackClientTransport::new(Arc::new(
                atm_core::observability::NullObservability,
            ))),
            &composition_observability,
        );

        let outcome = composition
            .ack(fixture.ack_request(message_id, "received and starting"))
            .expect("ack outcome");

        assert_eq!(outcome.team.as_str(), TEST_TEAM);
        assert_eq!(outcome.agent.as_str(), TEST_SENDER);
        assert_eq!(outcome.message_id, message_id);
        assert_eq!(
            outcome.reply_target.to_string(),
            format!("{TEST_LEAD}@{TEST_TEAM}")
        );

        let sender_inbox = fixture.inbox_contents(TEST_SENDER);
        assert_eq!(sender_inbox.len(), 1);
        assert!(sender_inbox[0].pending_ack_at.is_some());
        assert!(sender_inbox[0].acknowledged_at.is_none());
        let replies = fixture.inbox_contents(TEST_LEAD);
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].text, "received and starting");
        assert_eq!(replies[0].acknowledges_message_id, Some(message_id));
        assert!(replies[0].pending_ack_at.is_none());
    }

    #[test]
    fn daemon_path_newtypes_reject_empty_paths_and_preserve_path_access() {
        let tempdir = TempDir::new().expect("tempdir");
        let socket_path = tempdir.path().join("atm.sock");
        let daemon_path = tempdir.path().join("atm-daemon");
        let socket = DaemonLocalIpcEndpoint::new(socket_path.clone()).expect("socket path");
        let daemon = DaemonBinaryPath::new(daemon_path.clone()).expect("daemon path");

        assert_eq!(socket.as_ref(), socket_path.as_path());
        assert_eq!(daemon.as_ref(), daemon_path.as_path());

        let socket_error =
            DaemonLocalIpcEndpoint::new(std::path::PathBuf::new()).expect_err("empty");
        assert!(
            socket_error
                .to_string()
                .contains("daemon local IPC endpoint")
        );

        let daemon_error = DaemonBinaryPath::new(std::path::PathBuf::new()).expect_err("empty");
        assert!(daemon_error.to_string().contains("daemon binary path"));
    }

    #[test]
    fn launch_gate_is_host_wide_across_different_socket_paths() {
        let tempdir = TempDir::new().expect("tempdir");
        let runtime_dir = atm_core::home::host_runtime_dir_from_home(tempdir.path());
        let first =
            LaunchGateGuard::try_acquire_at(runtime_dir.join(HOST_RUNTIME_LAUNCH_LOCK_FILE))
                .expect("first acquire");
        assert!(first.is_some());
        let second =
            LaunchGateGuard::try_acquire_at(runtime_dir.join(HOST_RUNTIME_LAUNCH_LOCK_FILE))
                .expect("second acquire");
        assert!(second.is_none());
        drop(first);
    }

    #[test]
    fn launch_gate_is_host_wide_across_different_atm_home_roots() {
        let tempdir = TempDir::new().expect("tempdir");
        let user_home = tempdir.path().join("user-home");
        let runtime_dir = atm_core::home::host_runtime_dir_from_home(&user_home);
        let first_atm_home = user_home.join("workspace-a");
        let second_atm_home = user_home.join("workspace-b");
        let first_socket = first_atm_home.join(".atm").join("daemon.sock");
        let second_socket = second_atm_home.join(".atm").join("daemon.sock");

        let first =
            LaunchGateGuard::try_acquire_at(runtime_dir.join(HOST_RUNTIME_LAUNCH_LOCK_FILE))
                .expect("first acquire");
        assert!(first.is_some());
        let second =
            LaunchGateGuard::try_acquire_at(runtime_dir.join(HOST_RUNTIME_LAUNCH_LOCK_FILE))
                .expect("second acquire");
        assert!(
            second.is_none(),
            "different ATM_HOME roots must share one launch gate"
        );
        assert_ne!(first_socket, second_socket);
        drop(first);
    }

    #[test]
    fn host_runtime_lock_path_ignores_atm_home() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = atm_core::home::host_runtime_lock_path_from_home(
            tempdir.path(),
            HOST_RUNTIME_LAUNCH_LOCK_FILE,
        );

        assert_eq!(
            path,
            tempdir
                .path()
                .join(".atm")
                .join("daemon")
                .join("launch.lock")
        );
    }

    #[test]
    fn launch_gate_busy_maps_to_typed_rejection() {
        let tempdir = TempDir::new().expect("tempdir");
        let runtime_dir = atm_core::home::host_runtime_dir_from_home(tempdir.path());
        let _gate =
            LaunchGateGuard::try_acquire_at(runtime_dir.join(HOST_RUNTIME_LAUNCH_LOCK_FILE))
                .expect("acquire")
                .expect("gate");
        let socket_path =
            DaemonLocalIpcEndpoint::new(tempdir.path().join("one.sock")).expect("socket");
        let second =
            LaunchGateGuard::try_acquire_at(runtime_dir.join(HOST_RUNTIME_LAUNCH_LOCK_FILE))
                .expect("second acquire");

        assert!(second.is_none());
        let error = LaunchGateGuard::rejected_error(&socket_path);

        assert_eq!(
            error.code,
            atm_core::error_codes::AtmErrorCode::DaemonLaunchGateRejected
        );
    }

    #[test]
    fn gate_timeout_maps_to_launch_gate_rejected() {
        let tempdir = TempDir::new().expect("tempdir");
        let runtime_dir = atm_core::home::host_runtime_dir_from_home(tempdir.path());
        let launch_lock_path = runtime_dir.join(HOST_RUNTIME_LAUNCH_LOCK_FILE);
        let _gate = LaunchGateGuard::try_acquire_at(launch_lock_path.clone())
            .expect("acquire")
            .expect("gate");
        let socket_path =
            DaemonLocalIpcEndpoint::new(tempdir.path().join("missing.sock")).expect("socket");
        let daemon_bin = DaemonBinaryPath::new(tempdir.path().join("atm-daemon")).expect("daemon");
        let supervisor = DaemonSupervisor::new(socket_path.clone(), daemon_bin);
        let transport = LocalIpcClientTransportAdapter::new(socket_path);

        let error = supervisor
            .ensure_daemon_available_with_lock_path(
                &transport,
                Duration::from_millis(0),
                Duration::from_millis(0),
                launch_lock_path,
            )
            .expect_err("timeout should fail");

        assert_eq!(
            error.code,
            atm_core::error_codes::AtmErrorCode::DaemonLaunchGateRejected
        );
    }

    #[test]
    fn spawn_failure_maps_to_auto_start_failed() {
        use std::fs;

        let tempdir = TempDir::new().expect("tempdir");
        let runtime_dir = atm_core::home::host_runtime_dir_from_home(tempdir.path());
        let launch_lock_path = runtime_dir.join(HOST_RUNTIME_LAUNCH_LOCK_FILE);
        let socket_path =
            DaemonLocalIpcEndpoint::new(tempdir.path().join("missing.sock")).expect("socket");
        let daemon_path = tempdir.path().join(if cfg!(windows) {
            "invalid-atm-daemon.exe"
        } else {
            "invalid-atm-daemon"
        });
        fs::write(&daemon_path, b"not an executable daemon binary").expect("write daemon");
        let daemon_bin = DaemonBinaryPath::new(daemon_path).expect("daemon");
        let supervisor = DaemonSupervisor::new(socket_path.clone(), daemon_bin);
        let transport = LocalIpcClientTransportAdapter::new(socket_path);

        let error = supervisor
            .ensure_daemon_available_with_lock_path(
                &transport,
                Duration::from_millis(10),
                Duration::from_millis(0),
                launch_lock_path,
            )
            .expect_err("spawn should fail");

        assert_eq!(
            error.code,
            atm_core::error_codes::AtmErrorCode::DaemonAutoStartFailed
        );
    }

    #[cfg(unix)]
    #[test]
    // ADR-003 Tier 2: Unix-only non-UTF-8 path construction uses OsStringExt, which does
    // not have a portable cross-platform equivalent for this exact boundary case.
    fn daemon_path_newtypes_reject_non_utf8_paths_at_boundary() {
        use std::os::unix::ffi::OsStringExt;

        let invalid = std::ffi::OsString::from_vec(vec![0x66, 0x6f, 0x80]);

        let socket_error = DaemonLocalIpcEndpoint::new(std::path::PathBuf::from(invalid.clone()))
            .expect_err("utf8");
        assert!(socket_error.to_string().contains("valid UTF-8"));

        let daemon_error =
            DaemonBinaryPath::new(std::path::PathBuf::from(invalid)).expect_err("utf8");
        assert!(daemon_error.to_string().contains("valid UTF-8"));
    }
}
