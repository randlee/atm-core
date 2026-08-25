//! Tokio/Hyper connection serving for the owned Axum router.
//!
//! Axum supplies routing, extraction, and response handling. Hyper owns HTTP/1
//! parsing and its header-read deadline; this module owns only listener
//! lifecycle and graceful shutdown for the runtime's physical adapters.

use std::convert::Infallible;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper_util::rt::{TokioIo, TokioTimer};
use hyper_util::service::TowerToHyperService;
use tokio::net::TcpListener;
#[cfg(unix)]
use tokio::net::UnixListener;
use tokio::sync::{Semaphore, watch};
use tokio::task::JoinSet;
use tower::ServiceExt;

use crate::message_handler::AuthenticatedConnector;
use crate::{
    CanonicalWriteHandler, PeerStreamAdapter, RuntimeLimits, RuntimeTimeouts, canonical_api_router,
};

/// Serves the capability-authenticated loopback adapter with bounded
/// header-read time and cooperative graceful shutdown.
pub(crate) async fn serve_loopback_http1(
    listener: TcpListener,
    router: Router,
    max_connections: usize,
    header_read_timeout: Duration,
    mut shutdown_rx: watch::Receiver<()>,
) -> io::Result<()> {
    let mut connections = JoinSet::new();
    let permits = Arc::new(Semaphore::new(max_connections));

    loop {
        let permit = tokio::select! {
            _ = shutdown_rx.changed() => break,
            permit = Arc::clone(&permits).acquire_owned() => permit.expect("connection semaphore remains owned by the server"),
        };
        tokio::select! {
            _ = shutdown_rx.changed() => break,
            accepted = listener.accept() => {
                let (stream, peer) = accepted?;
                let router = router.clone();
                let connection_shutdown = shutdown_rx.clone();
                connections.spawn(async move {
                    let _permit = permit;
                    let service = router
                        .into_make_service_with_connect_info::<SocketAddr>()
                        .oneshot(peer)
                        .await
                        .expect("Axum's loopback make-service is infallible");
                    if let Err(error) = serve_connection(
                        TokioIo::new(stream),
                        service,
                        header_read_timeout,
                        connection_shutdown,
                    )
                    .await {
                        // A malformed or timed-out individual connection is
                        // not a listener failure. Hyper has already closed it;
                        // keep accepting healthy clients.
                        tracing::debug!(%error, peer = %peer, "HTTP/1 loopback connection ended");
                    }
                });
            }
        }
    }

    drain_connections(connections).await
}

/// Serves authenticated opaque peer streams through the exact canonical Axum
/// route used by every other runtime adapter. Authentication completes before
/// the router is constructed or HTTP bytes are decoded.
pub(crate) async fn serve_authenticated_peer_http1(
    listener: TcpListener,
    adapter: Arc<dyn PeerStreamAdapter>,
    handler: Arc<dyn CanonicalWriteHandler>,
    limits: RuntimeLimits,
    timeouts: RuntimeTimeouts,
    mut shutdown_rx: watch::Receiver<()>,
) -> io::Result<()> {
    let mut connections = JoinSet::new();
    let permits = Arc::new(Semaphore::new(limits.max_connections));

    loop {
        let permit = tokio::select! {
            _ = shutdown_rx.changed() => break,
            permit = Arc::clone(&permits).acquire_owned() => permit.expect("connection semaphore remains owned by the server"),
        };
        tokio::select! {
            _ = shutdown_rx.changed() => break,
            accepted = listener.accept() => {
                let (stream, peer) = accepted?;
                let adapter = Arc::clone(&adapter);
                let handler = Arc::clone(&handler);
                let connection_shutdown = shutdown_rx.clone();
                connections.spawn(async move {
                    let _permit = permit;
                    let accepted = match adapter.accept(stream).await {
                        Ok(accepted) => accepted,
                        Err(error) => {
                            tracing::debug!(%error, peer = %peer, "peer stream authentication failed before HTTP decode");
                            return;
                        }
                    };
                    let router = canonical_api_router(
                        handler,
                        AuthenticatedConnector::peer(accepted.source_host),
                        limits,
                        timeouts,
                    );
                    let service = router
                        .into_make_service_with_connect_info::<SocketAddr>()
                        .oneshot(peer)
                        .await
                        .expect("Axum's peer make-service is infallible");
                    if let Err(error) = serve_connection(
                        TokioIo::new(accepted.stream),
                        service,
                        timeouts.request,
                        connection_shutdown,
                    )
                    .await {
                        tracing::debug!(%error, peer = %peer, "authenticated peer HTTP/1 connection ended");
                    }
                });
            }
        }
    }

    drain_connections(connections).await
}

/// Serves the additive Unix adapter with the same HTTP protocol and deadline
/// policy as the loopback adapter.
#[cfg(unix)]
pub(crate) async fn serve_unix_http1(
    listener: UnixListener,
    router: Router,
    max_connections: usize,
    header_read_timeout: Duration,
    mut shutdown_rx: watch::Receiver<()>,
) -> io::Result<()> {
    let mut connections = JoinSet::new();
    let permits = Arc::new(Semaphore::new(max_connections));

    loop {
        let permit = tokio::select! {
            _ = shutdown_rx.changed() => break,
            permit = Arc::clone(&permits).acquire_owned() => permit.expect("connection semaphore remains owned by the server"),
        };
        tokio::select! {
            _ = shutdown_rx.changed() => break,
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let connection_shutdown = shutdown_rx.clone();
                let router = router.clone();
                connections.spawn(async move {
                    let _permit = permit;
                    if let Err(error) = serve_connection(
                        TokioIo::new(stream),
                        router,
                        header_read_timeout,
                        connection_shutdown,
                    )
                    .await {
                        tracing::debug!(%error, "HTTP/1 Unix connection ended");
                    }
                });
            }
        }
    }

    drain_connections(connections).await
}

pub(crate) async fn serve_connection<I, S>(
    io: TokioIo<I>,
    service: S,
    header_read_timeout: Duration,
    mut shutdown_rx: watch::Receiver<()>,
) -> io::Result<()>
where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    S: tower::Service<
            axum::http::Request<Body>,
            Response = axum::response::Response,
            Error = Infallible,
        > + Send
        + Clone
        + 'static,
    S::Future: Send + 'static,
{
    let service =
        service.map_request(|request: axum::http::Request<Incoming>| request.map(Body::new));
    let service = TowerToHyperService::new(service);
    let mut builder = http1::Builder::new();
    builder
        .timer(TokioTimer::new())
        .header_read_timeout(header_read_timeout)
        // Keep a bounded admitted connection available for its HTTP/1.1
        // request sequence. The connection semaphore still caps concurrent
        // peers, the header timer bounds idle/header waits, and callers send
        // `Connection: close` when their finite batch is complete.
        .keep_alive(true);
    let connection = builder.serve_connection(io, service);
    tokio::pin!(connection);

    tokio::select! {
        result = &mut connection => result.map_err(io::Error::other),
        _ = shutdown_rx.changed() => {
            connection.as_mut().graceful_shutdown();
            connection.await.map_err(io::Error::other)
        }
    }
}

async fn drain_connections(mut connections: JoinSet<()>) -> io::Result<()> {
    while let Some(result) = connections.join_next().await {
        result.map_err(io::Error::other)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::Router;
    use axum::http::StatusCode;
    use axum::routing::post;
    use hyper_util::rt::TokioIo;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::watch;

    use super::serve_connection;

    #[tokio::test]
    async fn http1_connection_serves_a_bounded_pipelined_request_batch() {
        let router = Router::new().route("/messages", post(|| async { StatusCode::CREATED }));
        let (mut client, server) = tokio::io::duplex(4096);
        let (_shutdown_tx, shutdown_rx) = watch::channel(());
        let server = tokio::spawn(serve_connection(
            TokioIo::new(server),
            router,
            Duration::from_secs(1),
            shutdown_rx,
        ));

        client
            .write_all(
                b"POST /messages HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: keep-alive\r\n\r\n\
                  POST /messages HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .await
            .expect("write pipelined HTTP requests");
        client.flush().await.expect("flush pipelined HTTP requests");

        let mut responses = Vec::new();
        tokio::time::timeout(Duration::from_secs(1), client.read_to_end(&mut responses))
            .await
            .expect("server closes after the bounded request batch")
            .expect("read pipelined HTTP responses");
        assert_eq!(
            responses
                .windows(b"HTTP/1.1 201 Created".len())
                .filter(|window| *window == b"HTTP/1.1 201 Created")
                .count(),
            2,
            "both pipelined requests must receive ordered responses on one connection"
        );
        server
            .await
            .expect("HTTP/1 task joins")
            .expect("HTTP/1 connection completes cleanly");
    }
}
