//! Minimal configured peer HTTP transport.
//!
//! AK.4 keeps outbound peer delivery deliberately synchronous: the origin
//! request worker writes its already-persisted immutable frame once, then
//! either receives the matching ordinary send response or returns the
//! persisted-but-undelivered error.  This module has no queue, retry state,
//! worker, resolver, or connection lifecycle.

use std::collections::HashSet;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use atm_core::api::{
    ApiRequest, ApiRouter, AuthenticatedIngress, RequestDeadline, decode_request,
    read_http_response_with_frame_reader, write_http_request_with_headers_and_connection,
};
use atm_core::error::AtmError;
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope, SendResponseEnvelope};
use atm_core::send::WriteRequest;
use atm_storage::{HostName, PeerEndpoint};

use crate::active_connection_registry::{
    ActiveConnectionGuard, ActiveConnectionRegistry, TrackedDispatchHandle,
};
use crate::daemon_worker_join::CompletionTrackedJoinHandle;

pub(crate) const PEER_HTTP_LOCAL_RESPONSE_BUDGET: Duration = Duration::from_secs(3);
pub(crate) const PLAINTEXT_PEER_SOURCE_HOST_HEADER: &str = "X-ATM-Peer-Source-Host";
pub(crate) const MAX_PEER_HTTP_CONNECTIONS: usize = 64;
const PEER_HTTP_ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(1);

/// Immutable local configuration captured by daemon assembly/reload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PeerHttpRuntimeConfig {
    pub(crate) source_host: HostName,
}

/// Finite configured peer-listener bind allowlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PeerHttpBindConfig {
    pub(crate) bind_addrs: Vec<SocketAddr>,
}

impl PeerHttpBindConfig {
    pub(crate) fn validate_at_startup(&self) -> Result<(), AtmError> {
        if self.bind_addrs.is_empty() {
            return Err(AtmError::bind_preflight(
                "configured peer HTTP listener requires at least one explicit bind address",
            ));
        }
        let mut seen = HashSet::with_capacity(self.bind_addrs.len());
        for bind_addr in &self.bind_addrs {
            if bind_addr.ip().is_unspecified() {
                return Err(AtmError::bind_preflight(format!(
                    "configured peer HTTP bind address `{bind_addr}` must not be unspecified"
                )));
            }
            if bind_addr.ip().is_multicast() {
                return Err(AtmError::bind_preflight(format!(
                    "configured peer HTTP bind address `{bind_addr}` must not be multicast"
                )));
            }
            if !seen.insert(*bind_addr) {
                return Err(AtmError::bind_preflight(format!(
                    "configured peer HTTP bind address `{bind_addr}` is duplicated"
                )));
            }
            // This asks the operating system whether the address is local
            // without DNS or interface discovery. The temporary socket is
            // dropped before the production listener binds it.
            TcpListener::bind(bind_addr).map_err(|source| {
                AtmError::bind_preflight(format!(
                    "configured peer HTTP bind address `{bind_addr}` is not available on this host: {source}"
                ))
            })?;
        }
        Ok(())
    }
}

/// One accepted peer socket with a reservation in the shared bounded registry.
pub(crate) struct PeerConnectionAdmission {
    _active_connection: ActiveConnectionGuard,
}

/// One explicit production peer listener.
pub(crate) struct PeerHttpListener {
    listener: TcpListener,
}

/// Lifecycle owner for configured plain-HTTP peer listeners.
pub(crate) struct PeerHttpListenerSet {
    listeners: Vec<PeerHttpListener>,
    stop: Arc<AtomicBool>,
    registry: Arc<ActiveConnectionRegistry>,
    accept_threads: Vec<JoinHandle<()>>,
}

impl PeerHttpListenerSet {
    pub(crate) fn bind(config: &PeerHttpBindConfig) -> Result<Self, AtmError> {
        config.validate_at_startup()?;
        let listeners = config
            .bind_addrs
            .iter()
            .map(|bind_addr| {
                let listener = TcpListener::bind(bind_addr).map_err(|source| {
                    AtmError::daemon_unavailable(format!(
                        "failed to bind configured peer HTTP listener `{bind_addr}`: {source}"
                    ))
                })?;
                listener.set_nonblocking(true).map_err(|source| {
                    AtmError::daemon_unavailable(format!(
                        "failed to configure configured peer HTTP listener `{bind_addr}`: {source}"
                    ))
                })?;
                Ok(PeerHttpListener { listener })
            })
            .collect::<Result<Vec<_>, AtmError>>()?;
        Ok(Self {
            listeners,
            stop: Arc::new(AtomicBool::new(false)),
            registry: Arc::new(ActiveConnectionRegistry::default()),
            accept_threads: Vec::new(),
        })
    }

    pub(crate) fn start(
        &mut self,
        router: Arc<dyn ApiRouter + Send + Sync>,
    ) -> Result<(), AtmError> {
        let listeners = self
            .listeners
            .iter()
            .map(|peer_listener| {
                let listener = peer_listener.listener.try_clone().map_err(|source| {
                    AtmError::daemon_unavailable(format!(
                        "failed to clone configured peer HTTP listener: {source}"
                    ))
                })?;
                listener.set_nonblocking(true).map_err(|source| {
                    AtmError::daemon_unavailable(format!(
                        "failed to configure cloned peer HTTP listener mode: {source}"
                    ))
                })?;
                Ok(listener)
            })
            .collect::<Result<Vec<_>, AtmError>>()?;
        for listener in listeners {
            let stop = Arc::clone(&self.stop);
            let registry = Arc::clone(&self.registry);
            let router = Arc::clone(&router);
            let accept_thread = match thread::Builder::new()
                .name("peer-http-accept".to_owned())
                .spawn(move || accept_peer_connections(listener, router, registry, stop))
            {
                Ok(handle) => handle,
                Err(source) => {
                    self.shutdown()?;
                    return Err(AtmError::daemon_unavailable(format!(
                        "failed to start configured peer HTTP accept thread: {source}"
                    )));
                }
            };
            self.accept_threads.push(accept_thread);
        }
        Ok(())
    }

    pub(crate) fn shutdown(&mut self) -> Result<(), AtmError> {
        self.stop.store(true, Ordering::SeqCst);
        for accept_thread in self.accept_threads.drain(..) {
            accept_thread.join().map_err(|_| {
                AtmError::daemon_unavailable("peer HTTP accept thread panicked during shutdown")
            })?;
        }
        self.registry
            .join_tracked_dispatches(PEER_HTTP_LOCAL_RESPONSE_BUDGET)
    }
}

fn accept_peer_connections(
    listener: TcpListener,
    router: Arc<dyn ApiRouter + Send + Sync>,
    registry: Arc<ActiveConnectionRegistry>,
    stop: Arc<AtomicBool>,
) {
    while !stop.load(Ordering::SeqCst) {
        if let Err(error) = registry.reap_finished_dispatches() {
            tracing::warn!(
                subsystem = "peer_http_listener",
                action = "reap_request_workers",
                outcome = "failed",
                %error,
                "configured peer HTTP request-worker cleanup failed"
            );
        }
        match listener.accept() {
            Ok((stream, _peer_addr)) => {
                let Some(active_connection) = registry.try_register(MAX_PEER_HTTP_CONNECTIONS)
                else {
                    if let Err(error) = write_peer_capacity_rejection(stream) {
                        tracing::warn!(
                            subsystem = "peer_http_listener",
                            action = "reject_over_capacity_connection",
                            outcome = "failed",
                            %error,
                            "configured peer HTTP capacity rejection could not be written"
                        );
                    }
                    continue;
                };
                let admission = PeerConnectionAdmission {
                    _active_connection: active_connection,
                };
                if let Err(error) =
                    spawn_request_worker(stream, router.clone(), registry.clone(), admission)
                {
                    tracing::warn!(
                        subsystem = "peer_http_listener",
                        action = "spawn_request_worker",
                        outcome = "failed",
                        %error,
                        "configured peer HTTP request worker could not be started"
                    );
                }
            }
            Err(source) if source.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(PEER_HTTP_ACCEPT_POLL_INTERVAL);
            }
            Err(source) => {
                tracing::warn!(
                    subsystem = "peer_http_listener",
                    action = "accept",
                    outcome = "failed",
                    %source,
                    "configured peer HTTP accept failed"
                );
                thread::sleep(PEER_HTTP_ACCEPT_POLL_INTERVAL);
            }
        }
    }
}

fn spawn_request_worker(
    stream: TcpStream,
    router: Arc<dyn ApiRouter + Send + Sync>,
    registry: Arc<ActiveConnectionRegistry>,
    admission: PeerConnectionAdmission,
) -> Result<(), AtmError> {
    let (completion_tx, completion_rx) = std::sync::mpsc::sync_channel(1);
    let join_handle = thread::Builder::new()
        .name("peer-http-request".to_owned())
        .spawn(move || {
            let _completion_tx = completion_tx;
            let _admission = admission;
            if let Err(error) = handle_peer_connection(stream, router) {
                tracing::debug!(
                    subsystem = "peer_http_listener",
                    action = "request",
                    outcome = "closed",
                    %error,
                    "peer HTTP request connection ended"
                );
            }
        })
        .map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to start configured peer HTTP request worker: {source}"
            ))
        })?;
    track_request_worker(
        registry.as_ref(),
        CompletionTrackedJoinHandle {
            completion_rx,
            join_handle,
        },
    )
}

fn track_request_worker(
    registry: &ActiveConnectionRegistry,
    handle: TrackedDispatchHandle,
) -> Result<(), AtmError> {
    registry.push_dispatch_handle(handle, MAX_PEER_HTTP_CONNECTIONS)
}

fn handle_peer_connection(
    mut stream: TcpStream,
    router: Arc<dyn ApiRouter + Send + Sync>,
) -> Result<(), AtmError> {
    // Accepted sockets can inherit the nonblocking listener mode on macOS.
    // Request framing is deliberately blocking under the explicit deadline.
    stream.set_nonblocking(false).map_err(|source| {
        AtmError::daemon_unavailable(format!(
            "failed to configure peer HTTP connection mode: {source}"
        ))
    })?;
    // Peer delivery is a request/response exchange with small immutable
    // frames. Keeping Nagle enabled turns a bounded multi-frame resend into
    // delayed-ACK round trips on Windows; this socket is never a bulk stream.
    stream.set_nodelay(true).map_err(|source| {
        AtmError::daemon_unavailable(format!(
            "failed to disable Nagle buffering for peer HTTP connection: {source}"
        ))
    })?;
    stream
        .set_read_timeout(Some(PEER_HTTP_LOCAL_RESPONSE_BUDGET))
        .map_err(|source| {
            AtmError::daemon_unavailable(format!("failed to set peer HTTP read deadline: {source}"))
        })?;
    stream
        .set_write_timeout(Some(PEER_HTTP_LOCAL_RESPONSE_BUDGET))
        .map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to set peer HTTP write deadline: {source}"
            ))
        })?;
    let mut frames = atm_core::api::HttpFrameReader::new();
    for request_count in 1..=crate::MAX_KEEP_ALIVE_REQUESTS {
        let Some(raw_request) = frames.read_request(&mut stream)? else {
            return Ok(());
        };
        let keep_alive = raw_request
            .header("connection")
            .is_some_and(|value| value.eq_ignore_ascii_case("keep-alive"))
            && request_count < crate::MAX_KEEP_ALIVE_REQUESTS;
        let response = route_peer_http_request(router.as_ref(), raw_request);
        atm_core::api::write_local_http_response(&mut stream, &response, keep_alive)?;
        if !keep_alive {
            return Ok(());
        }
    }
    Ok(())
}

fn route_peer_http_request(
    router: &dyn ApiRouter,
    raw_request: atm_core::api::HttpRequest,
) -> ResponseEnvelope {
    let source_host = raw_request
        .header(PLAINTEXT_PEER_SOURCE_HOST_HEADER)
        .ok_or_else(|| AtmError::validation("peer HTTP write is missing X-ATM-Peer-Source-Host"))
        .and_then(|value| {
            value.parse::<HostName>().map_err(|error| {
                AtmError::validation(format!("peer HTTP source host is invalid: {error}"))
            })
        });
    let response = source_host.and_then(|source_host| {
        let mut request = decode_request(raw_request)?;
        match &mut request {
            ApiRequest::Write(write) => {
                write.authenticated_source_host = Some(source_host);
                router
                    .route(
                        request,
                        AuthenticatedIngress::Peer,
                        RequestDeadline::after(PEER_HTTP_LOCAL_RESPONSE_BUDGET),
                    )
                    .map(|response| response.into_inner())
            }
            _ => Err(AtmError::validation(
                "configured peer HTTP listener accepts write requests only",
            )),
        }
    });
    response.unwrap_or_else(ResponseEnvelope::Error)
}

fn write_peer_capacity_rejection(mut stream: TcpStream) -> Result<(), AtmError> {
    atm_core::api::write_local_http_response(
        &mut stream,
        &ResponseEnvelope::Error(AtmError::daemon_unavailable(
            "configured peer HTTP listener is at its concurrent connection capacity",
        )),
        false,
    )
}

/// Sends one finite ordered slice through one operating-system-resolved TCP
/// connection.  It is intentionally the sole production peer sender so the
/// immediate and future batch paths cannot diverge.
pub(crate) fn send_peer_http_frames(
    config: &PeerHttpRuntimeConfig,
    endpoint: &PeerEndpoint,
    writes: &[WriteRequest],
    deadline: RequestDeadline,
) -> Result<Vec<ResponseEnvelope>, AtmError> {
    if writes.is_empty() {
        return Ok(Vec::new());
    }

    let mut stream = TcpStream::connect((endpoint.canonical_host.as_str(), endpoint.port.get()))
        .map_err(|source| peer_delivery_failure("connect to configured peer", source))?;
    stream.set_nodelay(true).map_err(|source| {
        peer_delivery_failure("disable Nagle buffering for configured peer", source)
    })?;
    let mut frames = atm_core::api::HttpFrameReader::new();
    let mut responses = Vec::with_capacity(writes.len());

    for (index, write) in writes.iter().enumerate() {
        let remaining = peer_response_budget(deadline)?;
        stream.set_read_timeout(Some(remaining)).map_err(|source| {
            peer_delivery_failure("configure configured-peer response timeout", source)
        })?;
        stream
            .set_write_timeout(Some(remaining))
            .map_err(|source| {
                peer_delivery_failure("configure configured-peer write timeout", source)
            })?;

        let request = RequestEnvelope::Write(Box::new(write.clone()));
        write_http_request_with_headers_and_connection(
            &mut stream,
            &request,
            &[(
                PLAINTEXT_PEER_SOURCE_HOST_HEADER,
                config.source_host.as_str(),
            )],
            index + 1 < writes.len(),
        )
        .map_err(|error| {
            AtmError::remote_delivery_unconfirmed(format!(
                "local persistence succeeded but write to configured peer `{}` failed: {error}",
                endpoint.canonical_host
            ))
        })?;

        let response = read_http_response_with_frame_reader(&mut frames, &mut stream, &request)
            .map_err(|error| {
                AtmError::remote_delivery_unconfirmed(format!(
                    "local persistence succeeded but configured peer `{}` did not return a valid response: {error}",
                    endpoint.canonical_host
                ))
            })?;
        ensure_matching_send_response(&response, write, endpoint)?;
        responses.push(response);
    }
    Ok(responses)
}

fn peer_response_budget(deadline: RequestDeadline) -> Result<Duration, AtmError> {
    deadline
        .remaining()
        .map(|remaining| remaining.min(PEER_HTTP_LOCAL_RESPONSE_BUDGET))
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| {
            AtmError::remote_delivery_unconfirmed(
                "local persistence succeeded but the peer response deadline expired before delivery completed",
            )
        })
}

fn ensure_matching_send_response(
    response: &ResponseEnvelope,
    write: &WriteRequest,
    endpoint: &PeerEndpoint,
) -> Result<(), AtmError> {
    let expected_message_id = write.origin_message_id.ok_or_else(|| {
        AtmError::remote_delivery_unconfirmed(
            "local persistence succeeded but peer delivery write had no immutable origin message ID",
        )
    })?;
    let actual_message_id = match response {
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => outcome.message_id,
        ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(outcome)) => outcome.message_id,
        ResponseEnvelope::Error(error) => {
            return Err(AtmError::remote_delivery_unconfirmed(format!(
                "local persistence succeeded but configured peer `{}` rejected the write: {error}",
                endpoint.canonical_host
            )));
        }
        response => {
            return Err(AtmError::remote_delivery_unconfirmed(format!(
                "local persistence succeeded but configured peer `{}` returned unexpected response `{response:?}`",
                endpoint.canonical_host
            )));
        }
    };
    if actual_message_id != expected_message_id {
        return Err(AtmError::remote_delivery_unconfirmed(format!(
            "local persistence succeeded but configured peer `{}` confirmed message `{actual_message_id}` instead of `{expected_message_id}`",
            endpoint.canonical_host
        )));
    }
    Ok(())
}

fn peer_delivery_failure(action: &str, source: std::io::Error) -> AtmError {
    AtmError::remote_delivery_unconfirmed(format!(
        "local persistence succeeded but failed to {action}: {source}"
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        PEER_HTTP_LOCAL_RESPONSE_BUDGET, PLAINTEXT_PEER_SOURCE_HOST_HEADER, PeerHttpBindConfig,
        PeerHttpListenerSet, PeerHttpRuntimeConfig, send_peer_http_frames,
    };
    use std::io::Write;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
    use std::num::NonZeroU16;
    use std::sync::{Arc, Mutex, mpsc};
    use std::thread;
    use std::time::Duration;

    use atm_core::api::{
        ApiRequest, ApiResponse, ApiRouter, AuthenticatedIngress, HttpFrameReader, RequestDeadline,
        decode_request, write_http_response,
    };
    use atm_core::error::AtmError;
    use atm_core::error_codes::AtmErrorCode;
    use atm_core::protocol::{RequestEnvelope, ResponseEnvelope, SendResponseEnvelope};
    use atm_core::send::{SendCommandOutcome, SendMessageSource, SendOutcome, WriteRequest};
    use atm_core::types::{AgentName, CommandAction, TeamName};
    use atm_storage::{AtmMessageId, HostName, PeerEndpoint};

    #[derive(Default)]
    struct PeerWriteRecorder {
        source_hosts: Mutex<Vec<HostName>>,
        message_ids: Mutex<Vec<AtmMessageId>>,
    }

    impl atm_core::boundary::sealed::Sealed for PeerWriteRecorder {}

    impl ApiRouter for PeerWriteRecorder {
        fn route(
            &self,
            request: ApiRequest,
            ingress: AuthenticatedIngress,
            _deadline: RequestDeadline,
        ) -> Result<ApiResponse, AtmError> {
            assert_eq!(ingress, AuthenticatedIngress::Peer);
            let ApiRequest::Write(write) = request else {
                return Err(AtmError::validation("peer listener must receive a write"));
            };
            let source_host = write.authenticated_source_host.clone().ok_or_else(|| {
                AtmError::validation("peer listener did not attach configured source host")
            })?;
            self.source_hosts
                .lock()
                .expect("source host recorder")
                .push(source_host);
            let message_id = write.origin_message_id.ok_or_else(|| {
                AtmError::validation("peer listener did not preserve origin message ID")
            })?;
            self.message_ids
                .lock()
                .expect("message ID recorder")
                .push(message_id);
            Ok(ApiResponse::new(ResponseEnvelope::Send(
                SendResponseEnvelope::Sent(send_outcome(message_id)),
            )))
        }
    }

    fn team() -> TeamName {
        "peer-test".parse().expect("team")
    }

    fn agent() -> AgentName {
        "sender".parse().expect("agent")
    }

    fn write(message_id: AtmMessageId) -> WriteRequest {
        WriteRequest::new(
            std::path::PathBuf::from("/tmp/atm-peer-test"),
            std::path::PathBuf::from("/tmp/atm-peer-test"),
            agent(),
            "receiver@peer-test.127.0.0.1",
            team(),
            SendMessageSource::Inline("peer frame".to_owned()),
            None,
            false,
            None,
            false,
        )
        .expect("write")
        .with_origin_message_id(message_id)
    }

    fn send_outcome(message_id: AtmMessageId) -> SendOutcome {
        SendOutcome {
            action: CommandAction::Send,
            team: team(),
            agent: "receiver".parse().expect("receiver"),
            sender: agent(),
            outcome: SendCommandOutcome::Sent,
            message_id,
            requires_ack: false,
            task_id: None,
            summary: None,
            message: None,
            warnings: Vec::new(),
            dry_run: false,
        }
    }

    fn loopback_listener() -> TcpListener {
        TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).expect("listener")
    }

    fn peer_endpoint(port: u16) -> PeerEndpoint {
        PeerEndpoint {
            canonical_host: "localhost".parse().expect("host"),
            port: NonZeroU16::new(port).expect("port"),
        }
    }

    fn peer_runtime_config() -> PeerHttpRuntimeConfig {
        PeerHttpRuntimeConfig {
            source_host: "origin.example.test".parse().expect("source host"),
        }
    }

    fn read_one_peer_request(stream: &mut TcpStream) {
        let _ = HttpFrameReader::new()
            .read_request(stream)
            .expect("read request")
            .expect("one request");
    }

    fn assert_delivery_unconfirmed(error: AtmError) {
        assert_eq!(error.code(), AtmErrorCode::RemoteDeliveryUnconfirmed);
    }

    #[test]
    fn direct_sender_uses_shared_http_frame_and_configured_source_header() {
        let listener = loopback_listener();
        let port = listener.local_addr().expect("address").port();
        let message_id = AtmMessageId::new();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let raw_request = HttpFrameReader::new()
                .read_request(&mut stream)
                .expect("read request")
                .expect("one request");
            assert_eq!(
                raw_request.header(PLAINTEXT_PEER_SOURCE_HOST_HEADER),
                Some("origin.example.test")
            );
            let request = decode_request(raw_request).expect("decode request");
            let RequestEnvelope::Write(write) = request.into_inner() else {
                panic!("peer sender must emit a write request");
            };
            assert_eq!(write.origin_message_id, Some(message_id));
            let response =
                ResponseEnvelope::Send(SendResponseEnvelope::Sent(send_outcome(message_id)));
            let mut encoded_response = Vec::new();
            write_http_response(&mut encoded_response, &response).expect("encode response");
            let split_at = encoded_response.len() / 2;
            stream
                .write_all(&encoded_response[..split_at])
                .expect("write split response prefix");
            stream.flush().expect("flush response");
            stream
                .write_all(&encoded_response[split_at..])
                .expect("write split response suffix");
            stream.flush().expect("flush response");
        });

        let endpoint = peer_endpoint(port);
        let response = send_peer_http_frames(
            &peer_runtime_config(),
            &endpoint,
            &[write(message_id)],
            RequestDeadline::after(PEER_HTTP_LOCAL_RESPONSE_BUDGET),
        )
        .expect("direct send succeeds");
        assert!(matches!(response.as_slice(), [ResponseEnvelope::Send(_)]));
        server.join().expect("server");
    }

    #[test]
    fn direct_sender_rejects_a_response_for_a_different_immutable_write() {
        let listener = loopback_listener();
        let port = listener.local_addr().expect("address").port();
        let message_id = AtmMessageId::new();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let _ = HttpFrameReader::new()
                .read_request(&mut stream)
                .expect("read request");
            write_http_response(
                &mut stream,
                &ResponseEnvelope::Send(SendResponseEnvelope::Sent(send_outcome(
                    AtmMessageId::new(),
                ))),
            )
            .expect("write mismatched response");
        });
        let endpoint = peer_endpoint(port);

        let error = send_peer_http_frames(
            &peer_runtime_config(),
            &endpoint,
            &[write(message_id)],
            RequestDeadline::after(PEER_HTTP_LOCAL_RESPONSE_BUDGET),
        )
        .expect_err("a response for a different write cannot confirm delivery");
        assert_delivery_unconfirmed(error);
        server.join().expect("server");
    }

    #[test]
    fn direct_sender_rejects_budget_expiry_under_stall() {
        let listener = loopback_listener();
        let port = listener.local_addr().expect("address").port();
        let (request_seen_tx, request_seen_rx) = mpsc::channel();
        let (release_stall_tx, release_stall_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            read_one_peer_request(&mut stream);
            request_seen_tx.send(()).expect("report request");
            release_stall_rx.recv().expect("release stalled peer");
        });

        let error = send_peer_http_frames(
            &peer_runtime_config(),
            &peer_endpoint(port),
            &[write(AtmMessageId::new())],
            RequestDeadline::after(Duration::from_millis(10)),
        )
        .expect_err("a stalled peer cannot outlive the caller deadline");
        assert_delivery_unconfirmed(error);
        request_seen_rx.recv().expect("peer received request");
        release_stall_tx.send(()).expect("release stalled peer");
        server.join().expect("server");
    }

    #[test]
    fn direct_sender_rejects_dns_failure() {
        let endpoint = PeerEndpoint {
            canonical_host: "peer-does-not-exist.invalid".parse().expect("host"),
            port: NonZeroU16::new(43101).expect("port"),
        };
        let error = send_peer_http_frames(
            &peer_runtime_config(),
            &endpoint,
            &[write(AtmMessageId::new())],
            RequestDeadline::after(PEER_HTTP_LOCAL_RESPONSE_BUDGET),
        )
        .expect_err("an unresolvable peer host cannot confirm delivery");
        assert_delivery_unconfirmed(error);
    }

    #[test]
    fn direct_sender_rejects_connect_refusal() {
        let port = loopback_listener().local_addr().expect("address").port();
        let error = send_peer_http_frames(
            &peer_runtime_config(),
            &peer_endpoint(port),
            &[write(AtmMessageId::new())],
            RequestDeadline::after(PEER_HTTP_LOCAL_RESPONSE_BUDGET),
        )
        .expect_err("a refused peer connection cannot confirm delivery");
        assert_delivery_unconfirmed(error);
    }

    #[test]
    fn direct_sender_rejects_receiver_4xx_and_5xx_responses() {
        for response in [
            ResponseEnvelope::Error(AtmError::validation("receiver rejected write")),
            ResponseEnvelope::Error(AtmError::daemon_unavailable("receiver unavailable")),
        ] {
            let listener = loopback_listener();
            let port = listener.local_addr().expect("address").port();
            let server = thread::spawn(move || {
                let (mut stream, _) = listener.accept().expect("accept");
                read_one_peer_request(&mut stream);
                write_http_response(&mut stream, &response).expect("write error response");
            });

            let error = send_peer_http_frames(
                &peer_runtime_config(),
                &peer_endpoint(port),
                &[write(AtmMessageId::new())],
                RequestDeadline::after(PEER_HTTP_LOCAL_RESPONSE_BUDGET),
            )
            .expect_err("a non-success peer response cannot confirm delivery");
            assert_delivery_unconfirmed(error);
            server.join().expect("server");
        }
    }

    #[test]
    fn direct_sender_rejects_malformed_response() {
        let listener = loopback_listener();
        let port = listener.local_addr().expect("address").port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            read_one_peer_request(&mut stream);
            stream
                .write_all(b"HTTP/1.1 201 Created\r\nContent-Length: invalid\r\n\r\n")
                .expect("write malformed response");
            stream.flush().expect("flush malformed response");
        });

        let error = send_peer_http_frames(
            &peer_runtime_config(),
            &peer_endpoint(port),
            &[write(AtmMessageId::new())],
            RequestDeadline::after(PEER_HTTP_LOCAL_RESPONSE_BUDGET),
        )
        .expect_err("a malformed peer response cannot confirm delivery");
        assert_delivery_unconfirmed(error);
        server.join().expect("server");
    }

    #[test]
    fn direct_sender_rejects_mismatched_response_count() {
        let listener = loopback_listener();
        let port = listener.local_addr().expect("address").port();
        let first_message_id = AtmMessageId::new();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let raw_request = HttpFrameReader::new()
                .read_request(&mut stream)
                .expect("read first request")
                .expect("first request");
            let RequestEnvelope::Write(write) = decode_request(raw_request)
                .expect("decode first request")
                .into_inner()
            else {
                panic!("peer sender must emit a write request");
            };
            let message_id = write.origin_message_id.expect("immutable message ID");
            write_http_response(
                &mut stream,
                &ResponseEnvelope::Send(SendResponseEnvelope::Sent(send_outcome(message_id))),
            )
            .expect("write first response");
        });

        let error = send_peer_http_frames(
            &peer_runtime_config(),
            &peer_endpoint(port),
            &[write(first_message_id), write(AtmMessageId::new())],
            RequestDeadline::after(PEER_HTTP_LOCAL_RESPONSE_BUDGET),
        )
        .expect_err("a peer must respond to every frame in a batch");
        assert_delivery_unconfirmed(error);
        server.join().expect("server");
    }

    #[test]
    fn bind_config_rejects_empty_duplicate_wildcard_and_multicast() {
        for bind_addrs in [
            vec![],
            vec![
                SocketAddr::from((Ipv4Addr::LOCALHOST, 43101)),
                SocketAddr::from((Ipv4Addr::LOCALHOST, 43101)),
            ],
            vec![SocketAddr::from((Ipv4Addr::UNSPECIFIED, 43101))],
            vec![SocketAddr::from((Ipv4Addr::new(192, 0, 2, 1), 43101))],
            vec![SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(224, 0, 0, 1)),
                43101,
            )],
        ] {
            assert!(
                PeerHttpBindConfig { bind_addrs }
                    .validate_at_startup()
                    .is_err()
            );
        }
    }

    #[test]
    fn configured_peer_listener_routes_http_write_as_peer_ingress() {
        let mut listener_set = PeerHttpListenerSet::bind(&PeerHttpBindConfig {
            bind_addrs: vec![SocketAddr::from((Ipv6Addr::LOCALHOST, 0))],
        })
        .expect("bind configured peer listener");
        let port = listener_set.listeners[0]
            .listener
            .local_addr()
            .expect("listener address")
            .port();
        let recorder = Arc::new(PeerWriteRecorder::default());
        listener_set
            .start(recorder.clone())
            .expect("start configured peer listener");

        let first_message_id = AtmMessageId::new();
        let second_message_id = AtmMessageId::new();
        let endpoint = peer_endpoint(port);
        let responses = send_peer_http_frames(
            &peer_runtime_config(),
            &endpoint,
            &[write(first_message_id), write(second_message_id)],
            RequestDeadline::after(PEER_HTTP_LOCAL_RESPONSE_BUDGET),
        )
        .expect("configured peer sender/receiver round trip");
        let returned_message_ids = responses
            .iter()
            .map(|response| match response {
                ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => outcome.message_id,
                response => panic!("expected sent response, got {response:?}"),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            returned_message_ids,
            vec![first_message_id, second_message_id]
        );
        assert_eq!(
            *recorder.message_ids.lock().expect("message ID recorder"),
            vec![first_message_id, second_message_id]
        );
        assert_eq!(
            *recorder.source_hosts.lock().expect("recorder"),
            vec![
                "origin.example.test".parse().expect("source host"),
                "origin.example.test".parse().expect("source host"),
            ]
        );
        listener_set.shutdown().expect("shutdown peer listener");
    }
}
