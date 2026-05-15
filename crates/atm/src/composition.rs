use std::fmt;
use std::sync::{Arc, Once};

use atm_core::ack::{AckOutcome, AckRequest};
use atm_core::boundary;
use atm_core::boundary::ClientTransport;
use atm_core::clear::{ClearOutcome, ClearQuery};
use atm_core::doctor::{BootstrapTraceReport, DoctorQuery, DoctorReport};
use atm_core::error::AtmError;
use atm_core::graft::{
    AdvisoryDrainRequest, AdvisoryDrainResponse, AdvisoryFetchRequest, AdvisoryFetchResponse,
    AdvisorySessionPort, AdvisorySessionRegistrationRequest, AdvisorySessionRegistrationResponse,
    AdvisorySessionUnregistrationRequest, AdvisorySessionUnregistrationResponse, AtmGraftClient,
};
use atm_core::list::{ListOutcome, ListQuery};
use atm_core::observability::{CommandEvent, ObservabilityPort};
use atm_core::protocol::{
    RequestEnvelope, ResponseEnvelope, SendRequestEnvelope, SendResponseEnvelope,
};
use atm_core::read::{ReadOutcome, ReadQuery};
use atm_core::send::{SendOutcome, SendRequest};
use atm_core::types::{AgentName, TeamName};
#[cfg(not(test))]
use atm_daemon_bootstrap::{
    install_sqlite_retained_runtime_factory, resolve_daemon_bin, resolve_daemon_local_ipc_endpoint,
};
#[cfg(test)]
use atm_daemon_bootstrap::{resolve_daemon_bin, resolve_daemon_local_ipc_endpoint};
use atm_daemon_client::{
    BootstrapTraceability, DaemonLocalIpcEndpoint, DaemonSupervisor, exchange as daemon_exchange,
    try_connect as daemon_try_connect, unexpected_response,
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

#[derive(Debug)]
struct LocalIpcClientTransportAdapter {
    endpoint: DaemonLocalIpcEndpoint,
}

impl LocalIpcClientTransportAdapter {
    fn new(endpoint: DaemonLocalIpcEndpoint) -> Self {
        Self { endpoint }
    }

    fn try_connect(&self) -> Result<interprocess::local_socket::Stream, AtmError> {
        daemon_try_connect(&self.endpoint)
    }

    /// This function performs blocking IPC I/O. Callers in async contexts must
    /// wrap this in `tokio::task::spawn_blocking`.
    fn exchange(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        daemon_exchange(&self.endpoint, request, SAME_HOST_REQUEST_DEADLINE)
    }
}

impl boundary::sealed::Sealed for LocalIpcClientTransportAdapter {}

impl ClientTransport for LocalIpcClientTransportAdapter {
    fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        self.exchange(request)
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

    #[cfg(test)]
    pub(crate) fn from_transport_with_bootstrap_trace(
        transport: Arc<dyn ClientTransport + Send + Sync + 'a>,
        observability_port: &'a CliObservability,
        bootstrap_trace: BootstrapTraceReport,
    ) -> Self {
        install_retained_runtime_factory();
        Self {
            transport,
            observability_port,
            bootstrap_trace: Some(bootstrap_trace),
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
                self.observability_port.emit_command_event(CommandEvent {
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
                self.observability_port.emit_command_event(CommandEvent {
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
                self.observability_port.emit_command_event(CommandEvent {
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

    pub(crate) fn list(&self, query: ListQuery) -> Result<ListOutcome, AtmError> {
        match self.send_request(RequestEnvelope::List(query))? {
            ResponseEnvelope::List(outcome) => {
                self.observability_port.emit_command_event(CommandEvent {
                    command: "list",
                    action: "list",
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
            other => Err(unexpected_response("list", other)),
        }
    }

    pub(crate) fn clear(&self, query: ClearQuery) -> Result<ClearOutcome, AtmError> {
        match self.send_request(RequestEnvelope::Clear(query))? {
            ResponseEnvelope::Clear(outcome) => {
                self.observability_port.emit_command_event(CommandEvent {
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
            ResponseEnvelope::Doctor(mut report) => {
                report.bootstrap_trace = self.bootstrap_trace.clone();
                Ok(report)
            }
            other => Err(unexpected_response("doctor", other)),
        }
    }

    pub(crate) fn register_graft_session(
        &self,
        request: AdvisorySessionRegistrationRequest,
    ) -> Result<AdvisorySessionRegistrationResponse, AtmError> {
        match self.send_request(RequestEnvelope::AdvisoryRegister(request))? {
            ResponseEnvelope::AdvisoryRegister(response) => Ok(response),
            other => Err(unexpected_response("graft register", other)),
        }
    }

    pub(crate) fn unregister_graft_session(
        &self,
        request: AdvisorySessionUnregistrationRequest,
    ) -> Result<AdvisorySessionUnregistrationResponse, AtmError> {
        match self.send_request(RequestEnvelope::AdvisoryUnregister(request))? {
            ResponseEnvelope::AdvisoryUnregister(response) => Ok(response),
            other => Err(unexpected_response("graft unregister", other)),
        }
    }

    pub(crate) fn fetch_graft_nudges(
        &self,
        request: AdvisoryFetchRequest,
    ) -> Result<AdvisoryFetchResponse, AtmError> {
        match self.send_request(RequestEnvelope::AdvisoryFetch(request))? {
            ResponseEnvelope::AdvisoryFetch(response) => Ok(response),
            other => Err(unexpected_response("graft fetch", other)),
        }
    }

    pub(crate) fn drain_graft_nudges(
        &self,
        request: AdvisoryDrainRequest,
    ) -> Result<AdvisoryDrainResponse, AtmError> {
        match self.send_request(RequestEnvelope::AdvisoryDrain(request))? {
            ResponseEnvelope::AdvisoryDrain(response) => Ok(response),
            other => Err(unexpected_response("graft drain", other)),
        }
    }

    pub(crate) fn bootstrap(
        command: &'static str,
        observability: &'a CliObservability,
    ) -> Result<Self, AtmError> {
        let endpoint = resolve_daemon_local_ipc_endpoint()?;
        let daemon_bin = resolve_daemon_bin("atm")?;
        let transport = Arc::new(LocalIpcClientTransportAdapter::new(endpoint.clone()));
        let supervisor = DaemonSupervisor::new(endpoint, daemon_bin);
        let traceability = BootstrapTraceability::new(
            command,
            observability,
            parse_bootstrap_team()?,
            parse_bootstrap_agent()?,
        );
        supervisor.ensure_daemon_available_with_traceability(&traceability, || {
            transport.try_connect().map(|_| ())
        })?;
        let mut composition = Self::from_transport(transport, observability);
        composition.bootstrap_trace = Some(traceability.snapshot());
        Ok(composition)
    }
}

fn parse_bootstrap_agent() -> Result<AgentName, AtmError> {
    std::env::var("ATM_IDENTITY")
        .unwrap_or_else(|_| "unknown".to_string())
        .parse()
        .map_err(|error: AtmError| {
            error.with_recovery("Check ATM_IDENTITY and ATM_TEAM env vars are set")
        })
}

fn parse_bootstrap_team() -> Result<TeamName, AtmError> {
    std::env::var("ATM_TEAM")
        .unwrap_or_else(|_| "unknown".to_string())
        .parse()
        .map_err(|error: AtmError| {
            error.with_recovery("Check ATM_IDENTITY and ATM_TEAM env vars are set")
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

impl AdvisorySessionPort for CliComposition<'_> {
    fn register_session(
        &self,
        request: AdvisorySessionRegistrationRequest,
    ) -> Result<AdvisorySessionRegistrationResponse, AtmError> {
        CliComposition::register_graft_session(self, request)
    }

    fn unregister_session(
        &self,
        request: AdvisorySessionUnregistrationRequest,
    ) -> Result<AdvisorySessionUnregistrationResponse, AtmError> {
        CliComposition::unregister_graft_session(self, request)
    }

    fn fetch_nudges(
        &self,
        request: AdvisoryFetchRequest,
    ) -> Result<AdvisoryFetchResponse, AtmError> {
        CliComposition::fetch_graft_nudges(self, request)
    }

    fn drain_nudges(
        &self,
        request: AdvisoryDrainRequest,
    ) -> Result<AdvisoryDrainResponse, AtmError> {
        CliComposition::drain_graft_nudges(self, request)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;
    use std::time::Duration;

    use atm_core::ack::AckRequest;
    use atm_core::boundary;
    use atm_core::boundary::ClientTransport;
    use atm_core::clear::ClearQuery;
    use atm_core::doctor::{
        BootstrapAutoStartOutcome, BootstrapConnectOutcome, BootstrapLaunchGateOutcome,
        BootstrapTraceReport, DoctorQuery, DoctorStatus,
    };
    use atm_core::error::AtmError;
    use atm_core::graft::AtmGraftClient;
    use atm_core::protocol::{
        ProtocolErrorEnvelope, RequestEnvelope, ResponseEnvelope, SendRequestEnvelope,
    };
    use atm_core::read::ReadQuery;
    use atm_core::schema::{AgentMember, AtmMessageId, MessageEnvelope, TeamConfig};
    use atm_core::send::{SendMessageSource, SendRequest};
    use atm_core::test_support::{
        EnvGuard, ROLE_TEAM_LEAD, TEST_LEAD, TEST_RECIPIENT, TEST_RECIPIENT_ADDRESS, TEST_SENDER,
        TEST_TEAM,
    };
    use atm_core::transport::testing::{
        FakeClientTransport, HealthyObservability, LoopbackClientTransport,
    };
    use atm_core::types::{AckActivationMode, ReadSelection};
    use atm_core::types::{AgentName, TeamName};
    use atm_daemon_client::DaemonBinaryPath;
    use atm_runtime_test_support::{
        SqliteRuntimeGuard,
        install_sqlite_retained_runtime_factory as install_test_runtime_factory,
        open_sqlite_boundary,
    };
    use chrono::Utc;
    use serde_json::Value;
    use tempfile::TempDir;

    use super::{
        CliComposition, DaemonLocalIpcEndpoint, DaemonSupervisor, HOST_RUNTIME_LAUNCH_LOCK_FILE,
        LaunchGateGuard, LocalIpcClientTransportAdapter,
    };
    use crate::observability::CliObservability;

    struct LoopbackFixture {
        _tempdir: TempDir,
        _env_guard: EnvGuard,
        _sqlite_guard: SqliteRuntimeGuard,
        home_dir: std::path::PathBuf,
        current_dir: std::path::PathBuf,
    }

    impl LoopbackFixture {
        fn new(recipient: &str) -> Self {
            install_test_runtime_factory();
            let tempdir = tempfile::tempdir().expect("tempdir");
            let home_dir = tempdir.path().to_path_buf();
            let env_guard = EnvGuard::set_many([(
                "ATM_HOME",
                Some(home_dir.to_str().expect("utf-8 tempdir path")),
            )]);
            let current_dir = tempdir.path().join("cwd");
            fs::create_dir_all(&current_dir).expect("cwd");
            let sqlite_guard =
                SqliteRuntimeGuard::install(home_dir.join("runtime").join("mail.sqlite3"));
            let fixture = Self {
                _tempdir: tempdir,
                _env_guard: env_guard,
                _sqlite_guard: sqlite_guard,
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

        fn sqlite_db_path(&self) -> std::path::PathBuf {
            self.home_dir.join("runtime").join("mail.sqlite3")
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
            let messages = values
                .iter()
                .cloned()
                .map(serde_json::from_value::<MessageEnvelope>)
                .collect::<Result<Vec<_>, _>>()
                .expect("message envelopes");
            self.seed_sqlite_mailbox(agent, &messages);
        }

        fn inbox_contents(&self, agent: &str) -> Vec<MessageEnvelope> {
            let raw = fs::read_to_string(self.inbox_path(agent)).expect("inbox contents");
            let values: Vec<Value> = serde_json::from_str(&raw).expect("json array");
            values
                .into_iter()
                .map(|value| serde_json::from_value(value).expect("message envelope"))
                .collect()
        }

        fn write_inbox_messages(&self, agent: &str, messages: &[MessageEnvelope]) {
            let values = messages
                .iter()
                .map(|message| serde_json::to_value(message).expect("message value"))
                .collect::<Vec<_>>();
            self.write_inbox_values(agent, &values);
            self.seed_sqlite_mailbox(agent, messages);
        }

        fn seed_sqlite_mailbox(&self, agent: &str, messages: &[MessageEnvelope]) {
            let assembly = open_sqlite_boundary(self.sqlite_db_path()).expect("sqlite db");
            let mail_store = assembly.mail_store();
            let team = TEST_TEAM.parse::<TeamName>().expect("team");
            let agent_name = agent.parse::<AgentName>().expect("agent");

            for (index, message) in messages.iter().enumerate() {
                let message_key = if let Some(message_id) = message.message_id {
                    boundary::MessageKey::for_atm_message(message_id).expect("message key")
                } else {
                    boundary::MessageKey::new(format!("ext:{agent}:{index}")).expect("message key")
                };
                mail_store
                    .upsert_message(boundary::MailStoreUpsertMessageRequest {
                        record: boundary::MailStoreMessageRecord {
                            team: team.clone(),
                            agent: agent_name.clone(),
                            message_key: message_key.clone(),
                            envelope: message.clone(),
                        },
                    })
                    .expect("seed sqlite message");
                mail_store
                    .upsert_message_state(boundary::UpsertMailMessageStateRequest {
                        team: team.clone(),
                        agent: agent_name.clone(),
                        actor: agent_name.clone(),
                        state: boundary::MailMessageState {
                            team: team.clone(),
                            agent: agent_name.clone(),
                            actor: agent_name.clone(),
                            message_key,
                            read: message.read,
                            pending_ack_at: message.pending_ack_at,
                            acknowledged_at: message.acknowledged_at,
                            expires_at: message.expires_at,
                            deleted_at: None,
                            updated_at: Some(message.timestamp),
                        },
                    })
                    .expect("seed sqlite message state");
            }
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

        fn ack_request(&self, message_id: AtmMessageId, reply_body: &str) -> AckRequest {
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
                expires_at: None,
                task_id: None,
                extra: serde_json::Map::new(),
            }
        }

        fn pending_ack_message(&self, text: &str) -> (AtmMessageId, MessageEnvelope) {
            let message_id = AtmMessageId::new();
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

        for (label, result) in [("first", &first), ("second", &second)] {
            if let Err(error) = result {
                assert_eq!(
                    error.code,
                    atm_core::error_codes::AtmErrorCode::MailboxLockTimeout,
                    "{label} response: {result:?}"
                );
            }
        }
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
        assert_eq!(
            outcome.message.expect("selected message").envelope.text,
            "read me"
        );
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
        let read_outcome = composition
            .receive(fixture.read_query())
            .expect("read outcome after clear");
        assert_eq!(read_outcome.count, 0);
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
    fn doctor_projects_bootstrap_trace_into_report() {
        let fixture = LoopbackFixture::new(TEST_RECIPIENT);
        let observability = Arc::new(HealthyObservability);
        let composition_observability = CliObservability::fallback();
        let composition = CliComposition::from_transport_with_bootstrap_trace(
            Arc::new(LoopbackClientTransport::new(observability)),
            &composition_observability,
            BootstrapTraceReport {
                daemon_connect: BootstrapConnectOutcome::Connected,
                daemon_launch_gate: BootstrapLaunchGateOutcome::Skipped,
                daemon_auto_start: BootstrapAutoStartOutcome::Skipped,
                connect_detail: None,
                launch_gate_detail: None,
                auto_start_detail: None,
            },
        );

        let report = composition
            .doctor(fixture.doctor_query())
            .expect("doctor report");

        assert_eq!(
            report.bootstrap_trace.as_ref().expect("bootstrap trace"),
            &BootstrapTraceReport {
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
    fn cli_composition_supports_graft_client_surface_without_daemon() {
        let fixture = LoopbackFixture::new(TEST_RECIPIENT);
        let composition_observability = CliObservability::fallback();
        let composition = CliComposition::from_transport(
            Arc::new(LoopbackClientTransport::new(Arc::new(
                atm_core::observability::NullObservability,
            ))),
            &composition_observability,
        );
        let client: &dyn AtmGraftClient = &composition;

        let send_outcome = client
            .send_message(fixture.send_request("graft send"))
            .expect("send through graft client surface");
        assert_eq!(send_outcome.sender.as_str(), TEST_SENDER);

        let read_outcome = client
            .read_message(fixture.read_query())
            .expect("read through graft client surface");
        assert_eq!(read_outcome.count, 1);

        let (message_id, pending_ack) = fixture.pending_ack_message("please ack");
        fixture.write_inbox_messages(TEST_SENDER, &[pending_ack]);
        let ack_outcome = client
            .acknowledge_message(fixture.ack_request(message_id, "received and starting"))
            .expect("ack through graft client surface");
        assert_eq!(ack_outcome.message_id, message_id);
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
                || transport.try_connect().map(|_| ()),
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
                || transport.try_connect().map(|_| ()),
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
