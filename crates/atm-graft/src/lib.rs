//! Thin embedded ATM client crate for graft-aware host agents.
//! Production nudge delivery uses one live advisory-stream receive loop when
//! the selected transport supports same-host streaming.

use std::fmt;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;
use std::time::Duration;

use atm_core::ack::{AckOutcome, AckRequest};
use atm_core::boundary::ClientTransport;
use atm_core::error::AtmError;
use atm_core::graft::AtmGraftClient;
use atm_core::observability::{
    CommandEvent, NullObservability, ObservabilityPort, action_name, outcome_label,
};
use atm_core::protocol::{
    RequestEnvelope, ResponseEnvelope, SendRequestEnvelope, SendResponseEnvelope,
};
use atm_core::read::{ReadOutcome, ReadQuery};
use atm_core::send::{SendOutcome, SendRequest};
use atm_core::types::{AgentName, IsoTimestamp, TeamName};
use atm_daemon_client::graft_rpc::{
    AdvisoryBatchLimit, AdvisoryDrainRequest, AdvisoryDrainResponse, AdvisoryEvent,
    AdvisoryFetchRequest, AdvisoryFetchResponse, AdvisorySession, AdvisorySessionId,
    AdvisorySessionRegistrationRequest, AdvisorySessionRegistrationResponse, AdvisorySessionState,
    AdvisorySessionUnregistrationRequest, AdvisorySessionUnregistrationResponse,
    AdvisoryStreamRequest,
};
use atm_daemon_client::{
    BootstrapTraceability, DaemonSupervisor, parse_bootstrap_agent, parse_bootstrap_team,
    resolve_daemon_bin, resolve_daemon_local_ipc_endpoint,
};

#[cfg(test)]
use atm_core::send::SendCommandOutcome;

mod runtime;
mod transport;

use runtime::{
    LiveReceiveLoopContext, ReceiveLoopContext, cleanup_registered_session_after_error,
    join_receive_loop_with_deadline, load_graft_config, read_snapshot,
    register_session_with_validated_batch_limit, run_live_receive_loop, run_receive_loop,
    set_session_state,
};
use transport::{ActiveAdvisoryStream, GraftLocalIpcClientTransport, unexpected_response};

const SAME_HOST_REQUEST_DEADLINE: Duration = Duration::from_secs(3);
const ADVISORY_STREAM_READ_DEADLINE: Duration = Duration::from_millis(250);
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(250);
const DEFAULT_BATCH_LIMIT: usize = 64;
const RECEIVE_LOOP_JOIN_DEADLINE: Duration = Duration::from_secs(5);

pub type SessionSnapshot = AdvisorySession;

pub use atm_core::{AtmConfig, GraftConfig};
pub use atm_daemon_client::graft_rpc::{
    AdvisoryDrainRequest as DrainRequest, AdvisoryEvent as Event,
    AdvisoryFetchRequest as FetchRequest, AdvisorySessionId as SessionId,
};

/// Preferred host-facing imports for embedding `atm-graft`.
pub mod prelude {
    pub use super::{
        GraftClient, GraftObservability, GraftSession, GraftSessionOptions, HostNudgeInjector,
        NoopGraftObservability,
    };
}

pub trait AdvisorySessionPort: Send + Sync {
    fn register_session(
        &self,
        request: AdvisorySessionRegistrationRequest,
    ) -> Result<AdvisorySessionRegistrationResponse, AtmError>;

    fn unregister_session(
        &self,
        request: AdvisorySessionUnregistrationRequest,
    ) -> Result<AdvisorySessionUnregistrationResponse, AtmError>;

    fn fetch_nudges(
        &self,
        request: AdvisoryFetchRequest,
    ) -> Result<AdvisoryFetchResponse, AtmError>;

    fn drain_nudges(
        &self,
        request: AdvisoryDrainRequest,
    ) -> Result<AdvisoryDrainResponse, AtmError>;
}

trait AdvisoryTransport: Send + Sync {
    fn register_session(
        &self,
        request: AdvisorySessionRegistrationRequest,
    ) -> Result<AdvisorySessionRegistrationResponse, AtmError>;

    fn unregister_session(
        &self,
        request: AdvisorySessionUnregistrationRequest,
    ) -> Result<AdvisorySessionUnregistrationResponse, AtmError>;

    fn fetch_nudges(
        &self,
        request: AdvisoryFetchRequest,
    ) -> Result<AdvisoryFetchResponse, AtmError>;

    fn drain_nudges(
        &self,
        request: AdvisoryDrainRequest,
    ) -> Result<AdvisoryDrainResponse, AtmError>;

    fn supports_live_advisory_stream(&self) -> bool;

    fn open_advisory_stream(
        &self,
        request: AdvisoryStreamRequest,
    ) -> Result<ActiveAdvisoryStream, AtmError>;
}

/// Host-owned bridge for automatic between-tool-call nudge injection.
pub trait HostNudgeInjector: Send + Sync {
    /// # Errors
    ///
    /// Returns [`AtmError`] when the host cannot safely inject the nudge into
    /// its between-tool-call context flow.
    fn inject_nudge(&self, nudge: AdvisoryEvent) -> Result<(), AtmError>;
}

/// ATM-owned graft observability boundary supplied by the embedding host.
pub trait GraftObservability: Send + Sync {
    fn session_state_changed(&self, _snapshot: &SessionSnapshot) {}

    fn nudge_delivered(&self, _session_id: &AdvisorySessionId, _nudge: &AdvisoryEvent) {}

    fn session_error(
        &self,
        _session_id: &AdvisorySessionId,
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
    session_id: AdvisorySessionId,
    batch_limit: AdvisoryBatchLimit,
    poll_interval: Duration,
}

impl GraftSessionOptions {
    pub fn new(
        workspace_root: impl Into<PathBuf>,
        team: TeamName,
        agent: AgentName,
        session_id: AdvisorySessionId,
    ) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            team,
            agent,
            session_id,
            batch_limit: AdvisoryBatchLimit::new(DEFAULT_BATCH_LIMIT)
                .expect("default graft batch limit must be valid"),
            poll_interval: DEFAULT_POLL_INTERVAL,
        }
    }

    pub fn for_current_process(
        workspace_root: impl Into<PathBuf>,
        team: TeamName,
        agent: AgentName,
    ) -> Self {
        let session_id = AdvisorySessionId::new(format!("{agent}-{}", std::process::id()))
            .expect("derived graft session id must be valid");
        Self::new(workspace_root, team, agent, session_id)
    }

    pub fn with_batch_limit(mut self, batch_limit: AdvisoryBatchLimit) -> Self {
        self.batch_limit = batch_limit;
        self
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
            session_id: self.session_id.clone(),
            state: AdvisorySessionState::Inactive,
        }
    }

    fn registration_request(&self) -> AdvisorySessionRegistrationRequest {
        AdvisorySessionRegistrationRequest {
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
    advisory_transport: Arc<dyn AdvisoryTransport>,
}

trait GraftSessionClient: AtmGraftClient + AdvisorySessionPort {
    fn supports_live_advisory_stream(&self) -> bool;

    fn open_advisory_stream(
        &self,
        request: AdvisoryStreamRequest,
    ) -> Result<ActiveAdvisoryStream, AtmError>;
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
        let advisory_transport = Arc::new(GraftLocalIpcClientTransport::new(endpoint.clone()));
        let transport = Arc::clone(&advisory_transport) as Arc<dyn ClientTransport + Send + Sync>;
        let supervisor = DaemonSupervisor::new(endpoint, daemon_bin);
        // GraftClient::connect() has no host-supplied observability port yet, so bootstrap
        // traceability currently preserves typed caller-facing errors but intentionally drops the
        // retained bootstrap event stream until a shared graft-host observability sink exists.
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
            advisory_transport.probe_connection().map(|_| ())
        })?;
        Ok(Self {
            transport,
            advisory_transport,
        })
    }

    #[cfg(test)]
    fn from_transport(transport: Arc<dyn ClientTransport + Send + Sync>) -> Self {
        Self::from_transport_with_advisory(transport, Arc::new(PanicAdvisoryTransport))
    }

    #[cfg(test)]
    fn from_transport_with_advisory(
        transport: Arc<dyn ClientTransport + Send + Sync>,
        advisory_transport: Arc<dyn AdvisoryTransport>,
    ) -> Self {
        Self {
            transport,
            advisory_transport,
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
}

#[cfg(test)]
#[derive(Debug)]
struct PanicAdvisoryTransport;

#[cfg(test)]
impl AdvisoryTransport for PanicAdvisoryTransport {
    fn register_session(
        &self,
        _request: AdvisorySessionRegistrationRequest,
    ) -> Result<AdvisorySessionRegistrationResponse, AtmError> {
        panic!("unexpected advisory register call in PanicAdvisoryTransport")
    }

    fn unregister_session(
        &self,
        _request: AdvisorySessionUnregistrationRequest,
    ) -> Result<AdvisorySessionUnregistrationResponse, AtmError> {
        panic!("unexpected advisory unregister call in PanicAdvisoryTransport")
    }

    fn fetch_nudges(
        &self,
        _request: AdvisoryFetchRequest,
    ) -> Result<AdvisoryFetchResponse, AtmError> {
        panic!("unexpected advisory fetch call in PanicAdvisoryTransport")
    }

    fn drain_nudges(
        &self,
        _request: AdvisoryDrainRequest,
    ) -> Result<AdvisoryDrainResponse, AtmError> {
        panic!("unexpected advisory drain call in PanicAdvisoryTransport")
    }

    fn supports_live_advisory_stream(&self) -> bool {
        false
    }

    fn open_advisory_stream(
        &self,
        _request: AdvisoryStreamRequest,
    ) -> Result<ActiveAdvisoryStream, AtmError> {
        panic!("unexpected advisory stream call in PanicAdvisoryTransport")
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

impl AdvisorySessionPort for GraftClient {
    fn register_session(
        &self,
        request: AdvisorySessionRegistrationRequest,
    ) -> Result<AdvisorySessionRegistrationResponse, AtmError> {
        self.advisory_transport.register_session(request)
    }

    fn unregister_session(
        &self,
        request: AdvisorySessionUnregistrationRequest,
    ) -> Result<AdvisorySessionUnregistrationResponse, AtmError> {
        self.advisory_transport.unregister_session(request)
    }

    fn fetch_nudges(
        &self,
        request: AdvisoryFetchRequest,
    ) -> Result<AdvisoryFetchResponse, AtmError> {
        self.advisory_transport.fetch_nudges(request)
    }

    fn drain_nudges(
        &self,
        request: AdvisoryDrainRequest,
    ) -> Result<AdvisoryDrainResponse, AtmError> {
        self.advisory_transport.drain_nudges(request)
    }
}

impl GraftSessionClient for GraftClient {
    fn supports_live_advisory_stream(&self) -> bool {
        self.advisory_transport.supports_live_advisory_stream()
    }

    fn open_advisory_stream(
        &self,
        request: AdvisoryStreamRequest,
    ) -> Result<ActiveAdvisoryStream, AtmError> {
        self.advisory_transport.open_advisory_stream(request)
    }
}

/// Concrete embedded graft session runtime.
pub struct GraftSession {
    client: Arc<dyn GraftSessionClient>,
    // Snapshot state is shared between the host thread and the receive loop; poisoned locks are
    // mapped into AtmError by read_snapshot()/set_session_state() instead of panicking.
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
        // Config is loaded from disk on each activate() call. Per-activation disk reads are
        // accepted by design: activate() is not a hot path, and caching would require
        // invalidation logic that adds complexity with no practical benefit.
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
            return inactive_session(client, snapshot, observability);
        };
        if !graft_config.enabled {
            return inactive_session(client, snapshot, observability);
        }

        set_session_state(
            &snapshot,
            AdvisorySessionState::Connecting,
            observability.as_ref(),
        )?;

        register_graft_session(client.as_ref(), &options)?;
        set_session_state(
            &snapshot,
            AdvisorySessionState::Registered,
            observability.as_ref(),
        )
        .map_err(|error| {
            cleanup_registered_session_after_error(
                client.as_ref(),
                &options.session_id,
                "graft activation state publication",
                error,
            )
        })?;

        let (stop_tx, join_handle) = Self::start_graft_receive_loop(
            client.as_ref(),
            &options,
            Arc::clone(&client),
            Arc::clone(&snapshot),
            injector,
            Arc::clone(&observability),
        )
        .map_err(|error| {
            cleanup_registered_session_after_error(
                client.as_ref(),
                &options.session_id,
                "graft receive-loop startup",
                error,
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

    fn start_graft_receive_loop(
        client: &dyn GraftSessionClient,
        options: &GraftSessionOptions,
        worker_client: Arc<dyn GraftSessionClient>,
        worker_snapshot: Arc<RwLock<SessionSnapshot>>,
        injector: Arc<dyn HostNudgeInjector>,
        worker_observability: Arc<dyn GraftObservability>,
    ) -> Result<GraftReceiveLoopWorker, AtmError> {
        let registration_request = options.registration_request();
        let drain_request = AdvisoryDrainRequest {
            session_id: options.session_id.clone(),
            limit: options.batch_limit,
        };
        let advisory_stream_request = AdvisoryStreamRequest {
            registration: registration_request.clone(),
            limit: options.batch_limit,
        };
        let (stop_tx, stop_rx) = mpsc::channel();
        let join_handle = spawn_graft_receive_loop(
            client,
            options,
            worker_client,
            registration_request,
            advisory_stream_request,
            drain_request,
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
    /// Returns [`AtmError`] when the receive loop cannot join cleanly or the
    /// daemon-side unregister path fails during shutdown.
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
                AdvisorySessionState::CloseFailed,
                self.observability.as_ref(),
            )?;
            return Err(error);
        }
        set_session_state(
            &self.snapshot,
            AdvisorySessionState::Closed,
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

fn register_graft_session(
    client: &dyn GraftSessionClient,
    options: &GraftSessionOptions,
) -> Result<(), AtmError> {
    register_session_with_validated_batch_limit(
        client,
        options.registration_request(),
        options.batch_limit,
    )
    .map(|_| ())
}

#[allow(clippy::too_many_arguments)]
fn spawn_graft_receive_loop(
    client: &dyn GraftSessionClient,
    options: &GraftSessionOptions,
    worker_client: Arc<dyn GraftSessionClient>,
    registration_request: AdvisorySessionRegistrationRequest,
    advisory_stream_request: AdvisoryStreamRequest,
    drain_request: AdvisoryDrainRequest,
    worker_snapshot: Arc<RwLock<SessionSnapshot>>,
    injector: Arc<dyn HostNudgeInjector>,
    worker_observability: Arc<dyn GraftObservability>,
    stop_rx: Receiver<()>,
) -> Result<std::thread::JoinHandle<Result<(), AtmError>>, AtmError> {
    if client.supports_live_advisory_stream() {
        return spawn_live_receive_loop(
            options,
            worker_client,
            registration_request,
            advisory_stream_request,
            worker_snapshot,
            injector,
            worker_observability,
            stop_rx,
        );
    }
    spawn_polling_receive_loop(
        options,
        worker_client,
        registration_request,
        drain_request,
        worker_snapshot,
        injector,
        worker_observability,
        stop_rx,
    )
}

#[allow(clippy::too_many_arguments)]
fn spawn_live_receive_loop(
    options: &GraftSessionOptions,
    worker_client: Arc<dyn GraftSessionClient>,
    registration_request: AdvisorySessionRegistrationRequest,
    advisory_stream_request: AdvisoryStreamRequest,
    worker_snapshot: Arc<RwLock<SessionSnapshot>>,
    injector: Arc<dyn HostNudgeInjector>,
    worker_observability: Arc<dyn GraftObservability>,
    stop_rx: Receiver<()>,
) -> Result<std::thread::JoinHandle<Result<(), AtmError>>, AtmError> {
    let thread_name = format!("atm-graft-{}", options.session_id);
    let limit = options.batch_limit;
    let reconnect_backoff = options.poll_interval;
    let stream = worker_client.open_advisory_stream(advisory_stream_request)?;
    std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            run_live_receive_loop(LiveReceiveLoopContext {
                client: worker_client,
                registration_request,
                advisory_stream: stream,
                limit,
                reconnect_backoff,
                snapshot: worker_snapshot,
                injector,
                observability: worker_observability,
                stop_rx,
            })
        })
        .map_err(spawn_receive_loop_error)
}

#[allow(clippy::too_many_arguments)]
fn spawn_polling_receive_loop(
    options: &GraftSessionOptions,
    worker_client: Arc<dyn GraftSessionClient>,
    registration_request: AdvisorySessionRegistrationRequest,
    drain_request: AdvisoryDrainRequest,
    worker_snapshot: Arc<RwLock<SessionSnapshot>>,
    injector: Arc<dyn HostNudgeInjector>,
    worker_observability: Arc<dyn GraftObservability>,
    stop_rx: Receiver<()>,
) -> Result<std::thread::JoinHandle<Result<(), AtmError>>, AtmError> {
    let thread_name = format!("atm-graft-{}", options.session_id);
    let poll_interval = options.poll_interval;
    std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            run_receive_loop(ReceiveLoopContext {
                client: worker_client,
                registration_request,
                drain_request,
                poll_interval,
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
        // Drop-triggered teardown emits AdvisorySessionState::Closed, identical to explicit close().
        // Merged observable state is accepted by design: callers that need to distinguish
        // drop-driven shutdown from user-directed shutdown should call close() explicitly.
        if let Err(error) = self.close_internal() {
            let session_id = self
                .snapshot()
                .map(|snapshot| snapshot.session_id.to_string())
                .unwrap_or_else(|snapshot_error| format!("unavailable:{snapshot_error}"));
            tracing::warn!(
                session_id,
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

impl AdvisorySessionPort for GraftSession {
    fn register_session(
        &self,
        request: AdvisorySessionRegistrationRequest,
    ) -> Result<AdvisorySessionRegistrationResponse, AtmError> {
        self.client.register_session(request)
    }

    fn unregister_session(
        &self,
        request: AdvisorySessionUnregistrationRequest,
    ) -> Result<AdvisorySessionUnregistrationResponse, AtmError> {
        self.client.unregister_session(request)
    }

    fn fetch_nudges(
        &self,
        request: AdvisoryFetchRequest,
    ) -> Result<AdvisoryFetchResponse, AtmError> {
        self.client.fetch_nudges(request)
    }

    fn drain_nudges(
        &self,
        request: AdvisoryDrainRequest,
    ) -> Result<AdvisoryDrainResponse, AtmError> {
        self.client.drain_nudges(request)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::mpsc;
    use std::time::Duration;

    use atm_core::protocol::{
        self, RequestEnvelope as CoreRequestEnvelope, ResponseEnvelope as CoreResponseEnvelope,
    };
    use atm_core::read::{BucketCounts, ReadOutcome};
    use atm_core::schema::AtmMessageId;
    use atm_core::test_support::{TEST_LEAD, TEST_TEAM};
    use atm_core::transport::testing::FakeClientTransport;
    use atm_core::types::{AckActivationMode, CommandAction, ReadSelection};
    use atm_daemon_client::RequestId as DaemonRequestId;
    use atm_daemon_client::graft_rpc::{
        self, AdvisoryMessage, AdvisorySessionRegistrationResponse,
        AdvisorySessionUnregistrationResponse, AdvisoryStreamResponse,
        RequestEnvelope as GraftRequestEnvelope, ResponseEnvelope as GraftResponseEnvelope,
    };
    use interprocess::local_socket::prelude::*;
    use interprocess::local_socket::{ListenerOptions, Stream as LocalSocketStream};
    use tempfile::TempDir;

    use super::*;

    #[derive(Debug, Default)]
    struct CollectingInjector {
        // Test threads inject nudges concurrently with assertions, so collected events need
        // one shared mutable buffer behind a Mutex.
        nudges: Mutex<Vec<AdvisoryEvent>>,
    }

    impl HostNudgeInjector for CollectingInjector {
        fn inject_nudge(&self, nudge: AdvisoryEvent) -> Result<(), AtmError> {
            self.nudges.lock().expect("nudges lock").push(nudge);
            Ok(())
        }
    }

    #[derive(Debug)]
    struct StateNotifyingObservability {
        // Registration state changes are emitted from the receive loop thread, so the optional
        // one-shot sender must be shared mutably behind a Mutex.
        registered_tx: Mutex<Option<mpsc::Sender<()>>>,
        // Disconnect notifications are emitted from the receive loop thread, so the optional
        // one-shot sender must be shared mutably behind a Mutex.
        disconnected_tx: Mutex<Option<mpsc::Sender<()>>>,
    }

    impl StateNotifyingObservability {
        fn new(registered_tx: mpsc::Sender<()>, disconnected_tx: Option<mpsc::Sender<()>>) -> Self {
            Self {
                registered_tx: Mutex::new(Some(registered_tx)),
                disconnected_tx: Mutex::new(disconnected_tx),
            }
        }
    }

    impl GraftObservability for StateNotifyingObservability {
        fn session_state_changed(&self, snapshot: &SessionSnapshot) {
            match snapshot.state {
                AdvisorySessionState::Registered => {
                    if let Some(tx) = self
                        .registered_tx
                        .lock()
                        .expect("registered tx lock")
                        .as_ref()
                    {
                        let _ = tx.send(());
                    }
                }
                AdvisorySessionState::Disconnected => {
                    if let Some(tx) = self
                        .disconnected_tx
                        .lock()
                        .expect("disconnected tx lock")
                        .as_ref()
                    {
                        let _ = tx.send(());
                    }
                }
                _ => {}
            }
        }
    }

    #[derive(Debug, Default)]
    struct PanicLiveClient;

    impl AtmGraftClient for PanicLiveClient {
        fn send_message(&self, _request: SendRequest) -> Result<SendOutcome, AtmError> {
            panic!("unexpected send_message call in live receive-loop test")
        }

        fn read_message(&self, _query: ReadQuery) -> Result<ReadOutcome, AtmError> {
            panic!("unexpected read_message call in live receive-loop test")
        }

        fn acknowledge_message(&self, _request: AckRequest) -> Result<AckOutcome, AtmError> {
            panic!("unexpected acknowledge_message call in live receive-loop test")
        }
    }

    impl AdvisorySessionPort for PanicLiveClient {
        fn register_session(
            &self,
            _request: AdvisorySessionRegistrationRequest,
        ) -> Result<AdvisorySessionRegistrationResponse, AtmError> {
            panic!("unexpected register_session call in live receive-loop test")
        }

        fn unregister_session(
            &self,
            _request: AdvisorySessionUnregistrationRequest,
        ) -> Result<AdvisorySessionUnregistrationResponse, AtmError> {
            panic!("unexpected unregister_session call in live receive-loop test")
        }

        fn fetch_nudges(
            &self,
            _request: AdvisoryFetchRequest,
        ) -> Result<AdvisoryFetchResponse, AtmError> {
            panic!("unexpected fetch_nudges call in live receive-loop test")
        }

        fn drain_nudges(
            &self,
            _request: AdvisoryDrainRequest,
        ) -> Result<AdvisoryDrainResponse, AtmError> {
            panic!("unexpected drain_nudges call in live receive-loop test")
        }
    }

    impl GraftSessionClient for PanicLiveClient {
        fn supports_live_advisory_stream(&self) -> bool {
            true
        }

        fn open_advisory_stream(
            &self,
            _request: AdvisoryStreamRequest,
        ) -> Result<ActiveAdvisoryStream, AtmError> {
            panic!("unexpected open_advisory_stream call in live receive-loop test")
        }
    }

    fn panic_shared_transport() -> Arc<dyn ClientTransport + Send + Sync> {
        Arc::new(FakeClientTransport::new(|request| {
            panic!("unexpected shared transport request in graft advisory test: {request:?}")
        }))
    }

    type AdvisoryHandler =
        dyn Fn(GraftRequestEnvelope) -> Result<GraftResponseEnvelope, AtmError> + Send + Sync;
    type AdvisoryStreamOpener =
        dyn Fn(AdvisoryStreamRequest) -> Result<ActiveAdvisoryStream, AtmError> + Send + Sync;

    #[derive(Clone)]
    struct FakeAdvisoryTransport {
        handler: Arc<AdvisoryHandler>,
        stream_opener: Option<Arc<AdvisoryStreamOpener>>,
    }

    impl std::fmt::Debug for FakeAdvisoryTransport {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("FakeAdvisoryTransport")
                .field("supports_live_stream", &self.stream_opener.is_some())
                .finish()
        }
    }

    impl FakeAdvisoryTransport {
        fn new(
            handler: impl Fn(GraftRequestEnvelope) -> Result<GraftResponseEnvelope, AtmError>
            + Send
            + Sync
            + 'static,
        ) -> Self {
            Self {
                handler: Arc::new(handler),
                stream_opener: None,
            }
        }
    }

    impl AdvisoryTransport for FakeAdvisoryTransport {
        fn register_session(
            &self,
            request: AdvisorySessionRegistrationRequest,
        ) -> Result<AdvisorySessionRegistrationResponse, AtmError> {
            match (self.handler)(GraftRequestEnvelope::AdvisoryRegister(request))? {
                GraftResponseEnvelope::AdvisoryRegister(response) => Ok(response),
                GraftResponseEnvelope::Error(error) => Err(error.into_atm_error()),
                other => Err(AtmError::validation(format!(
                    "unexpected advisory register response in test transport: {other:?}"
                ))),
            }
        }

        fn unregister_session(
            &self,
            request: AdvisorySessionUnregistrationRequest,
        ) -> Result<AdvisorySessionUnregistrationResponse, AtmError> {
            match (self.handler)(GraftRequestEnvelope::AdvisoryUnregister(request))? {
                GraftResponseEnvelope::AdvisoryUnregister(response) => Ok(response),
                GraftResponseEnvelope::Error(error) => Err(error.into_atm_error()),
                other => Err(AtmError::validation(format!(
                    "unexpected advisory unregister response in test transport: {other:?}"
                ))),
            }
        }

        fn fetch_nudges(
            &self,
            request: AdvisoryFetchRequest,
        ) -> Result<AdvisoryFetchResponse, AtmError> {
            match (self.handler)(GraftRequestEnvelope::AdvisoryFetch(request))? {
                GraftResponseEnvelope::AdvisoryFetch(response) => Ok(response),
                GraftResponseEnvelope::Error(error) => Err(error.into_atm_error()),
                other => Err(AtmError::validation(format!(
                    "unexpected advisory fetch response in test transport: {other:?}"
                ))),
            }
        }

        fn drain_nudges(
            &self,
            request: AdvisoryDrainRequest,
        ) -> Result<AdvisoryDrainResponse, AtmError> {
            match (self.handler)(GraftRequestEnvelope::AdvisoryDrain(request))? {
                GraftResponseEnvelope::AdvisoryDrain(response) => Ok(response),
                GraftResponseEnvelope::Error(error) => Err(error.into_atm_error()),
                other => Err(AtmError::validation(format!(
                    "unexpected advisory drain response in test transport: {other:?}"
                ))),
            }
        }

        fn supports_live_advisory_stream(&self) -> bool {
            self.stream_opener.is_some()
        }

        fn open_advisory_stream(
            &self,
            request: AdvisoryStreamRequest,
        ) -> Result<ActiveAdvisoryStream, AtmError> {
            self.stream_opener
                .as_ref()
                .expect("test requested live advisory stream without a stream opener")(
                request
            )
        }
    }

    fn write_config(root: &Path, body: &str) {
        std::fs::write(root.join(".atm.toml"), body).expect("write config");
    }

    fn read_query(root: &Path) -> ReadQuery {
        ReadQuery::new(
            root.to_path_buf(),
            root.to_path_buf(),
            TEST_LEAD.parse().expect("caller"),
            Some("agent-b@test-team"),
            TEST_TEAM.parse().expect("team"),
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
            TEST_LEAD.parse().expect("caller"),
            "agent-b@test-team",
            TEST_TEAM.parse().expect("team"),
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
            AdvisorySessionState::Inactive
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
            AdvisorySessionState::Inactive
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

        let advisory_transport =
            Arc::new(FakeAdvisoryTransport::new(move |request| match request {
                GraftRequestEnvelope::AdvisoryRegister(request) => {
                    *register_count_for_handler.lock().expect("register count") += 1;
                    Ok(GraftResponseEnvelope::AdvisoryRegister(
                        AdvisorySessionRegistrationResponse {
                            team: request.team,
                            agent: request.agent,
                            session_id: request.session_id,
                            registered_at: IsoTimestamp::now(),
                            queue_capacity: 16,
                        },
                    ))
                }
                GraftRequestEnvelope::AdvisoryDrain(request) => {
                    *drain_count_for_handler.lock().expect("drain count") += 1;
                    let mut sent = nudge_sent_for_handler.lock().expect("nudge sent");
                    let nudges = if *sent {
                        Vec::new()
                    } else {
                        *sent = true;
                        vec![AdvisoryEvent {
                            message_id: AtmMessageId::new(),
                            from: "sender".parse().expect("sender"),
                            message: AdvisoryMessage::new("hello graft").expect("message"),
                            received_at: IsoTimestamp::now(),
                            task_id: None,
                        }]
                    };
                    Ok(GraftResponseEnvelope::AdvisoryDrain(
                        AdvisoryDrainResponse {
                            session_id: request.session_id,
                            nudges,
                            remaining: 0,
                            dropped_count: 0,
                        },
                    ))
                }
                GraftRequestEnvelope::AdvisoryUnregister(request) => {
                    Ok(GraftResponseEnvelope::AdvisoryUnregister(
                        AdvisorySessionUnregistrationResponse {
                            session_id: request.session_id,
                            closed: true,
                        },
                    ))
                }
                other => panic!("unexpected request: {other:?}"),
            }));
        let client =
            GraftClient::from_transport_with_advisory(panic_shared_transport(), advisory_transport);
        let (delivered_tx, delivered_rx) = mpsc::channel();
        #[derive(Debug)]
        struct NotifyingInjector {
            nudges: Mutex<Vec<AdvisoryEvent>>,
            delivered_tx: mpsc::Sender<()>,
        }

        impl NotifyingInjector {
            fn count(&self) -> usize {
                self.nudges.lock().expect("nudges lock").len()
            }
        }

        impl HostNudgeInjector for NotifyingInjector {
            fn inject_nudge(&self, nudge: AdvisoryEvent) -> Result<(), AtmError> {
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
            .with_batch_limit(AdvisoryBatchLimit::new(16).expect("limit"))
            .with_poll_interval(Duration::from_millis(10)),
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
            AdvisorySessionState::Registered
        );
        assert_eq!(injected.count(), 1);
        assert_eq!(*register_count.lock().expect("register count"), 1);
        assert!(*drain_count.lock().expect("drain count") > 0);

        session.close().expect("close graft session");
    }

    #[test]
    fn client_routes_send_read_and_ack_over_transport() {
        let root = TempDir::new().expect("tempdir");
        let read_message_id = AtmMessageId::new();
        let transport = Arc::new(FakeClientTransport::new(move |request| match request {
            CoreRequestEnvelope::Send(SendRequestEnvelope::Compose(request)) => Ok(
                CoreResponseEnvelope::Send(SendResponseEnvelope::Sent(SendOutcome {
                    action: CommandAction::Send,
                    team: TEST_TEAM.parse().expect("team"),
                    agent: "agent-b".parse().expect("agent"),
                    sender: request.caller_identity,
                    outcome: SendCommandOutcome::Sent,
                    message_id: AtmMessageId::new(),
                    requires_ack: false,
                    task_id: None,
                    summary: Some("summary".to_string()),
                    message: Some("hello".to_string()),
                    warnings: Vec::new(),
                    dry_run: false,
                })),
            ),
            CoreRequestEnvelope::Receive(query) => {
                Ok(CoreResponseEnvelope::Receive(Box::new(ReadOutcome {
                    action: CommandAction::Read,
                    team: query.team_override().cloned().expect("team"),
                    agent: "agent-b".parse().expect("agent"),
                    selection_mode: query.selection_mode(),
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
                })))
            }
            CoreRequestEnvelope::Send(SendRequestEnvelope::Acknowledge(request)) => Ok(
                CoreResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(AckOutcome {
                    action: CommandAction::Ack,
                    team: TEST_TEAM.parse().expect("team"),
                    agent: TEST_LEAD.parse().expect("agent"),
                    message_id: request.message_id,
                    task_id: None,
                    reply_target: serde_json::from_str("\"agent-b@test-team\"")
                        .expect("reply target"),
                    reply_message_id: AtmMessageId::new(),
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
        assert_eq!(send_outcome.outcome, SendCommandOutcome::Sent);

        let read_outcome = client.read_message(read_query(root.path())).expect("read");
        assert_eq!(read_outcome.count, 1);

        let ack_outcome = client
            .acknowledge_message(AckRequest {
                home_dir: root.path().to_path_buf(),
                current_dir: root.path().to_path_buf(),
                caller_identity: TEST_LEAD.parse().expect("caller"),
                caller_team: TEST_TEAM.parse().expect("team"),
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

        let advisory_transport =
            Arc::new(FakeAdvisoryTransport::new(move |request| match request {
                GraftRequestEnvelope::AdvisoryRegister(request) => Ok(
                    GraftResponseEnvelope::AdvisoryRegister(AdvisorySessionRegistrationResponse {
                        team: request.team,
                        agent: request.agent,
                        session_id: request.session_id,
                        registered_at: IsoTimestamp::now(),
                        queue_capacity: 16,
                    }),
                ),
                GraftRequestEnvelope::AdvisoryDrain(request) => {
                    let mut state = state_for_handler.lock().expect("state");
                    *state += 1;
                    if *state == 1 {
                        Err(AtmError::daemon_unavailable("temporary failure"))
                    } else {
                        let _ = notify_tx.send(());
                        Ok(GraftResponseEnvelope::AdvisoryDrain(
                            AdvisoryDrainResponse {
                                session_id: request.session_id,
                                nudges: Vec::new(),
                                remaining: 0,
                                dropped_count: 0,
                            },
                        ))
                    }
                }
                GraftRequestEnvelope::AdvisoryUnregister(request) => {
                    Ok(GraftResponseEnvelope::AdvisoryUnregister(
                        AdvisorySessionUnregistrationResponse {
                            session_id: request.session_id,
                            closed: true,
                        },
                    ))
                }
                other => panic!("unexpected request: {other:?}"),
            }));
        let client =
            GraftClient::from_transport_with_advisory(panic_shared_transport(), advisory_transport);
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
            .with_batch_limit(AdvisoryBatchLimit::new(16).expect("limit"))
            .with_poll_interval(Duration::from_millis(10)),
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
            AdvisorySessionState::Registered
        );
        session.close().expect("close");
    }

    #[test]
    fn live_receive_loop_injects_streamed_nudge_and_exits_on_eof() {
        let tempdir = TempDir::new().expect("tempdir");
        let endpoint_path = tempdir.path().join("graft-live.sock");
        let listener = ListenerOptions::new()
            .name(protocol::daemon_local_ipc_name_from_path(&endpoint_path).expect("endpoint"))
            .create_sync()
            .expect("create listener");
        let request_id =
            DaemonRequestId::new(protocol::next_request_id().into_inner()).expect("request id");
        let expected_session_id = AdvisorySessionId::new("live-session").expect("session id");
        let server_session_id = expected_session_id.clone();
        let server = std::thread::spawn(move || {
            let mut stream = listener.accept().expect("accept");
            let frame = graft_rpc::response_to_frame_payload(
                request_id,
                GraftResponseEnvelope::AdvisoryStream(AdvisoryStreamResponse {
                    session_id: server_session_id,
                    nudges: vec![AdvisoryEvent {
                        message_id: AtmMessageId::new(),
                        from: "sender".parse().expect("sender"),
                        message: AdvisoryMessage::new("hello live graft").expect("message"),
                        received_at: IsoTimestamp::now(),
                        task_id: None,
                    }],
                    remaining: 0,
                    dropped_count: 0,
                }),
            )
            .expect("response frame");
            graft_rpc::write_frame(&mut stream, &frame, "write advisory frame")
                .expect("write advisory frame");
            stream.flush().expect("flush advisory frame");
        });

        let stream = LocalSocketStream::connect(
            protocol::daemon_local_ipc_name_from_path(&endpoint_path).expect("endpoint"),
        )
        .expect("connect");
        let snapshot = Arc::new(std::sync::RwLock::new(SessionSnapshot {
            team: TEST_TEAM.parse().expect("team"),
            agent: TEST_LEAD.parse().expect("agent"),
            session_id: expected_session_id.clone(),
            state: AdvisorySessionState::Connecting,
        }));
        let injector = Arc::new(CollectingInjector::default());
        let (registered_tx, registered_rx) = mpsc::channel();
        let observability = Arc::new(StateNotifyingObservability::new(registered_tx, None));
        let (_stop_tx, stop_rx) = mpsc::channel();

        run_live_receive_loop(LiveReceiveLoopContext {
            client: Arc::new(PanicLiveClient),
            registration_request: AdvisorySessionRegistrationRequest {
                team: TEST_TEAM.parse().expect("team"),
                agent: TEST_LEAD.parse().expect("agent"),
                session_id: expected_session_id,
                pid: std::process::id(),
                started_at: IsoTimestamp::now(),
            },
            advisory_stream: ActiveAdvisoryStream { stream, request_id },
            limit: AdvisoryBatchLimit::new(8).expect("limit"),
            reconnect_backoff: Duration::from_millis(10),
            snapshot: Arc::clone(&snapshot),
            injector: Arc::clone(&injector) as Arc<dyn HostNudgeInjector>,
            observability,
            stop_rx,
        })
        .expect("live receive loop");
        server.join().expect("join server");

        registered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("registered state notification");
        assert_eq!(injector.nudges.lock().expect("nudges lock").len(), 1);
        assert_eq!(
            injector.nudges.lock().expect("nudges lock")[0].message,
            "hello live graft"
        );
        assert_eq!(
            read_snapshot(&snapshot).expect("snapshot").state,
            AdvisorySessionState::Registered
        );
    }

    #[test]
    fn session_activation_cleans_up_registered_slot_when_batch_limit_validation_fails() {
        let root = TempDir::new().expect("tempdir");
        write_config(root.path(), "[atm.graft]\nenabled = true\n");
        let unregister_count = Arc::new(Mutex::new(0usize));
        let unregister_count_for_handler = Arc::clone(&unregister_count);

        let advisory_transport =
            Arc::new(FakeAdvisoryTransport::new(move |request| match request {
                GraftRequestEnvelope::AdvisoryRegister(request) => Ok(
                    GraftResponseEnvelope::AdvisoryRegister(AdvisorySessionRegistrationResponse {
                        team: request.team,
                        agent: request.agent,
                        session_id: request.session_id,
                        registered_at: IsoTimestamp::now(),
                        queue_capacity: 1,
                    }),
                ),
                GraftRequestEnvelope::AdvisoryUnregister(request) => {
                    *unregister_count_for_handler
                        .lock()
                        .expect("unregister count") += 1;
                    Ok(GraftResponseEnvelope::AdvisoryUnregister(
                        AdvisorySessionUnregistrationResponse {
                            session_id: request.session_id,
                            closed: true,
                        },
                    ))
                }
                other => panic!("unexpected request: {other:?}"),
            }));
        let client =
            GraftClient::from_transport_with_advisory(panic_shared_transport(), advisory_transport);
        let injector = Arc::new(CollectingInjector::default());

        let error = GraftSession::activate(
            client,
            GraftSessionOptions::for_current_process(
                root.path(),
                TEST_TEAM.parse().expect("team"),
                TEST_LEAD.parse().expect("agent"),
            )
            .with_batch_limit(AdvisoryBatchLimit::new(8).expect("limit")),
            injector,
        )
        .expect_err("batch-limit validation should fail");

        assert!(error.is_validation());
        assert_eq!(*unregister_count.lock().expect("unregister count"), 1);
    }

    #[derive(Debug, Default)]
    struct ActivationFailureClient {
        unregister_calls: Mutex<Vec<AdvisorySessionId>>,
    }

    impl AtmGraftClient for ActivationFailureClient {
        fn send_message(&self, _request: SendRequest) -> Result<SendOutcome, AtmError> {
            panic!("unexpected send_message call in activation cleanup test")
        }

        fn read_message(&self, _query: ReadQuery) -> Result<ReadOutcome, AtmError> {
            panic!("unexpected read_message call in activation cleanup test")
        }

        fn acknowledge_message(&self, _request: AckRequest) -> Result<AckOutcome, AtmError> {
            panic!("unexpected acknowledge_message call in activation cleanup test")
        }
    }

    impl AdvisorySessionPort for ActivationFailureClient {
        fn register_session(
            &self,
            request: AdvisorySessionRegistrationRequest,
        ) -> Result<AdvisorySessionRegistrationResponse, AtmError> {
            Ok(AdvisorySessionRegistrationResponse {
                team: request.team,
                agent: request.agent,
                session_id: request.session_id,
                registered_at: IsoTimestamp::now(),
                queue_capacity: 64,
            })
        }

        fn unregister_session(
            &self,
            request: AdvisorySessionUnregistrationRequest,
        ) -> Result<AdvisorySessionUnregistrationResponse, AtmError> {
            self.unregister_calls
                .lock()
                .expect("unregister calls")
                .push(request.session_id.clone());
            Ok(AdvisorySessionUnregistrationResponse {
                session_id: request.session_id,
                closed: true,
            })
        }

        fn fetch_nudges(
            &self,
            _request: AdvisoryFetchRequest,
        ) -> Result<AdvisoryFetchResponse, AtmError> {
            panic!("unexpected fetch_nudges call in activation cleanup test")
        }

        fn drain_nudges(
            &self,
            _request: AdvisoryDrainRequest,
        ) -> Result<AdvisoryDrainResponse, AtmError> {
            panic!("unexpected drain_nudges call in activation cleanup test")
        }
    }

    impl GraftSessionClient for ActivationFailureClient {
        fn supports_live_advisory_stream(&self) -> bool {
            true
        }

        fn open_advisory_stream(
            &self,
            _request: AdvisoryStreamRequest,
        ) -> Result<ActiveAdvisoryStream, AtmError> {
            Err(AtmError::daemon_unavailable(
                "simulated activation advisory-stream failure",
            ))
        }
    }

    #[test]
    fn session_activation_cleans_up_registered_slot_when_receive_loop_start_fails() {
        let root = TempDir::new().expect("tempdir");
        let client = Arc::new(ActivationFailureClient::default());

        let error = GraftSession::activate_with_graft_config(
            Arc::clone(&client) as Arc<dyn GraftSessionClient>,
            Some(GraftConfig { enabled: true }),
            GraftSessionOptions::for_current_process(
                root.path(),
                TEST_TEAM.parse().expect("team"),
                TEST_LEAD.parse().expect("agent"),
            ),
            Arc::new(CollectingInjector::default()),
            Arc::new(NoopGraftObservability),
        )
        .expect_err("activation should fail when advisory stream startup fails");

        assert_eq!(
            error.message,
            "simulated activation advisory-stream failure"
        );
        assert_eq!(
            client
                .unregister_calls
                .lock()
                .expect("unregister calls")
                .len(),
            1
        );
    }
}
