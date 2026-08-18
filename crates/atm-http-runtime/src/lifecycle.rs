//! Runtime lifecycle transitions and listener supervision.
//!
//! This module owns lifecycle-only code. The public configuration and typed
//! state markers remain in the crate facade so consumers retain one stable
//! public runtime surface.

use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use atm_core::PeerIoAdapter;
use atm_core::error::AtmError;
use atm_core::local_http::LocalCapability;
use atm_core::types::HostName;
use tokio::net::TcpListener;
#[cfg(unix)]
use tokio::net::UnixListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;

#[cfg(unix)]
use super::http1_server::serve_unix_http1;
use super::http1_server::{serve_loopback_http1, serve_peer_http1};
use super::loopback_tcp::{
    authenticated_loopback_router, cleanup_loopback_endpoint_record,
    publish_loopback_endpoint_record,
};
#[cfg(unix)]
use super::unix_socket::{
    UnixSocketPathGuard, UnixSocketStartupLock, bind_unix_listener, reclaim_stale_unix_socket,
};
use super::*;

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
        let BoundRuntimeListeners {
            listener,
            local_address,
            direct_peer_listener,
            direct_peer_address,
            #[cfg(unix)]
            unix_listener,
        } = bind_runtime_listeners(&self.config, &self.health).await?;
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
        let direct_peer = direct_peer_listener.map(|listener| {
            configured_direct_peer_server(
                listener,
                &self.config,
                Arc::clone(&self.handler),
                self.peer_io_adapter.clone(),
            )
        });
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
        self.health.mark_ready_with_detail(runtime_ready_detail(
            &self.config,
            self.peer_io_adapter.is_some(),
            direct_peer_address.is_some(),
        ));
        Ok(HttpRuntime {
            config: self.config,
            handler: self.handler,
            peer_io_adapter: self.peer_io_adapter,
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

fn runtime_ready_detail(
    config: &HttpRuntimeConfig,
    adapter_enabled: bool,
    direct_peer_bound: bool,
) -> Option<String> {
    if adapter_enabled && direct_peer_bound {
        Some("direct-peer transport: mTLS".to_owned())
    } else {
        config.direct_peer_plaintext_diagnostic.map(|diagnostic| {
            format!(
                "direct-peer transport: plaintext diagnostic override ({})",
                diagnostic.label()
            )
        })
    }
}

struct BoundRuntimeListeners {
    listener: TcpListener,
    local_address: SocketAddr,
    direct_peer_listener: Option<TcpListener>,
    direct_peer_address: Option<SocketAddr>,
    #[cfg(unix)]
    unix_listener: Option<(UnixListener, UnixSocketPathGuard)>,
}

async fn bind_runtime_listeners(
    config: &HttpRuntimeConfig,
    health: &RuntimeHealth,
) -> Result<BoundRuntimeListeners, AtmError> {
    let (listener, local_address) = bind_loopback_listener(config, health).await?;
    // Every enabled listener must be bound before publishing the loopback
    // endpoint record. Otherwise a client could observe a Ready-looking record
    // while the additive UDS adapter still fails to start.
    let direct_peer_listener = bind_configured_direct_peer_listener(config, health).await?;
    let direct_peer_address = direct_peer_listener
        .as_ref()
        .and_then(|listener| listener.local_addr().ok());
    #[cfg(unix)]
    let unix_listener = bind_configured_unix_listener(config, health).await?;
    Ok(BoundRuntimeListeners {
        listener,
        local_address,
        direct_peer_listener,
        direct_peer_address,
        #[cfg(unix)]
        unix_listener,
    })
}

fn configured_direct_peer_server(
    listener: TcpListener,
    config: &HttpRuntimeConfig,
    handler: Arc<dyn CanonicalWriteHandler>,
    peer_io_adapter: Option<Arc<dyn PeerIoAdapter>>,
) -> DirectPeerServer {
    if let Some(adapter) = peer_io_adapter {
        let limits = config.limits;
        let timeouts = config.timeouts;
        DirectPeerServer::Authenticated {
            listener,
            adapter,
            router_for_peer: Arc::new(move |source_host: HostName| {
                canonical_api_router(
                    Arc::clone(&handler),
                    AuthenticatedConnector::peer(source_host),
                    limits,
                    timeouts,
                )
            }),
        }
    } else {
        let diagnostic = config
            .direct_peer_plaintext_diagnostic
            .expect("build validates explicit plaintext diagnostic mode");
        tracing::warn!(
            mode = diagnostic.label(),
            "direct-peer listener is running in explicit plaintext diagnostic mode"
        );
        DirectPeerServer::PlaintextDiagnostic {
            listener,
            router: canonical_api_router(
                handler,
                AuthenticatedConnector::peer_socket(),
                config.limits,
                config.timeouts,
            ),
        }
    }
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
    Authenticated {
        listener: TcpListener,
        adapter: Arc<dyn PeerIoAdapter>,
        router_for_peer: Arc<dyn Fn(HostName) -> axum::Router + Send + Sync>,
    },
    PlaintextDiagnostic {
        listener: TcpListener,
        router: axum::Router,
    },
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
    spawn_loopback_server(
        &mut servers,
        loopback_listener,
        loopback_router,
        max_connections,
        header_read_timeout,
        shutdown_rx.clone(),
    );
    spawn_direct_peer_server(
        &mut servers,
        direct_peer,
        max_connections,
        header_read_timeout,
        shutdown_rx.clone(),
    );
    #[cfg(unix)]
    spawn_unix_socket_server(
        &mut servers,
        unix_socket,
        max_connections,
        header_read_timeout,
        shutdown_rx,
    );
    join_server_group(servers, shutdown_tx).await
}

fn spawn_loopback_server(
    servers: &mut tokio::task::JoinSet<std::io::Result<()>>,
    listener: TcpListener,
    router: axum::Router,
    max_connections: usize,
    header_read_timeout: Duration,
    shutdown_rx: watch::Receiver<()>,
) {
    servers.spawn(serve_loopback_http1(
        listener,
        router,
        max_connections,
        header_read_timeout,
        shutdown_rx,
    ));
}

fn spawn_direct_peer_server(
    servers: &mut tokio::task::JoinSet<std::io::Result<()>>,
    direct_peer: Option<DirectPeerServer>,
    max_connections: usize,
    header_read_timeout: Duration,
    shutdown_rx: watch::Receiver<()>,
) {
    if let Some(direct_peer) = direct_peer {
        match direct_peer {
            DirectPeerServer::Authenticated {
                listener,
                adapter,
                router_for_peer,
            } => {
                servers.spawn(serve_peer_http1(
                    listener,
                    router_for_peer,
                    adapter,
                    max_connections,
                    header_read_timeout,
                    shutdown_rx,
                ));
            }
            DirectPeerServer::PlaintextDiagnostic { listener, router } => {
                servers.spawn(serve_loopback_http1(
                    listener,
                    router,
                    max_connections,
                    header_read_timeout,
                    shutdown_rx,
                ));
            }
        }
    }
}

#[cfg(unix)]
fn spawn_unix_socket_server(
    servers: &mut tokio::task::JoinSet<std::io::Result<()>>,
    unix_socket: Option<(UnixListener, UnixSocketPathGuard, axum::Router)>,
    max_connections: usize,
    header_read_timeout: Duration,
    shutdown_rx: watch::Receiver<()>,
) {
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
}

async fn join_server_group(
    mut servers: tokio::task::JoinSet<std::io::Result<()>>,
    shutdown_tx: watch::Sender<()>,
) -> std::io::Result<()> {
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
            peer_io_adapter: self.peer_io_adapter,
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
            peer_io_adapter: self.peer_io_adapter,
            health: self.health,
            state: Stopped,
        })
    }
}
