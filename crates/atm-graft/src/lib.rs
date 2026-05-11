//! Thin embedded ATM client crate for graft-aware host agents.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use atm_core::ack::{AckOutcome, AckRequest};
use atm_core::boundary;
use atm_core::boundary::ClientTransport;
use atm_core::error::AtmError;
use atm_core::graft::{
    AtmGraftClient, GraftBatchLimit, GraftNudgeDrainRequest, GraftNudgeDrainResponse,
    GraftNudgeFetchRequest, GraftNudgeFetchResponse, GraftSession as GraftSessionSnapshot,
    GraftSessionId, GraftSessionPort, GraftSessionRegistrationRequest,
    GraftSessionRegistrationResponse, GraftSessionState, GraftSessionUnregistrationRequest,
    GraftSessionUnregistrationResponse, NudgeEvent,
};
use atm_core::protocol::{
    RequestEnvelope, ResponseEnvelope, SendRequestEnvelope, SendResponseEnvelope,
};
use atm_core::read::{ReadOutcome, ReadQuery};
use atm_core::send::{SendOutcome, SendRequest};
use atm_core::types::{AgentName, IsoTimestamp, TeamName};
use fs2::FileExt;
use interprocess::local_socket::Stream as LocalSocketStream;
use interprocess::local_socket::traits::Stream as _;

const SAME_HOST_REQUEST_DEADLINE: Duration = Duration::from_secs(3);
const AUTO_START_PUBLISH_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(250);
const DEFAULT_BATCH_LIMIT: usize = 64;
const HOST_RUNTIME_LAUNCH_LOCK_FILE: &str = "launch.lock";
const RECEIVE_LOOP_JOIN_DEADLINE: Duration = Duration::from_secs(5);

/// Public alias for the ATM-core graft session projection DTO.
pub type SessionSnapshot = GraftSessionSnapshot;

pub use atm_core::{
    AtmConfig, GraftConfig, GraftNudgeDrainRequest as DrainRequest,
    GraftNudgeFetchRequest as FetchRequest, GraftSessionId as SessionId, NudgeEvent as Event,
};

/// Preferred host-facing imports for embedding `atm-graft`.
pub mod prelude {
    pub use super::{
        AtmConfig, DrainRequest, Event, FetchRequest, GraftClient, GraftConfig,
        GraftObservability, GraftSession, GraftSessionOptions, HostNudgeInjector,
        NoopGraftObservability, SessionId, SessionSnapshot,
    };
}

/// Host-owned bridge for automatic between-tool-call nudge injection.
pub trait HostNudgeInjector: Send + Sync {
    /// # Errors
    ///
    /// Returns [`AtmError`] when the host cannot safely inject the nudge into
    /// its between-tool-call context flow.
    fn inject_nudge(&self, nudge: NudgeEvent) -> Result<(), AtmError>;
}

/// ATM-owned graft observability boundary supplied by the embedding host.
pub trait GraftObservability: Send + Sync {
    fn session_state_changed(&self, _snapshot: &SessionSnapshot) {}

    fn nudge_delivered(&self, _session_id: &GraftSessionId, _nudge: &NudgeEvent) {}

    fn session_error(
        &self,
        _session_id: &GraftSessionId,
        _action: &'static str,
        _error: &AtmError,
    ) {
    }
}

/// No-op graft observability adapter.
#[derive(Debug, Default)]
pub struct NoopGraftObservability;

impl GraftObservability for NoopGraftObservability {}

/// Options used to activate one graft session.
#[derive(Debug, Clone)]
pub struct GraftSessionOptions {
    workspace_root: PathBuf,
    team: TeamName,
    agent: AgentName,
    session_id: GraftSessionId,
    batch_limit: GraftBatchLimit,
    poll_interval: Duration,
}

impl GraftSessionOptions {
    pub fn new(
        workspace_root: impl Into<PathBuf>,
        team: TeamName,
        agent: AgentName,
        session_id: GraftSessionId,
    ) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            team,
            agent,
            session_id,
            batch_limit: GraftBatchLimit::new(DEFAULT_BATCH_LIMIT)
                .expect("default graft batch limit must be valid"),
            poll_interval: DEFAULT_POLL_INTERVAL,
        }
    }

    pub fn for_current_process(
        workspace_root: impl Into<PathBuf>,
        team: TeamName,
        agent: AgentName,
    ) -> Self {
        let session_id = GraftSessionId::new(format!("{agent}-{}", std::process::id()))
            .expect("derived graft session id must be valid");
        Self::new(workspace_root, team, agent, session_id)
    }

    pub fn with_batch_limit(mut self, batch_limit: GraftBatchLimit) -> Self {
        self.batch_limit = batch_limit;
        self
    }

    /// # Errors
    ///
    /// Returns [`AtmError`] when the poll interval is zero.
    pub fn with_poll_interval(mut self, poll_interval: Duration) -> Result<Self, AtmError> {
        if poll_interval.is_zero() {
            return Err(AtmError::validation(
                "graft session poll interval must be greater than zero",
            )
            .with_recovery(
                "Configure a positive receive-loop poll interval before activating graft mode.",
            ));
        }
        self.poll_interval = poll_interval;
        Ok(self)
    }

    fn activation_state(&self) -> SessionSnapshot {
        SessionSnapshot {
            team: self.team.clone(),
            agent: self.agent.clone(),
            session_id: self.session_id.clone(),
            state: GraftSessionState::Inactive,
        }
    }

    fn registration_request(&self) -> GraftSessionRegistrationRequest {
        GraftSessionRegistrationRequest {
            team: self.team.clone(),
            agent: self.agent.clone(),
            session_id: self.session_id.clone(),
            pid: std::process::id(),
            started_at: IsoTimestamp::now(),
        }
    }
}

/// Thin daemon-backed same-host client for embedded graft consumers.
#[derive(Clone)]
pub struct GraftClient {
    transport: Arc<dyn ClientTransport + Send + Sync>,
}

trait GraftSessionClient: AtmGraftClient + GraftSessionPort {}

impl<T> GraftSessionClient for T where T: AtmGraftClient + GraftSessionPort + ?Sized {}

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
        let daemon_bin = resolve_daemon_bin()?;
        let transport = Arc::new(GraftLocalIpcClientTransport::new(endpoint.clone()));
        let supervisor = DaemonSupervisor::new(endpoint, daemon_bin);
        supervisor.ensure_daemon_available(transport.as_ref())?;
        Ok(Self::from_transport(transport))
    }

    pub fn from_transport(transport: Arc<dyn ClientTransport + Send + Sync>) -> Self {
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

    /// # Errors
    ///
    /// Returns [`AtmError`] when the request cannot be delivered or the daemon
    /// returns an unexpected typed envelope for the command.
    fn send_request(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        match self.transport.send(request)? {
            ResponseEnvelope::Error(error) => Err(error.into_atm_error()),
            response => Ok(response),
        }
    }

    pub fn fetch_pending_nudges(
        &self,
        request: GraftNudgeFetchRequest,
    ) -> Result<GraftNudgeFetchResponse, AtmError> {
        self.fetch_nudges(request)
    }

    pub fn drain_pending_nudges(
        &self,
        request: GraftNudgeDrainRequest,
    ) -> Result<GraftNudgeDrainResponse, AtmError> {
        self.drain_nudges(request)
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
            ResponseEnvelope::Receive(outcome) => Ok(outcome),
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

impl GraftSessionPort for GraftClient {
    fn register_session(
        &self,
        request: GraftSessionRegistrationRequest,
    ) -> Result<GraftSessionRegistrationResponse, AtmError> {
        match self.send_request(RequestEnvelope::GraftRegister(request))? {
            ResponseEnvelope::GraftRegister(response) => Ok(response),
            other => Err(unexpected_response("graft register", other)),
        }
    }

    fn unregister_session(
        &self,
        request: GraftSessionUnregistrationRequest,
    ) -> Result<GraftSessionUnregistrationResponse, AtmError> {
        match self.send_request(RequestEnvelope::GraftUnregister(request))? {
            ResponseEnvelope::GraftUnregister(response) => Ok(response),
            other => Err(unexpected_response("graft unregister", other)),
        }
    }

    fn fetch_nudges(
        &self,
        request: GraftNudgeFetchRequest,
    ) -> Result<GraftNudgeFetchResponse, AtmError> {
        match self.send_request(RequestEnvelope::GraftFetch(request))? {
            ResponseEnvelope::GraftFetch(response) => Ok(response),
            other => Err(unexpected_response("graft fetch", other)),
        }
    }

    fn drain_nudges(
        &self,
        request: GraftNudgeDrainRequest,
    ) -> Result<GraftNudgeDrainResponse, AtmError> {
        match self.send_request(RequestEnvelope::GraftDrain(request))? {
            ResponseEnvelope::GraftDrain(response) => Ok(response),
            other => Err(unexpected_response("graft drain", other)),
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
    /// registration or receive-loop startup fails.
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
    /// registration or receive-loop startup fails.
    pub fn activate_with_observability(
        client: GraftClient,
        options: GraftSessionOptions,
        injector: Arc<dyn HostNudgeInjector>,
        observability: Arc<dyn GraftObservability>,
    ) -> Result<Self, AtmError> {
        let graft_config = load_graft_config(&options.workspace_root)?;
        Self::activate_with_graft_config(
            Arc::new(client),
            graft_config,
            options,
            injector,
            observability,
        )
    }

    fn activate_with_graft_config(
        client: Arc<dyn GraftSessionClient>,
        graft_config: Option<GraftConfig>,
        options: GraftSessionOptions,
        injector: Arc<dyn HostNudgeInjector>,
        observability: Arc<dyn GraftObservability>,
    ) -> Result<Self, AtmError> {
        let initial_snapshot = options.activation_state();
        let snapshot = Arc::new(RwLock::new(initial_snapshot));

        let Some(graft_config) = graft_config else {
            observability.session_state_changed(&read_snapshot(&snapshot)?);
            return Ok(Self {
                client,
                snapshot,
                observability,
                stop_tx: None,
                join_handle: None,
            });
        };

        if !graft_config.enabled {
            observability.session_state_changed(&read_snapshot(&snapshot)?);
            return Ok(Self {
                client,
                snapshot,
                observability,
                stop_tx: None,
                join_handle: None,
            });
        }

        set_session_state(&snapshot, GraftSessionState::Connecting, observability.as_ref())?;

        let register_response = client.register_session(options.registration_request())?;
        validate_batch_limit_against_capacity(options.batch_limit, register_response.queue_capacity)?;
        set_session_state(&snapshot, GraftSessionState::Registered, observability.as_ref())?;

        let worker_client = Arc::clone(&client);
        let worker_snapshot = Arc::clone(&snapshot);
        let worker_observability = Arc::clone(&observability);
        let registration_request = options.registration_request();
        let drain_request = GraftNudgeDrainRequest {
            session_id: options.session_id.clone(),
            limit: options.batch_limit,
        };
        let (stop_tx, stop_rx) = mpsc::channel();
        let join_handle = thread::Builder::new()
            .name(format!("atm-graft-{}", options.session_id))
            .spawn(move || {
                run_receive_loop(ReceiveLoopContext {
                    client: worker_client,
                    registration_request,
                    drain_request,
                    poll_interval: options.poll_interval,
                    snapshot: worker_snapshot,
                    injector,
                    observability: worker_observability,
                    stop_rx,
                })
            })
            .map_err(|source| {
                AtmError::daemon_unavailable("failed to spawn graft receive loop")
                    .with_source(source)
                    .with_recovery(
                        "Retry graft activation after the embedding host allows one live receive thread for the active session.",
                    )
            })?;

        Ok(Self {
            client,
            snapshot,
            observability,
            stop_tx: Some(stop_tx),
            join_handle: Some(join_handle),
        })
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

    pub fn fetch_pending_nudges(
        &self,
        request: GraftNudgeFetchRequest,
    ) -> Result<GraftNudgeFetchResponse, AtmError> {
        self.client.fetch_nudges(request)
    }

    pub fn drain_pending_nudges(
        &self,
        request: GraftNudgeDrainRequest,
    ) -> Result<GraftNudgeDrainResponse, AtmError> {
        self.client.drain_nudges(request)
    }

    /// # Errors
    ///
    /// Returns [`AtmError`] when the receive loop cannot join cleanly or the
    /// daemon-side unregister path fails during shutdown.
    pub fn close(mut self) -> Result<(), AtmError> {
        self.close_internal()
    }

    fn close_internal(&mut self) -> Result<(), AtmError> {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());
        }
        if let Some(join_handle) = self.join_handle.take() {
            if let Err(error) = join_receive_loop_with_deadline(join_handle) {
                set_session_state(
                    &self.snapshot,
                    GraftSessionState::CloseFailed,
                    self.observability.as_ref(),
                )?;
                return Err(error);
            }
        }
        set_session_state(
            &self.snapshot,
            GraftSessionState::Closed,
            self.observability.as_ref(),
        )?;
        Ok(())
    }
}

impl Drop for GraftSession {
    fn drop(&mut self) {
        let _ = self.close_internal();
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

impl GraftSessionPort for GraftSession {
    fn register_session(
        &self,
        request: GraftSessionRegistrationRequest,
    ) -> Result<GraftSessionRegistrationResponse, AtmError> {
        self.client.register_session(request)
    }

    fn unregister_session(
        &self,
        request: GraftSessionUnregistrationRequest,
    ) -> Result<GraftSessionUnregistrationResponse, AtmError> {
        self.client.unregister_session(request)
    }

    fn fetch_nudges(
        &self,
        request: GraftNudgeFetchRequest,
    ) -> Result<GraftNudgeFetchResponse, AtmError> {
        self.client.fetch_nudges(request)
    }

    fn drain_nudges(
        &self,
        request: GraftNudgeDrainRequest,
    ) -> Result<GraftNudgeDrainResponse, AtmError> {
        self.client.drain_nudges(request)
    }
}

fn load_graft_config(workspace_root: &Path) -> Result<Option<GraftConfig>, AtmError> {
    let config = atm_core::load_atm_config(workspace_root)?;
    Ok(config.map(|config| config.graft))
}

fn read_snapshot(snapshot: &Arc<RwLock<SessionSnapshot>>) -> Result<SessionSnapshot, AtmError> {
    snapshot
        .read()
        .map(|snapshot| snapshot.clone())
        .map_err(|_| {
            AtmError::daemon_unavailable("graft session snapshot lock poisoned").with_recovery(
                "Restart the embedding host before retrying graft session lifecycle operations.",
            )
        })
}

fn write_snapshot(
    snapshot: &Arc<RwLock<SessionSnapshot>>,
    state: GraftSessionState,
) -> Result<(), AtmError> {
    let mut snapshot = snapshot
        .write()
        .map_err(|_| {
            AtmError::daemon_unavailable("graft session snapshot lock poisoned").with_recovery(
                "Restart the embedding host before retrying graft session lifecycle operations.",
            )
        })?;
    snapshot.state = state;
    Ok(())
}

fn set_session_state(
    snapshot: &Arc<RwLock<SessionSnapshot>>,
    state: GraftSessionState,
    observability: &dyn GraftObservability,
) -> Result<(), AtmError> {
    write_snapshot(snapshot, state)?;
    observability.session_state_changed(&read_snapshot(snapshot)?);
    Ok(())
}

fn validate_batch_limit_against_capacity(
    batch_limit: GraftBatchLimit,
    queue_capacity: usize,
) -> Result<(), AtmError> {
    if batch_limit.get() > queue_capacity {
        return Err(AtmError::validation(format!(
            "graft batch limit {} exceeds daemon queue capacity {}",
            batch_limit.get(),
            queue_capacity
        ))
        .with_recovery(
            "Lower the graft batch limit or restart against a daemon that advertises a larger graft nudge queue before retrying session activation.",
        ));
    }
    Ok(())
}

fn join_receive_loop_with_deadline(
    join_handle: JoinHandle<Result<(), AtmError>>,
) -> Result<(), AtmError> {
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let join_helper = thread::Builder::new()
        .name("atm-graft-receive-loop-join".to_string())
        .spawn(move || {
            let result = match join_handle.join() {
                Ok(result) => result,
                Err(_) => Err(AtmError::daemon_unavailable("graft receive loop panicked")
                    .with_recovery(
                        "Restart the embedding host and atm-daemon before retrying graft mode.",
                    )),
            };
            let _ = result_tx.send(result);
        })
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to spawn graft receive-loop join helper")
                .with_source(source)
                .with_recovery(
                    "Retry graft shutdown after the embedding host can spawn one bounded join helper thread.",
                )
        })?;
    let join_helper_thread_id = join_helper.thread().id();
    match result_rx.recv_timeout(RECEIVE_LOOP_JOIN_DEADLINE) {
        Ok(result) => {
            join_helper.join().map_err(|_| {
                AtmError::daemon_unavailable("graft receive-loop join helper panicked")
            })?;
            result
        }
        Err(RecvTimeoutError::Timeout) => {
            tracing::debug!(
                timeout_ms = RECEIVE_LOOP_JOIN_DEADLINE.as_millis(),
                thread_id = ?join_helper_thread_id,
                "graft receive-loop join timed out; helper left detached after deadline"
            );
            Err(AtmError::daemon_unavailable(format!(
                "graft receive loop shutdown exceeded the {:?} join deadline",
                RECEIVE_LOOP_JOIN_DEADLINE
            ))
            .with_recovery(
                "Restart the embedding host if the graft receive loop does not shut down within the bounded join deadline.",
            ))
        }
        Err(RecvTimeoutError::Disconnected) => join_helper.join().map_or_else(
            |_| {
                Err(AtmError::daemon_unavailable(
                    "graft receive-loop join helper panicked",
                ))
            },
            |_| {
                Err(AtmError::daemon_unavailable(
                    "graft receive-loop join helper disconnected unexpectedly",
                ))
            },
        ),
    }
}

fn run_receive_loop(ctx: ReceiveLoopContext) -> Result<(), AtmError> {
    let session_id = ctx.drain_request.session_id.clone();
    loop {
        match ctx.stop_rx.recv_timeout(ctx.poll_interval) {
            Ok(()) => {
                match ctx
                    .client
                    .unregister_session(GraftSessionUnregistrationRequest {
                        session_id: session_id.clone(),
                    }) {
                    Ok(_) => set_session_state(
                        &ctx.snapshot,
                        GraftSessionState::Closed,
                        ctx.observability.as_ref(),
                    )?,
                    Err(error) => {
                        set_session_state(
                            &ctx.snapshot,
                            GraftSessionState::CloseFailed,
                            ctx.observability.as_ref(),
                        )?;
                        ctx.observability
                            .session_error(&session_id, "unregister_session", &error);
                        return Err(error);
                    }
                }
                return Ok(());
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                set_session_state(
                    &ctx.snapshot,
                    GraftSessionState::Closed,
                    ctx.observability.as_ref(),
                )?;
                return Ok(());
            }
        }

        match ctx.client.drain_nudges(ctx.drain_request.clone()) {
            Ok(response) => {
                if read_snapshot(&ctx.snapshot)?.state != GraftSessionState::Registered {
                    set_session_state(
                        &ctx.snapshot,
                        GraftSessionState::Registered,
                        ctx.observability.as_ref(),
                    )?;
                }
                for nudge in response.nudges {
                    ctx.injector.inject_nudge(nudge.clone())?;
                    ctx.observability.nudge_delivered(&session_id, &nudge);
                }
            }
            Err(error) => {
                set_session_state(
                    &ctx.snapshot,
                    GraftSessionState::Disconnected,
                    ctx.observability.as_ref(),
                )?;
                ctx.observability
                    .session_error(&session_id, "drain_nudges", &error);
                tracing::debug!(session_id = %session_id, error = %error.message, "graft receive loop will retry after drain failure");

                match ctx
                    .client
                    .register_session(ctx.registration_request.clone())
                {
                    Ok(response) => {
                        validate_batch_limit_against_capacity(
                            ctx.drain_request.limit,
                            response.queue_capacity,
                        )?;
                        set_session_state(
                            &ctx.snapshot,
                            GraftSessionState::Registered,
                            ctx.observability.as_ref(),
                        )?;
                    }
                    Err(register_error) if is_duplicate_registration(&register_error) => {
                        set_session_state(
                            &ctx.snapshot,
                            GraftSessionState::Registered,
                            ctx.observability.as_ref(),
                        )?;
                    }
                    Err(register_error) => {
                        ctx.observability.session_error(
                            &session_id,
                            "register_session",
                            &register_error,
                        );
                        ctx.observability
                            .session_state_changed(&read_snapshot(&ctx.snapshot)?);
                        tracing::debug!(session_id = %session_id, error = %register_error.message, "graft receive loop failed to re-register session");
                    }
                }
            }
        }
    }
}

struct ReceiveLoopContext {
    client: Arc<dyn GraftSessionClient>,
    registration_request: GraftSessionRegistrationRequest,
    drain_request: GraftNudgeDrainRequest,
    poll_interval: Duration,
    snapshot: Arc<RwLock<SessionSnapshot>>,
    injector: Arc<dyn HostNudgeInjector>,
    observability: Arc<dyn GraftObservability>,
    stop_rx: mpsc::Receiver<()>,
}

fn is_duplicate_registration(error: &AtmError) -> bool {
    error.code == atm_core::error_codes::AtmErrorCode::DaemonGraftSessionAlreadyRegistered
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
            "Set ATM_DAEMON_SOCKET to a non-empty UTF-8 daemon local IPC endpoint before invoking the graft daemon client.",
        ));
    }
    if path.to_str().is_none() {
        return Err(AtmError::validation(format!(
            "{label} must be valid UTF-8 at the ATM boundary"
        ))
        .with_recovery(
            "Set ATM_DAEMON_SOCKET to a non-empty UTF-8 daemon local IPC endpoint before invoking the graft daemon client.",
        ));
    }
    Ok(())
}

#[derive(Debug)]
struct GraftLocalIpcClientTransport {
    endpoint: DaemonLocalIpcEndpoint,
}

impl GraftLocalIpcClientTransport {
    fn new(endpoint: DaemonLocalIpcEndpoint) -> Self {
        Self { endpoint }
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
                AtmError::daemon_unavailable("failed to configure graft local IPC write timeout")
                    .with_source(source)
            })?;
        stream
            .set_recv_timeout(Some(SAME_HOST_REQUEST_DEADLINE))
            .map_err(|source| {
                AtmError::daemon_unavailable("failed to configure graft local IPC read timeout")
                    .with_source(source)
            })?;
        let request_id = atm_core::protocol::next_request_id();
        let frame = atm_core::protocol::request_to_frame_payload(request_id, request)?;
        atm_core::protocol::write_frame(
            &mut stream,
            &frame,
            "failed to write graft daemon request frame",
        )?;
        stream.flush().map_err(|source| {
            AtmError::daemon_unavailable("failed to flush graft daemon request frame")
                .with_source(source)
        })?;
        let response_frame = atm_core::protocol::read_frame(
            &mut stream,
            "failed to read graft daemon response frame",
            "graft daemon response frame exceeded the maximum supported size",
        )?
        .ok_or_else(|| {
            AtmError::daemon_unavailable(
                "daemon closed the local IPC connection before returning a graft response frame",
            )
            .with_recovery(
                "Retry the graft request after atm-daemon reaches serving state and inspect daemon logs if the problem persists.",
            )
        })?;
        let (response_id, response) =
            atm_core::protocol::response_from_frame_payload(response_frame)?;
        if response_id != request_id {
            return Err(AtmError::daemon_unavailable(format!(
                "graft daemon response request_id {} did not match request_id {}",
                response_id, request_id
            ))
            .with_recovery(
                "Align the embedding host, atm-graft, and atm-daemon builds so both sides use the same ATM daemon protocol contract before retrying.",
            ));
        }
        Ok(response)
    }
}

impl boundary::sealed::Sealed for GraftLocalIpcClientTransport {}

impl ClientTransport for GraftLocalIpcClientTransport {
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
        transport: &GraftLocalIpcClientTransport,
    ) -> Result<(), AtmError> {
        self.ensure_daemon_available_with_timeout(
            transport,
            AUTO_START_PUBLISH_TIMEOUT,
            Duration::from_millis(25),
        )
    }

    fn ensure_daemon_available_with_timeout(
        &self,
        transport: &GraftLocalIpcClientTransport,
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
        transport: &GraftLocalIpcClientTransport,
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
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(source) => Err(AtmError::daemon_unavailable(format!(
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

fn resolve_daemon_local_ipc_endpoint() -> Result<DaemonLocalIpcEndpoint, AtmError> {
    DaemonLocalIpcEndpoint::new(atm_core::protocol::daemon_socket_path()?)
}

fn resolve_daemon_bin() -> Result<DaemonBinaryPath, AtmError> {
    if let Some(path) = std::env::var_os("ATM_DAEMON_BIN").filter(|value| !value.is_empty()) {
        return DaemonBinaryPath::new(PathBuf::from(path));
    }
    let current = std::env::current_exe().map_err(|source| {
        AtmError::daemon_unavailable("failed to resolve the current graft host executable path")
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
        "Retry the graft operation once. If the mismatch persists, inspect daemon/client version alignment and retained daemon logs before retrying again.",
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::mpsc;
    use std::time::Duration;

    use atm_core::read::{BucketCounts, ReadOutcome};
    use atm_core::schema::LegacyMessageId;
    use atm_core::test_support::{TEST_LEAD, TEST_TEAM};
    use atm_core::transport::testing::FakeClientTransport;
    use atm_core::types::{AckActivationMode, CommandAction, ReadSelection};
    use tempfile::TempDir;

    use super::*;

    #[derive(Debug, Default)]
    struct CollectingInjector {
        nudges: Mutex<Vec<NudgeEvent>>,
    }

    impl HostNudgeInjector for CollectingInjector {
        fn inject_nudge(&self, nudge: NudgeEvent) -> Result<(), AtmError> {
            self.nudges.lock().expect("nudges lock").push(nudge);
            Ok(())
        }
    }

    #[derive(Debug)]
    struct StateNotifyingObservability {
        registered_tx: Mutex<Option<mpsc::Sender<()>>>,
        disconnected_tx: Mutex<Option<mpsc::Sender<()>>>,
    }

    impl StateNotifyingObservability {
        fn new(
            registered_tx: mpsc::Sender<()>,
            disconnected_tx: Option<mpsc::Sender<()>>,
        ) -> Self {
            Self {
                registered_tx: Mutex::new(Some(registered_tx)),
                disconnected_tx: Mutex::new(disconnected_tx),
            }
        }
    }

    impl GraftObservability for StateNotifyingObservability {
        fn session_state_changed(&self, snapshot: &SessionSnapshot) {
            match snapshot.state {
                GraftSessionState::Registered => {
                    if let Some(tx) = self.registered_tx.lock().expect("registered tx lock").as_ref()
                    {
                        let _ = tx.send(());
                    }
                }
                GraftSessionState::Disconnected => {
                    if let Some(tx) = self.disconnected_tx.lock().expect("disconnected tx lock").as_ref()
                    {
                        let _ = tx.send(());
                    }
                }
                _ => {}
            }
        }
    }

    fn write_config(root: &Path, body: &str) {
        std::fs::write(root.join(".atm.toml"), body).expect("write config");
    }

    fn read_query(root: &Path) -> ReadQuery {
        ReadQuery::new(
            root.to_path_buf(),
            root.to_path_buf(),
            Some(TEST_LEAD),
            Some("agent-b@test-team"),
            Some(TEST_TEAM),
            ReadSelection::Unread,
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

    fn send_request(root: &Path) -> SendRequest {
        SendRequest::new(
            root.to_path_buf(),
            root.to_path_buf(),
            Some(TEST_LEAD),
            "agent-b@test-team",
            Some(TEST_TEAM),
            atm_core::send::SendMessageSource::Inline("hello".to_string()),
            None,
            false,
            None,
            false,
        )
        .expect("send request")
    }

    #[test]
    fn session_stays_inactive_without_atm_config() {
        let root = TempDir::new().expect("tempdir");
        let transport = Arc::new(FakeClientTransport::new(|request| {
            panic!("transport should stay unused when graft config is absent: {request:?}")
        }));
        let client = GraftClient::from_transport(transport);
        let injector = Arc::new(CollectingInjector::default());

        let session = GraftSession::activate(
            client,
            GraftSessionOptions::for_current_process(
                root.path(),
                TEST_TEAM.parse().expect("team"),
                TEST_LEAD.parse().expect("agent"),
            ),
            injector,
        )
        .expect("inactive graft session");

        assert_eq!(
            session.snapshot().expect("snapshot").state,
            GraftSessionState::Inactive
        );
    }

    #[test]
    fn session_stays_inactive_when_graft_is_disabled() {
        let root = TempDir::new().expect("tempdir");
        write_config(root.path(), "[atm.graft]\nenabled = false\n");
        let transport = Arc::new(FakeClientTransport::new(|request| {
            panic!("transport should stay unused when graft config is disabled: {request:?}")
        }));
        let client = GraftClient::from_transport(transport);
        let injector = Arc::new(CollectingInjector::default());

        let session = GraftSession::activate(
            client,
            GraftSessionOptions::for_current_process(
                root.path(),
                TEST_TEAM.parse().expect("team"),
                TEST_LEAD.parse().expect("agent"),
            ),
            injector,
        )
        .expect("inactive graft session");

        assert_eq!(
            session.snapshot().expect("snapshot").state,
            GraftSessionState::Inactive
        );
    }

    #[test]
    fn session_registers_and_automatically_injects_nudges() {
        let root = TempDir::new().expect("tempdir");
        write_config(root.path(), "[atm.graft]\nenabled = true\n");
        let register_count = Arc::new(Mutex::new(0usize));
        let drain_count = Arc::new(Mutex::new(0usize));
        let register_count_for_handler = Arc::clone(&register_count);
        let drain_count_for_handler = Arc::clone(&drain_count);
        let nudge_sent = Arc::new(Mutex::new(false));
        let nudge_sent_for_handler = Arc::clone(&nudge_sent);

        let transport = Arc::new(FakeClientTransport::new(move |request| match request {
            RequestEnvelope::GraftRegister(request) => {
                *register_count_for_handler.lock().expect("register count") += 1;
                Ok(ResponseEnvelope::GraftRegister(
                    GraftSessionRegistrationResponse {
                        team: request.team,
                        agent: request.agent,
                        session_id: request.session_id,
                        registered_at: IsoTimestamp::now(),
                        queue_capacity: 16,
                    },
                ))
            }
            RequestEnvelope::GraftDrain(request) => {
                *drain_count_for_handler.lock().expect("drain count") += 1;
                let mut sent = nudge_sent_for_handler.lock().expect("nudge sent");
                let nudges = if *sent {
                    Vec::new()
                } else {
                    *sent = true;
                    vec![NudgeEvent {
                        message_id: LegacyMessageId::new(),
                        from: "sender".parse().expect("sender"),
                        message: "hello graft".to_string(),
                        received_at: IsoTimestamp::now(),
                        task_id: None,
                    }]
                };
                Ok(ResponseEnvelope::GraftDrain(GraftNudgeDrainResponse {
                    session_id: request.session_id,
                    nudges,
                    remaining: 0,
                    dropped_count: 0,
                }))
            }
            RequestEnvelope::GraftUnregister(request) => Ok(ResponseEnvelope::GraftUnregister(
                GraftSessionUnregistrationResponse {
                    session_id: request.session_id,
                    closed: true,
                },
            )),
            other => panic!("unexpected request: {other:?}"),
        }));
        let client = GraftClient::from_transport(transport);
        let (delivered_tx, delivered_rx) = mpsc::channel();
        #[derive(Debug)]
        struct NotifyingInjector {
            nudges: Mutex<Vec<NudgeEvent>>,
            delivered_tx: mpsc::Sender<()>,
        }

        impl NotifyingInjector {
            fn count(&self) -> usize {
                self.nudges.lock().expect("nudges lock").len()
            }
        }

        impl HostNudgeInjector for NotifyingInjector {
            fn inject_nudge(&self, nudge: NudgeEvent) -> Result<(), AtmError> {
                self.nudges.lock().expect("nudges lock").push(nudge);
                let _ = self.delivered_tx.send(());
                Ok(())
            }
        }

        let injected = Arc::new(NotifyingInjector {
            nudges: Mutex::new(Vec::new()),
            delivered_tx,
        });
        let (registered_tx, registered_rx) = mpsc::channel();
        let observability = Arc::new(StateNotifyingObservability::new(registered_tx, None));
        let session = GraftSession::activate_with_observability(
            client,
            GraftSessionOptions::for_current_process(
                root.path(),
                TEST_TEAM.parse().expect("team"),
                TEST_LEAD.parse().expect("agent"),
            )
            .with_batch_limit(GraftBatchLimit::new(16).expect("limit"))
            .with_poll_interval(Duration::from_millis(10))
            .expect("poll interval"),
            Arc::clone(&injected) as Arc<dyn HostNudgeInjector>,
            observability,
        )
        .expect("active graft session");

        registered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("session should report registered after the snapshot write completes");
        delivered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("receive injected nudge");

        assert_eq!(
            session.snapshot().expect("snapshot").state,
            GraftSessionState::Registered
        );
        assert_eq!(injected.count(), 1);
        assert_eq!(*register_count.lock().expect("register count"), 1);
        assert!(*drain_count.lock().expect("drain count") > 0);

        session.close().expect("close graft session");
    }

    #[test]
    fn client_routes_send_read_and_ack_over_transport() {
        let root = TempDir::new().expect("tempdir");
        let read_message_id = LegacyMessageId::new();
        let transport = Arc::new(FakeClientTransport::new(move |request| match request {
            RequestEnvelope::Send(SendRequestEnvelope::Compose(request)) => Ok(
                ResponseEnvelope::Send(SendResponseEnvelope::Sent(SendOutcome {
                    action: CommandAction::Send,
                    team: TEST_TEAM.parse().expect("team"),
                    agent: "agent-b".parse().expect("agent"),
                    sender: request.sender_override.expect("sender"),
                    outcome: "sent".to_string(),
                    message_id: LegacyMessageId::new(),
                    requires_ack: false,
                    task_id: None,
                    summary: Some("summary".to_string()),
                    message: Some("hello".to_string()),
                    warnings: Vec::new(),
                    dry_run: false,
                })),
            ),
            RequestEnvelope::Receive(query) => Ok(ResponseEnvelope::Receive(ReadOutcome {
                action: CommandAction::Read,
                team: query.team_override.expect("team"),
                agent: "agent-b".parse().expect("agent"),
                selection_mode: query.selection_mode,
                mutation_applied: false,
                count: 1,
                message: None,
                selected_message_id: Some(read_message_id),
                match_count: 1,
                additional_match_count: 0,
                bucket_counts: BucketCounts {
                    unread: 1,
                    pending_ack: 0,
                    history: 0,
                },
            })),
            RequestEnvelope::Send(SendRequestEnvelope::Acknowledge(request)) => Ok(
                ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(AckOutcome {
                    action: CommandAction::Ack,
                    team: TEST_TEAM.parse().expect("team"),
                    agent: TEST_LEAD.parse().expect("agent"),
                    message_id: request.message_id,
                    task_id: None,
                    reply_target: serde_json::from_str("\"agent-b@test-team\"")
                        .expect("reply target"),
                    reply_message_id: LegacyMessageId::new(),
                    reply_text: request.reply_body,
                    warnings: Vec::new(),
                })),
            ),
            other => panic!("unexpected request: {other:?}"),
        }));

        let client = GraftClient::from_transport(transport);
        let send_outcome = client
            .send_message(send_request(root.path()))
            .expect("send");
        assert_eq!(send_outcome.outcome, "sent");

        let read_outcome = client.read_message(read_query(root.path())).expect("read");
        assert_eq!(read_outcome.count, 1);

        let ack_outcome = client
            .acknowledge_message(AckRequest {
                home_dir: root.path().to_path_buf(),
                current_dir: root.path().to_path_buf(),
                actor_override: Some(TEST_LEAD.parse().expect("actor")),
                team_override: Some(TEST_TEAM.parse().expect("team")),
                message_id: read_message_id,
                reply_body: "received".to_string(),
            })
            .expect("ack");
        assert_eq!(ack_outcome.reply_text, "received");
    }

    #[test]
    fn session_retries_after_one_drain_failure() {
        let root = TempDir::new().expect("tempdir");
        write_config(root.path(), "[atm.graft]\nenabled = true\n");
        let state = Arc::new(Mutex::new(0usize));
        let state_for_handler = Arc::clone(&state);
        let (notify_tx, notify_rx) = mpsc::channel();

        let transport = Arc::new(FakeClientTransport::new(move |request| match request {
            RequestEnvelope::GraftRegister(request) => Ok(ResponseEnvelope::GraftRegister(
                GraftSessionRegistrationResponse {
                    team: request.team,
                    agent: request.agent,
                    session_id: request.session_id,
                    registered_at: IsoTimestamp::now(),
                    queue_capacity: 16,
                },
            )),
            RequestEnvelope::GraftDrain(request) => {
                let mut state = state_for_handler.lock().expect("state");
                *state += 1;
                if *state == 1 {
                    Err(AtmError::daemon_unavailable("temporary failure"))
                } else {
                    let _ = notify_tx.send(());
                    Ok(ResponseEnvelope::GraftDrain(GraftNudgeDrainResponse {
                        session_id: request.session_id,
                        nudges: Vec::new(),
                        remaining: 0,
                        dropped_count: 0,
                    }))
                }
            }
            RequestEnvelope::GraftUnregister(request) => Ok(ResponseEnvelope::GraftUnregister(
                GraftSessionUnregistrationResponse {
                    session_id: request.session_id,
                    closed: true,
                },
            )),
            other => panic!("unexpected request: {other:?}"),
        }));

        let client = GraftClient::from_transport(transport);
        let (registered_tx, registered_rx) = mpsc::channel();
        let (disconnected_tx, disconnected_rx) = mpsc::channel();
        let observability = Arc::new(StateNotifyingObservability::new(
            registered_tx,
            Some(disconnected_tx),
        ));
        let session = GraftSession::activate_with_observability(
            client,
            GraftSessionOptions::for_current_process(
                root.path(),
                TEST_TEAM.parse().expect("team"),
                TEST_LEAD.parse().expect("agent"),
            )
            .with_batch_limit(GraftBatchLimit::new(16).expect("limit"))
            .with_poll_interval(Duration::from_millis(10))
            .expect("poll interval"),
            Arc::new(CollectingInjector::default()),
            observability,
        )
        .expect("session");

        registered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("initial registration should publish a registered state");
        disconnected_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("drain failure should publish a disconnected state");
        notify_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("receive loop should retry and then re-register");
        registered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("re-registration should publish registered after the snapshot write");
        assert_eq!(
            session.snapshot().expect("snapshot").state,
            GraftSessionState::Registered
        );
        session.close().expect("close");
    }
}
