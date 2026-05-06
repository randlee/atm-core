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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;

    use atm_core::boundary::{self, ClientTransport};
    use atm_core::error::AtmError;
    use atm_core::protocol::{
        ProtocolErrorEnvelope, RequestEnvelope, ResponseEnvelope, SendRequestEnvelope,
        SendResponseEnvelope,
    };
    use atm_core::schema::{
        AgentMember, MessageEnvelope, TeamConfig, hydrate_legacy_fields_from_metadata,
    };
    use atm_core::send::{SendMessageSource, SendRequest};
    use atm_core::test_support::{
        ROLE_TEAM_LEAD, TEST_LEAD, TEST_RECIPIENT, TEST_RECIPIENT_ADDRESS, TEST_SENDER, TEST_TEAM,
    };
    use serde_json::Value;
    use tempfile::TempDir;

    use super::CliComposition;
    use crate::observability::CliObservability;

    #[derive(Clone)]
    struct FakeClientTransport {
        handler: Arc<dyn Fn(RequestEnvelope) -> Result<ResponseEnvelope, AtmError> + Send + Sync>,
    }

    impl std::fmt::Debug for FakeClientTransport {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("FakeClientTransport")
                .finish_non_exhaustive()
        }
    }

    impl FakeClientTransport {
        fn new<F>(handler: F) -> Self
        where
            F: Fn(RequestEnvelope) -> Result<ResponseEnvelope, AtmError> + Send + Sync + 'static,
        {
            Self {
                handler: Arc::new(handler),
            }
        }
    }

    impl boundary::sealed::Sealed for FakeClientTransport {}

    impl ClientTransport for FakeClientTransport {
        fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
            (self.handler)(request)
        }
    }

    #[derive(Debug, Default)]
    struct LoopbackClientTransport;

    impl boundary::sealed::Sealed for LoopbackClientTransport {}

    impl ClientTransport for LoopbackClientTransport {
        fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
            let observability = atm_core::observability::NullObservability;
            match request {
                RequestEnvelope::Send(SendRequestEnvelope::Compose(request)) => {
                    atm_core::send::send_mail(request, &observability)
                        .map(|outcome| ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)))
                }
                RequestEnvelope::Send(SendRequestEnvelope::Acknowledge(request)) => {
                    atm_core::ack::ack_mail(request, &observability).map(|outcome| {
                        ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(outcome))
                    })
                }
                RequestEnvelope::Receive(query) => {
                    atm_core::read::read_mail(query, &observability).map(ResponseEnvelope::Receive)
                }
                RequestEnvelope::Clear(query) => {
                    atm_core::clear::clear_mail(query, &observability).map(ResponseEnvelope::Clear)
                }
                RequestEnvelope::Doctor(query) => {
                    atm_core::doctor::run_doctor(query, &observability)
                        .map(ResponseEnvelope::Doctor)
                }
            }
        }
    }

    struct LoopbackFixture {
        tempdir: TempDir,
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
                tempdir,
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
            let config = TeamConfig {
                members: vec![
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
    }

    #[test]
    fn fake_transport_maps_protocol_error_envelope_to_atm_error() {
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
                home_dir: std::env::temp_dir(),
                current_dir: std::env::temp_dir(),
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
        let observability = CliObservability::fallback();
        let composition =
            CliComposition::from_transport(Arc::new(LoopbackClientTransport), &observability);

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
    fn loopback_transport_missing_config_notice_retains_at_most_two_team_lead_messages_under_concurrency()
     {
        let fixture = LoopbackFixture::new(TEST_RECIPIENT);
        fs::remove_file(fixture.team_dir().join("config.json")).expect("remove config");
        fixture.write_inbox_values(TEST_RECIPIENT, &[]);
        fixture.write_inbox_values(ROLE_TEAM_LEAD, &[]);

        let transport = Arc::new(LoopbackClientTransport);
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
            notices.len() <= 2,
            "loopback missing-config fallback should retain at most two notices; got {}",
            notices.len()
        );
    }
}
