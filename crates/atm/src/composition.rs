#![allow(
    deprecated,
    reason = "the retained CLI composition still seeds and bridges legacy atm-core boundary stores during the Phase AC transition"
)]

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Once};

use atm_core::ack::{AckOutcome, AckRequest, prepare_ack_send_request};
use atm_core::boundary;
use atm_core::boundary::ClientTransport;
use atm_core::clear::{ClearOutcome, ClearQuery};
use atm_core::doctor::{BootstrapTraceReport, DoctorQuery, DoctorReport};
use atm_core::error::AtmError;
use atm_core::graft::AtmGraftClient;
use atm_core::home;
use atm_core::list::{ListOutcome, ListQuery};
use atm_core::observability::{CommandEvent, ObservabilityPort, action_name, outcome_label};
use atm_core::protocol::{self, CompatibilityPreflight, RequestEnvelope, ResponseEnvelope};
use atm_core::read::{PeekQuery, ReadOutcome, ReadQuery};
use atm_core::send::{SendOutcome, SendRequest};
#[cfg(not(test))]
use atm_daemon_bootstrap::install_sqlite_retained_runtime_factory;
use atm_daemon_client::{
    BootstrapCommandEvent, BootstrapTraceability, DaemonLocalIpcEndpoint, DaemonSupervisor,
    FramePayload, MessageKind, RequestId as DaemonRequestId, RpcEnvelope,
    exchange_envelope as daemon_exchange_envelope, parse_bootstrap_agent, parse_bootstrap_team,
    resolve_daemon_bin, resolve_daemon_local_ipc_endpoint, try_connect as daemon_try_connect,
    unexpected_response,
};
#[cfg(test)]
use atm_daemon_client::{HOST_RUNTIME_LAUNCH_LOCK_FILE, LaunchGateGuard};
#[cfg(test)]
use atm_runtime_test_support::install_sqlite_retained_runtime_factory as install_test_runtime_factory;

use crate::observability::CliObservability;

const SAME_HOST_REQUEST_DEADLINE: std::time::Duration = std::time::Duration::from_secs(3);
static INSTALL_RETAINED_RUNTIME_FACTORY: Once = Once::new();

#[cfg(not(test))]
fn install_retained_runtime_factory() {
    INSTALL_RETAINED_RUNTIME_FACTORY.call_once(|| {
        install_sqlite_retained_runtime_factory();
    });
}

#[cfg(test)]
fn install_retained_runtime_factory() {
    INSTALL_RETAINED_RUNTIME_FACTORY.call_once(|| {
        install_test_runtime_factory();
    });
}

// ARCH: reserved for future command-routing phase — entry-point types hold
// per-command policy once send/receive gain context-sensitive dispatch logic.
#[derive(Debug, Default)]
pub(crate) struct SendCommandEntryPoint;

impl SendCommandEntryPoint {
    pub(crate) const fn new() -> Self {
        Self
    }
}

// ARCH: reserved for future command-routing phase — symmetric pair with
// SendCommandEntryPoint for receive-side dispatch policy.
#[derive(Debug, Default)]
pub(crate) struct ReceiveCommandEntryPoint;

impl ReceiveCommandEntryPoint {
    pub(crate) const fn new() -> Self {
        Self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CliBootstrapError {
    AtmHomeUnresolved { command: &'static str },
}

impl CliBootstrapError {
    fn into_atm_error(self) -> AtmError {
        match self {
            Self::AtmHomeUnresolved { command } => AtmError::atm_home_unresolved(format!(
                "failed to resolve ATM_HOME before bootstrapping `atm {command}`"
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InvocationDir<'a>(&'a Path);

impl<'a> InvocationDir<'a> {
    pub(crate) fn new(path: &'a Path) -> Self {
        Self(path)
    }

    fn as_path(self) -> &'a Path {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AtmHomePath<'a>(&'a Path);

impl<'a> AtmHomePath<'a> {
    pub(crate) fn new(path: &'a Path) -> Self {
        Self(path)
    }

    fn as_path(self) -> &'a Path {
        self.0
    }
}

pub(crate) fn resolve_command_runtime_context(
    command: &'static str,
) -> Result<(PathBuf, PathBuf), AtmError> {
    let invocation_dir = home::command_invocation_dir().inspect_err(|error| {
        log_runtime_root_failure(command, error);
    })?;
    let atm_home = home::atm_home().map_err(|source| {
        let error = CliBootstrapError::AtmHomeUnresolved { command }
            .into_atm_error()
            .with_source(source);
        log_runtime_root_failure(command, &error);
        error
    })?;
    Ok((atm_home, invocation_dir))
}

fn log_runtime_root_failure(command: &'static str, error: &AtmError) {
    tracing::error!(
        command,
        error_code = %error.code.as_str(),
        error = %error,
        "raw cli runtime-root failure"
    );
}

#[derive(Debug)]
struct LocalIpcClientTransportAdapter {
    endpoint: DaemonLocalIpcEndpoint,
}

impl LocalIpcClientTransportAdapter {
    fn new(endpoint: DaemonLocalIpcEndpoint) -> Self {
        Self { endpoint }
    }

    fn probe_connection(&self) -> Result<interprocess::local_socket::Stream, AtmError> {
        daemon_try_connect(&self.endpoint)
    }

    /// This function performs blocking IPC I/O on the synchronous ATM CLI path.
    fn round_trip(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        let envelope = encode_request_envelope(request.clone())?;
        let response = if request_requires_compatibility_verification(&request) {
            let mut verified = atm_daemon_client::verify_connection_compatibility(
                &self.endpoint,
                CompatibilityPreflight {
                    client_release: atm_daemon_client::ReleaseVersion::current(),
                    wire_version: protocol::ATM_FRAME_VERSION_V1,
                },
                SAME_HOST_REQUEST_DEADLINE,
            )?;
            verified.dispatch_write(&self.endpoint, envelope, SAME_HOST_REQUEST_DEADLINE)?
        } else {
            daemon_exchange_envelope(&self.endpoint, envelope, SAME_HOST_REQUEST_DEADLINE)?
        };
        decode_response_envelope(response)
    }
}

fn request_requires_compatibility_verification(request: &RequestEnvelope) -> bool {
    matches!(
        request,
        RequestEnvelope::Send(_) | RequestEnvelope::Clear(_)
    )
}

impl boundary::sealed::Sealed for LocalIpcClientTransportAdapter {}

impl ClientTransport for LocalIpcClientTransportAdapter {
    fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        self.round_trip(request)
    }
}

pub(crate) struct CliComposition<'a> {
    transport: Arc<dyn ClientTransport + Send + Sync + 'a>,
    observability_port: &'a CliObservability,
    bootstrap_trace: Option<BootstrapTraceReport>,
    send_command: SendCommandEntryPoint,
    receive_command: ReceiveCommandEntryPoint,
}

impl fmt::Debug for CliComposition<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CliComposition")
            .field("transport", &"dyn ClientTransport")
            .field("observability_port", &"dyn ObservabilityPort")
            .field("bootstrap_trace", &self.bootstrap_trace)
            .field("send_command", &self.send_command)
            .field("receive_command", &self.receive_command)
            .finish()
    }
}

impl<'a> CliComposition<'a> {
    pub(crate) fn from_transport(
        transport: Arc<dyn ClientTransport + Send + Sync + 'a>,
        observability_port: &'a CliObservability,
    ) -> Self {
        install_retained_runtime_factory();
        Self {
            transport,
            observability_port,
            bootstrap_trace: None,
            send_command: SendCommandEntryPoint::new(),
            receive_command: ReceiveCommandEntryPoint::new(),
        }
    }

    #[expect(
        dead_code,
        reason = "reserved for future phase that inspects the active transport variant"
    )]
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

    #[expect(
        dead_code,
        reason = "reserved for future phase that threads observability port into command helpers"
    )]
    pub(crate) fn observability_port(&self) -> &(dyn ObservabilityPort + Send + Sync) {
        self.observability_port
    }

    #[expect(
        dead_code,
        reason = "reserved for future command-routing phase — exposes send entry-point to callers"
    )]
    pub(crate) fn send_command(&self) -> &SendCommandEntryPoint {
        &self.send_command
    }

    #[expect(
        dead_code,
        reason = "reserved for future command-routing phase — exposes receive entry-point to callers"
    )]
    pub(crate) fn receive_command(&self) -> &ReceiveCommandEntryPoint {
        &self.receive_command
    }

    pub(crate) fn send(&self, request: SendRequest) -> Result<SendOutcome, AtmError> {
        match self.send_request(RequestEnvelope::Send(Box::new(request)))? {
            ResponseEnvelope::Send(outcome) => {
                self.observability_port.emit_command_event(CommandEvent {
                    command: "send",
                    action: action_name("send"),
                    outcome: outcome_label(match outcome.outcome {
                        atm_core::send::SendCommandOutcome::Sent => "sent",
                        atm_core::send::SendCommandOutcome::Deferred => "deferred",
                        atm_core::send::SendCommandOutcome::DryRun => "dry_run",
                    }),
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
        let ack_request = request.clone();
        match self.send_request(RequestEnvelope::Send(Box::new(prepare_ack_send_request(
            request,
        )?)))? {
            ResponseEnvelope::Send(outcome) => {
                self.observability_port.emit_command_event(CommandEvent {
                    command: "ack",
                    action: action_name("ack"),
                    outcome: outcome_label("ok"),
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
                Ok(AckOutcome::from_send_outcome(outcome, &ack_request))
            }
            other => Err(unexpected_response("ack", other)),
        }
    }

    pub(crate) fn receive(&self, query: ReadQuery) -> Result<ReadOutcome, AtmError> {
        match self.send_request(RequestEnvelope::Receive(query))? {
            ResponseEnvelope::Receive(outcome) => {
                self.observability_port.emit_command_event(CommandEvent {
                    command: "read",
                    action: action_name("read"),
                    outcome: outcome_label("ok"),
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
                Ok(*outcome)
            }
            other => Err(unexpected_response("receive", other)),
        }
    }

    pub(crate) fn peek(&self, query: PeekQuery) -> Result<ReadOutcome, AtmError> {
        match self.send_request(RequestEnvelope::Peek(query))? {
            ResponseEnvelope::Peek(outcome) => {
                self.observability_port.emit_command_event(CommandEvent {
                    command: "peek",
                    action: action_name("peek"),
                    outcome: outcome_label("ok"),
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
                Ok(*outcome)
            }
            other => Err(unexpected_response("peek", other)),
        }
    }

    pub(crate) fn list(&self, query: ListQuery) -> Result<ListOutcome, AtmError> {
        match self.send_request(RequestEnvelope::List(query))? {
            ResponseEnvelope::List(outcome) => {
                self.observability_port.emit_command_event(CommandEvent {
                    command: "list",
                    action: action_name("list"),
                    outcome: outcome_label("ok"),
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
            other => Err(unexpected_response("list", other)),
        }
    }

    pub(crate) fn clear(&self, query: ClearQuery) -> Result<ClearOutcome, AtmError> {
        match self.send_request(RequestEnvelope::Clear(query))? {
            ResponseEnvelope::Clear(outcome) => {
                self.observability_port.emit_command_event(CommandEvent {
                    command: "clear",
                    action: action_name("clear"),
                    outcome: outcome_label("ok"),
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

    #[allow(
        dead_code,
        reason = "AA.3 restores the direct local doctor path in DoctorCommand, but the daemon-routed doctor request seam remains covered by transport tests."
    )]
    pub(crate) fn doctor(&self, query: DoctorQuery) -> Result<DoctorReport, AtmError> {
        match self.send_request(RequestEnvelope::Doctor(query))? {
            ResponseEnvelope::Doctor(mut report) => {
                report.bootstrap_trace = self.bootstrap_trace.clone();
                Ok(*report)
            }
            other => Err(unexpected_response("doctor", other)),
        }
    }

    pub(crate) fn bootstrap(
        command: &'static str,
        observability: &'a CliObservability,
        invocation_dir: InvocationDir<'_>,
        atm_home: AtmHomePath<'_>,
    ) -> Result<Self, AtmError> {
        let _invocation_dir = invocation_dir.as_path();
        let _atm_home = atm_home.as_path();
        let endpoint = resolve_daemon_local_ipc_endpoint().inspect_err(|error| {
            log_runtime_root_failure(command, error);
        })?;
        let daemon_bin = resolve_daemon_bin("atm")?;
        let transport = Arc::new(LocalIpcClientTransportAdapter::new(endpoint.clone()));
        let supervisor = DaemonSupervisor::new(endpoint, daemon_bin);
        let emit_bootstrap_event = |event: BootstrapCommandEvent| {
            observability.emit(CommandEvent {
                command: event.command,
                action: action_name(event.action),
                outcome: outcome_label(event.outcome),
                team: event.team,
                agent: event.agent.clone(),
                sender: event.agent,
                message_id: None,
                requires_ack: false,
                dry_run: false,
                task_id: None,
                error_code: event.error_code,
                error_message: event.error_message,
            })
        };
        let traceability = BootstrapTraceability::new(
            command,
            &emit_bootstrap_event,
            parse_bootstrap_team()?,
            parse_bootstrap_agent()?,
        );
        supervisor.ensure_daemon_available_with_traceability(&traceability, || {
            transport.probe_connection().map(|_| ())
        })?;
        let mut composition = Self::from_transport(transport, observability);
        composition.bootstrap_trace = Some(traceability.snapshot());
        Ok(composition)
    }
}

fn encode_request_envelope(request: RequestEnvelope) -> Result<RpcEnvelope, AtmError> {
    let request_id = protocol::next_request_id();
    let frame = protocol::request_to_frame_payload(request_id, request)?;
    Ok(RpcEnvelope::from_frame_payload(encode_daemon_frame(frame)?))
}

fn decode_response_envelope(envelope: RpcEnvelope) -> Result<ResponseEnvelope, AtmError> {
    let frame = decode_daemon_frame(envelope.into_frame_payload())?;
    let (_, response) = protocol::response_from_frame_payload(frame)?;
    Ok(response)
}

fn encode_daemon_frame(frame: protocol::FramePayload) -> Result<FramePayload, AtmError> {
    Ok(FramePayload {
        request_id: DaemonRequestId::new(frame.request_id.into_inner())?,
        message_kind: MessageKind::try_from(frame.message_kind.code())?,
        flags: frame.flags,
        bytes: frame.bytes,
    })
}

fn decode_daemon_frame(frame: FramePayload) -> Result<protocol::FramePayload, AtmError> {
    Ok(protocol::FramePayload {
        request_id: protocol::RequestId::new(frame.request_id.into_inner())?,
        message_kind: protocol::MessageKind::try_from(frame.message_kind.code())?,
        flags: frame.flags,
        bytes: frame.bytes,
    })
}

impl AtmGraftClient for CliComposition<'_> {
    fn send_message(&self, request: SendRequest) -> Result<SendOutcome, AtmError> {
        CliComposition::send(self, request)
    }

    fn read_message(&self, query: ReadQuery) -> Result<ReadOutcome, AtmError> {
        CliComposition::receive(self, query)
    }

    fn acknowledge_message(&self, request: AckRequest) -> Result<AckOutcome, AtmError> {
        CliComposition::ack(self, request)
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::io::Write;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::time::Duration;

    use atm_core::error::AtmError;
    use atm_core::protocol::{ProtocolErrorEnvelope, RequestEnvelope, ResponseEnvelope};
    use atm_core::test_support::{EnvGuard, TEST_SENDER, TEST_TEAM};
    use atm_core::transport::testing::FakeClientTransport;
    use atm_daemon_client::DaemonBinaryPath;
    use serial_test::serial;
    use tempfile::TempDir;
    use tracing_subscriber::fmt::MakeWriter;

    use super::{
        AtmHomePath, CliComposition, DaemonLocalIpcEndpoint, DaemonSupervisor,
        HOST_RUNTIME_LAUNCH_LOCK_FILE, InvocationDir, LaunchGateGuard,
        LocalIpcClientTransportAdapter, resolve_command_runtime_context,
    };
    use crate::observability::CliObservability;

    struct CwdGuard {
        original: std::path::PathBuf,
    }

    impl CwdGuard {
        fn change_to(path: &std::path::Path) -> Self {
            let original = atm_core::home::command_invocation_dir().expect("current dir");
            std::env::set_current_dir(path).expect("set current dir");
            Self { original }
        }
    }

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.original).expect("restore current dir");
        }
    }

    #[derive(Clone, Default)]
    struct SharedLogBuffer(Arc<Mutex<Vec<u8>>>);

    struct SharedLogWriter(SharedLogBuffer);

    impl<'a> MakeWriter<'a> for SharedLogBuffer {
        type Writer = SharedLogWriter;

        fn make_writer(&'a self) -> Self::Writer {
            SharedLogWriter(self.clone())
        }
    }

    impl Write for SharedLogWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0
                .0
                .lock()
                .expect("log buffer lock")
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl SharedLogBuffer {
        fn contents(&self) -> String {
            String::from_utf8(self.0.lock().expect("log buffer lock").clone()).expect("utf8 logs")
        }
    }

    fn capture_runtime_root_logs<T>(f: impl FnOnce() -> T) -> (T, String) {
        let buffer = SharedLogBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .without_time()
            .with_writer(buffer.clone())
            .finish();
        let result = tracing::subscriber::with_default(subscriber, f);
        (result, buffer.contents())
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
                ..atm_core::doctor::DoctorQuery::default()
            }))
            .expect_err("protocol error");

        assert_eq!(
            error.code,
            atm_core::error_codes::AtmErrorCode::DaemonUnavailable
        );
        assert!(error.to_string().contains("synthetic daemon failure"));
        let recovery = error.primary_recovery().expect("daemon recovery");
        assert!(recovery.contains("atm-daemon binary is installed"));
        assert!(recovery.contains("daemon socket path is reachable"));
        assert!(recovery.contains("ATM_HOME are set correctly"));
    }

    #[test]
    fn bootstrap_propagates_daemon_availability_failure() {
        let tempdir = TempDir::new().expect("tempdir");
        let supervisor = DaemonSupervisor::new(
            DaemonLocalIpcEndpoint::new(tempdir.path().join("daemon.sock"))
                .expect("daemon endpoint"),
            DaemonBinaryPath::new(tempdir.path().join("missing-atm-daemon"))
                .expect("daemon binary path"),
        );

        let error = supervisor
            .ensure_daemon_available_with_lock_path(
                || Err(AtmError::daemon_unavailable("daemon not running for test")),
                Duration::from_millis(10),
                Duration::from_millis(1),
                tempdir.path().join(HOST_RUNTIME_LAUNCH_LOCK_FILE),
            )
            .expect_err("bootstrap should fail when daemon auto-start cannot launch");

        assert_eq!(
            error.code,
            atm_core::error_codes::AtmErrorCode::DaemonUnavailable
        );
        assert!(error.to_string().contains("daemon binary is missing"));
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
    fn host_runtime_lock_path_follows_the_explicit_home_root() {
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
    #[serial(env)]
    fn resolve_command_runtime_context_reuses_atm_home_across_sibling_worktrees() {
        let tempdir = TempDir::new().expect("tempdir");
        let atm_home = tempdir.path().join("atm-home");
        let workspace_a = tempdir.path().join("workspace-a");
        let workspace_b = tempdir.path().join("workspace-b");
        std::fs::create_dir_all(&atm_home).expect("atm home");
        std::fs::create_dir_all(&workspace_a).expect("workspace a");
        std::fs::create_dir_all(&workspace_b).expect("workspace b");
        let _env = EnvGuard::set_many([
            ("ATM_HOME", Some(atm_home.to_str().expect("utf8 atm home"))),
            ("HOME", Some(tempdir.path().to_str().expect("utf8 home"))),
            ("USERPROFILE", None),
        ]);

        let (home_a, invocation_a) = {
            let _cwd = CwdGuard::change_to(&workspace_a);
            resolve_command_runtime_context("send").expect("workspace a context")
        };
        let (home_b, invocation_b) = {
            let _cwd = CwdGuard::change_to(&workspace_b);
            resolve_command_runtime_context("send").expect("workspace b context")
        };
        let canonical_atm_home = std::fs::canonicalize(&atm_home).expect("canonical atm home");
        let canonical_workspace_a =
            std::fs::canonicalize(&workspace_a).expect("canonical workspace a");
        let canonical_workspace_b =
            std::fs::canonicalize(&workspace_b).expect("canonical workspace b");
        assert_eq!(
            std::fs::canonicalize(&home_a).expect("canonical home a"),
            canonical_atm_home
        );
        assert_eq!(
            std::fs::canonicalize(&home_b).expect("canonical home b"),
            canonical_atm_home
        );
        assert_eq!(
            std::fs::canonicalize(&invocation_a).expect("canonical invocation a"),
            canonical_workspace_a
        );
        assert_eq!(
            std::fs::canonicalize(&invocation_b).expect("canonical invocation b"),
            canonical_workspace_b
        );
        assert_eq!(
            atm_daemon_client::resolve_daemon_local_ipc_endpoint_from_home(&home_a)
                .expect("workspace a daemon endpoint")
                .as_ref(),
            atm_daemon_client::resolve_daemon_local_ipc_endpoint_from_home(&home_b)
                .expect("workspace b daemon endpoint")
                .as_ref()
        );
    }

    #[test]
    #[serial(env)]
    fn resolve_command_runtime_context_reports_atm_home_unresolved_and_logs() {
        let tempdir = TempDir::new().expect("tempdir");
        let workspace = tempdir.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("workspace");
        let _env = EnvGuard::set_many([("ATM_HOME", None), ("HOME", None), ("USERPROFILE", None)]);
        let (result, logs) = {
            let _cwd = CwdGuard::change_to(&workspace);
            capture_runtime_root_logs(|| resolve_command_runtime_context("send"))
        };

        let error = result.expect_err("missing ATM_HOME/home should fail");

        assert_eq!(
            error.code,
            atm_core::error_codes::AtmErrorCode::AtmHomeUnresolved
        );
        assert!(logs.contains("raw cli runtime-root failure"));
        assert!(logs.contains("ATM_HOME_UNRESOLVED"));
    }

    #[test]
    #[serial(env)]
    fn bootstrap_refuses_conflicting_daemon_socket_override() {
        let tempdir = TempDir::new().expect("tempdir");
        let atm_home = tempdir.path().join("atm-home");
        let invocation_dir = tempdir.path().join("workspace");
        std::fs::create_dir_all(&atm_home).expect("atm home");
        std::fs::create_dir_all(&invocation_dir).expect("invocation dir");
        let _env = EnvGuard::set_many([
            ("ATM_HOME", Some(atm_home.to_str().expect("utf8 atm home"))),
            (
                "ATM_DAEMON_SOCKET",
                Some(
                    tempdir
                        .path()
                        .join("other.sock")
                        .to_str()
                        .expect("utf8 socket"),
                ),
            ),
            ("ATM_IDENTITY", Some(TEST_SENDER)),
            ("ATM_TEAM", Some(TEST_TEAM)),
        ]);
        let observability = CliObservability::fallback();

        let (result, logs) = capture_runtime_root_logs(|| {
            CliComposition::bootstrap(
                "send",
                &observability,
                InvocationDir::new(&invocation_dir),
                AtmHomePath::new(&atm_home),
            )
        });
        let error = result.expect_err("conflicting daemon socket override should fail");

        assert_eq!(
            error.code,
            atm_core::error_codes::AtmErrorCode::SocketOverrideForbidden
        );
        assert!(logs.contains("raw cli runtime-root failure"));
        assert!(logs.contains("ATM_SOCKET_OVERRIDE_FORBIDDEN"));
    }

    #[test]
    #[serial(env)]
    fn resolve_command_runtime_context_reports_atm_home_unresolved() {
        let _env = EnvGuard::set_many([("ATM_HOME", None), ("HOME", None), ("USERPROFILE", None)]);

        let error =
            resolve_command_runtime_context("send").expect_err("missing ATM_HOME should fail");

        assert_eq!(
            error.code,
            atm_core::error_codes::AtmErrorCode::AtmHomeUnresolved
        );
    }

    #[test]
    #[cfg(unix)]
    #[serial(env)]
    fn bootstrap_reports_runtime_root_invalid_for_invalid_socket_override() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        use atm_core::test_support::{remove_env_var, set_env_var};

        struct SocketEnvRestore(Option<OsString>);

        impl Drop for SocketEnvRestore {
            fn drop(&mut self) {
                match self.0.take() {
                    Some(value) => {
                        set_env_var("ATM_DAEMON_SOCKET", value);
                    }
                    None => {
                        remove_env_var("ATM_DAEMON_SOCKET");
                    }
                }
            }
        }

        let tempdir = TempDir::new().expect("tempdir");
        let atm_home = tempdir.path().join("atm-home");
        let invocation_dir = tempdir.path().join("workspace");
        std::fs::create_dir_all(&atm_home).expect("atm home");
        std::fs::create_dir_all(&invocation_dir).expect("invocation dir");
        let _env = EnvGuard::set_many([
            ("ATM_HOME", Some(atm_home.to_str().expect("utf8 atm home"))),
            ("ATM_IDENTITY", Some(TEST_SENDER)),
            ("ATM_TEAM", Some(TEST_TEAM)),
        ]);
        let _socket_restore = SocketEnvRestore(std::env::var_os("ATM_DAEMON_SOCKET"));
        set_env_var(
            "ATM_DAEMON_SOCKET",
            OsString::from_vec(vec![0x66, 0x6f, 0x80]),
        );

        let error = CliComposition::bootstrap(
            "send",
            &CliObservability::fallback(),
            InvocationDir::new(&invocation_dir),
            AtmHomePath::new(&atm_home),
        )
        .expect_err("invalid daemon socket override should fail");

        assert_eq!(
            error.code,
            atm_core::error_codes::AtmErrorCode::SocketOverrideForbidden
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
                || transport.probe_connection().map(|_| ()),
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

    #[cfg(windows)]
    #[test]
    fn launch_gate_treats_windows_lock_and_sharing_violations_as_contention() {
        assert!(atm_daemon_client::is_launch_gate_contention_error(
            &std::io::Error::from_raw_os_error(32)
        ));
        assert!(atm_daemon_client::is_launch_gate_contention_error(
            &std::io::Error::from_raw_os_error(33)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn spawn_failure_maps_to_auto_start_failed() {
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
        std::fs::write(&daemon_path, b"not an executable daemon binary").expect("write daemon");
        let daemon_bin = DaemonBinaryPath::new(daemon_path).expect("daemon");
        let supervisor = DaemonSupervisor::new(socket_path.clone(), daemon_bin);
        let transport = LocalIpcClientTransportAdapter::new(socket_path);

        let error = supervisor
            .ensure_daemon_available_with_lock_path(
                || transport.probe_connection().map(|_| ()),
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
