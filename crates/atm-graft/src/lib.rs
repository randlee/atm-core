//! Thin embedded ATM client crate for graft-aware host agents.
//! Production embedded delivery uses a receiver-owned polling loop built on
//! shared unary ATM request/response calls.

use std::fmt;
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;
use std::time::Duration;

use atm_core::ack::{AckOutcome, AckRequest};
use atm_core::boundary::{ClientTransport, PostSendHookEvent};
use atm_core::error::AtmError;
use atm_core::graft::AtmGraftClient;
use atm_core::list::{ListOutcome, ListQuery};
use atm_core::observability::{
    CommandEvent, NullObservability, ObservabilityPort, action_name, outcome_label,
};
use atm_core::protocol::{
    RequestEnvelope, ResponseEnvelope, SendRequestEnvelope, SendResponseEnvelope,
};
use atm_core::read::{ReadOutcome, ReadQuery};
use atm_core::send::{SendOutcome, SendRequest};
use atm_core::types::{AgentName, TeamName};
use atm_daemon_client::{
    BootstrapTraceability, DaemonSupervisor, parse_bootstrap_agent, parse_bootstrap_team,
    resolve_daemon_bin, resolve_daemon_local_ipc_endpoint,
};

mod runtime;
mod transport;

use runtime::{
    ReceiveLoopContext, join_receive_loop_with_deadline, load_graft_config, read_snapshot,
    run_receive_loop, set_session_state,
};
use transport::{GraftLocalIpcClientTransport, unexpected_response};

const SAME_HOST_REQUEST_DEADLINE: Duration = Duration::from_secs(3);
pub(crate) const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(250);
pub(crate) const DEFAULT_LIST_LIMIT: usize = 200;
pub(crate) const RECEIVE_LOOP_JOIN_DEADLINE: Duration = Duration::from_secs(5);

pub use atm_core::{AtmConfig, GraftConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraftSessionState {
    Inactive,
    Polling,
    Degraded,
    Closed,
    CloseFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub team: TeamName,
    pub agent: AgentName,
    pub state: GraftSessionState,
}

/// Preferred host-facing imports for embedding `atm-graft`.
pub mod prelude {
    pub use super::{
        GraftClient, GraftObservability, GraftSession, GraftSessionOptions, GraftSessionState,
        HostNudgeInjector, NoopGraftObservability, SessionSnapshot,
    };
}

/// Host-owned bridge for automatic between-tool-call nudge injection.
pub trait HostNudgeInjector: Send + Sync {
    /// # Errors
    ///
    /// Returns [`AtmError`] when the host cannot safely inject the nudge into
    /// its between-tool-call context flow.
    fn inject_nudge(&self, nudge: PostSendHookEvent) -> Result<(), AtmError>;
}

/// ATM-owned graft observability boundary supplied by the embedding host.
pub trait GraftObservability: Send + Sync {
    fn session_state_changed(&self, _snapshot: &SessionSnapshot) {}

    fn nudge_delivered(&self, _snapshot: &SessionSnapshot, _nudge: &PostSendHookEvent) {}

    fn session_error(&self, _snapshot: &SessionSnapshot, _action: &'static str, _error: &AtmError) {
    }
}

/// No-op graft observability adapter.
#[derive(Debug, Default)]
pub struct NoopGraftObservability;

impl GraftObservability for NoopGraftObservability {}

/// Options used to activate one graft receiver loop.
#[derive(Debug, Clone)]
pub struct GraftSessionOptions {
    workspace_root: PathBuf,
    team: TeamName,
    agent: AgentName,
    poll_interval: Duration,
}

impl GraftSessionOptions {
    pub fn new(workspace_root: impl Into<PathBuf>, team: TeamName, agent: AgentName) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            team,
            agent,
            poll_interval: DEFAULT_POLL_INTERVAL,
        }
    }

    pub fn for_current_process(
        workspace_root: impl Into<PathBuf>,
        team: TeamName,
        agent: AgentName,
    ) -> Self {
        Self::new(workspace_root, team, agent)
    }

    #[cfg(test)]
    fn with_poll_interval(mut self, poll_interval: Duration) -> Self {
        assert!(!poll_interval.is_zero(), "poll_interval must be non-zero");
        self.poll_interval = poll_interval;
        self
    }

    fn activation_state(&self) -> SessionSnapshot {
        SessionSnapshot {
            team: self.team.clone(),
            agent: self.agent.clone(),
            state: GraftSessionState::Inactive,
        }
    }

    pub(crate) fn workspace_root(&self) -> &std::path::Path {
        &self.workspace_root
    }

    pub(crate) fn team(&self) -> &TeamName {
        &self.team
    }

    pub(crate) fn agent(&self) -> &AgentName {
        &self.agent
    }

    pub(crate) fn poll_interval(&self) -> Duration {
        self.poll_interval
    }
}

/// Thin daemon-backed same-host client for embedded graft consumers.
#[derive(Clone)]
pub struct GraftClient {
    transport: Arc<dyn ClientTransport + Send + Sync>,
}

pub(crate) trait GraftSessionClient: AtmGraftClient {
    fn list_messages(&self, query: ListQuery) -> Result<ListOutcome, AtmError>;
}

impl fmt::Debug for GraftClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GraftClient")
            .field("transport", &"dyn ClientTransport")
            .finish()
    }
}

impl GraftClient {
    /// # Errors
    ///
    /// Returns [`AtmError`] when the daemon endpoint or daemon binary cannot
    /// be resolved or the same-host daemon cannot be reached or started.
    pub fn connect() -> Result<Self, AtmError> {
        let endpoint = resolve_daemon_local_ipc_endpoint()?;
        let daemon_bin = resolve_daemon_bin("graft host")?;
        let transport = Arc::new(GraftLocalIpcClientTransport::new(endpoint.clone()));
        let supervisor = DaemonSupervisor::new(endpoint, daemon_bin);
        let observability = NullObservability;
        let emit_bootstrap_event = |event: atm_daemon_client::BootstrapCommandEvent| {
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
            "graft_connect",
            &emit_bootstrap_event,
            parse_bootstrap_team()?,
            parse_bootstrap_agent()?,
        );
        supervisor.ensure_daemon_available_with_traceability(&traceability, || {
            transport.probe_connection()
        })?;
        Ok(Self {
            transport: transport as Arc<dyn ClientTransport + Send + Sync>,
        })
    }

    #[cfg(test)]
    fn from_transport(transport: Arc<dyn ClientTransport + Send + Sync>) -> Self {
        Self { transport }
    }

    /// # Errors
    ///
    /// Returns [`AtmError`] when graft activation fails after configuration
    /// gating has permitted session startup.
    pub fn activate_session(
        &self,
        options: GraftSessionOptions,
        injector: Arc<dyn HostNudgeInjector>,
    ) -> Result<GraftSession, AtmError> {
        GraftSession::activate_with_observability(
            self.clone(),
            options,
            injector,
            Arc::new(NoopGraftObservability),
        )
    }

    fn send_request(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        match self.transport.send(request)? {
            ResponseEnvelope::Error(error) => Err(error.into_atm_error()),
            response => Ok(response),
        }
    }
}

impl AtmGraftClient for GraftClient {
    fn send_message(&self, request: SendRequest) -> Result<SendOutcome, AtmError> {
        match self.send_request(RequestEnvelope::Send(SendRequestEnvelope::Compose(request)))? {
            ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => Ok(outcome),
            other => Err(unexpected_response("send", other)),
        }
    }

    fn read_message(&self, query: ReadQuery) -> Result<ReadOutcome, AtmError> {
        match self.send_request(RequestEnvelope::Receive(query))? {
            ResponseEnvelope::Receive(outcome) => Ok(*outcome),
            other => Err(unexpected_response("read", other)),
        }
    }

    fn acknowledge_message(&self, request: AckRequest) -> Result<AckOutcome, AtmError> {
        match self.send_request(RequestEnvelope::Send(SendRequestEnvelope::Acknowledge(
            request,
        )))? {
            ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(outcome)) => Ok(outcome),
            other => Err(unexpected_response("ack", other)),
        }
    }
}

impl GraftSessionClient for GraftClient {
    fn list_messages(&self, query: ListQuery) -> Result<ListOutcome, AtmError> {
        match self.send_request(RequestEnvelope::List(query))? {
            ResponseEnvelope::List(outcome) => Ok(outcome),
            other => Err(unexpected_response("list", other)),
        }
    }
}

/// Concrete embedded graft session runtime.
pub struct GraftSession {
    client: Arc<dyn GraftSessionClient>,
    snapshot: Arc<RwLock<SessionSnapshot>>,
    observability: Arc<dyn GraftObservability>,
    stop_tx: Option<Sender<()>>,
    join_handle: Option<JoinHandle<Result<(), AtmError>>>,
}

type GraftReceiveLoopHandle = JoinHandle<Result<(), AtmError>>;
type GraftReceiveLoopWorker = (Sender<()>, GraftReceiveLoopHandle);

impl fmt::Debug for GraftSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let snapshot = match self.snapshot() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return f
                    .debug_struct("GraftSession")
                    .field("snapshot_error", &error)
                    .finish();
            }
        };
        f.debug_struct("GraftSession")
            .field("snapshot", &snapshot)
            .finish()
    }
}

impl GraftSession {
    /// # Errors
    ///
    /// Returns [`AtmError`] when configuration gating allows graft mode but
    /// receiver-loop startup fails.
    pub fn activate(
        client: GraftClient,
        options: GraftSessionOptions,
        injector: Arc<dyn HostNudgeInjector>,
    ) -> Result<Self, AtmError> {
        Self::activate_with_observability(
            client,
            options,
            injector,
            Arc::new(NoopGraftObservability),
        )
    }

    /// # Errors
    ///
    /// Returns [`AtmError`] when configuration gating allows graft mode but
    /// receiver-loop startup fails.
    pub fn activate_with_observability(
        client: GraftClient,
        options: GraftSessionOptions,
        injector: Arc<dyn HostNudgeInjector>,
        observability: Arc<dyn GraftObservability>,
    ) -> Result<Self, AtmError> {
        let graft_config = load_graft_config(&options.workspace_root)?;
        let home_dir = atm_core::home::atm_home()?;
        Self::activate_with_graft_config_and_home_dir(
            Arc::new(client),
            graft_config,
            options,
            injector,
            observability,
            home_dir,
        )
    }

    #[cfg(test)]
    fn activate_with_graft_config_and_home_dir(
        client: Arc<dyn GraftSessionClient>,
        graft_config: Option<GraftConfig>,
        options: GraftSessionOptions,
        injector: Arc<dyn HostNudgeInjector>,
        observability: Arc<dyn GraftObservability>,
        home_dir: PathBuf,
    ) -> Result<Self, AtmError> {
        let initial_snapshot = options.activation_state();
        let snapshot = Arc::new(RwLock::new(initial_snapshot));

        let Some(graft_config) = graft_config else {
            return inactive_session(client, snapshot, observability);
        };
        if !graft_config.enabled {
            return inactive_session(client, snapshot, observability);
        }

        set_session_state(
            &snapshot,
            GraftSessionState::Polling,
            observability.as_ref(),
        )?;
        let (stop_tx, join_handle) = Self::start_graft_receive_loop(
            Arc::clone(&client),
            options,
            home_dir,
            Arc::clone(&snapshot),
            injector,
            Arc::clone(&observability),
        )?;

        Ok(Self {
            client,
            snapshot,
            observability,
            stop_tx: Some(stop_tx),
            join_handle: Some(join_handle),
        })
    }

    #[cfg(not(test))]
    fn activate_with_graft_config_and_home_dir(
        client: Arc<dyn GraftSessionClient>,
        graft_config: Option<GraftConfig>,
        options: GraftSessionOptions,
        injector: Arc<dyn HostNudgeInjector>,
        observability: Arc<dyn GraftObservability>,
        home_dir: PathBuf,
    ) -> Result<Self, AtmError> {
        let initial_snapshot = options.activation_state();
        let snapshot = Arc::new(RwLock::new(initial_snapshot));

        let Some(graft_config) = graft_config else {
            return inactive_session(client, snapshot, observability);
        };
        if !graft_config.enabled {
            return inactive_session(client, snapshot, observability);
        }

        set_session_state(
            &snapshot,
            GraftSessionState::Polling,
            observability.as_ref(),
        )?;
        let (stop_tx, join_handle) = Self::start_graft_receive_loop(
            Arc::clone(&client),
            options,
            home_dir,
            Arc::clone(&snapshot),
            injector,
            Arc::clone(&observability),
        )?;

        Ok(Self {
            client,
            snapshot,
            observability,
            stop_tx: Some(stop_tx),
            join_handle: Some(join_handle),
        })
    }

    fn start_graft_receive_loop(
        worker_client: Arc<dyn GraftSessionClient>,
        options: GraftSessionOptions,
        home_dir: PathBuf,
        worker_snapshot: Arc<RwLock<SessionSnapshot>>,
        injector: Arc<dyn HostNudgeInjector>,
        worker_observability: Arc<dyn GraftObservability>,
    ) -> Result<GraftReceiveLoopWorker, AtmError> {
        let (stop_tx, stop_rx) = mpsc::channel();
        let join_handle = spawn_graft_receive_loop(
            worker_client,
            options,
            home_dir,
            worker_snapshot,
            injector,
            worker_observability,
            stop_rx,
        )?;
        Ok((stop_tx, join_handle))
    }

    pub fn snapshot(&self) -> Result<SessionSnapshot, AtmError> {
        read_snapshot(&self.snapshot)
    }

    pub fn send(&self, request: SendRequest) -> Result<SendOutcome, AtmError> {
        self.client.send_message(request)
    }

    pub fn read(&self, query: ReadQuery) -> Result<ReadOutcome, AtmError> {
        self.client.read_message(query)
    }

    pub fn ack(&self, request: AckRequest) -> Result<AckOutcome, AtmError> {
        self.client.acknowledge_message(request)
    }

    /// # Errors
    ///
    /// Returns [`AtmError`] when the receive loop cannot join cleanly during
    /// shutdown.
    pub fn close(mut self) -> Result<(), AtmError> {
        self.close_internal()
    }

    fn close_internal(&mut self) -> Result<(), AtmError> {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());
        }
        if let Some(join_handle) = self.join_handle.take()
            && let Err(error) = join_receive_loop_with_deadline(join_handle)
        {
            set_session_state(
                &self.snapshot,
                GraftSessionState::CloseFailed,
                self.observability.as_ref(),
            )?;
            return Err(error);
        }
        set_session_state(
            &self.snapshot,
            GraftSessionState::Closed,
            self.observability.as_ref(),
        )?;
        Ok(())
    }
}

fn inactive_session(
    client: Arc<dyn GraftSessionClient>,
    snapshot: Arc<RwLock<SessionSnapshot>>,
    observability: Arc<dyn GraftObservability>,
) -> Result<GraftSession, AtmError> {
    observability.session_state_changed(&read_snapshot(&snapshot)?);
    Ok(GraftSession {
        client,
        snapshot,
        observability,
        stop_tx: None,
        join_handle: None,
    })
}

fn spawn_graft_receive_loop(
    worker_client: Arc<dyn GraftSessionClient>,
    options: GraftSessionOptions,
    home_dir: PathBuf,
    worker_snapshot: Arc<RwLock<SessionSnapshot>>,
    injector: Arc<dyn HostNudgeInjector>,
    worker_observability: Arc<dyn GraftObservability>,
    stop_rx: std::sync::mpsc::Receiver<()>,
) -> Result<std::thread::JoinHandle<Result<(), AtmError>>, AtmError> {
    let thread_name = format!("atm-graft-{}", options.agent());
    std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            run_receive_loop(ReceiveLoopContext {
                client: worker_client,
                options,
                home_dir,
                snapshot: worker_snapshot,
                injector,
                observability: worker_observability,
                stop_rx,
            })
        })
        .map_err(spawn_receive_loop_error)
}

fn spawn_receive_loop_error(source: std::io::Error) -> AtmError {
    AtmError::daemon_unavailable("failed to spawn graft receive loop")
        .with_source(source)
        .with_recovery(
            "Retry graft activation after the embedding host allows one live receive thread for the active session.",
        )
}

impl Drop for GraftSession {
    fn drop(&mut self) {
        if let Err(error) = self.close_internal() {
            let identity = self
                .snapshot()
                .map(|snapshot| format!("{}@{}", snapshot.agent, snapshot.team))
                .unwrap_or_else(|snapshot_error| format!("unavailable:{snapshot_error}"));
            tracing::warn!(
                identity,
                error_code = %error.code,
                error_message = %error.message,
                "graft session drop cleanup failed"
            );
        }
    }
}

impl AtmGraftClient for GraftSession {
    fn send_message(&self, request: SendRequest) -> Result<SendOutcome, AtmError> {
        self.client.send_message(request)
    }

    fn read_message(&self, query: ReadQuery) -> Result<ReadOutcome, AtmError> {
        self.client.read_message(query)
    }

    fn acknowledge_message(&self, request: AckRequest) -> Result<AckOutcome, AtmError> {
        self.client.acknowledge_message(request)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use atm_core::list::{ListOutcome, ListQuery};
    use atm_core::protocol::{
        RequestEnvelope as CoreRequestEnvelope, ResponseEnvelope as CoreResponseEnvelope,
    };
    use atm_core::read::{BucketCounts, ReadOutcome};
    use atm_core::send::SendCommandOutcome;
    use atm_core::test_support::{EnvGuard, TEST_LEAD, TEST_TEAM};
    use atm_core::transport::testing::FakeClientTransport;
    use atm_core::types::{AgentName, CommandAction, ReadSelection, TeamName};
    use serde_json::json;

    use super::*;

    #[derive(Debug, Default)]
    struct NoopInjector;

    impl HostNudgeInjector for NoopInjector {
        fn inject_nudge(&self, _nudge: PostSendHookEvent) -> Result<(), AtmError> {
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    struct StubSessionClient;

    impl AtmGraftClient for StubSessionClient {
        fn send_message(&self, _request: SendRequest) -> Result<SendOutcome, AtmError> {
            panic!("send_message should not run in inactive-session tests")
        }

        fn read_message(&self, _query: ReadQuery) -> Result<ReadOutcome, AtmError> {
            panic!("read_message should not run in inactive-session tests")
        }

        fn acknowledge_message(&self, _request: AckRequest) -> Result<AckOutcome, AtmError> {
            panic!("acknowledge_message should not run in inactive-session tests")
        }
    }

    impl GraftSessionClient for StubSessionClient {
        fn list_messages(&self, _query: ListQuery) -> Result<ListOutcome, AtmError> {
            panic!("list_messages should not run in inactive-session tests")
        }
    }

    fn session_options() -> GraftSessionOptions {
        GraftSessionOptions::for_current_process(
            PathBuf::from("/tmp/workspace"),
            TeamName::from_validated(TEST_TEAM),
            AgentName::from_validated("qa-a"),
        )
        .with_poll_interval(Duration::from_millis(5))
    }

    #[test]
    fn client_routes_send_read_and_ack_over_transport() {
        let transport = Arc::new(FakeClientTransport::new(Box::new(
            |request| match request {
                CoreRequestEnvelope::Send(SendRequestEnvelope::Compose(_)) => Ok(
                    CoreResponseEnvelope::Send(SendResponseEnvelope::Sent(SendOutcome {
                        action: CommandAction::Send,
                        team: TeamName::from_validated(TEST_TEAM),
                        agent: AgentName::from_validated(TEST_LEAD),
                        sender: AgentName::from_validated(TEST_LEAD),
                        outcome: SendCommandOutcome::Sent,
                        message_id: atm_core::schema::AtmMessageId::new(),
                        requires_ack: false,
                        task_id: None,
                        summary: None,
                        message: None,
                        warnings: Vec::new(),
                        dry_run: false,
                    })),
                ),
                CoreRequestEnvelope::Receive(_) => {
                    Ok(CoreResponseEnvelope::Receive(Box::new(ReadOutcome {
                        action: CommandAction::Read,
                        team: TeamName::from_validated(TEST_TEAM),
                        agent: AgentName::from_validated(TEST_LEAD),
                        selection_mode: ReadSelection::Unread,
                        mutation_applied: false,
                        count: 0,
                        message: None,
                        selected_message_id: None,
                        match_count: 0,
                        additional_match_count: 0,
                        bucket_counts: BucketCounts {
                            unread: 0,
                            pending_ack: 0,
                            history: 0,
                        },
                    })))
                }
                CoreRequestEnvelope::Send(SendRequestEnvelope::Acknowledge(_)) => Ok(
                    CoreResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(
                        serde_json::from_value(json!({
                            "action": "ack",
                            "team": TEST_TEAM,
                            "agent": TEST_LEAD,
                            "message_id": atm_core::schema::AtmMessageId::new().to_string(),
                            "task_id": null,
                            "reply_target": format!("{TEST_LEAD}@{TEST_TEAM}"),
                            "reply_message_id": atm_core::schema::AtmMessageId::new().to_string(),
                            "reply_text": "ack",
                            "warnings": [],
                        }))
                        .expect("ack outcome"),
                    )),
                ),
                other => panic!("unexpected request: {other:?}"),
            },
        )));
        let client = GraftClient::from_transport(transport);

        let send_request = SendRequest::new(
            PathBuf::from("/tmp/home"),
            PathBuf::from("/tmp/workspace"),
            AgentName::from_validated(TEST_LEAD),
            "qa-a@test-team",
            TeamName::from_validated(TEST_TEAM),
            atm_core::send::SendMessageSource::Inline("ping".to_string()),
            None,
            false,
            None,
            false,
        )
        .expect("send request");
        client.send_message(send_request).expect("send");

        let read_query = ReadQuery::new(
            PathBuf::from("/tmp/home"),
            PathBuf::from("/tmp/workspace"),
            AgentName::from_validated(TEST_LEAD),
            None,
            TeamName::from_validated(TEST_TEAM),
            ReadSelection::Unread,
            false,
            false,
            atm_core::types::AckActivationMode::ReadOnly,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("read query");
        client.read_message(read_query).expect("read");

        let ack_request = AckRequest {
            home_dir: PathBuf::from("/tmp/home"),
            current_dir: PathBuf::from("/tmp/workspace"),
            caller_identity: AgentName::from_validated(TEST_LEAD),
            caller_team: TeamName::from_validated(TEST_TEAM),
            message_id: atm_core::schema::AtmMessageId::new(),
            reply_body: "ack".to_string(),
        };
        client.acknowledge_message(ack_request).expect("ack");
    }

    #[test]
    fn session_stays_inactive_without_atm_config() {
        let home_dir = tempfile::TempDir::new().expect("tempdir");
        let session = GraftSession::activate_with_graft_config_and_home_dir(
            Arc::new(StubSessionClient),
            None,
            session_options(),
            Arc::new(NoopInjector),
            Arc::new(NoopGraftObservability),
            home_dir.path().to_path_buf(),
        )
        .expect("inactive session");

        assert_eq!(
            session.snapshot().expect("snapshot").state,
            GraftSessionState::Inactive
        );
    }

    #[test]
    fn session_stays_inactive_when_graft_is_disabled() {
        let home_dir = tempfile::TempDir::new().expect("tempdir");
        let session = GraftSession::activate_with_graft_config_and_home_dir(
            Arc::new(StubSessionClient),
            Some(GraftConfig { enabled: false }),
            session_options(),
            Arc::new(NoopInjector),
            Arc::new(NoopGraftObservability),
            home_dir.path().to_path_buf(),
        )
        .expect("inactive session");

        assert_eq!(
            session.snapshot().expect("snapshot").state,
            GraftSessionState::Inactive
        );
    }

    #[test]
    fn load_config_drives_public_activation_path() {
        let tempdir = tempfile::TempDir::new().expect("tempdir");
        std::fs::write(
            tempdir.path().join(".atm.toml"),
            "[atm.graft]\nenabled = false\n",
        )
        .expect("write config");
        let _env = EnvGuard::set_many([
            (
                "ATM_HOME",
                Some(tempdir.path().to_str().expect("utf8 home")),
            ),
            ("HOME", Some(tempdir.path().to_str().expect("utf8 home"))),
            ("USERPROFILE", None),
        ]);

        let session = GraftSession::activate_with_graft_config_and_home_dir(
            Arc::new(StubSessionClient),
            load_graft_config(tempdir.path()).expect("graft config"),
            GraftSessionOptions::for_current_process(
                tempdir.path(),
                TeamName::from_validated(TEST_TEAM),
                AgentName::from_validated("qa-a"),
            ),
            Arc::new(NoopInjector),
            Arc::new(NoopGraftObservability),
            tempdir.path().to_path_buf(),
        )
        .expect("inactive session");

        assert_eq!(
            session.snapshot().expect("snapshot").state,
            GraftSessionState::Inactive
        );
    }
}
