//! Thin embedded ATM client crate for graft-aware host agents.
//! Production embedded delivery uses a receiver-owned same-host listener that
//! accepts one bounded nudge request per connection.
use std::fmt;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;
use std::time::Duration;

use atm_core::api::{ApiRequest, DaemonApiClient};
use atm_core::boundary::{NudgeKind, PostSendHookEvent};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::graft::AtmGraftClient;
use atm_core::list::{ListOutcome, ListQuery};
use atm_core::local_http::LocalCapability;
use atm_core::protocol::{
    GraftReceiverRefreshRequest, GraftReceiverRegistration, GraftReceiverUnregistration,
    OwnerGeneration, RequestEnvelope, ResponseEnvelope, SendResponseEnvelope,
};
use atm_core::read::{PeekQuery, ReadOutcome, ReadQuery};
use atm_core::send::{SendOutcome, SendRequest, WriteOutcome};
use atm_core::types::{AgentName, ChatId, TeamName};
use atm_daemon_client::{resolve_daemon_local_ipc_endpoint, unexpected_response};
use atm_http_runtime::SAME_HOST_REQUEST_DEADLINE;

mod nudge_sink;
mod runtime;

use runtime::{
    GraftReceiverLeaseClient, GraftReceiverLoopContext, RECEIVE_LOOP_READY_DEADLINE,
    ReceiverReadyLatch, join_receive_loop_with_deadline, load_graft_config, read_snapshot,
    run_graft_receiver_loop, set_session_state,
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
    pub kind: NudgeKind,
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
        GraftSession::activate_with_client(
            options,
            injector,
            Arc::new(NoopGraftObservability),
            Some(Arc::new(self.clone()) as Arc<dyn GraftReceiverLeaseClient>),
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

    pub(crate) fn execute_request_sync(
        &self,
        request: RequestEnvelope,
    ) -> Result<ResponseEnvelope, AtmError> {
        atm_daemon_client::execute_api_request(self.async_transport.clone(), request)
    }

    pub(crate) fn register_receiver_sync(
        &self,
        team: TeamName,
        agent: AgentName,
        endpoint: SocketAddr,
        capability: LocalCapability,
        owner_generation: OwnerGeneration,
    ) -> Result<(), AtmError> {
        match self.execute_request_sync(RequestEnvelope::GraftReceiverRegister(
            GraftReceiverRegistration {
                team,
                agent,
                endpoint,
                capability,
                owner_generation,
            },
        ))? {
            ResponseEnvelope::GraftReceiverRegister => Ok(()),
            other => Err(unexpected_response("graft receiver register", other)),
        }
    }

    /// Owner-checked liveness keepalive (ADR-056's `refresh`): fails with
    /// `AtmErrorCode::GraftReceiverNotOwner` when another generation now owns
    /// the stored lease, unlike [`Self::register_receiver_sync`]'s
    /// unconditional upsert.
    pub(crate) fn refresh_receiver_sync(
        &self,
        team: TeamName,
        agent: AgentName,
        owner_generation: OwnerGeneration,
    ) -> Result<(), AtmError> {
        match self.execute_request_sync(RequestEnvelope::GraftReceiverRefresh(
            GraftReceiverRefreshRequest {
                team,
                agent,
                owner_generation,
            },
        ))? {
            ResponseEnvelope::GraftReceiverRefresh => Ok(()),
            other => Err(unexpected_response("graft receiver refresh", other)),
        }
    }

    pub(crate) fn unregister_receiver_sync(
        &self,
        team: TeamName,
        agent: AgentName,
        owner_generation: OwnerGeneration,
    ) -> Result<(), AtmError> {
        match self.execute_request_sync(RequestEnvelope::GraftReceiverUnregister(
            GraftReceiverUnregistration {
                team,
                agent,
                owner_generation,
            },
        ))? {
            ResponseEnvelope::GraftReceiverUnregister => Ok(()),
            other => Err(unexpected_response("graft receiver unregister", other)),
        }
    }

    /// Read a mailbox projection without changing the selected message state.
    pub async fn peek_message(&self, query: PeekQuery) -> Result<ReadOutcome, AtmError> {
        match self.execute_request(RequestEnvelope::Peek(query)).await? {
            ResponseEnvelope::Peek(outcome) => Ok(*outcome),
            other => Err(unexpected_response("peek", other)),
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

/// Exposes the narrow daemon-lease surface `runtime`'s receive loop needs
/// (sc-boundary SCB-CYCLE-001): `GraftReceiverLoopContext`/
/// `RegisteredGraftReceiver` depend on this trait, never on the concrete
/// `GraftClient` type, so `GraftSession` (which the receive loop's context
/// is built for) and `GraftClient` (which owns `activate_session`, a
/// `GraftSession` constructor) do not reference each other's concrete types
/// in a cycle.
impl GraftReceiverLeaseClient for GraftClient {
    fn register_receiver_sync(
        &self,
        team: TeamName,
        agent: AgentName,
        endpoint: SocketAddr,
        capability: LocalCapability,
        owner_generation: OwnerGeneration,
    ) -> Result<(), AtmError> {
        GraftClient::register_receiver_sync(
            self,
            team,
            agent,
            endpoint,
            capability,
            owner_generation,
        )
    }

    fn refresh_receiver_sync(
        &self,
        team: TeamName,
        agent: AgentName,
        owner_generation: OwnerGeneration,
    ) -> Result<(), AtmError> {
        GraftClient::refresh_receiver_sync(self, team, agent, owner_generation)
    }

    fn unregister_receiver_sync(
        &self,
        team: TeamName,
        agent: AgentName,
        owner_generation: OwnerGeneration,
    ) -> Result<(), AtmError> {
        GraftClient::unregister_receiver_sync(self, team, agent, owner_generation)
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
    /// Returns [`AtmError`] when optional configuration cannot be loaded or
    /// receiver-loop startup fails.
    pub fn activate(
        options: GraftSessionOptions,
        injector: Arc<dyn HostNudgeInjector>,
    ) -> Result<Self, AtmError> {
        Self::activate_with_observability(options, injector, Arc::new(NoopGraftObservability))
    }

    /// # Errors
    ///
    /// Returns [`AtmError`] when optional configuration cannot be loaded or
    /// receiver-loop startup fails.
    pub fn activate_with_observability(
        options: GraftSessionOptions,
        injector: Arc<dyn HostNudgeInjector>,
        observability: Arc<dyn GraftObservability>,
    ) -> Result<Self, AtmError> {
        Self::activate_with_client(options, injector, observability, None)
    }

    fn activate_with_client(
        options: GraftSessionOptions,
        injector: Arc<dyn HostNudgeInjector>,
        observability: Arc<dyn GraftObservability>,
        client: Option<Arc<dyn GraftReceiverLeaseClient>>,
    ) -> Result<Self, AtmError> {
        // Retain ATM-owned configuration parsing so malformed configuration is
        // surfaced, but a missing config (or legacy graft.enabled setting)
        // never changes the receiver activation contract. A clean return from
        // this method means the receiver is listening.
        let _optional_config = load_graft_config(&options.workspace_root)?;
        let initial_snapshot = options.activation_state();
        let snapshot = Arc::new(RwLock::new(initial_snapshot));

        let (stop_tx, join_handle) = Self::start_graft_receive_loop(
            options.workspace_root().to_path_buf(),
            options,
            Arc::clone(&snapshot),
            injector,
            Arc::clone(&observability),
            client,
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
        graft_root: PathBuf,
        options: GraftSessionOptions,
        worker_snapshot: Arc<RwLock<SessionSnapshot>>,
        injector: Arc<dyn HostNudgeInjector>,
        worker_observability: Arc<dyn GraftObservability>,
        client: Option<Arc<dyn GraftReceiverLeaseClient>>,
    ) -> Result<GraftReceiveLoopWorker, AtmError> {
        let (stop_tx, stop_rx) = mpsc::channel();
        let mut ready_latch = ReceiverReadyLatch::new();
        let join_handle = spawn_graft_receive_loop(
            graft_root,
            options,
            worker_snapshot,
            injector,
            worker_observability,
            client,
            ReceiveLoopChannels {
                ready_tx: ready_latch.notifier(),
                stop_rx,
            },
        )?;
        match ready_latch.wait_until_listening(RECEIVE_LOOP_READY_DEADLINE) {
            Ok(()) => Ok((stop_tx, join_handle)),
            Err(readiness_error) => {
                // A receiver can fail before it reports ready (for example,
                // when its endpoint is already owned). Join the worker so its
                // typed bind/ownership error is never replaced by the latch's
                // generic timeout or disconnect diagnostic.
                let _ = stop_tx.send(());
                Err(receive_loop_startup_error(
                    readiness_error,
                    join_receive_loop_with_deadline(join_handle),
                ))
            }
        }
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

struct ReceiveLoopChannels {
    ready_tx: std::sync::mpsc::SyncSender<()>,
    stop_rx: std::sync::mpsc::Receiver<()>,
}

fn spawn_graft_receive_loop(
    graft_root: PathBuf,
    options: GraftSessionOptions,
    worker_snapshot: Arc<RwLock<SessionSnapshot>>,
    injector: Arc<dyn HostNudgeInjector>,
    worker_observability: Arc<dyn GraftObservability>,
    client: Option<Arc<dyn GraftReceiverLeaseClient>>,
    channels: ReceiveLoopChannels,
) -> Result<std::thread::JoinHandle<Result<(), AtmError>>, AtmError> {
    let thread_name = format!("atm-graft-{}", options.agent());
    std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            run_graft_receiver_loop(GraftReceiverLoopContext {
                graft_root,
                team: options.team().clone(),
                agent: options.agent().clone(),
                owner_chat_id: options.owner_chat_id(),
                client,
                snapshot: worker_snapshot,
                injector,
                observability: worker_observability,
                stop_rx: channels.stop_rx,
                ready_tx: Some(channels.ready_tx),
                receiver_target_tx: None,
            })
        })
        .map_err(spawn_receive_loop_error)
}

/// Reports the cause a caller can act on when receiver startup did not reach
/// readiness.
///
/// The worker's own typed failure (bind refused, endpoint already owned, a
/// lease/announce failure that ended the loop) is always the actionable one,
/// so it replaces the latch's generic diagnostic outright. When the worker
/// neither failed nor signaled in time, the readiness error is returned with
/// the join outcome appended so a timeout is never reported bare.
fn receive_loop_startup_error(
    readiness_error: AtmError,
    worker_result: Result<(), AtmError>,
) -> AtmError {
    match worker_result {
        Err(worker_error) => worker_error,
        Ok(()) => AtmError::new(
            readiness_error.code(),
            format!(
                "{} (the receive loop stopped without reporting a failure)",
                readiness_error.message()
            ),
        ),
    }
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
    use std::sync::{Arc, Mutex};

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

    #[test]
    fn receiver_registers_at_bind_and_unregisters_its_generation_on_drop() {
        let paths = test_paths();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        let transport = Arc::new(FakeClientTransport::new(Box::new(move |request| {
            let response = match &request {
                CoreRequestEnvelope::GraftReceiverRegister(_) => {
                    CoreResponseEnvelope::GraftReceiverRegister
                }
                CoreRequestEnvelope::GraftReceiverUnregister(_) => {
                    CoreResponseEnvelope::GraftReceiverUnregister
                }
                other => panic!("unexpected receiver request: {other:?}"),
            };
            captured.lock().expect("request capture lock").push(request);
            Ok(response)
        })));
        let client = GraftClient::from_fake_transport_for_test(transport);
        let session = client
            .activate_session(session_options(&paths), Arc::new(NoopInjector))
            .expect("receiver activation");

        let registration = {
            let requests = requests.lock().expect("request capture lock");
            assert_eq!(requests.len(), 1, "bind announces exactly one lease");
            let CoreRequestEnvelope::GraftReceiverRegister(registration) = &requests[0] else {
                panic!("bind must register the receiver")
            };
            registration.clone()
        };
        session.close().expect("receiver close");

        let requests = requests.lock().expect("request capture lock");
        assert_eq!(requests.len(), 2, "drop unregisters the receiver lease");
        let CoreRequestEnvelope::GraftReceiverUnregister(unregistration) = &requests[1] else {
            panic!("drop must unregister the receiver")
        };
        assert_eq!(unregistration.team, registration.team);
        assert_eq!(unregistration.agent, registration.agent);
        assert_eq!(
            unregistration.owner_generation,
            registration.owner_generation
        );
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
    fn session_activates_in_a_bare_workspace_without_atm_config() {
        let paths = test_paths();
        let legacy_endpoint_path = paths
            .workspace_root
            .join(".atm")
            .join("graft")
            .join(TEST_TEAM)
            .join("qa-a.json");
        let lock_path = paths
            .workspace_root
            .join(".atm")
            .join("graft")
            .join(TEST_TEAM)
            .join("qa-a.lock");
        let session = GraftSession::activate(session_options(&paths), Arc::new(NoopInjector))
            .expect("bare workspace must activate or return an error");

        assert_eq!(
            session.snapshot().expect("snapshot").state,
            GraftSessionState::Listening
        );
        assert!(
            lock_path.exists(),
            "receiver ownership lock must be retained"
        );
        assert!(
            !legacy_endpoint_path.exists(),
            "receiver must not publish a legacy endpoint artifact"
        );
        session.close().expect("close active receiver");
    }

    #[test]
    fn legacy_graft_enabled_setting_does_not_gate_receiver_activation() {
        let paths = test_paths();
        fs::write(
            paths.workspace_root.join(".atm.toml"),
            "[atm.graft]\nenabled = false\n",
        )
        .expect("write legacy config");
        let session = GraftSession::activate(session_options(&paths), Arc::new(NoopInjector))
            .expect("explicit legacy config must not suppress activation");

        assert_eq!(
            session.snapshot().expect("snapshot").state,
            GraftSessionState::Listening
        );
        session.close().expect("close active receiver");
    }

    #[test]
    fn activation_surfaces_receiver_ownership_conflicts_from_the_worker() {
        let paths = test_paths();
        let active = GraftSession::activate(session_options(&paths), Arc::new(NoopInjector))
            .expect("first receiver must activate");

        let error = GraftSession::activate(session_options(&paths), Arc::new(NoopInjector))
            .expect_err("second receiver must report the endpoint ownership conflict");

        assert_eq!(error.code(), AtmErrorCode::GraftReceiverAlreadyActive);
        active.close().expect("close active receiver");
    }

    #[test]
    fn minimal_workspace_activation_conflict_reports_a_cause_not_a_bare_timeout() {
        // RRG-HERMES-FLEET-NUDGE-001 shape: a Hermes host activates a
        // receiver against a minimal workspace root whose `.atm.toml` carries
        // only a default team. When that activation cannot succeed, the
        // caller must receive the receive loop's own typed cause, promptly,
        // rather than the readiness latch's bare `WaitTimeout`.
        let paths = test_paths();
        fs::write(
            paths.workspace_root.join(".atm.toml"),
            "[atm]\ndefault_team = \"test-team\"\n",
        )
        .expect("write minimal config");
        let active = GraftSession::activate(session_options(&paths), Arc::new(NoopInjector))
            .expect("minimal workspace root must activate a receiver");
        assert_eq!(
            active.snapshot().expect("snapshot").state,
            GraftSessionState::Listening
        );

        let started = std::time::Instant::now();
        let error = GraftSession::activate(session_options(&paths), Arc::new(NoopInjector))
            .expect_err("a conflicting activation must fail");

        assert_eq!(error.code(), AtmErrorCode::GraftReceiverAlreadyActive);
        assert!(
            !error.message().contains("readiness was not signaled"),
            "startup failure must not be masked by the readiness timeout: {}",
            error.message()
        );
        assert!(
            started.elapsed() < RECEIVE_LOOP_READY_DEADLINE,
            "a failed receive loop must be reported before the readiness deadline"
        );
        active.close().expect("close active receiver");
    }

    #[test]
    fn malformed_optional_config_fails_loudly_before_receiver_startup() {
        let tempdir = tempfile::TempDir::new().expect("tempdir");
        std::fs::write(tempdir.path().join(".atm.toml"), "[atm\n").expect("write config");
        let _env = EnvGuard::set_many([
            (
                "ATM_HOME",
                Some(tempdir.path().to_str().expect("utf8 home")),
            ),
            ("HOME", Some(tempdir.path().to_str().expect("utf8 home"))),
            ("USERPROFILE", None),
        ]);

        let error = GraftSession::activate_with_observability(
            GraftSessionOptions::new(
                tempdir.path(),
                TeamName::from_validated(TEST_TEAM),
                AgentName::from_validated("qa-a"),
            ),
            Arc::new(NoopInjector),
            Arc::new(NoopGraftObservability),
        )
        .expect_err("malformed optional configuration must fail loudly");
        assert_ne!(error.code(), AtmErrorCode::InternalError);
    }
}
