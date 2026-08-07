//! Tokio/Hyper connection serving for the owned Axum router.
//!
//! Axum supplies routing, extraction, and response handling. Hyper owns HTTP/1
//! parsing and its header-read deadline; this module owns only listener
//! lifecycle and graceful shutdown for the runtime's physical adapters.

use std::convert::Infallible;
use std::io;
use std::net::SocketAddr;
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
use tokio::sync::watch;
use tokio::task::JoinSet;
use tower::ServiceExt;

/// Serves the capability-authenticated loopback adapter with bounded
/// header-read time and cooperative graceful shutdown.
pub(crate) async fn serve_loopback_http1(
    listener: TcpListener,
    router: Router,
    header_read_timeout: Duration,
    mut shutdown_rx: watch::Receiver<()>,
) -> io::Result<()> {
    let mut connections = JoinSet::new();

    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => break,
            accepted = listener.accept() => {
                let (stream, peer) = accepted?;
                let router = router.clone();
                let connection_shutdown = shutdown_rx.clone();
                connections.spawn(async move {
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

/// Serves the additive Unix adapter with the same HTTP protocol and deadline
/// policy as the loopback adapter.
#[cfg(unix)]
pub(crate) async fn serve_unix_http1(
    listener: UnixListener,
    router: Router,
    header_read_timeout: Duration,
    mut shutdown_rx: watch::Receiver<()>,
) -> io::Result<()> {
    let mut connections = JoinSet::new();

    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => break,
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let connection_shutdown = shutdown_rx.clone();
                let router = router.clone();
                connections.spawn(async move {
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

async fn serve_connection<I, S>(
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
        .keep_alive(false);
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
