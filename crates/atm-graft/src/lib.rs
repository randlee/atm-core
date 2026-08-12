//! Thin embedded ATM client crate for graft-aware host agents.
//! Production embedded delivery uses a receiver-owned same-host listener that
//! accepts one bounded nudge request per connection.
use std::fmt;
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;
use std::time::Duration;

use atm_core::api::{ApiRequest, DaemonApiClient};
use atm_core::boundary::PostSendHookEvent;
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::graft::AtmGraftClient;
use atm_core::list::{ListOutcome, ListQuery};
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope, SendResponseEnvelope};
use atm_core::read::{ReadOutcome, ReadQuery};
use atm_core::send::{SendOutcome, SendRequest, WriteOutcome};
use atm_core::types::{AgentName, ChatId, TeamName};
use atm_daemon_client::{resolve_daemon_local_ipc_endpoint, unexpected_response};
use atm_http_runtime::SAME_HOST_REQUEST_DEADLINE;

mod nudge_sink;
mod runtime;

use runtime::{
    GraftReceiverLoopContext, RECEIVE_LOOP_READY_DEADLINE, ReceiverReadyLatch,
    join_receive_loop_with_deadline, load_graft_config, read_snapshot, run_graft_receiver_loop,
    set_session_state,
};

pub(crate) const RECEIVE_LOOP_JOIN_DEADLINE: Duration = Duration::from_secs(5);

pub use atm_core::{AtmConfig, GraftConfig};

/// Count-only durable mailbox work projection for graft recovery notices.
///
/// This intentionally contains no message body, identifier, or mutable state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MailboxWorkCounts {
    pub unread: usize,
    pub pending_ack: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraftSessionState {
    Inactive,
    Listening,
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

/// One nudge as presented to a host-owned agent session.
///
/// `body` is the canonical ATM dispatch payload for the agent loop.  The
/// separately rendered `notice_text` is safe, human-facing context for the
/// user-visible gateway channel; it must never be inferred by parsing `body`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostNudge {
    pub event: PostSendHookEvent,
    pub body: String,
    pub notice_text: String,
}

/// Host-owned bridge for automatic between-tool-call nudge injection.
pub trait HostNudgeInjector: Send + Sync {
    /// # Errors
    ///
    /// Returns [`AtmError`] when the host cannot safely inject the nudge into
    /// its between-tool-call context flow.
    fn inject_nudge(&self, nudge: &HostNudge) -> Result<(), AtmError>;
}

/// ATM-owned graft observability boundary supplied by the embedding host.
pub trait GraftObservability: Send + Sync {
    fn session_state_changed(&self, _snapshot: &SessionSnapshot) {}

    fn nudge_delivered(&self, _snapshot: &SessionSnapshot, _nudge: &PostSendHookEvent) {}

    fn session_error(&self, _snapshot: &SessionSnapshot, _action: &'static str, _error: &AtmError) {
    }

    /// Records receiver-ownership lifecycle without exposing endpoint capability material.
    fn receiver_ownership(
        &self,
        _snapshot: &SessionSnapshot,
        _action: &'static str,
        _outcome: &'static str,
    ) {
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
    owner_chat_id: Option<ChatId>,
}

impl GraftSessionOptions {
    pub fn new(workspace_root: impl Into<PathBuf>, team: TeamName, agent: AgentName) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            team,
            agent,
            owner_chat_id: None,
        }
    }

    /// Retain the host session identity as receiver-owner metadata.
    #[must_use]
    pub fn with_owner_chat_id(mut self, owner_chat_id: Option<ChatId>) -> Self {
        self.owner_chat_id = owner_chat_id;
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

    pub(crate) fn owner_chat_id(&self) -> Option<ChatId> {
        self.owner_chat_id.clone()
    }
}

/// Thin daemon-backed same-host client for embedded graft consumers.
#[derive(Clone)]
pub struct GraftClient {
    /// The one Tokio/Axum client boundary for every graft daemon operation.
    async_transport: Arc<dyn DaemonApiClient + Send + Sync>,
}

impl fmt::Debug for GraftClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GraftClient")
            .field("async_transport", &"dyn DaemonApiClient")
            .finish()
    }
}

impl GraftClient {
    /// Connect only to the daemon selected and already running for this host.
    ///
    /// This deliberately never resolves a daemon executable or invokes the
    /// supervisor. Embedded hosts use it so a graft session cannot create a
    /// competing local daemon while the operator-owned runtime is being
    /// switched or recovered.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when the published local endpoint is absent or the
    /// selected daemon cannot be reached. Recover by restoring the one managed
    /// runtime through `/daemon-switch`, then verify `atm doctor --json`.
    pub fn connect_existing() -> Result<Self, AtmError> {
        let endpoint = resolve_daemon_local_ipc_endpoint()?;
        Self::from_existing_endpoint(endpoint)
    }

    fn from_existing_endpoint(
        endpoint: atm_daemon_client::DaemonLocalIpcEndpoint,
    ) -> Result<Self, AtmError> {
        Ok(Self {
            async_transport: atm_http_runtime::preferred_local_client(
                endpoint.as_ref(),
                SAME_HOST_REQUEST_DEADLINE,
            )?,
        })
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn from_fake_transport_for_test(
        transport: Arc<atm_core::transport::testing::FakeClientTransport>,
    ) -> Self {
        Self {
            async_transport: transport,
        }
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
            options,
            injector,
            Arc::new(NoopGraftObservability),
        )
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

    /// Execute the one canonical write operation, including acknowledgement writes.
    pub async fn write_message(&self, request: SendRequest) -> Result<WriteOutcome, AtmError> {
        let transport =
            atm_http_runtime::selected_write_transport(&request, &self.async_transport)?;
        match transport
            .execute(ApiRequest::new(RequestEnvelope::Write(Box::new(request))))
            .await?
            .into_inner()
        {
            ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => {
                Ok(WriteOutcome::Sent(outcome))
            }
            ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(outcome)) => {
                Ok(WriteOutcome::Acknowledged(outcome))
            }
            other => Err(unexpected_response("write", other)),
        }
    }

    /// Read the daemon's existing mailbox bucket counts without mutating mail.
    pub async fn mailbox_work_counts(
        &self,
        query: ReadQuery,
    ) -> Result<MailboxWorkCounts, AtmError> {
        if query.seen_state_update() {
            return Err(AtmError::new(
                AtmErrorCode::CallerContextRequestInvalid,
                "mailbox work counts require a non-mutating read query",
            ));
        }
        let outcome = self.read_message(query).await?;
        Ok(MailboxWorkCounts {
            unread: outcome.bucket_counts.unread,
            pending_ack: outcome.bucket_counts.pending_ack,
        })
    }
}

#[async_trait::async_trait]
impl AtmGraftClient for GraftClient {
    async fn send_message(&self, request: SendRequest) -> Result<SendOutcome, AtmError> {
        let transport =
            atm_http_runtime::selected_write_transport(&request, &self.async_transport)?;
        match transport
            .execute(ApiRequest::new(RequestEnvelope::Write(Box::new(request))))
            .await?
            .into_inner()
        {
            ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => Ok(outcome),
            other => Err(unexpected_response("send", other)),
        }
    }

    async fn read_message(&self, query: ReadQuery) -> Result<ReadOutcome, AtmError> {
        match self
            .execute_request(RequestEnvelope::Receive(query))
            .await?
        {
            ResponseEnvelope::Receive(outcome) => Ok(*outcome),
            other => Err(unexpected_response("read", other)),
        }
    }

    async fn list_messages(&self, query: ListQuery) -> Result<ListOutcome, AtmError> {
        match self.execute_request(RequestEnvelope::List(query)).await? {
            ResponseEnvelope::List(outcome) => Ok(outcome),
            other => Err(unexpected_response("list", other)),
        }
    }
}

/// Concrete embedded graft session runtime.
pub struct GraftSession {
    // Reads dominate (status projection and hook delivery) while updates only
    // replace a complete snapshot, so an RwLock permits concurrent readers
    // without exposing partial session state to a receiver callback.
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
        options: GraftSessionOptions,
        injector: Arc<dyn HostNudgeInjector>,
    ) -> Result<Self, AtmError> {
        Self::activate_with_observability(options, injector, Arc::new(NoopGraftObservability))
    }

    /// # Errors
    ///
    /// Returns [`AtmError`] when configuration gating allows graft mode but
    /// receiver-loop startup fails.
    pub fn activate_with_observability(
        options: GraftSessionOptions,
        injector: Arc<dyn HostNudgeInjector>,
        observability: Arc<dyn GraftObservability>,
    ) -> Result<Self, AtmError> {
        let graft_config = load_graft_config(&options.workspace_root)?;
        Self::activate_with_graft_config(graft_config, options, injector, observability)
    }

    fn activate_with_graft_config(
        graft_config: Option<GraftConfig>,
        options: GraftSessionOptions,
        injector: Arc<dyn HostNudgeInjector>,
        observability: Arc<dyn GraftObservability>,
    ) -> Result<Self, AtmError> {
        let initial_snapshot = options.activation_state();
        let snapshot = Arc::new(RwLock::new(initial_snapshot));

        let Some(graft_config) = graft_config else {
            return inactive_session(snapshot, observability);
        };
        if !graft_config.enabled {
            return inactive_session(snapshot, observability);
        }

        let endpoint_path = atm_core::graft::graft_receiver_record_path_from_root(
            options.workspace_root(),
            options.team(),
            options.agent(),
        );
        let (stop_tx, join_handle) = Self::start_graft_receive_loop(
            endpoint_path,
            options,
            Arc::clone(&snapshot),
            injector,
            Arc::clone(&observability),
        )?;

        set_session_state(
            &snapshot,
            GraftSessionState::Listening,
            observability.as_ref(),
        )?;
        Ok(Self {
            snapshot,
            observability,
            stop_tx: Some(stop_tx),
            join_handle: Some(join_handle),
        })
    }

    fn start_graft_receive_loop(
        endpoint_path: PathBuf,
        options: GraftSessionOptions,
        worker_snapshot: Arc<RwLock<SessionSnapshot>>,
        injector: Arc<dyn HostNudgeInjector>,
        worker_observability: Arc<dyn GraftObservability>,
    ) -> Result<GraftReceiveLoopWorker, AtmError> {
        let (stop_tx, stop_rx) = mpsc::channel();
        let ready_latch = ReceiverReadyLatch::new();
        let join_handle = spawn_graft_receive_loop(
            endpoint_path,
            options,
            worker_snapshot,
            injector,
            worker_observability,
            ready_latch.notifier(),
            stop_rx,
        )?;
        ready_latch.wait_until_listening(RECEIVE_LOOP_READY_DEADLINE)?;
        Ok((stop_tx, join_handle))
    }

    pub fn snapshot(&self) -> Result<SessionSnapshot, AtmError> {
        read_snapshot(&self.snapshot)
    }

    /// # Errors
    ///
    /// Returns [`AtmError`] when the receive loop cannot join cleanly during
    /// shutdown.
    pub fn close(mut self) -> Result<(), AtmError> {
        self.close_internal()
    }

    fn close_internal(&mut self) -> Result<(), AtmError> {
        // The non-blocking accept loop notices the stop signal within one poll
        // interval, so stopping needs no wake-by-connect side channel.
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
    snapshot: Arc<RwLock<SessionSnapshot>>,
    observability: Arc<dyn GraftObservability>,
) -> Result<GraftSession, AtmError> {
    observability.session_state_changed(&read_snapshot(&snapshot)?);
    Ok(GraftSession {
        snapshot,
        observability,
        stop_tx: None,
        join_handle: None,
    })
}

fn spawn_graft_receive_loop(
    endpoint_path: PathBuf,
    options: GraftSessionOptions,
    worker_snapshot: Arc<RwLock<SessionSnapshot>>,
    injector: Arc<dyn HostNudgeInjector>,
    worker_observability: Arc<dyn GraftObservability>,
    ready_tx: std::sync::mpsc::SyncSender<()>,
    stop_rx: std::sync::mpsc::Receiver<()>,
) -> Result<std::thread::JoinHandle<Result<(), AtmError>>, AtmError> {
    let thread_name = format!("atm-graft-{}", options.agent());
    std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            run_graft_receiver_loop(GraftReceiverLoopContext {
                endpoint_path,
                owner_chat_id: options.owner_chat_id(),
                snapshot: worker_snapshot,
                injector,
                observability: worker_observability,
                stop_rx,
                ready_tx: Some(ready_tx),
            })
        })
        .map_err(spawn_receive_loop_error)
}

fn spawn_receive_loop_error(source: std::io::Error) -> AtmError {
    AtmError::daemon_unavailable("failed to spawn graft receive loop").with_cause(source)
}

impl Drop for GraftSession {
    fn drop(&mut self) {
        // Drop performs bounded blocking shutdown because close_internal()
        // stops the receive loop and waits for its join deadline before this
        // session can release ownership cleanly.
        if let Err(error) = self.close_internal() {
            let identity = self
                .snapshot()
                .map(|snapshot| format!("{}@{}", snapshot.agent, snapshot.team))
                .unwrap_or_else(|snapshot_error| format!("unavailable:{snapshot_error}"));
            tracing::warn!(
                identity,
                error_code = %error.code(),
                error_message = %error.message(),
                "graft session drop cleanup failed"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;

    use atm_core::protocol::{
        RequestEnvelope as CoreRequestEnvelope, ResponseEnvelope as CoreResponseEnvelope,
    };
    use atm_core::read::{BucketCounts, ReadOutcome};
    use atm_core::send::SendCommandOutcome;
    use atm_core::test_support::{EnvGuard, TEST_LEAD, TEST_TEAM};
    use atm_core::transport::testing::FakeClientTransport;
    use atm_core::types::{AgentName, CommandAction, ReadSelection, TeamName};
    use tempfile::TempDir;

    use super::*;

    #[derive(Debug, Default)]
    struct NoopInjector;

    impl HostNudgeInjector for NoopInjector {
        fn inject_nudge(&self, _nudge: &HostNudge) -> Result<(), AtmError> {
            Ok(())
        }
    }

    struct TestPaths {
        _tempdir: TempDir,
        home_dir: PathBuf,
        workspace_root: PathBuf,
    }

    fn test_paths() -> TestPaths {
        let tempdir = TempDir::new().expect("tempdir");
        let home_dir = tempdir.path().join("home");
        let workspace_root = tempdir.path().join("workspace");
        fs::create_dir_all(&home_dir).expect("create home dir");
        fs::create_dir_all(&workspace_root).expect("create workspace dir");
        TestPaths {
            _tempdir: tempdir,
            home_dir,
            workspace_root,
        }
    }

    fn session_options(paths: &TestPaths) -> GraftSessionOptions {
        GraftSessionOptions::new(
            paths.workspace_root.clone(),
            TeamName::from_validated(TEST_TEAM),
            AgentName::from_validated("qa-a"),
        )
    }

    #[tokio::test]
    async fn client_routes_send_and_read_over_transport() {
        let paths = test_paths();
        let transport = Arc::new(FakeClientTransport::new(Box::new(
            |request| match request {
                CoreRequestEnvelope::Write(request) if request.to.is_some() => Ok(
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
                CoreRequestEnvelope::Peek(_) => {
                    Ok(CoreResponseEnvelope::Peek(Box::new(ReadOutcome {
                        action: CommandAction::Peek,
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
                other => panic!("unexpected request: {other:?}"),
            },
        )));
        let client = GraftClient::from_fake_transport_for_test(transport);

        let send_request = SendRequest::new(
            paths.home_dir.clone(),
            paths.workspace_root.clone(),
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
        client.send_message(send_request).await.expect("send");

        let read_query = ReadQuery::new(
            paths.home_dir.clone(),
            paths.workspace_root.clone(),
            AgentName::from_validated(TEST_LEAD),
            None,
            TeamName::from_validated(TEST_TEAM),
            ReadSelection::Unread,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("read query");
        client.read_message(read_query).await.expect("read");
    }

    #[tokio::test]
    async fn mailbox_work_counts_projects_existing_non_mutating_read_buckets() {
        for (unread, pending_ack) in [(0, 0), (2, 0), (0, 3), (2, 3)] {
            let paths = test_paths();
            let transport = Arc::new(FakeClientTransport::new(Box::new(
                move |request| match request {
                    CoreRequestEnvelope::Receive(query) => {
                        assert!(
                            !query.seen_state_update(),
                            "count projection must not mutate read state"
                        );
                        Ok(CoreResponseEnvelope::Receive(Box::new(ReadOutcome {
                            action: CommandAction::Read,
                            team: TeamName::from_validated(TEST_TEAM),
                            agent: AgentName::from_validated(TEST_LEAD),
                            selection_mode: ReadSelection::All,
                            mutation_applied: false,
                            count: unread + pending_ack,
                            message: None,
                            selected_message_id: None,
                            match_count: unread + pending_ack,
                            additional_match_count: 0,
                            bucket_counts: BucketCounts {
                                unread,
                                pending_ack,
                                history: 9,
                            },
                        })))
                    }
                    other => panic!("unexpected request: {other:?}"),
                },
            )));
            let client = GraftClient::from_fake_transport_for_test(transport);
            let query = ReadQuery::new(
                paths.home_dir,
                paths.workspace_root,
                AgentName::from_validated(TEST_LEAD),
                None,
                TeamName::from_validated(TEST_TEAM),
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
            .expect("query");
            assert_eq!(
                client.mailbox_work_counts(query).await.expect("counts"),
                MailboxWorkCounts {
                    unread,
                    pending_ack
                }
            );
        }
    }

    #[tokio::test]
    async fn mailbox_work_counts_rejects_a_mutating_query_before_transport() {
        let paths = test_paths();
        let transport = Arc::new(FakeClientTransport::new(Box::new(|request| {
            panic!("mutating count query reached transport: {request:?}")
        })));
        let query = ReadQuery::new(
            paths.home_dir,
            paths.workspace_root,
            AgentName::from_validated(TEST_LEAD),
            None,
            TeamName::from_validated(TEST_TEAM),
            ReadSelection::All,
            false,
            true,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("query");

        let error = GraftClient::from_fake_transport_for_test(transport)
            .mailbox_work_counts(query)
            .await
            .expect_err("mutating count query must be rejected");
        assert_eq!(error.code(), AtmErrorCode::CallerContextRequestInvalid);
    }

    #[test]
    fn session_stays_inactive_without_atm_config() {
        let paths = test_paths();
        let session = GraftSession::activate_with_graft_config(
            None,
            session_options(&paths),
            Arc::new(NoopInjector),
            Arc::new(NoopGraftObservability),
        )
        .expect("inactive session");

        assert_eq!(
            session.snapshot().expect("snapshot").state,
            GraftSessionState::Inactive
        );
    }

    #[test]
    fn session_stays_inactive_when_graft_is_disabled() {
        let paths = test_paths();
        let session = GraftSession::activate_with_graft_config(
            Some(GraftConfig { enabled: false }),
            session_options(&paths),
            Arc::new(NoopInjector),
            Arc::new(NoopGraftObservability),
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

        let session = GraftSession::activate_with_graft_config(
            load_graft_config(tempdir.path()).expect("graft config"),
            GraftSessionOptions::new(
                tempdir.path(),
                TeamName::from_validated(TEST_TEAM),
                AgentName::from_validated("qa-a"),
            ),
            Arc::new(NoopInjector),
            Arc::new(NoopGraftObservability),
        )
        .expect("inactive session");

        assert_eq!(
            session.snapshot().expect("snapshot").state,
            GraftSessionState::Inactive
        );
    }
}
