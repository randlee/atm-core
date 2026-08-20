//! Tokio HTTP runtime composition contract for ATM.
//!
//! This crate owns the replacement Tokio/Axum listener and canonical typed
//! message route. It validates runtime-owned configuration before binding and
//! keeps lifecycle ownership with the Tokio task that serves that route. See
//! the [Phase AL/AM runtime boundary checklist](../../../docs/plans/phase-al-am-runtime-boundary-checklist.md).
//!
//! The only ATM dependency is `atm-core`, specifically its existing sealed
//! canonical [`atm_core::AtmError`] and the existing core storage and hook
//! contracts supplied by replacement composition.
//! Runtime construction never accepts a storage backend, tmux, graft, CLI, or
//! daemon-bootstrap type.

//! The state-owning handle deliberately has consuming transitions.  This is
//! part of the public contract, not merely an implementation detail:
//!
//! ```compile_fail
//! use atm_http_runtime::{Configured, HttpRuntime};
//!
//! async fn cannot_start_twice(runtime: HttpRuntime<Configured>) {
//!     let running = runtime.start().await.expect("first transition");
//!     let _ = runtime.start().await; // use after move: does not compile
//!     let _ = running;
//! }
//! ```
//!
//! ```compile_fail
//! use atm_http_runtime::{Configured, HttpRuntime};
//!
//! fn cannot_shutdown_before_start(runtime: HttpRuntime<Configured>) {
//!     let _ = runtime.begin_shutdown(); // method is not available yet
//! }
//! ```

use std::future::Future;
use std::net::SocketAddr;
use std::num::{NonZeroU32, NonZeroUsize};
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use atm_core::error::AtmError;
use atm_core::local_http::LocalCapability;
use tokio::net::TcpListener;
#[cfg(unix)]
use tokio::net::UnixListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;

mod client;
mod http1_server;
mod loopback_tcp;
mod message_handler;
mod peer_stream;
mod private_staging;
mod runtime_health;
mod storage_and_nudge_router;
#[cfg(unix)]
mod unix_socket;

#[cfg(unix)]
use http1_server::serve_unix_http1;
use http1_server::{serve_authenticated_peer_http1, serve_loopback_http1};
use loopback_tcp::{
    LoopbackEndpointRecordGuard, authenticated_loopback_router, cleanup_loopback_endpoint_record,
    publish_loopback_endpoint_record, validate_loopback_config,
};
#[cfg(unix)]
use unix_socket::{
    UnixSocketPathGuard, UnixSocketStartupLock, bind_unix_listener, reclaim_stale_unix_socket,
};

/// An aborted Tokio task should stop at its next cancellation point. Keep this
/// grace deliberately short and fixed so a pathological task cannot extend the
/// configured shutdown deadline without bound.
const ABORT_JOIN_GRACE: Duration = Duration::from_millis(100);

#[cfg(unix)]
pub use client::unix_socket_client;
pub use client::{
    DIRECT_PEER_TCP_PORT, SAME_HOST_REQUEST_DEADLINE, direct_peer_port, direct_peer_tcp_client,
    loopback_tcp_client, preferred_local_client, selected_write_transport,
};
pub use loopback_tcp::LoopbackTcpConfig;
pub use message_handler::{
    AuthenticatedConnector, CanonicalWriteHandler, canonical_api_router, canonical_message_router,
};
pub use peer_stream::{
    AcceptedPeerStream, AuthenticatedPeerStream, EstablishedPeerStream, PeerStreamAdapter,
    PeerStreamFuture,
};
pub use runtime_health::RuntimeHealth;
pub use storage_and_nudge_router::StorageAndNudgeRouter;

/// Validated configuration for the maintained Tokio HTTP runtime.
///
/// The fields remain private so composition cannot bypass validation before a
/// listener is introduced in a later AL sprint.
#[derive(Clone)]
pub struct HttpRuntimeConfig {
    loopback_tcp: LoopbackTcpConfig,
    unix_socket: Option<UnixSocketConfig>,
    direct_peer_tcp: Option<DirectPeerTcpConfig>,
    peer_stream_adapter: Option<Arc<dyn PeerStreamAdapter>>,
    limits: RuntimeLimits,
    timeouts: RuntimeTimeouts,
}

impl std::fmt::Debug for HttpRuntimeConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HttpRuntimeConfig")
            .field("loopback_tcp", &self.loopback_tcp)
            .field("unix_socket", &self.unix_socket)
            .field("direct_peer_tcp", &self.direct_peer_tcp)
            .field(
                "peer_stream_adapter",
                &self.peer_stream_adapter.as_ref().map(|_| "configured"),
            )
            .field("limits", &self.limits)
            .field("timeouts", &self.timeouts)
            .finish()
    }
}

impl HttpRuntimeConfig {
    /// Creates runtime configuration with one capability-authenticated
    /// loopback TCP bind and an optional additive Unix-domain socket bind.
    #[must_use]
    pub fn new(
        loopback_tcp: LoopbackTcpConfig,
        unix_socket: Option<UnixSocketConfig>,
        limits: RuntimeLimits,
        timeouts: RuntimeTimeouts,
    ) -> Self {
        Self {
            loopback_tcp,
            unix_socket,
            direct_peer_tcp: None,
            peer_stream_adapter: None,
            limits,
            timeouts,
        }
    }

    /// Enables the plain-TCP peer adapter.
    ///
    /// The production daemon uses [`DirectPeerTcpConfig::standard`].  It has
    /// no operator-provided local address or peer identity: the listener owns
    /// its fixed protocol port and the accepted socket supplies peer
    /// provenance before the request reaches the canonical router.
    #[must_use]
    pub fn with_direct_peer_tcp(mut self, direct_peer_tcp: DirectPeerTcpConfig) -> Self {
        self.direct_peer_tcp = Some(direct_peer_tcp);
        self
    }

    /// Composes an already-selected authenticated stream adapter around the
    /// direct peer listener. The runtime receives only opaque streams; TLS
    /// construction and peer configuration remain bootstrap-owned.
    #[must_use]
    pub fn with_peer_stream_adapter(mut self, adapter: Arc<dyn PeerStreamAdapter>) -> Self {
        self.peer_stream_adapter = Some(adapter);
        self
    }
}

/// Validated plain-TCP peer listener configuration for the non-TLS MVP.
///
/// The production constructor fixes the protocol port.  It intentionally has
/// no local address or peer-host field: Tokio binds every active IPv4
/// interface, and accepted connections supply their own source address to the
/// peer adapter.
#[derive(Debug, Clone)]
pub struct DirectPeerTcpConfig {
    port: u16,
    allow_ephemeral_test_port: bool,
}

impl DirectPeerTcpConfig {
    #[must_use]
    pub fn standard() -> Self {
        Self::new(DIRECT_PEER_TCP_PORT)
    }

    /// Crate-private port selection keeps isolated runtime tests possible.
    /// Production composition can construct only [`Self::standard`].
    #[must_use]
    pub(crate) fn new(port: u16) -> Self {
        Self {
            port,
            allow_ephemeral_test_port: false,
        }
    }

    /// Test-only isolated listener selection without a probe/rebind race.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn ephemeral_for_test() -> Self {
        Self {
            port: 0,
            allow_ephemeral_test_port: true,
        }
    }

    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }
}

/// Unix-domain socket preflight input.
///
/// The data type remains available on every target so shared configuration can
/// be decoded consistently, but a configured Unix socket is accepted only on
/// Unix. AL.1 never binds it; later Unix adapter work owns binding, ownership,
/// and permission application.
#[derive(Debug, Clone)]
pub struct UnixSocketConfig {
    #[cfg_attr(
        not(unix),
        expect(
            dead_code,
            reason = "AL.5 owns Unix socket ownership application; AL.1 retains its validated configuration input"
        )
    )]
    path: PathBuf,
    #[cfg_attr(
        not(unix),
        expect(
            dead_code,
            reason = "AL.5 owns Unix socket ownership application; AL.1 retains its validated configuration input"
        )
    )]
    owner_uid: UnixSocketOwnerUid,
    #[cfg_attr(
        not(unix),
        expect(
            dead_code,
            reason = "AL.5 owns Unix socket ownership application; AL.1 retains its validated configuration input"
        )
    )]
    mode: UnixSocketMode,
}

impl UnixSocketConfig {
    #[must_use]
    pub fn new(path: PathBuf, owner_uid: UnixSocketOwnerUid, mode: UnixSocketMode) -> Self {
        Self {
            path,
            owner_uid,
            mode,
        }
    }
}

/// Validated Unix socket owner identity, kept distinct from its mode so
/// composition cannot accidentally swap two numeric configuration values.
#[cfg_attr(
    not(unix),
    expect(
        dead_code,
        reason = "AL.5 retains Unix socket configuration for cross-platform decoding; ownership application is Unix-only"
    )
)]
#[derive(Debug, Clone, Copy)]
pub struct UnixSocketOwnerUid(NonZeroU32);

impl UnixSocketOwnerUid {
    #[must_use]
    pub const fn new(value: NonZeroU32) -> Self {
        Self(value)
    }

    #[must_use]
    #[cfg(unix)]
    const fn get(self) -> u32 {
        self.0.get()
    }
}

/// Configured Unix socket permission bits, distinct from the owner identity.
#[cfg_attr(
    not(unix),
    expect(
        dead_code,
        reason = "AL.5 retains Unix socket configuration for cross-platform decoding; permission application is Unix-only"
    )
)]
#[derive(Debug, Clone, Copy)]
pub struct UnixSocketMode(NonZeroU32);

impl UnixSocketMode {
    #[must_use]
    pub const fn new(value: NonZeroU32) -> Self {
        Self(value)
    }

    #[must_use]
    #[cfg(unix)]
    const fn get(self) -> u32 {
        self.0.get()
    }
}

/// Bounded HTTP admission settings.
#[derive(Debug, Clone, Copy)]
pub struct RuntimeLimits {
    max_body_bytes: usize,
    max_connections: usize,
}

impl RuntimeLimits {
    #[must_use]
    pub const fn new(max_body_bytes: NonZeroUsize, max_connections: NonZeroUsize) -> Self {
        Self {
            max_body_bytes: max_body_bytes.get(),
            max_connections: max_connections.get(),
        }
    }
}

/// A non-zero duration required by runtime timeout configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NonZeroDuration(Duration);

impl NonZeroDuration {
    #[must_use]
    pub const fn new(value: Duration) -> Option<Self> {
        if value.is_zero() {
            None
        } else {
            Some(Self(value))
        }
    }

    #[must_use]
    pub const fn get(self) -> Duration {
        self.0
    }
}

/// Absolute limits used by the future framework adapter.
#[derive(Debug, Clone, Copy)]
pub struct RuntimeTimeouts {
    request: Duration,
    shutdown: Duration,
}

impl RuntimeTimeouts {
    #[must_use]
    pub const fn new(request: NonZeroDuration, shutdown: NonZeroDuration) -> Self {
        Self {
            request: request.get(),
            shutdown: shutdown.get(),
        }
    }
}

/// Composition input for the replacement async write boundary.
#[derive(Clone)]
pub struct HttpRuntimeBuilder {
    config: HttpRuntimeConfig,
    handler: Arc<dyn CanonicalWriteHandler>,
    health: RuntimeHealth,
}

impl HttpRuntimeBuilder {
    #[must_use]
    pub fn new(config: HttpRuntimeConfig, handler: Arc<dyn CanonicalWriteHandler>) -> Self {
        Self {
            config,
            handler,
            health: RuntimeHealth::default(),
        }
    }

    /// Attaches the one process-owned health projection to lifecycle
    /// transitions. The caller keeps a clone for the doctor/status route.
    #[must_use]
    pub fn with_runtime_health(mut self, health: RuntimeHealth) -> Self {
        self.health = health;
        self
    }

    /// Validates all runtime-owned input without binding or publishing.
    ///
    /// # Errors
    ///
    /// Returns the existing configuration error with the invalid field and cause.
    pub fn build(self) -> Result<HttpRuntime<Configured>, AtmError> {
        if let Err(error) = validate_config(&self.config) {
            self.health.mark_not_ready(error.to_string());
            return Err(error);
        }
        Ok(HttpRuntime {
            config: self.config,
            handler: self.handler,
            health: self.health,
            state: Configured,
        })
    }
}

/// Validated but not started runtime state.
pub struct Configured;
/// Runtime lifecycle state while its owned Axum server is accepting requests.
pub struct Running {
    local_address: SocketAddr,
    direct_peer_address: Option<SocketAddr>,
    shutdown_tx: watch::Sender<()>,
    server_stopped_rx: watch::Receiver<bool>,
    server_task: JoinHandle<std::io::Result<()>>,
    endpoint_record: LoopbackEndpointRecordGuard,
}
/// Runtime lifecycle state after cancellation and while the Axum task drains.
pub struct Draining {
    server_task: JoinHandle<std::io::Result<()>>,
    endpoint_record: LoopbackEndpointRecordGuard,
}
/// Terminal lifecycle state with no live runtime-owned handles.
pub struct Stopped;

/// Non-cloneable lifecycle owner. State transitions consume this value.
pub struct HttpRuntime<State> {
    config: HttpRuntimeConfig,
    handler: Arc<dyn CanonicalWriteHandler>,
    health: RuntimeHealth,
    state: State,
}

impl HttpRuntime<Configured> {
    /// Binds the replacement listener(s) and starts their one owned Axum task.
    ///
    /// The caller supplies the Tokio runtime. This method never creates a
    /// nested runtime and all request handling runs through the one typed
    /// route built from the injected application boundary.
    ///
    /// # Errors
    ///
    /// Returns `AtmError` when the configured loopback address cannot be bound,
    /// endpoint publication fails, or its local address cannot be read. A
    /// configured Unix socket is bound additively and uses the same router as
    /// the authenticated loopback listener.
    pub async fn start(self) -> Result<HttpRuntime<Running>, AtmError> {
        let (listener, local_address) = bind_loopback_listener(&self.config, &self.health).await?;
        // Every enabled listener must be bound before publishing the loopback
        // endpoint record.  Otherwise a client could observe a Ready-looking
        // record while the additive UDS adapter still fails to start.
        let direct_peer_listener =
            bind_configured_direct_peer_listener(&self.config, &self.health).await?;
        let direct_peer_address = direct_peer_listener
            .as_ref()
            .and_then(|listener| listener.local_addr().ok());
        #[cfg(unix)]
        let unix_listener = bind_configured_unix_listener(&self.config, &self.health).await?;
        let (capability, endpoint_record) =
            publish_loopback_endpoint(&self.config, local_address, &self.health).await?;
        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let (server_stopped_tx, server_stopped_rx) = watch::channel(false);
        let canonical_router = canonical_api_router(
            Arc::clone(&self.handler),
            AuthenticatedConnector::local(),
            self.config.limits,
            self.config.timeouts,
        );
        let loopback_router = authenticated_loopback_router(canonical_router.clone(), capability);
        let direct_peer = build_direct_peer_server(
            direct_peer_listener,
            &self.config,
            Arc::clone(&self.handler),
        );
        let server_task = match start_server_task(ServerTaskInputs {
            listener,
            loopback_router,
            direct_peer,
            #[cfg(unix)]
            canonical_router,
            #[cfg(unix)]
            unix_listener,
            max_connections: self.config.limits.max_connections,
            header_read_timeout: self.config.timeouts.request,
            shutdown_tx: shutdown_tx.clone(),
            shutdown_rx,
            server_stopped_tx,
            health: self.health.clone(),
        })
        .await
        {
            Ok(server_task) => server_task,
            Err(error) => {
                let _ = cleanup_loopback_endpoint_record(endpoint_record).await;
                self.health.mark_not_ready(error.to_string());
                return Err(error);
            }
        };
        self.health.mark_ready();
        Ok(HttpRuntime {
            config: self.config,
            handler: self.handler,
            health: self.health,
            state: Running {
                local_address,
                direct_peer_address,
                shutdown_tx,
                server_stopped_rx,
                server_task,
                endpoint_record,
            },
        })
    }
}

fn build_direct_peer_server(
    listener: Option<TcpListener>,
    config: &HttpRuntimeConfig,
    handler: Arc<dyn CanonicalWriteHandler>,
) -> Option<DirectPeerServer> {
    let listener = listener?;
    Some(match config.peer_stream_adapter.as_ref() {
        Some(adapter) => DirectPeerServer::Authenticated(
            listener,
            Arc::clone(adapter),
            handler,
            config.limits,
            config.timeouts,
        ),
        None => DirectPeerServer::Plaintext(
            listener,
            canonical_api_router(
                handler,
                AuthenticatedConnector::peer_socket(),
                config.limits,
                config.timeouts,
            ),
        ),
    })
}

async fn bind_configured_direct_peer_listener(
    config: &HttpRuntimeConfig,
    _health: &RuntimeHealth,
) -> Result<Option<TcpListener>, AtmError> {
    let Some(peer) = config.direct_peer_tcp.as_ref() else {
        return Ok(None);
    };
    let bind_address = SocketAddr::from(([0, 0, 0, 0], peer.port()));
    match TcpListener::bind(bind_address).await {
        Ok(listener) => Ok(Some(listener)),
        Err(error) => {
            // Local IPC remains usable when the optional cross-host listener
            // cannot claim its fixed port.  Do not let a port collision or an
            // interface transition take down the daemon; a cross-host smoke
            // will surface this listener as unavailable.
            tracing::warn!(
                %bind_address,
                error = %error,
                "replacement direct peer listener is unavailable; continuing with local listeners"
            );
            Ok(None)
        }
    }
}

async fn bind_loopback_listener(
    config: &HttpRuntimeConfig,
    health: &RuntimeHealth,
) -> Result<(TcpListener, SocketAddr), AtmError> {
    let listener = TcpListener::bind(config.loopback_tcp.bind_address)
        .await
        .map_err(|source| {
            let error = AtmError::daemon_unavailable(format!(
                "failed to bind replacement HTTP runtime at {}",
                config.loopback_tcp.bind_address
            ))
            .with_cause(source);
            health.mark_not_ready(error.to_string());
            error
        })?;
    let local_address = listener.local_addr().map_err(|source| {
        let error = AtmError::daemon_unavailable("failed to read replacement HTTP runtime address")
            .with_cause(source);
        health.mark_not_ready(error.to_string());
        error
    })?;
    if !local_address.ip().is_loopback() {
        let error = AtmError::local_http_endpoint_non_loopback(
            "replacement HTTP runtime bound a non-loopback TCP address",
        );
        health.mark_not_ready(error.to_string());
        return Err(error);
    }
    Ok((listener, local_address))
}

#[cfg(unix)]
async fn bind_configured_unix_listener(
    config: &HttpRuntimeConfig,
    health: &RuntimeHealth,
) -> Result<Option<(UnixListener, UnixSocketPathGuard)>, AtmError> {
    let Some(socket) = config.unix_socket.clone() else {
        return Ok(None);
    };
    let lock_socket = socket.clone();
    let startup_lock =
        match tokio::task::spawn_blocking(move || UnixSocketStartupLock::acquire(&lock_socket))
            .await
        {
            Ok(Ok(lock)) => lock,
            Ok(Err(error)) => {
                health.mark_not_ready(error.to_string());
                return Err(error);
            }
            Err(source) => {
                let error = AtmError::daemon_unavailable(
                    "replacement Unix HTTP socket lock task ended unexpectedly",
                )
                .with_cause(source);
                health.mark_not_ready(error.to_string());
                return Err(error);
            }
        };
    if let Err(error) = reclaim_stale_unix_socket(&socket).await {
        health.mark_not_ready(error.to_string());
        return Err(error);
    }
    let result = match tokio::task::spawn_blocking(move || bind_unix_listener(&socket)).await {
        Ok(Ok(listener)) => Ok(Some(listener)),
        Ok(Err(error)) => {
            health.mark_not_ready(error.to_string());
            Err(error)
        }
        Err(source) => {
            let error = AtmError::daemon_unavailable(
                "replacement Unix HTTP socket setup task ended unexpectedly",
            )
            .with_cause(source);
            health.mark_not_ready(error.to_string());
            Err(error)
        }
    };
    drop(startup_lock);
    result
}

async fn publish_loopback_endpoint(
    config: &HttpRuntimeConfig,
    local_address: SocketAddr,
    health: &RuntimeHealth,
) -> Result<(LocalCapability, LoopbackEndpointRecordGuard), AtmError> {
    let capability = LocalCapability::generate()
        .inspect_err(|error| health.mark_not_ready(error.to_string()))?;
    let record_config = config.loopback_tcp.clone();
    let record_capability = capability.clone();
    let publication = tokio::task::spawn_blocking(move || {
        publish_loopback_endpoint_record(&record_config, local_address, &record_capability)
    })
    .await
    .map_err(|source| {
        let error = AtmError::daemon_unavailable(
            "replacement loopback endpoint publication task ended unexpectedly",
        )
        .with_cause(source);
        health.mark_not_ready(error.to_string());
        error
    })?;
    let endpoint_record =
        publication.inspect_err(|error| health.mark_not_ready(error.to_string()))?;
    Ok((capability, endpoint_record))
}

struct ServerTaskInputs {
    listener: TcpListener,
    loopback_router: axum::Router,
    direct_peer: Option<DirectPeerServer>,
    #[cfg(unix)]
    canonical_router: axum::Router,
    #[cfg(unix)]
    unix_listener: Option<(UnixListener, UnixSocketPathGuard)>,
    max_connections: usize,
    header_read_timeout: Duration,
    shutdown_tx: watch::Sender<()>,
    shutdown_rx: watch::Receiver<()>,
    server_stopped_tx: watch::Sender<bool>,
    health: RuntimeHealth,
}

enum DirectPeerServer {
    Plaintext(TcpListener, axum::Router),
    Authenticated(
        TcpListener,
        Arc<dyn PeerStreamAdapter>,
        Arc<dyn CanonicalWriteHandler>,
        RuntimeLimits,
        RuntimeTimeouts,
    ),
}

/// Marks the process-owned status projection when the one managed server task
/// leaves supervision, including task cancellation. The bootstrap observes the
/// paired watch receiver and exits through the normal endpoint-cleanup path.
struct ServerTaskTerminationGuard {
    health: RuntimeHealth,
    server_stopped_tx: watch::Sender<bool>,
}

impl Drop for ServerTaskTerminationGuard {
    fn drop(&mut self) {
        self.health
            .mark_not_ready("replacement HTTP runtime server task stopped");
        let _ = self.server_stopped_tx.send(true);
    }
}

fn spawn_supervised_server<F>(
    health: RuntimeHealth,
    server_stopped_tx: watch::Sender<bool>,
    server: F,
) -> JoinHandle<std::io::Result<()>>
where
    F: Future<Output = std::io::Result<()>> + Send + 'static,
{
    // Construct the guard before spawning: an abort that wins before Tokio
    // first polls the task still drops this captured value and revokes Ready.
    let termination = ServerTaskTerminationGuard {
        health,
        server_stopped_tx,
    };
    tokio::spawn(async move {
        let _termination = termination;
        server.await
    })
}

async fn start_server_task(
    inputs: ServerTaskInputs,
) -> Result<JoinHandle<std::io::Result<()>>, AtmError> {
    let ServerTaskInputs {
        listener,
        loopback_router,
        direct_peer,
        #[cfg(unix)]
        canonical_router,
        #[cfg(unix)]
        unix_listener,
        max_connections,
        header_read_timeout,
        shutdown_tx,
        shutdown_rx,
        server_stopped_tx,
        health,
    } = inputs;
    Ok(spawn_supervised_server(
        health,
        server_stopped_tx,
        async move {
            drain_server_group(ServerGroupInputs {
                loopback: (listener, loopback_router),
                direct_peer,
                #[cfg(unix)]
                unix_socket: unix_listener
                    .map(|(listener, cleanup)| (listener, cleanup, canonical_router)),
                max_connections,
                header_read_timeout,
                shutdown_rx,
                shutdown_tx,
            })
            .await
        },
    ))
}

struct ServerGroupInputs {
    loopback: (TcpListener, axum::Router),
    direct_peer: Option<DirectPeerServer>,
    #[cfg(unix)]
    unix_socket: Option<(UnixListener, UnixSocketPathGuard, axum::Router)>,
    max_connections: usize,
    header_read_timeout: Duration,
    shutdown_rx: watch::Receiver<()>,
    shutdown_tx: watch::Sender<()>,
}

/// Supervises every enabled physical adapter under the one runtime-owned
/// Tokio task.  Each adapter is only a listener plus its connector-specific
/// router; the application route and lifecycle are never duplicated.
async fn drain_server_group(inputs: ServerGroupInputs) -> std::io::Result<()> {
    let ServerGroupInputs {
        loopback: (loopback_listener, loopback_router),
        direct_peer,
        #[cfg(unix)]
        unix_socket,
        max_connections,
        header_read_timeout,
        shutdown_rx,
        shutdown_tx,
    } = inputs;
    let mut servers = tokio::task::JoinSet::new();
    servers.spawn(serve_loopback_http1(
        loopback_listener,
        loopback_router,
        max_connections,
        header_read_timeout,
        shutdown_rx.clone(),
    ));
    if let Some(direct_peer) = direct_peer {
        match direct_peer {
            DirectPeerServer::Plaintext(listener, router) => {
                servers.spawn(serve_loopback_http1(
                    listener,
                    router,
                    max_connections,
                    header_read_timeout,
                    shutdown_rx.clone(),
                ));
            }
            DirectPeerServer::Authenticated(listener, adapter, handler, limits, timeouts) => {
                servers.spawn(serve_authenticated_peer_http1(
                    listener,
                    adapter,
                    handler,
                    limits,
                    timeouts,
                    shutdown_rx.clone(),
                ));
            }
        }
    }
    #[cfg(unix)]
    if let Some((listener, cleanup, router)) = unix_socket {
        servers.spawn(async move {
            // This guard is tied to the inode bound by this runtime and cannot
            // remove a replacement endpoint.
            let _cleanup = cleanup;
            serve_unix_http1(
                listener,
                router,
                max_connections,
                header_read_timeout,
                shutdown_rx,
            )
            .await
        });
    }

    let first = match servers.join_next().await {
        Some(Ok(result)) => result,
        Some(Err(error)) => Err(std::io::Error::other(format!(
            "replacement HTTP listener task panicked: {error}"
        ))),
        None => Ok(()),
    };
    let _ = shutdown_tx.send(());
    while let Some(result) = servers.join_next().await {
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) if first.is_ok() => return Err(error),
            Ok(Err(_)) => {}
            Err(error) if first.is_ok() => {
                return Err(std::io::Error::other(format!(
                    "replacement HTTP listener task panicked: {error}"
                )));
            }
            Err(_) => {}
        }
    }
    first
}

impl HttpRuntime<Running> {
    /// Returns the actual listener address selected at start.
    #[must_use]
    pub const fn local_address(&self) -> SocketAddr {
        self.state.local_address
    }

    /// Returns the direct-peer listener address when that optional adapter bound.
    #[must_use]
    pub const fn direct_peer_address(&self) -> Option<SocketAddr> {
        self.state.direct_peer_address
    }

    /// Waits until the one framework-managed server task has stopped.
    ///
    /// The running owner remains usable for the normal consuming shutdown
    /// transition afterwards, which performs endpoint-record cleanup and joins
    /// the already-completed task. This is used by process composition to
    /// avoid advertising a live daemon after its only server exits.
    pub async fn wait_for_server_stop(&mut self) {
        if *self.state.server_stopped_rx.borrow() {
            return;
        }
        let _ = self.state.server_stopped_rx.changed().await;
    }

    /// Consumes the only running owner and begins the drain transition.
    #[must_use]
    pub fn begin_shutdown(self) -> HttpRuntime<Draining> {
        let _ = self.state.shutdown_tx.send(());
        self.health.begin_drain();
        HttpRuntime {
            config: self.config,
            handler: self.handler,
            health: self.health,
            state: Draining {
                server_task: self.state.server_task,
                endpoint_record: self.state.endpoint_record,
            },
        }
    }
}

impl HttpRuntime<Draining> {
    /// Completes the drain transition.
    ///
    /// The runtime waits only for its actual Axum task. A shutdown deadline
    /// aborts that task and gives cancellation one short, bounded join grace
    /// before endpoint cleanup proceeds.
    ///
    /// # Errors
    ///
    /// Returns `AtmError` when the server fails while draining or exceeds the
    /// configured shutdown bound.
    pub async fn finish(self) -> Result<HttpRuntime<Stopped>, AtmError> {
        let Draining {
            mut server_task,
            endpoint_record,
        } = self.state;
        let finished = tokio::time::timeout(self.config.timeouts.shutdown, &mut server_task).await;
        let server_result = match finished {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(source))) => Err(AtmError::daemon_unavailable(
                "replacement HTTP runtime stopped with an I/O error",
            )
            .with_cause(source)),
            Ok(Err(source)) => Err(AtmError::daemon_unavailable(
                "replacement HTTP runtime task ended unexpectedly",
            )
            .with_cause(source)),
            Err(_) => {
                server_task.abort();
                if tokio::time::timeout(ABORT_JOIN_GRACE, &mut server_task)
                    .await
                    .is_err()
                {
                    tracing::warn!(
                        abort_join_grace_ms = ABORT_JOIN_GRACE.as_millis(),
                        "replacement HTTP runtime task exceeded the bounded abort-join grace"
                    );
                }
                Err(AtmError::daemon_unavailable(
                    "replacement HTTP runtime exceeded its shutdown deadline",
                ))
            }
        };
        let cleanup_result = cleanup_loopback_endpoint_record(endpoint_record).await;
        let result = server_result.and(cleanup_result);
        self.health.mark_stopped();
        result?;
        Ok(HttpRuntime {
            config: self.config,
            handler: self.handler,
            health: self.health,
            state: Stopped,
        })
    }
}

fn validate_config(config: &HttpRuntimeConfig) -> Result<(), AtmError> {
    debug_assert!(config.limits.max_body_bytes > 0);
    debug_assert!(config.limits.max_connections > 0);
    debug_assert!(!config.timeouts.request.is_zero());
    debug_assert!(!config.timeouts.shutdown.is_zero());
    validate_loopback_config(&config.loopback_tcp)?;
    if let Some(peer) = &config.direct_peer_tcp
        && peer.port() == 0
        && !peer.allow_ephemeral_test_port
    {
        return Err(preflight(
            "direct_peer_tcp.port",
            "must use a non-zero port",
        ));
    }
    if config.peer_stream_adapter.is_some() && config.direct_peer_tcp.is_none() {
        return Err(preflight(
            "peer_stream_adapter",
            "requires an enabled direct peer listener",
        ));
    }
    #[cfg(not(unix))]
    if config.unix_socket.is_some() {
        return Err(preflight(
            "unix_socket",
            "Unix-domain socket configuration is unsupported on this platform",
        ));
    }
    #[cfg(unix)]
    if let Some(socket) = &config.unix_socket {
        validate_unix_socket_path(&socket.path)?;
        if socket.mode.get() & !0o777 != 0 {
            return Err(preflight(
                "unix_socket.mode",
                "must contain only permission bits",
            ));
        }
        if socket.mode.get() & 0o077 != 0 {
            return Err(preflight(
                "unix_socket.mode",
                "must grant access only to the configured owner",
            ));
        }
        if socket.mode.get() & 0o200 == 0 {
            return Err(preflight(
                "unix_socket.mode",
                "must grant the configured owner write permission",
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn validate_unix_socket_path(path: &Path) -> Result<(), AtmError> {
    if path.as_os_str().is_empty() {
        return Err(preflight("unix_socket.path", "must not be empty"));
    }
    Ok(())
}

fn preflight(field: &str, cause: impl std::fmt::Display) -> AtmError {
    AtmError::config(format!("invalid runtime configuration field `{field}`")).with_cause(cause)
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::num::{NonZeroU32, NonZeroUsize};
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use atm_core::api::ApiRequest;
    use atm_core::api::{ApiResponse, AuthenticatedIngress, RequestDeadline};
    use atm_core::error::AtmError;
    use atm_core::home::HOST_RUNTIME_OWNER_LOCK_FILE;
    use atm_core::local_http::{LOCAL_CAPABILITY_HEADER, LocalCapability, LocalHttpEndpointRecord};
    use atm_core::protocol::{RequestEnvelope, ResponseEnvelope, RuntimeReadinessState};
    use atm_core::send::{SendMessageSource, SendRequest};
    use atm_core::test_support::{TEST_RECIPIENT, TEST_SENDER, TEST_TEAM};
    use atm_core::types::{AgentName, TeamName};
    use tokio::net::{TcpListener, TcpStream};

    use super::{
        AcceptedPeerStream, AuthenticatedPeerStream, CanonicalWriteHandler, DirectPeerTcpConfig,
        HttpRuntimeBuilder, HttpRuntimeConfig, LoopbackTcpConfig, NonZeroDuration,
        PeerStreamAdapter, RuntimeHealth, RuntimeLimits, RuntimeTimeouts, UnixSocketConfig,
        UnixSocketMode, UnixSocketOwnerUid, direct_peer_tcp_client,
    };
    use ulid::Ulid;

    struct TestRouter;

    /// Test-only adapter proving the runtime treats an already-authenticated
    /// stream as opaque. Real certificate and pin verification remains tested
    /// in `peer-tls`; this double deliberately contains no security policy.
    struct PassthroughPeerAdapter;

    impl PeerStreamAdapter for PassthroughPeerAdapter {
        fn connect<'a>(
            &'a self,
            stream: TcpStream,
            _peer: &'a atm_core::types::HostName,
        ) -> Pin<
            Box<
                dyn Future<Output = Result<Box<dyn AuthenticatedPeerStream>, AtmError>> + Send + 'a,
            >,
        > {
            Box::pin(async move {
                let stream: Box<dyn AuthenticatedPeerStream> = Box::new(stream);
                Ok(stream)
            })
        }

        fn accept<'a>(
            &'a self,
            stream: TcpStream,
        ) -> Pin<Box<dyn Future<Output = Result<AcceptedPeerStream, AtmError>> + Send + 'a>>
        {
            Box::pin(async move {
                Ok(AcceptedPeerStream {
                    source_host: "trusted.example.test".parse().expect("test host"),
                    stream: Box::new(stream),
                })
            })
        }
    }

    #[derive(Default)]
    struct RecordingPeerRouter {
        calls: Mutex<Vec<(atm_core::send::WriteRequest, AuthenticatedIngress)>>,
    }

    struct CountingLoopbackRouter {
        calls: AtomicUsize,
        entered: AtomicBool,
        entered_notify: tokio::sync::Notify,
        release: tokio::sync::Notify,
    }

    impl CountingLoopbackRouter {
        fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
                entered: AtomicBool::new(false),
                entered_notify: tokio::sync::Notify::new(),
                release: tokio::sync::Notify::new(),
            }
        }

        async fn wait_until_entered(&self) {
            while !self.entered.load(Ordering::SeqCst) {
                self.entered_notify.notified().await;
            }
        }
    }

    impl atm_core::boundary::sealed::Sealed for CountingLoopbackRouter {}

    impl CanonicalWriteHandler for CountingLoopbackRouter {
        fn write(
            &self,
            _request: atm_core::send::WriteRequest,
            _ingress: AuthenticatedIngress,
            _deadline: RequestDeadline,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<ApiResponse, AtmError>> + Send + '_>,
        > {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.entered.store(true, Ordering::SeqCst);
                self.entered_notify.notify_waiters();
                self.release.notified().await;
                Ok(ApiResponse::new(ResponseEnvelope::Error(
                    AtmError::validation("loopback test handler reached"),
                )))
            })
        }
    }

    impl atm_core::boundary::sealed::Sealed for TestRouter {}

    impl atm_core::boundary::sealed::Sealed for RecordingPeerRouter {}

    impl CanonicalWriteHandler for RecordingPeerRouter {
        fn write(
            &self,
            request: atm_core::send::WriteRequest,
            ingress: AuthenticatedIngress,
            _deadline: RequestDeadline,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<ApiResponse, AtmError>> + Send + '_>,
        > {
            self.calls
                .lock()
                .expect("recorded peer calls")
                .push((request, ingress));
            Box::pin(async {
                Err(AtmError::validation(
                    "peer provenance fixture deliberately returns a typed error",
                ))
            })
        }
    }

    impl CanonicalWriteHandler for TestRouter {
        fn write(
            &self,
            _request: atm_core::send::WriteRequest,
            _ingress: AuthenticatedIngress,
            _deadline: RequestDeadline,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<ApiResponse, AtmError>> + Send + '_>,
        > {
            Box::pin(async {
                Err(AtmError::validation(
                    "test handler is not invoked by the lifecycle contract",
                ))
            })
        }
    }

    struct CanonicalUdsRouter;

    impl atm_core::boundary::sealed::Sealed for CanonicalUdsRouter {}

    impl CanonicalWriteHandler for CanonicalUdsRouter {
        fn write(
            &self,
            _request: atm_core::send::WriteRequest,
            _ingress: AuthenticatedIngress,
            _deadline: RequestDeadline,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<ApiResponse, AtmError>> + Send + '_>,
        > {
            Box::pin(async {
                Ok(ApiResponse::new(ResponseEnvelope::Error(
                    AtmError::validation("canonical UDS test handler reached"),
                )))
            })
        }
    }

    #[cfg(unix)]
    struct BlockingUdsRouter {
        entered: AtomicBool,
        entered_notify: tokio::sync::Notify,
        release: tokio::sync::Notify,
    }

    #[cfg(unix)]
    impl BlockingUdsRouter {
        fn new() -> Self {
            Self {
                entered: AtomicBool::new(false),
                entered_notify: tokio::sync::Notify::new(),
                release: tokio::sync::Notify::new(),
            }
        }

        async fn wait_until_entered(&self) {
            while !self.entered.load(Ordering::SeqCst) {
                self.entered_notify.notified().await;
            }
        }
    }

    #[cfg(unix)]
    impl atm_core::boundary::sealed::Sealed for BlockingUdsRouter {}

    #[cfg(unix)]
    impl CanonicalWriteHandler for BlockingUdsRouter {
        fn write(
            &self,
            _request: atm_core::send::WriteRequest,
            _ingress: AuthenticatedIngress,
            _deadline: RequestDeadline,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<ApiResponse, AtmError>> + Send + '_>,
        > {
            Box::pin(async move {
                self.entered.store(true, Ordering::SeqCst);
                self.entered_notify.notify_waiters();
                self.release.notified().await;
                Ok(ApiResponse::new(ResponseEnvelope::Error(
                    AtmError::validation("UDS drain fixture released"),
                )))
            })
        }
    }

    fn write_request() -> RequestEnvelope {
        RequestEnvelope::Write(Box::new(
            SendRequest::new(
                ".".into(),
                ".".into(),
                AgentName::from_validated(TEST_SENDER),
                TEST_RECIPIENT,
                TeamName::from_validated(TEST_TEAM),
                SendMessageSource::Inline("Unix runtime test".to_owned()),
                None,
                false,
                None,
                false,
            )
            .expect("test write request"),
        ))
    }

    #[cfg(unix)]
    fn owner_uid(path: &std::path::Path) -> NonZeroU32 {
        use std::os::unix::fs::MetadataExt;

        NonZeroU32::new(
            std::fs::metadata(path)
                .expect("test directory metadata")
                .uid(),
        )
        .expect("test process must not use uid zero")
    }

    #[cfg(unix)]
    fn uds_config(socket_path: std::path::PathBuf, owner_uid: NonZeroU32) -> HttpRuntimeConfig {
        let endpoint_record_path = socket_path.with_file_name("local-http.json");
        HttpRuntimeConfig::new(
            loopback_tcp(
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                endpoint_record_path,
            ),
            Some(UnixSocketConfig::new(
                socket_path,
                UnixSocketOwnerUid::new(owner_uid),
                UnixSocketMode::new(NonZeroU32::new(0o600).expect("owner-only socket mode")),
            )),
            limits(1024, 8),
            timeouts(Duration::from_secs(1), Duration::from_secs(1)),
        )
    }

    fn config_with_record(
        port: u16,
        endpoint_record_path: std::path::PathBuf,
    ) -> HttpRuntimeConfig {
        HttpRuntimeConfig::new(
            loopback_tcp(
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
                endpoint_record_path,
            ),
            None,
            limits(1024, 8),
            timeouts(Duration::from_secs(1), Duration::from_secs(1)),
        )
    }

    fn loopback_tcp(
        bind_address: SocketAddr,
        endpoint_record_path: std::path::PathBuf,
    ) -> LoopbackTcpConfig {
        LoopbackTcpConfig::new(bind_address, endpoint_record_path, Ulid::new())
    }

    fn loopback_tcp_with_instance(
        bind_address: SocketAddr,
        endpoint_record_path: std::path::PathBuf,
        daemon_instance_id: Ulid,
    ) -> LoopbackTcpConfig {
        LoopbackTcpConfig::new(bind_address, endpoint_record_path, daemon_instance_id)
    }

    fn write_owner_record(record_path: &std::path::Path, daemon_instance_id: Ulid) {
        let parent = record_path.parent().expect("test endpoint record parent");
        std::fs::write(
            parent.join(HOST_RUNTIME_OWNER_LOCK_FILE),
            format!("1:test-owner:{daemon_instance_id}\n"),
        )
        .expect("write test daemon owner record");
    }

    fn loopback_runtime_config(
        bind_address: SocketAddr,
        record_path: std::path::PathBuf,
        daemon_instance_id: Ulid,
        unix_socket: Option<UnixSocketConfig>,
        max_body_bytes: usize,
    ) -> HttpRuntimeConfig {
        HttpRuntimeConfig::new(
            loopback_tcp_with_instance(bind_address, record_path, daemon_instance_id),
            unix_socket,
            limits(max_body_bytes, 8),
            timeouts(Duration::from_secs(1), Duration::from_secs(1)),
        )
    }

    fn limits(max_body_bytes: usize, max_connections: usize) -> RuntimeLimits {
        RuntimeLimits::new(
            NonZeroUsize::new(max_body_bytes).expect("test limit is non-zero"),
            NonZeroUsize::new(max_connections).expect("test limit is non-zero"),
        )
    }

    fn timeouts(request: Duration, shutdown: Duration) -> RuntimeTimeouts {
        RuntimeTimeouts::new(
            NonZeroDuration::new(request).expect("test timeout is non-zero"),
            NonZeroDuration::new(shutdown).expect("test timeout is non-zero"),
        )
    }

    fn bounded_test_http_client() -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(1))
            .build()
            .expect("bounded test HTTP client")
    }

    #[test]
    fn invalid_configuration_fails_before_lifecycle_start() {
        let health = RuntimeHealth::with_owner(99);
        let error = match HttpRuntimeBuilder::new(
            config_with_record(0, std::path::PathBuf::new()),
            Arc::new(TestRouter),
        )
        .with_runtime_health(health.clone())
        .build()
        {
            Ok(_) => panic!("invalid bind configuration must fail before any listener exists"),
            Err(error) => error,
        };
        assert_eq!(error.code().as_str(), "ATM_CONFIG_PARSE_FAILED");
        assert!(error.message().contains("endpoint_record_path"));
        assert!(
            error
                .message()
                .contains("Repair the active ATM configuration and retry.")
        );
        assert!(!error.message().contains("reinstall/restart daemon"));
        assert_eq!(error.cause(), Some("must not be empty"));
        assert_eq!(
            health.snapshot().readiness,
            RuntimeReadinessState::Unavailable,
            "invalid configuration never reports Ready"
        );
    }

    #[test]
    fn direct_peer_configuration_rejects_zero_port_before_binding() {
        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let config = config_with_record(0, temporary_directory.path().join("local-http.json"))
            .with_direct_peer_tcp(DirectPeerTcpConfig::new(0));

        let error = match HttpRuntimeBuilder::new(config, Arc::new(TestRouter)).build() {
            Ok(_) => panic!("peer listener port zero must fail preflight"),
            Err(error) => error,
        };
        assert_eq!(error.code().as_str(), "ATM_CONFIG_PARSE_FAILED");
        assert!(error.message().contains("direct_peer_tcp.port"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn direct_peer_port_collision_keeps_the_local_runtime_ready() {
        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let occupied_listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0))
            .await
            .expect("reserve a direct peer port");
        let occupied_port = occupied_listener
            .local_addr()
            .expect("read reserved peer address")
            .port();
        let endpoint_record = temporary_directory.path().join("local-http.json");
        let health = RuntimeHealth::with_owner(42);
        let config = config_with_record(0, endpoint_record.clone())
            .with_direct_peer_tcp(DirectPeerTcpConfig::new(occupied_port));

        let running = HttpRuntimeBuilder::new(config, Arc::new(TestRouter))
            .with_runtime_health(health.clone())
            .build()
            .expect("the fixed peer port is valid configuration")
            .start()
            .await
            .expect("a peer-port collision must not stop local daemon startup");

        assert!(endpoint_record.exists(), "local endpoint remains published");
        assert_eq!(
            health.snapshot().readiness,
            RuntimeReadinessState::Ready,
            "direct-peer unavailability must not make the local daemon unready"
        );

        running
            .begin_shutdown()
            .finish()
            .await
            .expect("runtime shuts down cleanly");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn direct_peer_listener_uses_the_canonical_router_and_normalizes_provenance() {
        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let config = config_with_record(0, temporary_directory.path().join("local-http.json"))
            .with_direct_peer_tcp(DirectPeerTcpConfig::ephemeral_for_test());
        let handler = Arc::new(RecordingPeerRouter::default());
        let running = HttpRuntimeBuilder::new(config, handler.clone())
            .build()
            .expect("valid peer configuration")
            .start()
            .await
            .expect("runtime starts both adapters");
        let peer_port = running
            .direct_peer_address()
            .expect("ephemeral direct peer listener is bound")
            .port();
        let client = direct_peer_tcp_client(
            "localhost".parse().expect("direct host"),
            std::num::NonZeroU16::new(peer_port).expect("non-zero port"),
            Duration::from_secs(1),
        )
        .expect("direct typed client");
        let mut request = write_request();
        let RequestEnvelope::Write(write) = &mut request else {
            unreachable!("fixture is a write");
        };
        write.to = Some(
            "recipient@atm-dev.localhost"
                .parse()
                .expect("host-qualified recipient"),
        );

        let response = client
            .execute(ApiRequest::new(request))
            .await
            .expect("typed response reaches the direct client");
        assert!(matches!(
            response.into_inner(),
            ResponseEnvelope::Error(ref error)
                if error.code().as_str() == "ATM_MESSAGE_VALIDATION_FAILED"
        ));

        {
            let calls = handler.calls.lock().expect("recorded peer calls");
            assert_eq!(calls.len(), 1, "one request reaches one canonical write");
            let (request, ingress) = &calls[0];
            assert!(
                matches!(ingress, AuthenticatedIngress::UntrustedSmoke(provenance)
                if provenance.declared_source_host().as_str() == "127.0.0.1")
            );
            assert_eq!(
                request
                    .authenticated_source_host
                    .as_ref()
                    .map(|host| host.as_str()),
                Some("127.0.0.1")
            );
            assert!(request.origin_message_id.is_some());
            assert!(request.origin_timestamp.is_some());
            assert!(
                request.to.as_ref().expect("recipient").host().is_none(),
                "the delivered mailbox address has no physical host qualifier"
            );
        }
        running
            .begin_shutdown()
            .finish()
            .await
            .expect("runtime shuts down cleanly");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn authenticated_peer_stream_uses_the_same_canonical_router_after_the_adapter() {
        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let config = config_with_record(0, temporary_directory.path().join("local-http.json"))
            .with_direct_peer_tcp(DirectPeerTcpConfig::ephemeral_for_test())
            .with_peer_stream_adapter(Arc::new(PassthroughPeerAdapter));
        let handler = Arc::new(RecordingPeerRouter::default());
        let runtime_handler: Arc<dyn CanonicalWriteHandler> = handler.clone();
        let running = HttpRuntimeBuilder::new(config, runtime_handler)
            .build()
            .expect("valid opaque stream configuration")
            .start()
            .await
            .expect("runtime starts the authenticated stream listener");
        let peer_port = running
            .direct_peer_address()
            .expect("ephemeral direct peer listener is bound")
            .port();
        let client = direct_peer_tcp_client(
            "localhost".parse().expect("direct host"),
            std::num::NonZeroU16::new(peer_port).expect("non-zero port"),
            Duration::from_secs(1),
        )
        .expect("typed test client");

        let _ = client
            .execute(ApiRequest::new(write_request()))
            .await
            .expect("the opaque stream reaches the shared HTTP route");
        let (call_count, ingress, authenticated_source_host) = {
            let calls = handler.calls.lock().expect("recorded peer calls");
            (
                calls.len(),
                calls[0].1.clone(),
                calls[0]
                    .0
                    .authenticated_source_host
                    .as_ref()
                    .map(atm_core::types::HostName::as_str)
                    .map(str::to_owned),
            )
        };
        assert_eq!(call_count, 1, "one opaque peer stream has one dispatch");
        assert_eq!(ingress, AuthenticatedIngress::Peer);
        assert_eq!(
            authenticated_source_host.as_deref(),
            Some("trusted.example.test"),
            "only the adapter supplies peer authentication provenance"
        );
        running
            .begin_shutdown()
            .finish()
            .await
            .expect("runtime shuts down cleanly");
    }

    #[test]
    fn authenticated_peer_stream_adapter_requires_the_single_peer_listener() {
        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let config = config_with_record(0, temporary_directory.path().join("local-http.json"))
            .with_peer_stream_adapter(Arc::new(PassthroughPeerAdapter));
        let error = match HttpRuntimeBuilder::new(config, Arc::new(TestRouter)).build() {
            Ok(_) => panic!("an opaque peer stream has no standalone listener"),
            Err(error) => error,
        };
        assert!(error.message().contains("peer_stream_adapter"));
    }

    #[test]
    fn non_loopback_tcp_configuration_fails_before_lifecycle_start() {
        let error = match HttpRuntimeBuilder::new(
            HttpRuntimeConfig::new(
                loopback_tcp(
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
                    std::path::PathBuf::from("local-http.json"),
                ),
                None,
                limits(1, 1),
                timeouts(Duration::from_secs(1), Duration::from_secs(1)),
            ),
            Arc::new(TestRouter),
        )
        .build()
        {
            Ok(_) => panic!("a non-loopback listener must be rejected before bind"),
            Err(error) => error,
        };
        assert_eq!(error.code().as_str(), "ATM_CONFIG_PARSE_FAILED");
        assert!(error.message().contains("loopback_tcp.bind_address"));
        assert!(
            error
                .cause()
                .is_some_and(|cause| cause.contains("loopback"))
        );
    }

    #[test]
    fn runtime_config_leaves_cannot_represent_zero_values() {
        assert!(NonZeroUsize::new(0).is_none());
        assert!(NonZeroU32::new(0).is_none());
        assert!(NonZeroDuration::new(Duration::ZERO).is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runtime_health_becomes_ready_only_after_all_enabled_binds_and_not_ready_during_drain()
    {
        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let record_path = temporary_directory.path().join("local-http.json");
        let instance_id = Ulid::new();
        write_owner_record(&record_path, instance_id);
        let health = RuntimeHealth::with_owner(99);
        let configured = HttpRuntimeBuilder::new(
            loopback_runtime_config(
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                record_path,
                instance_id,
                None,
                1024,
            ),
            Arc::new(TestRouter),
        )
        .with_runtime_health(health.clone())
        .build()
        .expect("valid runtime configuration");
        assert_eq!(
            health.snapshot().readiness,
            RuntimeReadinessState::Unavailable
        );

        let running = configured.start().await.expect("all listeners bind");
        assert_eq!(health.snapshot().readiness, RuntimeReadinessState::Ready);

        let draining = running.begin_shutdown();
        assert_eq!(
            health.snapshot().readiness,
            RuntimeReadinessState::Unavailable
        );
        draining.finish().await.expect("runtime drains");
        assert_eq!(
            health.snapshot().readiness,
            RuntimeReadinessState::Unavailable
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_socket_preflight_rejects_invalid_material() {
        use std::path::PathBuf;

        let invalid_uds = HttpRuntimeConfig::new(
            loopback_tcp(
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                PathBuf::from("local-http.json"),
            ),
            Some(UnixSocketConfig::new(
                PathBuf::new(),
                UnixSocketOwnerUid::new(NonZeroU32::new(1).expect("test uid is non-zero")),
                UnixSocketMode::new(NonZeroU32::new(0o1000).expect("test mode is non-zero")),
            )),
            limits(1, 1),
            timeouts(Duration::from_secs(1), Duration::from_secs(1)),
        );
        let error = match HttpRuntimeBuilder::new(invalid_uds, Arc::new(TestRouter)).build() {
            Ok(_) => panic!("invalid UDS configuration must be rejected"),
            Err(error) => error,
        };
        assert_eq!(error.code().as_str(), "ATM_CONFIG_PARSE_FAILED");
        assert!(error.message().contains("unix_socket.path"), "{error:?}");

        let group_access = HttpRuntimeConfig::new(
            loopback_tcp(
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                PathBuf::from("local-http.json"),
            ),
            Some(UnixSocketConfig::new(
                PathBuf::from("owner-only.sock"),
                UnixSocketOwnerUid::new(NonZeroU32::new(1).expect("test uid is non-zero")),
                UnixSocketMode::new(NonZeroU32::new(0o660).expect("test mode is non-zero")),
            )),
            limits(1, 1),
            timeouts(Duration::from_secs(1), Duration::from_secs(1)),
        );
        let error = match HttpRuntimeBuilder::new(group_access, Arc::new(TestRouter)).build() {
            Ok(_) => panic!("group-accessible UDS mode must be rejected"),
            Err(error) => error,
        };
        assert_eq!(error.code().as_str(), "ATM_CONFIG_PARSE_FAILED");
        assert!(error.message().contains("unix_socket.mode"), "{error:?}");
        assert!(
            error
                .cause()
                .is_some_and(|cause| cause.contains("only to the configured owner")),
            "owner-only UDS mode must reject group access"
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_socket_is_additive_and_cannot_replace_tcp_bind() {
        use tempfile::tempdir;

        let temporary_directory = tempdir().expect("temporary directory");
        let unix_socket = UnixSocketConfig::new(
            temporary_directory
                .path()
                .join("atm-http-runtime-test.sock"),
            UnixSocketOwnerUid::new(NonZeroU32::new(1).expect("test uid is non-zero")),
            UnixSocketMode::new(NonZeroU32::new(0o600).expect("test mode is non-zero")),
        );
        HttpRuntimeBuilder::new(
            HttpRuntimeConfig::new(
                loopback_tcp(
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                    temporary_directory.path().join("local-http.json"),
                ),
                Some(unix_socket.clone()),
                limits(1, 1),
                timeouts(Duration::from_secs(1), Duration::from_secs(1)),
            ),
            Arc::new(TestRouter),
        )
        .build()
        .expect("TCP and UDS configuration is valid together");

        HttpRuntimeBuilder::new(
            HttpRuntimeConfig::new(
                loopback_tcp(
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                    temporary_directory.path().join("local-http.json"),
                ),
                Some(unix_socket),
                limits(1, 1),
                timeouts(Duration::from_secs(1), Duration::from_secs(1)),
            ),
            Arc::new(TestRouter),
        )
        .build()
        .expect("an additive UDS bind may use an OS-selected loopback TCP port");
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn unix_socket_uses_the_shared_client_router_and_owner_only_endpoint() {
        use std::os::unix::fs::MetadataExt;

        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let socket_path = temporary_directory
            .path()
            .join(atm_core::home::HOST_RUNTIME_SOCKET_FILE);
        let configured = HttpRuntimeBuilder::new(
            uds_config(socket_path.clone(), owner_uid(temporary_directory.path())),
            Arc::new(CanonicalUdsRouter),
        )
        .build()
        .expect("valid UDS configuration");
        let running = configured.start().await.expect("UDS runtime starts");

        let metadata = std::fs::metadata(&socket_path).expect("bound UDS metadata");
        assert_eq!(metadata.uid(), owner_uid(temporary_directory.path()).get());
        assert_eq!(metadata.mode() & 0o777, 0o600);

        let client = super::preferred_local_client(
            temporary_directory.path().join("local-http.json"),
            Duration::from_secs(1),
        )
        .expect("UDS-preferred shared Unix client");
        let response = client
            .execute(ApiRequest::new(write_request()))
            .await
            .expect("canonical error remains a typed API response");
        assert!(matches!(
            response.into_inner(),
            ResponseEnvelope::Error(error) if error.message().contains("canonical UDS test handler reached")
        ));

        running
            .begin_shutdown()
            .finish()
            .await
            .expect("UDS request drains with the runtime");
        assert!(
            !socket_path.exists(),
            "the runtime removes only its own Unix socket during shutdown"
        );
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn unix_socket_response_matches_the_in_process_canonical_route_bytes() {
        use axum::body::{Body, to_bytes};
        use axum::http::Request;
        use axum::http::header::{CONTENT_TYPE, LOCATION};
        use tower::ServiceExt;

        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let socket_path = temporary_directory.path().join("atm-http-runtime.sock");
        let handler = Arc::new(CanonicalUdsRouter);
        let RequestEnvelope::Write(write) = write_request() else {
            unreachable!("UDS fixture always builds a write request")
        };
        let body = serde_json::to_vec(&write).expect("encode canonical write body");
        let expected = super::canonical_message_router(
            handler.clone(),
            super::AuthenticatedConnector::local(),
            limits(1024, 8),
            timeouts(Duration::from_secs(1), Duration::from_secs(1)),
        )
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/atm/messages")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(body.clone()))
                .expect("canonical in-process request"),
        )
        .await
        .expect("canonical in-process route is infallible");
        let expected_status = expected.status();
        let expected_content_type = expected.headers().get(CONTENT_TYPE).cloned();
        let expected_location = expected.headers().get(LOCATION).cloned();
        let expected_body = to_bytes(expected.into_body(), usize::MAX)
            .await
            .expect("read canonical in-process body");

        let configured = HttpRuntimeBuilder::new(
            uds_config(socket_path.clone(), owner_uid(temporary_directory.path())),
            handler,
        )
        .build()
        .expect("valid UDS configuration");
        let running = configured.start().await.expect("UDS runtime starts");
        let raw_uds_client = reqwest::Client::builder()
            .unix_socket(socket_path.clone())
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(1))
            .build()
            .expect("raw UDS comparison client");
        let actual = raw_uds_client
            .post("http://localhost/v1/atm/messages")
            .header(CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .expect("real UDS request");
        let actual_status = actual.status();
        let actual_content_type = actual.headers().get(CONTENT_TYPE).cloned();
        let actual_location = actual.headers().get(LOCATION).cloned();
        let actual_body = actual.bytes().await.expect("read UDS response body");

        assert_eq!(actual_status, expected_status, "UDS keeps canonical status");
        assert_eq!(
            actual_content_type, expected_content_type,
            "UDS keeps canonical content type"
        );
        assert_eq!(
            actual_location, expected_location,
            "UDS keeps canonical location"
        );
        assert_eq!(
            actual_body.as_ref(),
            expected_body.as_ref(),
            "UDS keeps canonical JSON bytes"
        );

        running
            .begin_shutdown()
            .finish()
            .await
            .expect("UDS response-parity runtime drains");
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn unix_socket_shutdown_drains_an_in_flight_canonical_request_before_cleanup() {
        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let socket_path = temporary_directory.path().join("atm-http-runtime.sock");
        let handler = Arc::new(BlockingUdsRouter::new());
        let configured = HttpRuntimeBuilder::new(
            uds_config(socket_path.clone(), owner_uid(temporary_directory.path())),
            handler.clone(),
        )
        .build()
        .expect("valid UDS configuration");
        let running = configured.start().await.expect("UDS runtime starts");
        let client = super::unix_socket_client(&socket_path, Duration::from_secs(1))
            .expect("shared UDS client");
        let request =
            tokio::spawn(async move { client.execute(ApiRequest::new(write_request())).await });

        handler.wait_until_entered().await;
        let drain = tokio::spawn(async move { running.begin_shutdown().finish().await });
        tokio::task::yield_now().await;
        assert!(
            !drain.is_finished(),
            "graceful shutdown must wait for an active UDS canonical request"
        );
        assert!(
            socket_path.exists(),
            "the UDS endpoint remains present while its active request drains"
        );

        handler.release.notify_waiters();
        let response = request
            .await
            .expect("UDS request task joins")
            .expect("canonical error is a typed response");
        assert!(matches!(response.into_inner(), ResponseEnvelope::Error(_)));
        drain
            .await
            .expect("drain task joins")
            .expect("runtime drains after request completion");
        assert!(
            !socket_path.exists(),
            "the UDS endpoint is removed only after the active request drains"
        );
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn unix_socket_owner_mismatch_fails_closed_without_leaving_an_endpoint() {
        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let socket_path = temporary_directory.path().join("atm-http-runtime.sock");
        let actual_owner = owner_uid(temporary_directory.path()).get();
        let configured_owner = if actual_owner == 1 { 2 } else { 1 };
        let health = RuntimeHealth::with_owner(99);
        let record_path = socket_path.with_file_name("local-http.json");
        let configured = HttpRuntimeBuilder::new(
            uds_config(
                socket_path.clone(),
                NonZeroU32::new(configured_owner).expect("non-zero mismatched uid"),
            ),
            Arc::new(CanonicalUdsRouter),
        )
        .with_runtime_health(health.clone())
        .build()
        .expect("configuration shape is valid before bind ownership check");

        let error = match configured.start().await {
            Ok(_) => panic!("runtime must reject a bound socket owned by another uid"),
            Err(error) => error,
        };
        assert_eq!(error.code().as_str(), "ATM_CONFIG_PARSE_FAILED");
        assert!(error.message().contains("parent owner"));
        assert!(
            !socket_path.exists(),
            "failed UDS startup must not leave a reachable endpoint"
        );
        assert!(
            !record_path.exists(),
            "the loopback record is not published before every enabled listener binds"
        );
        assert_eq!(
            health.snapshot().readiness,
            RuntimeReadinessState::Unavailable
        );
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn unix_socket_parent_writable_by_others_fails_closed_before_bind() {
        use std::os::unix::fs::PermissionsExt;

        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        std::fs::set_permissions(
            temporary_directory.path(),
            std::fs::Permissions::from_mode(0o722),
        )
        .expect("make test parent group writable");
        let socket_path = temporary_directory.path().join("atm-http-runtime.sock");
        let configured = HttpRuntimeBuilder::new(
            uds_config(socket_path.clone(), owner_uid(temporary_directory.path())),
            Arc::new(CanonicalUdsRouter),
        )
        .build()
        .expect("configuration shape is valid before parent safety check");

        let error = match configured.start().await {
            Ok(_) => panic!("runtime must reject a UDS parent writable by others"),
            Err(error) => error,
        };
        assert_eq!(error.code().as_str(), "ATM_CONFIG_PARSE_FAILED");
        assert!(
            error.message().contains("must not be writable"),
            "unsafe endpoint publication must fail before a listener starts: {error}"
        );
        assert!(
            !socket_path.exists(),
            "unsafe parent never receives a socket"
        );
    }

    #[cfg(unix)]
    #[test]
    fn private_staging_directory_is_owner_private_and_inode_safe_on_drop() {
        use std::os::unix::fs::MetadataExt;

        let parent = tempfile::tempdir().expect("temporary staging parent");
        let staging_owner_uid = std::fs::metadata(parent.path())
            .expect("staging parent metadata")
            .uid();
        let staging =
            super::unix_socket::PrivateStagingDirectory::create(parent.path(), staging_owner_uid)
                .expect("allocate private staging directory");
        let path = staging.path().to_path_buf();
        let metadata = std::fs::metadata(&path).expect("staging directory metadata");
        assert_eq!(metadata.mode() & 0o777, 0o700);
        assert_eq!(metadata.uid(), owner_uid(parent.path()).get());

        drop(staging);
        assert!(
            !path.exists(),
            "the runtime-owned staging directory is cleaned after publication work"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn an_unexpected_server_task_exit_revokes_readiness_and_cleans_the_endpoint() {
        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let endpoint_record = temporary_directory.path().join("local-http.json");
        let health = RuntimeHealth::with_owner(99);
        let mut running = HttpRuntimeBuilder::new(
            config_with_record(0, endpoint_record.clone()),
            Arc::new(TestRouter),
        )
        .with_runtime_health(health.clone())
        .build()
        .expect("valid configuration")
        .start()
        .await
        .expect("runtime starts");

        running.state.server_task.abort();
        tokio::time::timeout(Duration::from_secs(1), running.wait_for_server_stop())
            .await
            .expect("supervision observes the stopped server task");
        assert_eq!(
            health.snapshot().readiness,
            RuntimeReadinessState::Unavailable,
            "a daemon without its server is never Ready"
        );

        let error = match running.begin_shutdown().finish().await {
            Ok(_) => panic!("an aborted server task reports a typed runtime failure"),
            Err(error) => error,
        };
        assert_eq!(error.code().as_str(), "ATM_DAEMON_UNAVAILABLE");
        assert!(
            !endpoint_record.exists(),
            "the normal drain cleanup removes the stale endpoint record"
        );
    }

    #[cfg(not(unix))]
    #[test]
    fn unix_socket_configuration_is_rejected_on_non_unix_targets() {
        use std::path::PathBuf;

        let error = match HttpRuntimeBuilder::new(
            HttpRuntimeConfig::new(
                loopback_tcp(
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                    PathBuf::from("local-http.json"),
                ),
                Some(UnixSocketConfig::new(
                    PathBuf::from("atm-http-runtime-test.sock"),
                    UnixSocketOwnerUid::new(NonZeroU32::new(1).expect("test uid is non-zero")),
                    UnixSocketMode::new(NonZeroU32::new(0o600).expect("test mode is non-zero")),
                )),
                limits(1, 1),
                timeouts(Duration::from_secs(1), Duration::from_secs(1)),
            ),
            Arc::new(TestRouter),
        )
        .build()
        {
            Ok(_) => panic!("non-Unix targets cannot configure a Unix socket"),
            Err(error) => error,
        };
        assert_eq!(error.code().as_str(), "ATM_CONFIG_PARSE_FAILED");
        assert_eq!(
            error.cause(),
            Some("Unix-domain socket configuration is unsupported on this platform")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn lifecycle_is_consuming_and_requires_validated_configuration() {
        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let configured = HttpRuntimeBuilder::new(
            config_with_record(0, temporary_directory.path().join("local-http.json")),
            Arc::new(TestRouter),
        )
        .build()
        .expect("valid configuration");
        let running = configured.start().await.expect("AL.1 start transition");
        let draining = running.begin_shutdown();
        let _stopped = draining.finish().await.expect("runtime must drain");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runtime_binds_serves_and_joins_the_axum_task() {
        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let configured = HttpRuntimeBuilder::new(
            config_with_record(0, temporary_directory.path().join("local-http.json")),
            Arc::new(TestRouter),
        )
        .build()
        .expect("valid configuration");
        let running = configured.start().await.expect("replacement server starts");
        let record: LocalHttpEndpointRecord = serde_json::from_slice(
            &std::fs::read(temporary_directory.path().join("local-http.json"))
                .expect("read active loopback endpoint record"),
        )
        .expect("decode active loopback endpoint record");
        let response = bounded_test_http_client()
            .get(format!(
                "http://{}/v1/atm/messages",
                running.local_address()
            ))
            .header(LOCAL_CAPABILITY_HEADER, record.capability_base64url)
            .send()
            .await
            .expect("replacement server responds");
        // `GET /messages` is a retained core route. An empty request body is
        // therefore a typed request-validation failure, rather than proof
        // that the replacement exposed only the old write route.
        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
        running
            .begin_shutdown()
            .finish()
            .await
            .expect("replacement server joins after shutdown");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn an15_http_differential_probe_exercises_live_loopback_request_shapes() {
        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let configured = HttpRuntimeBuilder::new(
            config_with_record(0, temporary_directory.path().join("local-http.json")),
            Arc::new(TestRouter),
        )
        .build()
        .expect("valid configuration");
        let running = configured.start().await.expect("replacement server starts");
        let record: LocalHttpEndpointRecord = serde_json::from_slice(
            &std::fs::read(temporary_directory.path().join("local-http.json"))
                .expect("read active loopback endpoint record"),
        )
        .expect("decode active loopback endpoint record");
        let endpoint = format!("http://{}", running.local_address());
        let client = bounded_test_http_client();

        for case_index in 0..100 {
            // Each case changes the search-query shape consumed by
            // `decode_search_query`, not an ignored HTTP header. All forms
            // intentionally reject before the test router can dispatch.
            let (path, shape, expected_message) = match case_index % 5 {
                0 => (
                    "/v1/atm/messages/search",
                    "missing-query",
                    "missing its request query parameter",
                ),
                1 => (
                    "/v1/atm/messages/search?other=value",
                    "missing-request-key",
                    "missing its request query parameter",
                ),
                2 => (
                    "/v1/atm/messages/search?request=AA&request=AA",
                    "repeated-request-key",
                    "repeats its request query parameter",
                ),
                3 => (
                    "/v1/atm/messages/search?request=.",
                    "invalid-base64url",
                    "not valid base64url",
                ),
                _ => (
                    "/v1/atm/messages/search?request=e30",
                    "invalid-search-json",
                    "JSON is invalid",
                ),
            };
            eprintln!("AN15_CASE_SHAPE={shape}");
            let response = client
                .get(format!("{endpoint}{path}"))
                .header(LOCAL_CAPABILITY_HEADER, &record.capability_base64url)
                .header("x-atm-request-id", (case_index + 1).to_string())
                .send()
                .await
                .expect("replacement server responds");
            assert_eq!(
                response.status(),
                reqwest::StatusCode::BAD_REQUEST,
                "case {case_index}"
            );
            let error: atm_core::error::AtmError = serde_json::from_slice(
                &response
                    .bytes()
                    .await
                    .expect("read typed search-query validation response"),
            )
            .expect("decode typed search-query validation response");
            assert_eq!(
                error.code(),
                atm_core::error::AtmErrorCode::MessageValidationFailed,
                "case {case_index}"
            );
            assert!(
                error.message().contains(expected_message),
                "case {case_index} must reach its intended query-decoder branch: {error:?}"
            );
        }
        running
            .begin_shutdown()
            .finish()
            .await
            .expect("replacement server joins after HTTP fuzz probe");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn os_selected_loopback_port_is_published_from_the_bound_listener() {
        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let record_path = temporary_directory.path().join("local-http.json");
        let instance_id = Ulid::new();
        write_owner_record(&record_path, instance_id);
        let running = HttpRuntimeBuilder::new(
            loopback_runtime_config(
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                record_path.clone(),
                instance_id,
                None,
                1024,
            ),
            Arc::new(TestRouter),
        )
        .build()
        .expect("port zero is valid loopback configuration")
        .start()
        .await
        .expect("Tokio selects and binds a loopback port");

        let record: LocalHttpEndpointRecord = serde_json::from_slice(
            &std::fs::read(&record_path).expect("read active endpoint record"),
        )
        .expect("decode active endpoint record");
        assert_eq!(record.ipv4_loopback, Some(running.local_address()));
        assert_ne!(running.local_address().port(), 0);

        running
            .begin_shutdown()
            .finish()
            .await
            .expect("runtime drains after publishing its selected port");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn loopback_header_read_deadline_closes_an_incomplete_request() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let configured = HttpRuntimeBuilder::new(
            HttpRuntimeConfig::new(
                loopback_tcp(
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                    temporary_directory.path().join("local-http.json"),
                ),
                None,
                limits(1024, 8),
                timeouts(Duration::from_millis(25), Duration::from_secs(1)),
            ),
            Arc::new(TestRouter),
        )
        .build()
        .expect("valid loopback configuration");
        let running = configured.start().await.expect("runtime starts");

        let mut stream = tokio::net::TcpStream::connect(running.local_address())
            .await
            .expect("connect incomplete request fixture");
        stream
            .write_all(b"POST /v1/messages HTTP/1.1\r\nHost: localhost\r\n")
            .await
            .expect("write incomplete HTTP headers");
        let mut response = [0_u8; 1];
        let bytes_read = tokio::time::timeout(Duration::from_secs(1), stream.read(&mut response))
            .await
            .expect("HTTP header deadline must close the connection")
            .expect("read closure after HTTP header deadline");
        assert_eq!(bytes_read, 0, "slow header connection must be closed");

        running
            .begin_shutdown()
            .finish()
            .await
            .expect("runtime shuts down after header deadline test");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn loopback_connection_admission_stops_before_router_work() {
        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let record_path = temporary_directory.path().join("local-http.json");
        let instance_id = Ulid::new();
        write_owner_record(&record_path, instance_id);
        let handler = Arc::new(CountingLoopbackRouter::new());
        let running = HttpRuntimeBuilder::new(
            HttpRuntimeConfig::new(
                loopback_tcp_with_instance(
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                    record_path.clone(),
                    instance_id,
                ),
                None,
                limits(1024, 1),
                timeouts(Duration::from_secs(1), Duration::from_secs(1)),
            ),
            handler.clone(),
        )
        .build()
        .expect("valid one-connection loopback configuration")
        .start()
        .await
        .expect("runtime starts");

        let first_client = super::loopback_tcp_client(&record_path, Duration::from_secs(1))
            .expect("first loopback client");
        let first =
            tokio::spawn(
                async move { first_client.execute(ApiRequest::new(write_request())).await },
            );
        handler.wait_until_entered().await;

        let second_client = super::loopback_tcp_client(&record_path, Duration::from_millis(25))
            .expect("second loopback client");
        let error = second_client
            .execute(ApiRequest::new(write_request()))
            .await
            .expect_err("a second connection must not reach the router while the first is active");
        assert_eq!(error.code().as_str(), "ATM_WAIT_TIMEOUT");
        assert_eq!(handler.calls.load(Ordering::SeqCst), 1);

        handler.release.notify_waiters();
        first
            .await
            .expect("first request task joins")
            .expect("first request receives its canonical response");
        tokio::time::timeout(Duration::from_secs(1), running.begin_shutdown().finish())
            .await
            .expect("runtime must drain rather than wait on an unadmitted connection")
            .expect("runtime shuts down after connection-admission test");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn loopback_shared_client_uses_the_active_record_and_canonical_handler() {
        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let record_path = temporary_directory.path().join("local-http.json");
        let instance_id = Ulid::new();
        write_owner_record(&record_path, instance_id);
        let configured = HttpRuntimeBuilder::new(
            loopback_runtime_config(
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                record_path.clone(),
                instance_id,
                None,
                1024,
            ),
            Arc::new(CanonicalUdsRouter),
        )
        .build()
        .expect("valid loopback runtime configuration");
        let running = configured.start().await.expect("loopback runtime starts");

        let client = super::loopback_tcp_client(&record_path, Duration::from_secs(1))
            .expect("shared loopback client");
        let response = client
            .execute(ApiRequest::new(write_request()))
            .await
            .expect("canonical error remains a typed API response");
        assert!(matches!(
            response.into_inner(),
            ResponseEnvelope::Error(error) if error.message().contains("canonical UDS test handler reached")
        ));

        running
            .begin_shutdown()
            .finish()
            .await
            .expect("loopback runtime drains");
        assert!(
            !record_path.exists(),
            "the runtime removes only its own endpoint record after drain"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn loopback_rejects_missing_and_mismatched_capability_before_handler() {
        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let record_path = temporary_directory.path().join("local-http.json");
        let instance_id = Ulid::new();
        write_owner_record(&record_path, instance_id);
        let handler = Arc::new(CountingLoopbackRouter::new());
        let running = HttpRuntimeBuilder::new(
            loopback_runtime_config(
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                record_path.clone(),
                instance_id,
                None,
                1024,
            ),
            handler.clone(),
        )
        .build()
        .expect("valid loopback runtime configuration")
        .start()
        .await
        .expect("loopback runtime starts");
        let record: LocalHttpEndpointRecord = serde_json::from_slice(
            &std::fs::read(&record_path).expect("read active endpoint record"),
        )
        .expect("decode active endpoint record");
        let RequestEnvelope::Write(write) = write_request() else {
            unreachable!("write fixture")
        };
        let body = serde_json::to_vec(&write).expect("encode canonical request body");
        let base_url = format!("http://{}", running.local_address());

        for capability in [None, Some("wrong-capability")] {
            let request = bounded_test_http_client()
                .post(format!("{base_url}/v1/atm/messages"))
                .header("content-type", "application/json")
                .body(body.clone());
            let request = if let Some(capability) = capability {
                request.header(LOCAL_CAPABILITY_HEADER, capability)
            } else {
                request
            };
            let response = request.send().await.expect("loopback rejection response");
            assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
            let error: AtmError =
                serde_json::from_slice(&response.bytes().await.expect("read ADR-032 error body"))
                    .expect("decode ADR-032 error body");
            assert_eq!(error.code().as_str(), "ATM_LOCAL_HTTP_CAPABILITY_INVALID");
            assert_eq!(
                handler.calls.load(Ordering::SeqCst),
                0,
                "capability rejection must happen before CanonicalWriteHandler"
            );
        }

        let mut duplicate_headers = reqwest::header::HeaderMap::new();
        duplicate_headers.append(
            reqwest::header::HeaderName::from_static("content-type"),
            reqwest::header::HeaderValue::from_static("application/json"),
        );
        for _ in 0..2 {
            duplicate_headers.append(
                reqwest::header::HeaderName::from_static("x-atm-local-capability"),
                reqwest::header::HeaderValue::from_str(&record.capability_base64url)
                    .expect("test capability is an HTTP header value"),
            );
        }
        let duplicate_capability_response = bounded_test_http_client()
            .post(format!("{base_url}/v1/atm/messages"))
            .headers(duplicate_headers)
            .body(body.clone())
            .send()
            .await
            .expect("duplicate capability rejection response");
        assert_eq!(
            duplicate_capability_response.status(),
            reqwest::StatusCode::BAD_REQUEST
        );
        let duplicate_error: AtmError = serde_json::from_slice(
            &duplicate_capability_response
                .bytes()
                .await
                .expect("read duplicate-capability ADR-032 error body"),
        )
        .expect("duplicate capability uses the ADR-032 error body");
        assert_eq!(
            duplicate_error.code().as_str(),
            "ATM_LOCAL_HTTP_CAPABILITY_INVALID"
        );
        assert_eq!(
            handler.calls.load(Ordering::SeqCst),
            0,
            "duplicate capability must happen before CanonicalWriteHandler"
        );

        let valid_request = bounded_test_http_client()
            .post(format!("{base_url}/v1/atm/messages"))
            .header("content-type", "application/json")
            .header(LOCAL_CAPABILITY_HEADER, record.capability_base64url)
            .body(body)
            .send();
        let valid_request = tokio::spawn(valid_request);
        handler.wait_until_entered().await;
        assert_eq!(handler.calls.load(Ordering::SeqCst), 1);
        handler.release.notify_waiters();
        assert_eq!(
            valid_request
                .await
                .expect("request task joins")
                .expect("valid request returns")
                .status(),
            reqwest::StatusCode::BAD_REQUEST
        );
        running
            .begin_shutdown()
            .finish()
            .await
            .expect("loopback runtime drains");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn loopback_client_rejects_a_stale_owner_record_before_connecting() {
        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let record_path = temporary_directory.path().join("local-http.json");
        let record_instance = Ulid::new();
        write_owner_record(&record_path, Ulid::new());
        let capability = LocalCapability::generate().expect("capability");
        let stale_record = LocalHttpEndpointRecord::active(
            record_instance,
            Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9)),
            None,
            &capability,
        );
        std::fs::write(
            &record_path,
            serde_json::to_vec(&stale_record).expect("encode stale record"),
        )
        .expect("write stale record");
        let client = super::loopback_tcp_client(&record_path, Duration::from_secs(1))
            .expect("shared loopback client");
        let error = client
            .execute(ApiRequest::new(write_request()))
            .await
            .expect_err("stale endpoint record must fail before connection");
        assert_eq!(error.code().as_str(), "ATM_DAEMON_UNAVAILABLE");
        assert!(error.message().contains("different daemon instance"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn loopback_client_rejects_non_loopback_endpoint_records_before_connecting() {
        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let record_path = temporary_directory.path().join("local-http.json");
        let instance_id = Ulid::new();
        write_owner_record(&record_path, instance_id);
        let capability = LocalCapability::generate().expect("capability");
        let invalid_record = LocalHttpEndpointRecord::active(
            instance_id,
            Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 9)),
            None,
            &capability,
        );
        std::fs::write(
            &record_path,
            serde_json::to_vec(&invalid_record).expect("encode invalid record"),
        )
        .expect("write invalid record");

        let client = super::loopback_tcp_client(&record_path, Duration::from_secs(1))
            .expect("shared loopback client");
        let error = client
            .execute(ApiRequest::new(write_request()))
            .await
            .expect_err("a non-loopback record must fail before connection");
        assert_eq!(
            error.code().as_str(),
            "ATM_LOCAL_HTTP_ENDPOINT_NON_LOOPBACK"
        );
        assert!(error.message().contains("non-loopback"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn loopback_client_rejects_a_missing_endpoint_record_before_connecting() {
        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let record_path = temporary_directory.path().join("missing-local-http.json");
        let client = super::loopback_tcp_client(&record_path, Duration::from_secs(1))
            .expect("shared loopback client");
        let error = client
            .execute(ApiRequest::new(write_request()))
            .await
            .expect_err("missing endpoint record must fail before connection");
        assert_eq!(error.code().as_str(), "ATM_DAEMON_UNAVAILABLE");
        assert!(error.message().contains("read local HTTP endpoint record"));
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn loopback_and_uds_return_identical_canonical_json() {
        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let socket_path = temporary_directory.path().join("atm-http-runtime.sock");
        let record_path = temporary_directory.path().join("local-http.json");
        let instance_id = Ulid::new();
        write_owner_record(&record_path, instance_id);
        let configured = HttpRuntimeBuilder::new(
            loopback_runtime_config(
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                record_path.clone(),
                instance_id,
                Some(UnixSocketConfig::new(
                    socket_path.clone(),
                    UnixSocketOwnerUid::new(owner_uid(temporary_directory.path())),
                    UnixSocketMode::new(NonZeroU32::new(0o600).expect("owner-only mode")),
                )),
                1024,
            ),
            Arc::new(CanonicalUdsRouter),
        )
        .build()
        .expect("valid additive runtime configuration");
        let running = configured.start().await.expect("runtime starts");
        let request = ApiRequest::new(write_request());
        let uds_response = super::unix_socket_client(&socket_path, Duration::from_secs(1))
            .expect("shared UDS client")
            .execute(request.clone())
            .await
            .expect("typed UDS response");
        let loopback_response = super::loopback_tcp_client(&record_path, Duration::from_secs(1))
            .expect("shared loopback client")
            .execute(request)
            .await
            .expect("typed loopback response");
        assert_eq!(
            serde_json::to_value(uds_response.into_inner()).expect("serialize UDS response"),
            serde_json::to_value(loopback_response.into_inner())
                .expect("serialize loopback response"),
            "the typed UDS and loopback clients preserve one response contract"
        );
        running
            .begin_shutdown()
            .finish()
            .await
            .expect("parity runtime drains");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn loopback_body_limit_rejects_before_handler() {
        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let record_path = temporary_directory.path().join("local-http.json");
        let instance_id = Ulid::new();
        write_owner_record(&record_path, instance_id);
        let handler = Arc::new(CountingLoopbackRouter::new());
        let running = HttpRuntimeBuilder::new(
            loopback_runtime_config(
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                record_path.clone(),
                instance_id,
                None,
                8,
            ),
            handler.clone(),
        )
        .build()
        .expect("valid loopback runtime configuration")
        .start()
        .await
        .expect("loopback runtime starts");
        let record: LocalHttpEndpointRecord = serde_json::from_slice(
            &std::fs::read(&record_path).expect("read active endpoint record"),
        )
        .expect("decode active endpoint record");
        let response = bounded_test_http_client()
            .post(format!(
                "http://{}/v1/atm/messages",
                running.local_address()
            ))
            .header("content-type", "application/json")
            .header(LOCAL_CAPABILITY_HEADER, record.capability_base64url)
            .body("x".repeat(64))
            .send()
            .await
            .expect("body-limit response");
        assert!(response.status().is_client_error());
        let error: AtmError = serde_json::from_slice(
            &response
                .bytes()
                .await
                .expect("read ADR-032 body-limit error"),
        )
        .expect("decode ADR-032 body-limit error");
        assert_eq!(error.code().as_str(), "ATM_MESSAGE_VALIDATION_FAILED");
        assert_eq!(handler.calls.load(Ordering::SeqCst), 0);
        running
            .begin_shutdown()
            .finish()
            .await
            .expect("body-limit runtime drains");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn loopback_shutdown_drains_an_in_flight_canonical_request_before_record_cleanup() {
        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let record_path = temporary_directory.path().join("local-http.json");
        let instance_id = Ulid::new();
        write_owner_record(&record_path, instance_id);
        let handler = Arc::new(CountingLoopbackRouter::new());
        let configured = HttpRuntimeBuilder::new(
            loopback_runtime_config(
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                record_path.clone(),
                instance_id,
                None,
                1024,
            ),
            handler.clone(),
        )
        .build()
        .expect("valid loopback runtime configuration");
        let running = configured.start().await.expect("loopback runtime starts");
        let client = super::loopback_tcp_client(&record_path, Duration::from_secs(1))
            .expect("shared loopback client");
        let request =
            tokio::spawn(async move { client.execute(ApiRequest::new(write_request())).await });

        handler.wait_until_entered().await;
        let drain = tokio::spawn(async move { running.begin_shutdown().finish().await });
        tokio::task::yield_now().await;
        assert!(
            !drain.is_finished(),
            "shutdown must retain an in-flight loopback request"
        );
        assert!(
            record_path.exists(),
            "endpoint record remains while the request drains"
        );

        handler.release.notify_waiters();
        let response = request
            .await
            .expect("request task joins")
            .expect("canonical error remains a typed response");
        assert!(matches!(response.into_inner(), ResponseEnvelope::Error(_)));
        drain
            .await
            .expect("drain task joins")
            .expect("loopback runtime drains after active request completes");
        assert!(
            !record_path.exists(),
            "endpoint record is removed only after the active request drains"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn loopback_shutdown_deadline_aborts_an_in_flight_request_and_cleans_up() {
        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let record_path = temporary_directory.path().join("local-http.json");
        let instance_id = Ulid::new();
        write_owner_record(&record_path, instance_id);
        let handler = Arc::new(CountingLoopbackRouter::new());
        let configured = HttpRuntimeBuilder::new(
            HttpRuntimeConfig::new(
                loopback_tcp_with_instance(
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                    record_path.clone(),
                    instance_id,
                ),
                None,
                limits(1024, 8),
                timeouts(Duration::from_secs(1), Duration::from_millis(1)),
            ),
            handler.clone(),
        )
        .build()
        .expect("valid short-shutdown runtime configuration");
        let running = configured.start().await.expect("loopback runtime starts");
        let client = super::loopback_tcp_client(&record_path, Duration::from_secs(1))
            .expect("shared loopback client");
        let request =
            tokio::spawn(async move { client.execute(ApiRequest::new(write_request())).await });

        handler.wait_until_entered().await;
        let shutdown =
            tokio::time::timeout(Duration::from_secs(1), running.begin_shutdown().finish())
                .await
                .expect("forced-abort shutdown completes within a bounded test window");
        let error = match shutdown {
            Ok(_) => panic!("an in-flight request exceeds the configured shutdown deadline"),
            Err(error) => error,
        };
        assert_eq!(error.code().as_str(), "ATM_DAEMON_UNAVAILABLE");
        assert!(
            !record_path.exists(),
            "forced abort removes the endpoint record after owning the server task"
        );
        assert!(
            tokio::time::timeout(Duration::from_secs(1), request)
                .await
                .expect("aborted request task joins")
                .expect("request task itself joins")
                .is_err(),
            "the aborted in-flight request cannot report a successful response"
        );
    }

    #[cfg(windows)]
    #[tokio::test(flavor = "current_thread")]
    async fn windows_loopback_fixture_uses_the_same_capability_authenticated_route() {
        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let record_path = temporary_directory.path().join("local-http.json");
        let instance_id = Ulid::new();
        write_owner_record(&record_path, instance_id);
        let running = HttpRuntimeBuilder::new(
            loopback_runtime_config(
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                record_path.clone(),
                instance_id,
                None,
                1024,
            ),
            Arc::new(CanonicalUdsRouter),
        )
        .build()
        .expect("valid Windows loopback runtime configuration")
        .start()
        .await
        .expect("Windows loopback runtime starts");
        let client = super::loopback_tcp_client(&record_path, Duration::from_secs(1))
            .expect("shared loopback client");
        assert!(matches!(
            client
                .execute(ApiRequest::new(write_request()))
                .await
                .expect("typed response")
                .into_inner(),
            ResponseEnvelope::Error(_)
        ));
        running
            .begin_shutdown()
            .finish()
            .await
            .expect("Windows drain");
    }
}
