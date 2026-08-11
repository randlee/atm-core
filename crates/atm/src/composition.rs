#![allow(
    deprecated,
    reason = "the retained CLI composition still seeds and bridges legacy atm-core boundary stores during the Phase AC transition"
)]

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Once};

use async_trait::async_trait;
use atm_core::ack::{AckOutcome, AckRequest};
use atm_core::api::{ApiRequest, DaemonApiClient};
use atm_core::clear::{ClearOutcome, ClearQuery};
use atm_core::doctor::{BootstrapTraceReport, DoctorQuery, DoctorReport};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::graft::AtmGraftClient;
use atm_core::home;
use atm_core::list::{ListOutcome, ListQuery};
use atm_core::observability::{CommandEvent, ObservabilityPort, action_name, outcome_label};
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope, SendResponseEnvelope};
use atm_core::read::{PeekQuery, ReadOutcome, ReadQuery};
use atm_core::send::{SendOutcome, SendRequest};
#[cfg(not(test))]
use atm_daemon_bootstrap::install_sqlite_retained_runtime_factory;
#[cfg(test)]
use atm_daemon_client::{HOST_RUNTIME_LAUNCH_LOCK_FILE, LaunchGateGuard};
use atm_daemon_client::{resolve_daemon_local_ipc_endpoint, unexpected_response};
use atm_http_runtime::SAME_HOST_REQUEST_DEADLINE;
#[cfg(test)]
use atm_runtime_test_support::{
    SQLITE_RUNTIME_PATH_ENV,
    install_sqlite_retained_runtime_factory as install_test_runtime_factory, open_sqlite_boundary,
};

use crate::observability::CliObservability;

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
    let atm_home = home::atm_home().map_err(|_source| {
        let error = AtmError::atm_home_unresolved(format!(
            "failed to resolve ATM_HOME before bootstrapping `atm {command}`"
        ));
        log_runtime_root_failure(command, &error);
        error
    })?;
    Ok((atm_home, invocation_dir))
}

/// Refresh an already-running daemon after a durable control-plane mutation.
///
/// Administrative commands must not start a daemon solely to invalidate an
/// in-memory snapshot: a later daemon startup reads the durable roster itself.
/// If a daemon is already serving, this authenticated reload makes the
/// mutation visible before the command reports completion.
pub(crate) async fn reload_running_runtime_view() -> Result<(), AtmError> {
    let endpoint = resolve_daemon_local_ipc_endpoint()?;
    let transport =
        atm_http_runtime::preferred_local_client(endpoint.as_ref(), SAME_HOST_REQUEST_DEADLINE)?;
    match transport
        .execute(ApiRequest::new(RequestEnvelope::ReloadRuntimeView))
        .await
    {
        Ok(response) => match response.into_inner() {
            ResponseEnvelope::RuntimeViewReloaded => Ok(()),
            other => Err(unexpected_response("runtime reload", other)),
        },
        Err(error)
            if matches!(
                error.code(),
                AtmErrorCode::DaemonUnavailable | AtmErrorCode::WaitTimeout
            ) =>
        {
            // Administrative mutations persist independently; only refresh an
            // already-running runtime view, never start one just for reload.
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn log_runtime_root_failure(command: &'static str, error: &AtmError) {
    tracing::error!(
        command,
        error_code = %error.code().as_str(),
        error = %error,
        "raw cli runtime-root failure"
    );
}

pub(crate) struct CliComposition<'a> {
    /// The one Tokio/Axum client boundary for every CLI daemon operation.
    async_transport: Arc<dyn DaemonApiClient + Send + Sync + 'a>,
    observability_port: &'a CliObservability,
    bootstrap_trace: Option<BootstrapTraceReport>,
}

impl fmt::Debug for CliComposition<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CliComposition")
            .field("async_transport", &"dyn DaemonApiClient")
            .field("observability_port", &"dyn ObservabilityPort")
            .field("bootstrap_trace", &self.bootstrap_trace)
            .finish()
    }
}

impl<'a> CliComposition<'a> {
    #[cfg(test)]
    fn from_fake_transport(
        transport: Arc<atm_core::transport::testing::FakeClientTransport>,
        observability_port: &'a CliObservability,
    ) -> Self {
        install_retained_runtime_factory();
        Self {
            async_transport: transport,
            observability_port,
            bootstrap_trace: None,
        }
    }

    #[cfg(test)]
    fn from_loopback_transport(
        transport: Arc<atm_core::transport::testing::LoopbackClientTransport>,
        observability_port: &'a CliObservability,
    ) -> Self {
        install_retained_runtime_factory();
        Self {
            async_transport: transport,
            observability_port,
            bootstrap_trace: None,
        }
    }

    #[cfg(test)]
    fn from_loopback_transport_with_bootstrap_trace(
        transport: Arc<atm_core::transport::testing::LoopbackClientTransport>,
        observability_port: &'a CliObservability,
        bootstrap_trace: BootstrapTraceReport,
    ) -> Self {
        let mut composition = Self::from_loopback_transport(transport, observability_port);
        composition.bootstrap_trace = Some(bootstrap_trace);
        composition
    }

    #[expect(
        dead_code,
        reason = "reserved for future phase that inspects the active transport variant"
    )]
    pub(crate) fn transport(&self) -> &(dyn DaemonApiClient + Send + Sync + 'a) {
        self.async_transport.as_ref()
    }

    async fn execute_request(
        &self,
        request: RequestEnvelope,
    ) -> Result<ResponseEnvelope, AtmError> {
        match self
            .async_transport
            .execute(ApiRequest::new(request))
            .await?
            .into_inner()
        {
            ResponseEnvelope::Error(error) => Err(error),
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

    pub(crate) async fn send(&self, request: SendRequest) -> Result<SendOutcome, AtmError> {
        let transport =
            atm_http_runtime::selected_write_transport(&request, &self.async_transport)?;
        match transport
            .execute(ApiRequest::new(RequestEnvelope::Write(Box::new(request))))
            .await?
            .into_inner()
        {
            ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => {
                self.observability_port.emit_command_event(CommandEvent {
                    command: "send",
                    action: action_name("send"),
                    outcome: outcome_label(if outcome.dry_run { "dry_run" } else { "sent" }),
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

    pub(crate) async fn ack(&self, request: AckRequest) -> Result<AckOutcome, AtmError> {
        match self
            .execute_request(RequestEnvelope::Write(Box::new(
                request.into_write_request(),
            )))
            .await?
        {
            ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(outcome)) => {
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
                Ok(outcome)
            }
            other => Err(unexpected_response("ack", other)),
        }
    }

    pub(crate) async fn receive(&self, query: ReadQuery) -> Result<ReadOutcome, AtmError> {
        match self
            .execute_request(RequestEnvelope::Receive(query))
            .await?
        {
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

    pub(crate) async fn peek(&self, query: PeekQuery) -> Result<ReadOutcome, AtmError> {
        match self.execute_request(RequestEnvelope::Peek(query)).await? {
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

    pub(crate) async fn list(&self, query: ListQuery) -> Result<ListOutcome, AtmError> {
        match self.execute_request(RequestEnvelope::List(query)).await? {
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

    pub(crate) async fn clear(&self, query: ClearQuery) -> Result<ClearOutcome, AtmError> {
        match self.execute_request(RequestEnvelope::Clear(query)).await? {
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
    pub(crate) async fn doctor(&self, query: DoctorQuery) -> Result<DoctorReport, AtmError> {
        match self.execute_request(RequestEnvelope::Doctor(query)).await? {
            ResponseEnvelope::Doctor(mut report) => {
                report.bootstrap_trace = self.bootstrap_trace.clone();
                Ok(*report)
            }
            other => Err(unexpected_response("doctor", other)),
        }
    }

    pub(crate) async fn reload_runtime_view(&self) -> Result<(), AtmError> {
        match self
            .execute_request(RequestEnvelope::ReloadRuntimeView)
            .await?
        {
            ResponseEnvelope::RuntimeViewReloaded => Ok(()),
            other => Err(unexpected_response("runtime reload", other)),
        }
    }

    pub(crate) fn bootstrap(
        command: &'static str,
        observability: &'a CliObservability,
        invocation_dir: InvocationDir<'_>,
        atm_home: AtmHomePath<'_>,
    ) -> Result<Self, AtmError> {
        install_retained_runtime_factory();
        let _invocation_dir = invocation_dir.as_path();
        let _atm_home = atm_home.as_path();
        let endpoint = resolve_daemon_local_ipc_endpoint().inspect_err(|error| {
            log_runtime_root_failure(command, error);
        })?;
        // The one managed Tokio/Axum daemon is selected by `/daemon-switch`.
        // Do not probe or start the frozen synchronous daemon here: the first
        // typed API request carries the same capability-authenticated HTTP
        // contract and reports its own actionable availability failure.
        Ok(Self {
            async_transport: atm_http_runtime::preferred_local_client(
                endpoint.as_ref(),
                SAME_HOST_REQUEST_DEADLINE,
            )?,
            observability_port: observability,
            bootstrap_trace: None,
        })
    }
}

#[async_trait]
impl AtmGraftClient for CliComposition<'_> {
    async fn send_message(&self, request: SendRequest) -> Result<SendOutcome, AtmError> {
        CliComposition::send(self, request).await
    }

    async fn read_message(&self, query: ReadQuery) -> Result<ReadOutcome, AtmError> {
        CliComposition::receive(self, query).await
    }

    async fn acknowledge_message(&self, request: AckRequest) -> Result<AckOutcome, AtmError> {
        CliComposition::ack(self, request).await
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io;
    use std::io::Write;
    use std::sync::Arc;
    use std::sync::Mutex;

    use atm_core::ApiRequest;
    use atm_core::ack::AckRequest;
    use atm_core::boundary;
    use atm_core::clear::ClearQuery;
    use atm_core::doctor::{
        BootstrapAutoStartOutcome, BootstrapConnectOutcome, BootstrapLaunchGateOutcome,
        BootstrapTraceReport, DoctorQuery, DoctorStatus,
    };
    use atm_core::error::AtmError;
    use atm_core::graft::AtmGraftClient;
    use atm_core::protocol::{RequestEnvelope, ResponseEnvelope, SendResponseEnvelope};
    use atm_core::read::{PeekQuery, ReadQuery};
    use atm_core::schema::{AgentMember, AtmMessageId, InboxMessage, TeamConfig};
    use atm_core::send::{SendCommandOutcome, SendMessageSource, SendOutcome, SendRequest};
    use atm_core::test_support::{
        EnvGuard, ROLE_TEAM_LEAD, TEST_LEAD, TEST_RECIPIENT, TEST_RECIPIENT_ADDRESS, TEST_SENDER,
        TEST_TEAM,
    };
    use atm_core::transport::testing::{
        FakeClientTransport, HealthyObservability, LoopbackClientTransport,
    };
    use atm_core::types::ReadSelection;
    use atm_core::types::{AgentName, ChatId, CommandAction, TeamName};
    use atm_daemon_client::{DaemonBinaryPath, DaemonLocalIpcEndpoint};
    use chrono::Utc;
    use serde_json::{Map, Value};
    use serial_test::serial;
    use tempfile::TempDir;
    use tracing_subscriber::fmt::MakeWriter;

    use super::{
        CliComposition, HOST_RUNTIME_LAUNCH_LOCK_FILE, LaunchGateGuard, SQLITE_RUNTIME_PATH_ENV,
        open_sqlite_boundary, resolve_command_runtime_context,
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

    struct LoopbackFixture {
        _env_guard: EnvGuard,
        _tempdir: TempDir,
        home_dir: std::path::PathBuf,
        current_dir: std::path::PathBuf,
    }

    impl LoopbackFixture {
        fn new(recipient: &str) -> Self {
            super::install_retained_runtime_factory();
            let tempdir = tempfile::tempdir().expect("tempdir");
            let home_dir = tempdir.path().to_path_buf();
            let sqlite_db_path = home_dir.join("runtime").join("mail.sqlite3");
            let current_dir = tempdir.path().join("cwd");
            fs::create_dir_all(&current_dir).expect("cwd");
            fs::write(current_dir.join(".atm.toml"), "[atm]\n").expect("fixture atm config");
            let env_guard = EnvGuard::set_many([
                (
                    "ATM_HOME",
                    Some(home_dir.to_str().expect("utf-8 tempdir path")),
                ),
                (
                    SQLITE_RUNTIME_PATH_ENV,
                    Some(sqlite_db_path.to_str().expect("utf-8 sqlite db path")),
                ),
            ]);
            let fixture = Self {
                _env_guard: env_guard,
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
            self.seed_sqlite_roster(recipient);
        }

        fn seed_sqlite_roster(&self, recipient: &str) {
            let assembly = open_sqlite_boundary(self.sqlite_db_path()).expect("sqlite db");
            let roster_store = assembly.roster_store_arc();
            let team = TEST_TEAM.parse::<TeamName>().expect("team");
            let members = [TEST_SENDER, recipient, TEST_LEAD]
                .into_iter()
                .map(|agent| boundary::RosterEntry {
                    team_name: team.clone(),
                    agent_name: agent.parse().expect("agent"),
                    member_kind: boundary::RosterMemberKind::Permanent,
                    harness: boundary::RosterHarness::ClaudeCode,
                    agent_type: atm_core::schema::AgentType::default(),
                    model: atm_core::types::ModelName::default(),
                    recipient_pane_id: None,
                    metadata_json: Map::new(),
                })
                .collect::<Vec<_>>();
            roster_store
                .replace_roster(&team, &members)
                .expect("seed sqlite roster");
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
                .map(serde_json::from_value::<InboxMessage>)
                .collect::<Result<Vec<_>, _>>()
                .expect("message envelopes");
            self.seed_sqlite_mailbox(agent, &messages);
        }

        fn inbox_contents(&self, agent: &str) -> Vec<InboxMessage> {
            if self.sqlite_db_path().exists() {
                let assembly = open_sqlite_boundary(self.sqlite_db_path()).expect("sqlite db");
                let mail_store = assembly.mail_store_arc();
                let team = TEST_TEAM.parse::<TeamName>().expect("team");
                let agent_name = agent.parse::<AgentName>().expect("agent");
                let metadata_rows = mail_store
                    .query_mailbox_metadata(&team, &agent_name, None)
                    .expect("mailbox rows");
                return metadata_rows
                    .into_iter()
                    .map(|row| {
                        mail_store
                            .load_message(&team, &agent_name, &row.message_key)
                            .expect("message record")
                            .expect("stored message")
                            .envelope
                    })
                    .collect();
            }

            let inbox_path = self.inbox_path(agent);
            if let Ok(raw) = fs::read_to_string(&inbox_path) {
                if raw.trim_start().starts_with('[') {
                    let values: Vec<Value> = serde_json::from_str(&raw).expect("json array");
                    return values
                        .into_iter()
                        .map(|value| serde_json::from_value(value).expect("message envelope"))
                        .collect();
                }
                return raw
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .map(|line| serde_json::from_str::<InboxMessage>(line).expect("json line"))
                    .collect();
            }

            Vec::new()
        }

        fn write_inbox_messages(&self, agent: &str, messages: &[InboxMessage]) {
            let values = messages
                .iter()
                .map(|message| serde_json::to_value(message).expect("message value"))
                .collect::<Vec<_>>();
            self.write_inbox_values(agent, &values);
            self.seed_sqlite_mailbox(agent, messages);
        }

        fn seed_sqlite_mailbox(&self, agent: &str, messages: &[InboxMessage]) {
            let assembly = open_sqlite_boundary(self.sqlite_db_path()).expect("sqlite db");
            let mail_store = assembly.mail_store_arc();
            let team = TEST_TEAM.parse::<TeamName>().expect("team");
            let agent_name = agent.parse::<AgentName>().expect("agent");

            for (index, message) in messages.iter().enumerate() {
                let message_key = if let Some(message_id) = message.message_id {
                    boundary::MessageKey::from(message_id)
                } else {
                    boundary::MessageKey::new(format!("ext:{agent}:{index}")).expect("message key")
                };
                mail_store
                    .upsert_message(boundary::Message {
                        team: team.clone(),
                        agent: agent_name.clone(),
                        message_key: message_key.clone(),
                        envelope: message.clone(),
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
                TEST_SENDER.parse().expect("caller"),
                TEST_RECIPIENT_ADDRESS,
                TEST_TEAM.parse().expect("team"),
                SendMessageSource::Inline(body.to_string()),
                None,
                false,
                None,
                false,
            )
            .expect("send request")
        }

        fn send_request_to(&self, recipient: &str, body: &str) -> SendRequest {
            SendRequest::new(
                self.home_dir.clone(),
                self.current_dir.clone(),
                TEST_SENDER.parse().expect("caller"),
                recipient,
                TEST_TEAM.parse().expect("team"),
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
                TEST_SENDER.parse().expect("caller"),
                TEST_RECIPIENT_ADDRESS,
                TEST_TEAM.parse().expect("team"),
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
                caller_identity: TEST_SENDER.parse().expect("caller"),
                caller_chat_id: None,
                caller_team: TEST_TEAM.parse().expect("team"),
                activity_observation: None,
                message_id,
                reply_body: reply_body.to_string(),
            }
        }

        fn read_query(&self) -> ReadQuery {
            ReadQuery::new(
                self.home_dir.clone(),
                self.current_dir.clone(),
                TEST_RECIPIENT.parse().expect("caller"),
                None,
                TEST_TEAM.parse().expect("team"),
                ReadSelection::All,
                false,
                false,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .expect("read query")
        }

        fn read_query_for(&self, caller: &str, message_id: AtmMessageId) -> ReadQuery {
            ReadQuery::new(
                self.home_dir.clone(),
                self.current_dir.clone(),
                caller.parse().expect("caller"),
                None,
                TEST_TEAM.parse().expect("team"),
                ReadSelection::All,
                false,
                false,
                Some(&message_id.to_string()),
                None,
                None,
                None,
                None,
                None,
            )
            .expect("read query")
        }

        fn peek_query_for(
            &self,
            caller: &str,
            target: Option<&str>,
            message_id: AtmMessageId,
        ) -> PeekQuery {
            PeekQuery::new(
                self.home_dir.clone(),
                self.current_dir.clone(),
                caller.parse().expect("caller"),
                target,
                TEST_TEAM.parse().expect("team"),
                ReadSelection::All,
                false,
                Some(&message_id.to_string()),
                None,
                None,
                None,
                None,
                None,
            )
            .expect("peek query")
        }

        fn clear_query(&self) -> ClearQuery {
            ClearQuery {
                home_dir: self.home_dir.clone(),
                current_dir: self.current_dir.clone(),
                caller_identity: TEST_RECIPIENT.parse().expect("caller"),
                caller_team: TEST_TEAM.parse().expect("team"),
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
                ..DoctorQuery::default()
            }
        }

        fn message(&self, text: &str, read: bool) -> InboxMessage {
            InboxMessage {
                from: TEST_LEAD.parse().expect("lead"),
                source_chat_id: None,
                text: text.to_string(),
                timestamp: Utc::now().into(),
                read,
                source_team: Some(TEST_TEAM.parse().expect("team")),
                destination_chat_id: None,
                summary: None,
                message_id: Some(AtmMessageId::new()),
                requires_ack: false,
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

        fn pending_ack_message(&self, text: &str) -> (AtmMessageId, InboxMessage) {
            let message_id = AtmMessageId::new();
            let mut message = self.message(text, true);
            message.message_id = Some(message_id);
            message.requires_ack = true;
            message.pending_ack_at = Some(Utc::now().into());
            (message_id, message)
        }
    }

    #[tokio::test]
    async fn fake_transport_maps_protocol_error_envelope_to_atm_error() {
        let tempdir = TempDir::new().expect("tempdir");
        let observability = CliObservability::fallback();
        let transport = Arc::new(FakeClientTransport::new(|_| {
            Ok(ResponseEnvelope::Error(AtmError::daemon_unavailable(
                "synthetic daemon failure",
            )))
        }));
        let composition = CliComposition::from_fake_transport(transport, &observability);

        let error = composition
            .execute_request(RequestEnvelope::Doctor(atm_core::doctor::DoctorQuery {
                home_dir: tempdir.path().join("home"),
                current_dir: tempdir.path().join("cwd"),
                team_override: None,
                ..atm_core::doctor::DoctorQuery::default()
            }))
            .await
            .expect_err("protocol error");

        assert_eq!(
            error.code(),
            atm_core::error_codes::AtmErrorCode::DaemonUnavailable
        );
        assert!(error.to_string().contains("synthetic daemon failure"));
        assert!(error.message().contains("Recovery:"));
    }

    #[tokio::test]
    async fn cli_runtime_reload_uses_the_authenticated_shared_api_request() {
        let observability = CliObservability::fallback();
        let transport = Arc::new(FakeClientTransport::new(|request| {
            assert!(matches!(request, RequestEnvelope::ReloadRuntimeView));
            Ok(ResponseEnvelope::RuntimeViewReloaded)
        }));
        let composition = CliComposition::from_fake_transport(transport, &observability);

        composition
            .reload_runtime_view()
            .await
            .expect("CLI runtime reload response");
    }

    #[tokio::test]
    async fn cli_graft_daemon_and_read_preserve_one_chat_identity_contract() {
        let chat_id = "chat-42".parse::<ChatId>().expect("chat id");
        let send_request = SendRequest::new(
            std::path::PathBuf::from("/tmp/home"),
            std::path::PathBuf::from("/tmp/current"),
            TEST_SENDER.parse().expect("caller"),
            "recipient:target-chat@test-team",
            TEST_TEAM.parse().expect("team"),
            SendMessageSource::Inline("chat parity".to_string()),
            None,
            false,
            None,
            false,
        )
        .expect("send request")
        .with_caller_chat_id(Some(chat_id.clone()));
        let response = ResponseEnvelope::Send(SendResponseEnvelope::Sent(SendOutcome {
            action: CommandAction::Send,
            team: TEST_TEAM.parse().expect("team"),
            agent: TEST_RECIPIENT.parse().expect("recipient"),
            sender: TEST_SENDER.parse().expect("sender"),
            outcome: SendCommandOutcome::Sent,
            message_id: AtmMessageId::new(),
            requires_ack: false,
            task_id: None,
            summary: None,
            message: None,
            warnings: Vec::new(),
            dry_run: false,
        }));
        let cli_requests = Arc::new(Mutex::new(Vec::new()));
        let cli_transport = Arc::new(FakeClientTransport::new({
            let cli_requests = cli_requests.clone();
            let response = response.clone();
            move |request| {
                cli_requests.lock().expect("cli request log").push(request);
                Ok(response.clone())
            }
        }));
        let observability = CliObservability::fallback();
        CliComposition::from_fake_transport(cli_transport, &observability)
            .send(send_request.clone())
            .await
            .expect("cli send");

        let graft_requests = Arc::new(Mutex::new(Vec::new()));
        let graft_transport = Arc::new(FakeClientTransport::new({
            let graft_requests = graft_requests.clone();
            let response = response.clone();
            move |request| {
                graft_requests
                    .lock()
                    .expect("graft request log")
                    .push(request);
                Ok(response.clone())
            }
        }));
        atm_graft::GraftClient::from_fake_transport_for_test(graft_transport)
            .send_message(send_request)
            .await
            .expect("graft send");

        let cli_request = cli_requests
            .lock()
            .expect("cli request log")
            .pop()
            .expect("request");
        let graft_request = graft_requests
            .lock()
            .expect("graft request log")
            .pop()
            .expect("request");
        assert_eq!(
            serde_json::to_value(&cli_request).expect("cli JSON"),
            serde_json::to_value(&graft_request).expect("graft JSON")
        );
        let RequestEnvelope::Write(request) = cli_request else {
            panic!("daemon must receive the canonical compose write request");
        };
        assert_eq!(request.caller_chat_id, Some(chat_id.clone()));
        assert_eq!(
            request
                .to
                .as_ref()
                .and_then(|target| target.chat_id())
                .map(ToString::to_string)
                .as_deref(),
            Some("target-chat")
        );

        let read = ReadQuery::new(
            std::path::PathBuf::from("/tmp/home"),
            std::path::PathBuf::from("/tmp/current"),
            TEST_SENDER.parse().expect("caller"),
            None,
            TEST_TEAM.parse().expect("team"),
            ReadSelection::All,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("read query")
        .with_caller_chat_id(Some(chat_id));
        assert_eq!(
            read.caller_chat_id().map(ToString::to_string).as_deref(),
            Some("chat-42")
        );
    }

    #[tokio::test]
    #[serial(env)]
    async fn loopback_transport_send_persists_inbox_without_daemon() {
        let fixture = LoopbackFixture::new(TEST_RECIPIENT);
        let transport_observability = Arc::new(atm_core::observability::NullObservability);
        let composition_observability = CliObservability::fallback();
        let composition = CliComposition::from_loopback_transport(
            Arc::new(LoopbackClientTransport::new(transport_observability)),
            &composition_observability,
        );

        let outcome = composition
            .send(fixture.send_request("hello from loopback"))
            .await
            .expect("send outcome");

        assert_eq!(outcome.agent.as_str(), TEST_RECIPIENT);
        assert_eq!(outcome.sender.as_str(), TEST_SENDER);
        let inbox = fixture.inbox_contents(TEST_RECIPIENT);
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0].text, "hello from loopback");
        assert_eq!(inbox[0].from.as_str(), TEST_SENDER);
    }

    #[tokio::test]
    #[serial(env)]
    async fn loopback_transport_rejects_self_addressed_send_without_persisting_inbox() {
        let fixture = LoopbackFixture::new(TEST_RECIPIENT);
        let transport_observability = Arc::new(atm_core::observability::NullObservability);
        let composition_observability = CliObservability::fallback();
        let composition = CliComposition::from_loopback_transport(
            Arc::new(LoopbackClientTransport::new(transport_observability)),
            &composition_observability,
        );
        let self_address = format!("{TEST_SENDER}@{TEST_TEAM}");

        let error = composition
            .send(fixture.send_request_to(&self_address, "hello self"))
            .await
            .expect_err("self-addressed send must fail");

        assert_eq!(
            error.code(),
            atm_core::error_codes::AtmErrorCode::SelfAddressedSendInvalid
        );
        assert!(fixture.inbox_contents(TEST_SENDER).is_empty());
    }

    #[test]
    #[serial(env)]
    fn loopback_transport_no_longer_emits_missing_config_notice_under_concurrency() {
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
                first_transport.execute_for_test(ApiRequest::new(RequestEnvelope::Write(Box::new(
                    first_request,
                ))))
            });
            let second = scope.spawn(move || {
                second_transport.execute_for_test(ApiRequest::new(RequestEnvelope::Write(
                    Box::new(second_request),
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
                    error.code(),
                    atm_core::error_codes::AtmErrorCode::MailboxLockTimeout,
                    "{label} response: {result:?}"
                );
            }
        }
        let notices = fixture.inbox_contents(ROLE_TEAM_LEAD);
        assert_eq!(
            notices.len(),
            0,
            "missing config is no longer a runtime send fallback, so no team-lead repair notice should be emitted"
        );
    }

    #[tokio::test]
    #[serial(env)]
    async fn loopback_transport_send_preserves_ack_and_task_metadata_without_daemon() {
        let fixture = LoopbackFixture::new(TEST_RECIPIENT);
        let composition_observability = CliObservability::fallback();
        let composition = CliComposition::from_loopback_transport(
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
            .await
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

    #[tokio::test]
    #[serial(env)]
    async fn loopback_transport_phase_ad_messaging_regression_matrix_without_daemon() {
        let fixture = LoopbackFixture::new(TEST_RECIPIENT);
        let composition_observability = CliObservability::fallback();
        let composition = CliComposition::from_loopback_transport(
            Arc::new(LoopbackClientTransport::new(Arc::new(
                atm_core::observability::NullObservability,
            ))),
            &composition_observability,
        );

        // Plain informational send stays non-ack-requiring.
        let plain_outcome = composition
            .send(fixture.send_request("plain informational"))
            .await
            .expect("plain send outcome");
        assert!(!plain_outcome.requires_ack);
        let plain_message_id = plain_outcome.message_id;

        // Explicit requires-ack send persists durable pending-ack state.
        let ack_required_outcome = composition
            .send(fixture.send_request_with_flags("needs acknowledgement", true, None))
            .await
            .expect("ack-required send outcome");
        assert!(ack_required_outcome.requires_ack);
        let ack_required_message_id = ack_required_outcome.message_id;

        // Task send also persists durable pending-ack state.
        let task_outcome = composition
            .send(fixture.send_request_with_flags(
                "task payload",
                false,
                Some("TASK-314".parse().expect("task id")),
            ))
            .await
            .expect("task send outcome");
        assert!(task_outcome.requires_ack);
        let task_message_id = task_outcome.message_id;

        // Peek is the explicit non-mutating inspection path.
        let peek_outcome = composition
            .peek(fixture.peek_query_for(TEST_RECIPIENT, None, plain_message_id))
            .await
            .expect("peek outcome");
        assert!(!peek_outcome.mutation_applied);
        assert_eq!(peek_outcome.selected_message_id, Some(plain_message_id));
        assert_eq!(
            peek_outcome
                .message
                .as_ref()
                .map(|message| message.envelope.read),
            Some(false)
        );

        let inbox_after_peek = fixture.inbox_contents(TEST_RECIPIENT);
        let plain_after_peek = inbox_after_peek
            .iter()
            .find(|message| message.message_id == Some(plain_message_id))
            .expect("plain inbox message after peek");
        assert!(!plain_after_peek.read);
        assert!(plain_after_peek.pending_ack_at.is_none());
        assert!(plain_after_peek.acknowledged_at.is_none());

        // Cross-agent peek via target address also stays non-mutating.
        let cross_agent_peek = composition
            .peek(fixture.peek_query_for(
                TEST_SENDER,
                Some(TEST_RECIPIENT_ADDRESS),
                plain_message_id,
            ))
            .await
            .expect("cross-agent peek outcome");
        assert!(!cross_agent_peek.mutation_applied);
        assert_eq!(cross_agent_peek.selected_message_id, Some(plain_message_id));
        assert_eq!(
            cross_agent_peek
                .message
                .as_ref()
                .map(|message| message.envelope.read),
            Some(false)
        );

        let inbox_after_cross_agent_peek = fixture.inbox_contents(TEST_RECIPIENT);
        let plain_after_cross_agent_peek = inbox_after_cross_agent_peek
            .iter()
            .find(|message| message.message_id == Some(plain_message_id))
            .expect("plain inbox message after cross-agent peek");
        assert!(!plain_after_cross_agent_peek.read);
        assert!(plain_after_cross_agent_peek.pending_ack_at.is_none());
        assert!(plain_after_cross_agent_peek.acknowledged_at.is_none());

        // Read mutates read state but never manufactures pending-ack state.
        let read_outcome = composition
            .receive(fixture.read_query_for(TEST_RECIPIENT, plain_message_id))
            .await
            .expect("read outcome");
        assert!(read_outcome.mutation_applied);
        assert_eq!(read_outcome.selected_message_id, Some(plain_message_id));
        assert_eq!(
            read_outcome
                .message
                .as_ref()
                .map(|message| message.envelope.read),
            Some(true)
        );
        assert_eq!(
            read_outcome
                .message
                .as_ref()
                .and_then(|message| message.envelope.pending_ack_at),
            None
        );

        let inbox_after_read = fixture.inbox_contents(TEST_RECIPIENT);
        let plain_after_read = inbox_after_read
            .iter()
            .find(|message| message.message_id == Some(plain_message_id))
            .expect("plain inbox message after read");
        assert!(plain_after_read.read);
        assert!(plain_after_read.pending_ack_at.is_none());

        let ack_required_after_send = inbox_after_read
            .iter()
            .find(|message| message.message_id == Some(ack_required_message_id))
            .expect("ack-required inbox message");
        assert!(ack_required_after_send.pending_ack_at.is_some());

        let task_after_send = inbox_after_read
            .iter()
            .find(|message| message.message_id == Some(task_message_id))
            .expect("task inbox message");
        assert!(task_after_send.pending_ack_at.is_some());
        assert_eq!(
            task_after_send.task_id.as_ref().map(|value| value.as_str()),
            Some("TASK-314")
        );

        // Canonical same-team self-addressed sends fail before persistence.
        let self_address = format!("{TEST_SENDER}@{TEST_TEAM}");
        let error = composition
            .send(fixture.send_request_to(&self_address, "hello self"))
            .await
            .expect_err("self-addressed send must fail");
        assert_eq!(
            error.code(),
            atm_core::error_codes::AtmErrorCode::SelfAddressedSendInvalid
        );
    }

    #[tokio::test]
    #[serial(env)]
    async fn loopback_transport_read_surfaces_messages_without_daemon() {
        let fixture = LoopbackFixture::new(TEST_RECIPIENT);
        fixture.write_inbox_messages(TEST_RECIPIENT, &[fixture.message("read me", false)]);
        let composition_observability = CliObservability::fallback();
        let composition = CliComposition::from_loopback_transport(
            Arc::new(LoopbackClientTransport::new(Arc::new(
                atm_core::observability::NullObservability,
            ))),
            &composition_observability,
        );

        let outcome = composition
            .receive(fixture.read_query())
            .await
            .expect("read outcome");

        assert_eq!(outcome.agent.as_str(), TEST_RECIPIENT);
        assert_eq!(outcome.count, 1);
        assert_eq!(
            outcome.message.expect("selected message").envelope.text,
            "read me"
        );
    }

    #[tokio::test]
    #[serial(env)]
    async fn loopback_transport_read_rejects_cross_agent_target_without_daemon() {
        let fixture = LoopbackFixture::new(TEST_RECIPIENT);
        let composition_observability = CliObservability::fallback();
        let composition = CliComposition::from_loopback_transport(
            Arc::new(LoopbackClientTransport::new(Arc::new(
                atm_core::observability::NullObservability,
            ))),
            &composition_observability,
        );

        let error = composition
            .receive(
                ReadQuery::new(
                    fixture.home_dir.clone(),
                    fixture.current_dir.clone(),
                    TEST_SENDER.parse().expect("caller"),
                    Some(TEST_RECIPIENT_ADDRESS),
                    TEST_TEAM.parse().expect("team"),
                    ReadSelection::All,
                    false,
                    false,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                )
                .expect("query"),
            )
            .await
            .expect_err("cross-agent loopback read must fail");

        assert!(error.is_validation(), "{error:?}");
        assert!(
            error.message().contains("owner-only `atm read`"),
            "{error:?}"
        );
    }

    #[tokio::test]
    #[serial(env)]
    async fn loopback_transport_clear_removes_read_messages_without_daemon() {
        let fixture = LoopbackFixture::new(TEST_RECIPIENT);
        fixture.write_inbox_messages(TEST_RECIPIENT, &[fixture.message("done", true)]);
        let composition_observability = CliObservability::fallback();
        let composition = CliComposition::from_loopback_transport(
            Arc::new(LoopbackClientTransport::new(Arc::new(
                atm_core::observability::NullObservability,
            ))),
            &composition_observability,
        );

        let outcome = composition
            .clear(fixture.clear_query())
            .await
            .expect("clear outcome");

        assert_eq!(outcome.removed_total, 1);
        assert_eq!(outcome.remaining_total, 0);
        let read_outcome = composition
            .receive(fixture.read_query())
            .await
            .expect("read outcome after clear");
        assert_eq!(read_outcome.count, 0);
    }

    #[tokio::test]
    #[serial(env)]
    async fn loopback_transport_doctor_reports_health_without_daemon() {
        let fixture = LoopbackFixture::new(TEST_RECIPIENT);
        let observability = Arc::new(HealthyObservability);
        let composition_observability = CliObservability::fallback();
        let composition = CliComposition::from_loopback_transport(
            Arc::new(LoopbackClientTransport::new(observability)),
            &composition_observability,
        );

        let report = composition
            .doctor(fixture.doctor_query())
            .await
            .expect("doctor report");

        assert_eq!(report.summary.status, DoctorStatus::Healthy);
        assert_eq!(report.summary.error_count, 0);
    }

    #[tokio::test]
    #[serial(env)]
    async fn doctor_projects_bootstrap_trace_into_report() {
        let fixture = LoopbackFixture::new(TEST_RECIPIENT);
        let observability = Arc::new(HealthyObservability);
        let composition_observability = CliObservability::fallback();
        let composition = CliComposition::from_loopback_transport_with_bootstrap_trace(
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
            .await
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

    #[tokio::test]
    #[serial(env)]
    async fn loopback_transport_ack_appends_reply_without_daemon() {
        let fixture = LoopbackFixture::new(TEST_RECIPIENT);
        let (message_id, pending_ack) = fixture.pending_ack_message("please ack");
        fixture.write_inbox_messages(TEST_SENDER, &[pending_ack]);
        let composition_observability = CliObservability::fallback();
        let composition = CliComposition::from_loopback_transport(
            Arc::new(LoopbackClientTransport::new(Arc::new(
                atm_core::observability::NullObservability,
            ))),
            &composition_observability,
        );

        let outcome = composition
            .ack(fixture.ack_request(message_id, "received and starting"))
            .await
            .expect("ack outcome");

        assert_eq!(outcome.team.as_str(), TEST_TEAM);
        assert_eq!(outcome.agent.as_str(), TEST_SENDER);
        assert_eq!(outcome.message_id, message_id);
        assert!(matches!(
            &outcome.reply_disposition,
            atm_core::ack::AckReplyDisposition::Sent { reply_target, .. }
                if reply_target.to_string() == format!("{TEST_LEAD}@{TEST_TEAM}")
        ));

        let sender_inbox = fixture.inbox_contents(TEST_SENDER);
        assert_eq!(sender_inbox.len(), 1);
        assert!(sender_inbox[0].pending_ack_at.is_none());
        assert!(sender_inbox[0].acknowledged_at.is_some());
        let replies = fixture.inbox_contents(TEST_LEAD);
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].text, "received and starting");
        assert_eq!(replies[0].acknowledges_message_id, Some(message_id));
        assert!(replies[0].pending_ack_at.is_none());
    }

    #[tokio::test]
    #[serial(env)]
    async fn cli_composition_supports_graft_client_surface_without_daemon() {
        let fixture = LoopbackFixture::new(TEST_RECIPIENT);
        let composition_observability = CliObservability::fallback();
        let composition = CliComposition::from_loopback_transport(
            Arc::new(LoopbackClientTransport::new(Arc::new(
                atm_core::observability::NullObservability,
            ))),
            &composition_observability,
        );
        let client: &dyn AtmGraftClient = &composition;

        let send_outcome = client
            .send_message(fixture.send_request("graft send"))
            .await
            .expect("send through graft client surface");
        assert_eq!(send_outcome.sender.as_str(), TEST_SENDER);

        let read_outcome = client
            .read_message(fixture.read_query())
            .await
            .expect("read through graft client surface");
        assert_eq!(read_outcome.count, 1);

        let (message_id, pending_ack) = fixture.pending_ack_message("please ack");
        fixture.write_inbox_messages(TEST_SENDER, &[pending_ack]);
        let ack_outcome = client
            .acknowledge_message(fixture.ack_request(message_id, "received and starting"))
            .await
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
            error.code(),
            atm_core::error_codes::AtmErrorCode::AtmHomeUnresolved
        );
        assert!(logs.contains("raw cli runtime-root failure"));
        assert!(logs.contains("ATM_HOME_UNRESOLVED"));
    }

    #[test]
    #[serial(env)]
    fn resolve_command_runtime_context_reports_atm_home_unresolved() {
        let _env = EnvGuard::set_many([("ATM_HOME", None), ("HOME", None), ("USERPROFILE", None)]);

        let error =
            resolve_command_runtime_context("send").expect_err("missing ATM_HOME should fail");

        assert_eq!(
            error.code(),
            atm_core::error_codes::AtmErrorCode::AtmHomeUnresolved
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
            error.code(),
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
