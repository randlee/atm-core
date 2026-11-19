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

/// Consecutive accept failures tolerated before a listener is declared dead.
///
/// Accept can fail for reasons that have nothing to do with the listener
/// socket: descriptor exhaustion (`EMFILE`/`ENFILE`), buffer pressure, or a
/// client that resets between the kernel completing the handshake and the
/// server reaping it. Returning such an error ends the listener task, and
/// `drain_server_group` turns the first finished listener into a shutdown of
/// every adapter -- tearing down connections whose writes have already
/// committed, so the client never sees the response for a durable row. Retry
/// instead, and give up only when a listener fails this many times in a row
/// without a single successful accept in between.
const MAX_CONSECUTIVE_ACCEPT_FAILURES: u32 = 64;

/// Upper bound on the cooperative pause between failed accept attempts.
const ACCEPT_RETRY_BACKOFF_CEILING: Duration = Duration::from_millis(50);

/// Listener abstraction so accept-failure recovery is exercised directly
/// instead of depending on process-wide descriptor exhaustion.
pub(crate) trait Http1Acceptor {
    type Stream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static;
    type Peer: Send + 'static;

    fn accept_connection(
        &self,
    ) -> impl std::future::Future<Output = io::Result<(Self::Stream, Self::Peer)>> + Send;
}

impl Http1Acceptor for TcpListener {
    type Stream = tokio::net::TcpStream;
    type Peer = SocketAddr;

    async fn accept_connection(&self) -> io::Result<(Self::Stream, Self::Peer)> {
        self.accept().await
    }
}

#[cfg(unix)]
impl Http1Acceptor for UnixListener {
    type Stream = tokio::net::UnixStream;
    type Peer = tokio::net::unix::SocketAddr;

    async fn accept_connection(&self) -> io::Result<(Self::Stream, Self::Peer)> {
        self.accept().await
    }
}

/// Backoff applied after `consecutive_failures` accept failures in a row.
fn accept_retry_backoff(consecutive_failures: u32) -> Duration {
    let shift = consecutive_failures.saturating_sub(1).min(6);
    Duration::from_millis(1u64 << shift).min(ACCEPT_RETRY_BACKOFF_CEILING)
}

/// Whether a listener that has now failed `consecutive_failures` times in a
/// row should keep accepting.
fn accept_failure_is_recoverable(consecutive_failures: u32) -> bool {
    consecutive_failures < MAX_CONSECUTIVE_ACCEPT_FAILURES
}

#[sc_lint_attributes::sc_lint(allow(SCB-RUNTIME-005), reason = "ADR-070: EMFILE accept backoff is cancelled by server shutdown.")]
async fn recover_from_accept_error(
    adapter: &'static str,
    error: io::Error,
    consecutive_failures: &mut u32,
) -> io::Result<()> {
    *consecutive_failures = consecutive_failures.saturating_add(1);
    if !accept_failure_is_recoverable(*consecutive_failures) {
        return Err(error);
    }
    tracing::warn!(
        %error,
        adapter,
        consecutive_failures = *consecutive_failures,
        "HTTP/1 accept failed; retrying without ending the listener"
    );
    tokio::time::sleep(accept_retry_backoff(*consecutive_failures)).await;
    Ok(())
}

/// Shared accept loop for every physical adapter: bounded concurrency,
/// cooperative shutdown, and accept-failure recovery in one place.
async fn accept_loop<L, F>(
    listener: L,
    max_connections: usize,
    mut shutdown_rx: watch::Receiver<()>,
    adapter: &'static str,
    mut on_connection: F,
) -> io::Result<()>
where
    L: Http1Acceptor,
    F: FnMut(
        &mut JoinSet<()>,
        L::Stream,
        L::Peer,
        watch::Receiver<()>,
        tokio::sync::OwnedSemaphorePermit,
    ),
{
    let mut connections = JoinSet::new();
    let permits = Arc::new(Semaphore::new(max_connections));
    let mut consecutive_accept_failures = 0u32;
    // Every physical adapter owns this signal for its active connections.
    let (connection_shutdown_tx, connection_shutdown_rx) = watch::channel(());
    let mut terminal_error = None;

    loop {
        let permit = tokio::select! {
            _ = shutdown_rx.changed() => break,
            permit = Arc::clone(&permits).acquire_owned() => permit.expect("connection semaphore remains owned by the server"),
        };
        tokio::select! {
            _ = shutdown_rx.changed() => break,
            accepted = listener.accept_connection() => {
                match accepted {
                    Ok((stream, peer)) => {
                        consecutive_accept_failures = 0;
                        let connection_shutdown = connection_shutdown_rx.clone();
                        on_connection(&mut connections, stream, peer, connection_shutdown, permit);
                    }
                    Err(error) => {
                        drop(permit);
                        let recovery = recover_from_accept_error(adapter, error, &mut consecutive_accept_failures);
                        tokio::select! {
                            _ = shutdown_rx.changed() => break,
                            result = recovery => if let Err(error) = result {
                                terminal_error = Some(error);
                                break;
                            },
                        }
                    }
                }
            }
        }
    }

    let _ = connection_shutdown_tx.send(());
    let drain_result = drain_connections(connections).await;
    if let Some(error) = terminal_error {
        if let Err(drain_error) = drain_result {
            tracing::warn!(%drain_error, adapter, "connection drain failed after accept error");
        }
        Err(error)
    } else {
        drain_result
    }
}

/// Serves the capability-authenticated loopback adapter with bounded
/// header-read time and cooperative graceful shutdown.
pub(crate) async fn serve_loopback_http1<L>(
    listener: L,
    router: Router,
    max_connections: usize,
    header_read_timeout: Duration,
    shutdown_rx: watch::Receiver<()>,
) -> io::Result<()>
where
    L: Http1Acceptor<Peer = SocketAddr>,
{
    accept_loop(
        listener,
        max_connections,
        shutdown_rx,
        "loopback",
        |connections, stream, peer, connection_shutdown, permit| {
            let router = router.clone();
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
                .await
                {
                    // A malformed or timed-out individual connection is
                    // not a listener failure. Hyper has already closed it;
                    // keep accepting healthy clients.
                    tracing::debug!(%error, peer = %peer, "HTTP/1 loopback connection ended");
                }
            });
        },
    )
    .await
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
    shutdown_rx: watch::Receiver<()>,
) -> io::Result<()> {
    accept_loop(
        listener,
        limits.max_connections,
        shutdown_rx,
        "peer",
        |connections, stream, peer, connection_shutdown, permit| {
            let adapter = Arc::clone(&adapter);
            let handler = Arc::clone(&handler);
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
                .await
                {
                    tracing::debug!(%error, peer = %peer, "authenticated peer HTTP/1 connection ended");
                }
            });
        },
    )
    .await
}

/// Serves the additive Unix adapter with the same HTTP protocol and deadline
/// policy as the loopback adapter.
#[sc_lint_attributes::sc_lint(allow(PORT-010), reason = "Unix serves UnixListener; Windows serve_loopback_http1 serves TCP (crates/atm-http-runtime/src/http1_server.rs).")]
#[cfg(unix)]
pub(crate) async fn serve_unix_http1(
    listener: UnixListener,
    router: Router,
    max_connections: usize,
    header_read_timeout: Duration,
    shutdown_rx: watch::Receiver<()>,
) -> io::Result<()> {
    accept_loop(
        listener,
        max_connections,
        shutdown_rx,
        "unix",
        |connections, stream, _peer, connection_shutdown, permit| {
            let router = router.clone();
            connections.spawn(async move {
                let _permit = permit;
                if let Err(error) = serve_connection(
                    TokioIo::new(stream),
                    router,
                    header_read_timeout,
                    connection_shutdown,
                )
                .await
                {
                    tracing::debug!(%error, "HTTP/1 Unix connection ended");
                }
            });
        },
    )
    .await
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
    let mut first_error = None;
    while let Some(result) = connections.join_next().await {
        if let Err(error) = result {
            first_error.get_or_insert_with(|| io::Error::other(error));
        }
    }
    first_error.map_or(Ok(()), Err)
}

#[cfg(test)]
pub(crate) struct ChannelAcceptor<S> {
    pub(crate) events: tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<io::Result<S>>>,
    pub(crate) observed: Option<std::sync::Arc<tokio::sync::Notify>>,
}

#[cfg(test)]
impl<S> Http1Acceptor for ChannelAcceptor<S>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    type Stream = S;
    type Peer = SocketAddr;

    async fn accept_connection(&self) -> io::Result<(Self::Stream, Self::Peer)> {
        let event = self
            .events
            .lock()
            .await
            .recv()
            .await
            .unwrap_or_else(|| Err(io::Error::other("script exhausted")));
        if let Some(observed) = &self.observed {
            observed.notify_one();
        }
        event.map(|stream| (stream, SocketAddr::from(([127, 0, 0, 1], 1234))))
    }
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

    use std::io;

    use super::{
        ChannelAcceptor, MAX_CONSECUTIVE_ACCEPT_FAILURES, accept_failure_is_recoverable,
        accept_retry_backoff, serve_connection, serve_loopback_http1,
    };

    struct ObservedRead {
        stream: tokio::io::DuplexStream,
        read: tokio::sync::mpsc::UnboundedSender<usize>,
    }

    impl tokio::io::AsyncRead for ObservedRead {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<io::Result<()>> {
            let before = buf.filled().len();
            let result = std::pin::Pin::new(&mut self.stream).poll_read(cx, buf);
            if matches!(result, std::task::Poll::Ready(Ok(()))) && buf.filled().len() > before {
                self.read.send(buf.filled().len() - before).unwrap();
            }
            result
        }
    }

    impl tokio::io::AsyncWrite for ObservedRead {
        fn poll_write(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> std::task::Poll<io::Result<usize>> {
            std::pin::Pin::new(&mut self.stream).poll_write(cx, buf)
        }
        fn poll_flush(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<io::Result<()>> {
            std::pin::Pin::new(&mut self.stream).poll_flush(cx)
        }
        fn poll_shutdown(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<io::Result<()>> {
            std::pin::Pin::new(&mut self.stream).poll_shutdown(cx)
        }
    }

    #[tokio::test(start_paused = true)]
    async fn loopback_header_read_deadline_closes_an_incomplete_request() {
        let (events_tx, events_rx) = tokio::sync::mpsc::unbounded_channel();
        let (read_tx, mut read_rx) = tokio::sync::mpsc::unbounded_channel();
        let (mut client, server) = tokio::io::duplex(1024);
        events_tx
            .send(Ok(ObservedRead {
                stream: server,
                read: read_tx,
            }))
            .unwrap();
        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let server = tokio::spawn(serve_loopback_http1(
            ChannelAcceptor {
                events: tokio::sync::Mutex::new(events_rx),
                observed: None,
            },
            Router::new(),
            8,
            Duration::from_millis(25),
            shutdown_rx,
        ));
        let start = tokio::time::Instant::now();
        let partial = b"POST /v1/messages HTTP/1.1\r\nHost: localhost\r\n";
        client.write_all(partial).await.unwrap();
        assert_eq!(read_rx.recv().await, Some(partial.len()));
        tokio::time::advance(Duration::from_millis(25)).await;
        let mut response = [0_u8; 1];
        assert_eq!(
            client.read(&mut response).await.unwrap(),
            0,
            "slow header connection must be closed"
        );
        assert_eq!(
            tokio::time::Instant::now(),
            start + Duration::from_millis(25)
        );
        shutdown_tx.send(()).unwrap();
        server.await.unwrap().unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn shutdown_cancels_accept_backoff_without_advancing_clock() {
        let (events_tx, events_rx) =
            tokio::sync::mpsc::unbounded_channel::<io::Result<tokio::io::DuplexStream>>();
        events_tx
            .send(Err(io::Error::other("resource pressure")))
            .unwrap();
        let observed = std::sync::Arc::new(tokio::sync::Notify::new());
        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let start = tokio::time::Instant::now();
        let server = tokio::spawn(super::accept_loop(
            ChannelAcceptor {
                events: tokio::sync::Mutex::new(events_rx),
                observed: Some(observed.clone()),
            },
            1,
            shutdown_rx,
            "test",
            |_, _, _, _, _| panic!("no connection expected"),
        ));
        observed.notified().await;
        shutdown_tx.send(()).expect("signal shutdown");
        server
            .await
            .expect("server task joins")
            .expect("clean shutdown");
        assert_eq!(tokio::time::Instant::now(), start);
    }

    #[tokio::test(start_paused = true)]
    async fn fatal_accept_error_notifies_and_drains_active_connection() {
        let (events_tx, events_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_client, server) = tokio::io::duplex(64);
        events_tx.send(Ok(server)).expect("queue connection");
        for _ in 0..MAX_CONSECUTIVE_ACCEPT_FAILURES {
            events_tx
                .send(Err(io::Error::other("scripted accept failure")))
                .expect("queue error");
        }
        let (_shutdown_tx, shutdown_rx) = watch::channel(());
        let (drained_tx, drained_rx) = tokio::sync::oneshot::channel();
        let mut drained_tx = Some(drained_tx);
        let error = super::accept_loop(
            ChannelAcceptor {
                events: tokio::sync::Mutex::new(events_rx),
                observed: None,
            },
            2,
            shutdown_rx,
            "test",
            move |connections, _stream, _peer, mut shutdown, permit| {
                let drained = drained_tx.take().expect("one connection");
                connections.spawn(async move {
                    let _permit = permit;
                    shutdown
                        .changed()
                        .await
                        .expect("connection shutdown signal");
                    drained.send(()).expect("report drained");
                });
            },
        )
        .await
        .expect_err("fatal accept must return error");
        assert_eq!(error.to_string(), "scripted accept failure");
        drained_rx.await.expect("connection drained before return");
    }

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
        client
            .read_to_end(&mut responses)
            .await
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

    #[tokio::test(start_paused = true)]
    async fn resource_pressure_accept_failures_do_not_end_the_listener() {
        let started = tokio::time::Instant::now();
        let (events_tx, events_rx) = tokio::sync::mpsc::unbounded_channel();
        for _ in 0..3 {
            events_tx
                .send(Err(io::Error::other("resource pressure")))
                .unwrap();
        }
        let (mut client, server_stream) = tokio::io::duplex(4096);
        events_tx.send(Ok(server_stream)).unwrap();
        let acceptor = ChannelAcceptor {
            events: tokio::sync::Mutex::new(events_rx),
            observed: None,
        };
        let router = Router::new().route("/messages", post(|| async { StatusCode::CREATED }));
        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let server = tokio::spawn(serve_loopback_http1(
            acceptor,
            router,
            8,
            Duration::from_secs(5),
            shutdown_rx,
        ));
        client.write_all(
            b"POST /messages HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        ).await.expect("write request");
        let mut response = Vec::new();
        client
            .read_to_end(&mut response)
            .await
            .expect("read response");
        assert_eq!(
            tokio::time::Instant::now() - started,
            Duration::from_millis(7)
        );
        assert!(
            response.starts_with(b"HTTP/1.1 201 Created"),
            "a listener that hit transient accept failures must still answer healthy clients"
        );
        shutdown_tx.send(()).expect("signal shutdown");
        server
            .await
            .expect("listener task joins")
            .expect("listener survives transient accept failures");
    }

    #[test]
    fn accept_failures_are_recoverable_until_the_documented_ceiling() {
        assert!(accept_failure_is_recoverable(1));
        assert!(accept_failure_is_recoverable(
            MAX_CONSECUTIVE_ACCEPT_FAILURES - 1
        ));
        assert!(
            !accept_failure_is_recoverable(MAX_CONSECUTIVE_ACCEPT_FAILURES),
            "a listener failing without a single success in between is declared dead"
        );
    }

    #[test]
    fn accept_retry_backoff_is_bounded() {
        assert_eq!(accept_retry_backoff(1), Duration::from_millis(1));
        assert!(accept_retry_backoff(1) <= accept_retry_backoff(4));
        for failures in 1..=MAX_CONSECUTIVE_ACCEPT_FAILURES {
            assert!(
                accept_retry_backoff(failures) <= super::ACCEPT_RETRY_BACKOFF_CEILING,
                "accept backoff must stay bounded so recovery never stalls the adapter"
            );
        }
    }
}
