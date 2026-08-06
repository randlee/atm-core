//! HTTPS peer adapter.
//!
//! This module is intentionally only a transport adapter: it authenticates a
//! TCP/TLS peer, translates the shared HTTP envelope, and calls `ApiRouter`.
//! It has no mailbox, roster, acknowledgement, nudge, receipt, retry, or
//! replay state.

use std::any::Any;
use std::fmt;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use atm_core::api::{
    ApiRequest, ApiRouter, AuthenticatedIngress, HttpFrameReader, MessageCollectionRequest,
    PEER_SOURCE_HOST_HEADER, RequestDeadline, UntrustedSmokeProvenance, decode_request,
    read_http_request, read_http_response_with_frame_reader, write_http_request,
    write_http_request_with_headers, write_http_response,
};
use atm_core::error::AtmError;
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};
use atm_core::send::WriteRequest;
use atm_core::types::HostName;
use atm_storage::{HttpsInterface, LocalCertificate, TrustedPeer};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime, pem::PemObject};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{
    ClientConfig, ClientConnection, DigitallySignedStruct, Error, ServerConfig, ServerConnection,
    SignatureScheme, StreamOwned,
};
use sha2::{Digest, Sha256};

use crate::active_connection_registry::{
    ActiveConnectionGuard, ActiveConnectionRegistry, TrackedDispatchHandle,
};

const HTTPS_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_PEER_HTTP_CONNECTIONS: usize = 64;

/// The peer wire-security setting. Mutual TLS is the production default.
/// Plain HTTP is deliberately available only for an explicit, temporary smoke
/// run so connectivity can be isolated from certificate configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "the explicit smoke-wire selector is retained for externally driven daemon smoke runs"
)]
pub enum PeerWireSecurity {
    MutualTls,
    PlaintextTest,
}

impl std::str::FromStr for PeerWireSecurity {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "mutual-tls" => Ok(Self::MutualTls),
            "plaintext-test" => Ok(Self::PlaintextTest),
            _ => Err(AtmError::validation(
                "--peer-wire-security must be `mutual-tls` or `plaintext-test`",
            )),
        }
    }
}

/// The only outbound cross-host capability. It serializes the canonical
/// request envelope; it never receives a storage or post-write capability.
pub(crate) trait HttpsMessageTransport: Send + Sync {
    fn deliver(
        &self,
        request: WriteRequest,
        peer: &TrustedPeer,
        deadline: RequestDeadline,
    ) -> Result<ResponseEnvelope, AtmError>;
}

// The runtime swaps the transport atomically during trust refresh while
// request workers retain a cloned immutable transport for their own exchange.
pub(crate) type SharedHttpsTransport = Arc<Mutex<Option<Arc<dyn HttpsMessageTransport>>>>;

struct TlsIdentity {
    certificates: Vec<CertificateDer<'static>>,
    private_key: PrivateKeyDer<'static>,
    fingerprint: String,
}

impl fmt::Debug for TlsIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TlsIdentity")
            .field("fingerprint", &self.fingerprint)
            .finish_non_exhaustive()
    }
}

impl TlsIdentity {
    fn load(certificate: &LocalCertificate) -> Result<Self, AtmError> {
        let path = Path::new(certificate.private_key_ref.as_str());
        let pem = std::fs::read(path).map_err(|source| {
            AtmError::daemon_unavailable_with_cause(
                "failed to open configured TLS certificate/key PEM bundle",
                source,
            )
        })?;
        let certificates = CertificateDer::pem_slice_iter(&pem)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|source| {
                AtmError::daemon_unavailable_with_cause(
                    "configured TLS certificate PEM is invalid; repair the configured PEM bundle",
                    source,
                )
            })?;
        let private_key = PrivateKeyDer::from_pem_slice(&pem).map_err(|source| {
            AtmError::daemon_unavailable_with_cause(
                "configured TLS private key PEM is invalid; repair the configured PEM bundle",
                source,
            )
        })?;
        let first = certificates
            .first()
            .ok_or_else(|| AtmError::validation("configured TLS PEM bundle has no certificate"))?;
        let fingerprint = certificate_fingerprint(first);
        if normalize_fingerprint(certificate.fingerprint.as_str()) != fingerprint {
            return Err(AtmError::validation(
                "configured TLS certificate fingerprint does not match the PEM bundle",
            ));
        }
        Ok(Self {
            certificates,
            private_key,
            fingerprint,
        })
    }
}

/// Concrete outbound adapter. Configuration is read during construction and
/// retained only as TLS material; no storage trait crosses this boundary.
#[derive(Debug)]
pub(crate) struct HttpsTransport {
    mode: HttpsTransportMode,
}

#[derive(Debug)]
enum HttpsTransportMode {
    MutualTls(TlsIdentity),
    #[allow(
        dead_code,
        reason = "the explicit smoke transport is constructed by externally driven daemon smoke runs"
    )]
    PlaintextTest {
        source_host: HostName,
    },
}

impl HttpsTransport {
    pub(crate) fn from_local_certificate(certificate: &LocalCertificate) -> Result<Self, AtmError> {
        Ok(Self {
            mode: HttpsTransportMode::MutualTls(TlsIdentity::load(certificate)?),
        })
    }

    #[allow(
        dead_code,
        reason = "the explicit smoke transport is constructed by externally driven daemon smoke runs"
    )]
    pub(crate) fn plaintext_test(source_host: HostName) -> Self {
        Self {
            mode: HttpsTransportMode::PlaintextTest { source_host },
        }
    }
}

impl HttpsMessageTransport for HttpsTransport {
    fn deliver(
        &self,
        request: WriteRequest,
        peer: &TrustedPeer,
        deadline: RequestDeadline,
    ) -> Result<ResponseEnvelope, AtmError> {
        self.open_connection(peer, deadline)?
            .deliver(request, deadline)
    }
}

enum HttpsPeerConnection {
    MutualTls(Box<StreamOwned<ClientConnection, TcpStream>>),
    Plaintext(TcpStream, HostName),
}

impl HttpsPeerConnection {
    fn deliver(
        &mut self,
        request: WriteRequest,
        deadline: RequestDeadline,
    ) -> Result<ResponseEnvelope, AtmError> {
        let request = RequestEnvelope::Write(Box::new(request));
        match self {
            Self::MutualTls(tls) => {
                apply_deadline(tls.get_ref(), remaining_budget(deadline)?)?;
                write_http_request(tls, &request)?;
                apply_deadline(tls.get_ref(), remaining_budget(deadline)?)?;
                read_http_response_with_frame_reader(&mut HttpFrameReader::new(), tls, &request)
            }
            Self::Plaintext(stream, source_host) => {
                apply_deadline(stream, remaining_budget(deadline)?)?;
                write_plaintext_http_request_with_source_host(stream, &request, source_host)?;
                apply_deadline(stream, remaining_budget(deadline)?)?;
                read_http_response_with_frame_reader(&mut HttpFrameReader::new(), stream, &request)
            }
        }
    }
}

impl HttpsTransport {
    fn open_connection(
        &self,
        peer: &TrustedPeer,
        deadline: RequestDeadline,
    ) -> Result<HttpsPeerConnection, AtmError> {
        if !peer.enabled {
            return Err(AtmError::validation("configured HTTPS peer is disabled"));
        }
        let host = peer.host.to_string();
        // Resolve anew for every connection. The registered hostname remains
        // the TLS authority; resolver output is never stored.
        let address =
            resolve_peer_address(&host, peer.https_port.get(), remaining_budget(deadline)?)?;
        let stream = TcpStream::connect_timeout(&address, remaining_budget(deadline)?).map_err(
            |source| {
                AtmError::remote_delivery_unconfirmed(format!(
                    "failed to connect to HTTPS peer {host}"
                ))
                .with_cause(source)
            },
        )?;
        match &self.mode {
            HttpsTransportMode::MutualTls(identity) => {
                apply_deadline(&stream, remaining_budget(deadline)?)?;
                let config = client_config(identity, peer)?;
                let server_name = ServerName::try_from(host.clone()).map_err(|source| {
                    AtmError::validation(format!(
                        "configured HTTPS peer host is not a valid TLS server name: {source}",
                    ))
                    .with_cause(source)
                })?;
                let connection =
                    ClientConnection::new(Arc::new(config), server_name).map_err(|source| {
                        AtmError::daemon_unavailable_with_cause(
                            "failed to initialize HTTPS peer TLS client",
                            source,
                        )
                    })?;
                let mut tls = StreamOwned::new(connection, stream);
                complete_handshake_with_deadline(&mut tls, deadline)?;
                Ok(HttpsPeerConnection::MutualTls(Box::new(tls)))
            }
            HttpsTransportMode::PlaintextTest { source_host } => {
                apply_deadline(&stream, remaining_budget(deadline)?)?;
                Ok(HttpsPeerConnection::Plaintext(stream, source_host.clone()))
            }
        }
    }
}

/// Starts enabled peer listeners. The caller owns lifecycle shutdown through
/// `HttpsListenerSet::shutdown`; the listener has no daemon state of its own.
pub(crate) struct HttpsListenerSet {
    stop: Arc<std::sync::atomic::AtomicBool>,
    listeners: Vec<HttpsListener>,
    requests: Arc<ActiveConnectionRegistry>,
    peer_verifier: Arc<PinnedClientVerifier>,
}

struct HttpsListener {
    address: SocketAddr,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[derive(Clone)]
enum ListenerSecurity {
    MutualTls {
        config: Arc<ServerConfig>,
        verifier: Arc<PinnedClientVerifier>,
    },
    #[allow(
        dead_code,
        reason = "the explicit smoke listener is constructed by externally driven daemon smoke runs"
    )]
    PlaintextTest,
}

impl HttpsListenerSet {
    pub(crate) fn bind_enabled(
        interfaces: &[HttpsInterface],
        certificate: &LocalCertificate,
        peers: Vec<TrustedPeer>,
        router: Arc<dyn ApiRouter + Send + Sync>,
    ) -> Result<Self, AtmError> {
        let identity = TlsIdentity::load(certificate)?;
        let peer_verifier = Arc::new(PinnedClientVerifier::new(peers));
        let server_config = Arc::new(server_config(&identity, Arc::clone(&peer_verifier))?);
        Self::bind(
            interfaces,
            ListenerSecurity::MutualTls {
                config: server_config,
                verifier: peer_verifier,
            },
            router,
        )
    }

    /// Binds the same HTTP peer ingress without TLS only for an explicitly
    /// configured smoke run. The HTTP decoder and router remain identical to
    /// the authenticated listener; only peer authentication is absent.
    #[allow(
        dead_code,
        reason = "the explicit smoke listener is constructed by externally driven daemon smoke runs"
    )]
    pub(crate) fn bind_plaintext_test(
        interfaces: &[HttpsInterface],
        router: Arc<dyn ApiRouter + Send + Sync>,
    ) -> Result<Self, AtmError> {
        Self::bind(interfaces, ListenerSecurity::PlaintextTest, router)
    }

    fn bind(
        interfaces: &[HttpsInterface],
        security: ListenerSecurity,
        router: Arc<dyn ApiRouter + Send + Sync>,
    ) -> Result<Self, AtmError> {
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let bound = interfaces
            .iter()
            .filter(|interface| interface.enabled)
            .map(|interface| {
                let listener = TcpListener::bind(interface.bind_addr).map_err(|source| {
                    AtmError::daemon_unavailable(format!(
                        "failed to bind configured peer HTTP listener {}",
                        interface.bind_addr
                    ))
                    .with_cause(source)
                })?;
                listener.set_nonblocking(true).map_err(|source| {
                    AtmError::daemon_unavailable("failed to configure peer HTTP listener")
                        .with_cause(source)
                })?;
                let address = listener.local_addr().map_err(|source| {
                    AtmError::daemon_unavailable("failed to inspect configured peer HTTP listener")
                        .with_cause(source)
                })?;
                Ok((listener, address))
            })
            .collect::<Result<Vec<_>, AtmError>>()?;
        let requests = Arc::new(ActiveConnectionRegistry::default());
        let peer_verifier = match &security {
            ListenerSecurity::MutualTls { verifier, .. } => Arc::clone(verifier),
            ListenerSecurity::PlaintextTest => Arc::new(PinnedClientVerifier::new(Vec::new())),
        };
        let mut listeners = Vec::new();
        for (listener, address) in bound {
            let thread_stop = Arc::clone(&stop);
            let thread_router = Arc::clone(&router);
            let thread_requests = Arc::clone(&requests);
            let thread_security = security.clone();
            let thread = std::thread::Builder::new()
                .name("atm-peer-http-listener".to_string())
                .spawn(move || {
                    accept_loop(
                        listener,
                        thread_stop,
                        thread_security,
                        thread_router,
                        thread_requests,
                    )
                })
                .map_err(|source| {
                    AtmError::daemon_unavailable("failed to start peer HTTP listener")
                        .with_cause(source)
                })?;
            listeners.push(HttpsListener {
                address,
                thread: Some(thread),
            });
        }
        Ok(Self {
            stop,
            listeners,
            requests,
            peer_verifier,
        })
    }

    pub(crate) fn refresh_trusted_peers(&self, peers: Vec<TrustedPeer>) -> Result<(), AtmError> {
        self.peer_verifier.replace(peers)
    }

    pub(crate) fn shutdown(mut self) -> Result<(), AtmError> {
        self.stop_accepting();
        for listener in &mut self.listeners {
            if let Some(thread) = listener.thread.take() {
                thread.join().map_err(|panic| {
                    AtmError::daemon_unavailable("HTTPS listener worker panicked during shutdown")
                        .with_cause(panic_payload_message(panic.as_ref()))
                })?;
            }
        }
        self.requests.join_tracked_dispatches(HTTPS_TIMEOUT)?;
        Ok(())
    }

    /// Stops listener admission immediately; joining accepted request work stays
    /// with [`Self::shutdown`] so composition can share its drain budget.
    pub(crate) fn stop_accepting(&self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        for listener in &self.listeners {
            let _ = TcpStream::connect_timeout(&listener.address, Duration::from_millis(100));
        }
    }
}

fn accept_loop(
    listener: TcpListener,
    stop: Arc<std::sync::atomic::AtomicBool>,
    security: ListenerSecurity,
    router: Arc<dyn ApiRouter + Send + Sync>,
    requests: Arc<ActiveConnectionRegistry>,
) {
    while !stop.load(std::sync::atomic::Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                match peer_connection_admission(stop.load(std::sync::atomic::Ordering::SeqCst)) {
                    PeerConnectionAdmission::Stop => break,
                    PeerConnectionAdmission::Admit => {
                        if let Some(reservation) = requests.try_register(MAX_PEER_HTTP_CONNECTIONS)
                        {
                            spawn_request_worker(
                                stream,
                                &security,
                                &router,
                                &requests,
                                reservation,
                            );
                        } else {
                            tracing::warn!(
                                subsystem = "https_transport",
                                action = "accept",
                                outcome = "capacity_exceeded",
                                cap = MAX_PEER_HTTP_CONNECTIONS,
                                "peer HTTP listener rejected connection at its bounded concurrency cap"
                            );
                        }
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if let Err(error) = requests.reap_finished_dispatches() {
                    tracing::warn!(
                        subsystem = "https_transport",
                        action = "reap_request_workers",
                        outcome = "failed",
                        %error,
                        "peer HTTP listener could not reap finished request workers"
                    );
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => {
                tracing::warn!(
                    subsystem = "https_transport",
                    action = "accept",
                    outcome = "failed",
                    %error,
                    "peer HTTP listener accept failed"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

/// Decides whether an accepted TCP connection may become peer work.
///
/// Shutdown deliberately wakes a blocked `accept()` with one local connection. Once the stop
/// flag is set that wake-up is never peer work, regardless of scheduling order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PeerConnectionAdmission {
    Stop,
    Admit,
}

const fn peer_connection_admission(stop_requested: bool) -> PeerConnectionAdmission {
    if stop_requested {
        PeerConnectionAdmission::Stop
    } else {
        PeerConnectionAdmission::Admit
    }
}

fn spawn_request_worker(
    stream: TcpStream,
    security: &ListenerSecurity,
    router: &Arc<dyn ApiRouter + Send + Sync>,
    requests: &Arc<ActiveConnectionRegistry>,
    reservation: ActiveConnectionGuard,
) {
    if let Err(error) = stream.set_nonblocking(false) {
        tracing::warn!(
            subsystem = "https_transport",
            action = "configure_connection",
            outcome = "failed",
            %error,
            "peer HTTP listener could not configure an accepted connection"
        );
        return;
    }
    let (completion_tx, completion_rx) = std::sync::mpsc::sync_channel(1);
    let request = std::thread::Builder::new()
        .name("atm-peer-http-request".to_string())
        .spawn({
            let security = security.clone();
            let router = Arc::clone(router);
            move || {
                let _active = reservation;
                log_peer_request_result(handle_peer_connection(stream, security, router));
                let _ = completion_tx.send(());
            }
        })
        .map_err(|source| {
            AtmError::daemon_unavailable_with_cause(
                "failed to start peer HTTP request worker",
                source,
            )
        });
    match request {
        Ok(request) => track_request_worker(requests, request, completion_rx),
        Err(error) => tracing::warn!(
            subsystem = "https_transport",
            action = "start_request",
            outcome = "failed",
            error_code = %error.code(),
            "peer HTTP listener rejected connection because request worker startup failed"
        ),
    }
}

fn log_peer_request_result(result: Result<(), AtmError>) {
    if let Err(error) = result {
        tracing::warn!(
            subsystem = "https_transport",
            action = "request",
            outcome = "rejected",
            error_code = %error.code(),
            error_message = %error.message(),
            "HTTPS peer request was rejected before or during shared API routing"
        );
    }
}

fn track_request_worker(
    requests: &ActiveConnectionRegistry,
    request: std::thread::JoinHandle<()>,
    completion_rx: std::sync::mpsc::Receiver<()>,
) {
    if let Err(error) = requests.push_dispatch_handle(
        TrackedDispatchHandle {
            completion_rx,
            join_handle: request,
        },
        MAX_PEER_HTTP_CONNECTIONS,
    ) {
        tracing::error!(
            subsystem = "https_transport",
            action = "track_connection",
            outcome = "failed",
            %error,
            "HTTPS request worker could not be tracked for shutdown"
        );
    }
}

fn handle_peer_connection(
    mut stream: TcpStream,
    security: ListenerSecurity,
    router: Arc<dyn ApiRouter + Send + Sync>,
) -> Result<(), AtmError> {
    let deadline = RequestDeadline::after(HTTPS_TIMEOUT);
    apply_deadline(&stream, remaining_budget(deadline)?)?;
    match security {
        ListenerSecurity::MutualTls { config, verifier } => {
            let connection = ServerConnection::new(config).map_err(|source| {
                AtmError::daemon_unavailable("failed to initialize HTTPS peer TLS server")
                    .with_cause(source)
            })?;
            let mut tls = StreamOwned::new(connection, stream);
            complete_server_handshake_with_deadline(&mut tls, deadline)?;
            let authenticated_source_host = verifier.authenticated_host(&tls.conn)?;
            route_peer_http_request(&mut tls, router, Some(authenticated_source_host), deadline)
        }
        ListenerSecurity::PlaintextTest => {
            route_peer_http_request(&mut stream, router, None, deadline)
        }
    }
}

fn route_peer_http_request(
    stream: &mut (impl Read + Write),
    router: Arc<dyn ApiRouter + Send + Sync>,
    authenticated_source_host: Option<HostName>,
    deadline: RequestDeadline,
) -> Result<(), AtmError> {
    // Keep the peer connection on the same absolute budget after TLS setup.
    // The router receives this exact deadline, so a slow inbound request cannot
    // obtain a fresh dispatch window after spending time in framing or decode.
    remaining_budget(deadline)?;
    let request = match read_http_request(stream)? {
        Some(request) => request,
        None => return Ok(()),
    };
    remaining_budget(deadline)?;
    let plaintext_source_host = request
        .header(PEER_SOURCE_HOST_HEADER)
        .map(str::parse)
        .transpose()
        .map_err(|source| {
            AtmError::validation(
                format!(
                    "invalid {PEER_SOURCE_HOST_HEADER} header; use a valid configured test host or restart without --peer-wire-security plaintext-test"
                ),
            )
            .with_cause(source)
        })?;
    let mut request = decode_request(request)?;
    clear_remote_activity_observation(&mut request);
    let ingress = match (authenticated_source_host, plaintext_source_host) {
        (Some(source_host), _) => {
            normalize_peer_write_for_local_delivery(&mut request, source_host);
            AuthenticatedIngress::Peer
        }
        (None, Some(source_host)) => {
            normalize_untrusted_smoke_write_for_local_delivery(&mut request);
            AuthenticatedIngress::UntrustedSmoke(UntrustedSmokeProvenance::new(source_host))
        }
        (None, None) if matches!(request, ApiRequest::Write(_)) => {
            return Err(AtmError::validation(format!(
                "plaintext peer write requests require {PEER_SOURCE_HOST_HEADER}; restart without --peer-wire-security plaintext-test to restore mTLS"
            )));
        }
        // Read-only plaintext diagnostics are deliberately anonymous. They
        // must never be labelled as the authenticated mTLS peer ingress.
        (None, None) => AuthenticatedIngress::AnonymousSmoke,
    };
    let response = router
        .route(request, ingress, deadline)
        .map(|response| response.into_inner())
        .unwrap_or_else(ResponseEnvelope::Error);
    remaining_budget(deadline)?;
    write_http_response(stream, &response)
}

fn normalize_untrusted_smoke_write_for_local_delivery(request: &mut ApiRequest) {
    if let ApiRequest::Write(write) = request {
        write.authenticated_source_host = None;
    }
}

fn clear_remote_activity_observation(request: &mut ApiRequest) {
    match request {
        ApiRequest::Write(write) => write.activity_observation = None,
        ApiRequest::Messages(messages) => {
            if let MessageCollectionRequest::Receive(query) = messages.as_mut() {
                query.activity_observation = None;
            }
        }
        ApiRequest::Heartbeat(heartbeat) => heartbeat.session_id = None,
        _ => {}
    }
}

fn write_plaintext_http_request_with_source_host(
    stream: &mut TcpStream,
    request: &RequestEnvelope,
    source_host: &HostName,
) -> Result<(), AtmError> {
    write_http_request_with_headers(
        stream,
        request,
        &[(PEER_SOURCE_HOST_HEADER, source_host.as_str())],
    )
}

/// The HTTPS adapter has already selected this daemon. It preserves the
/// canonical host-qualified address and records only the adapter-authenticated
/// source provenance before submitting the shared write request used by local
/// UDS and graft clients. Origin metadata prevents re-forwarding after write.
fn normalize_peer_write_for_local_delivery(request: &mut ApiRequest, source_host: HostName) {
    if let ApiRequest::Write(write) = request {
        write.authenticated_source_host = Some(source_host);
    }
}

fn complete_server_handshake_with_deadline(
    stream: &mut StreamOwned<ServerConnection, TcpStream>,
    deadline: RequestDeadline,
) -> Result<(), AtmError> {
    while stream.conn.is_handshaking() {
        apply_deadline(&stream.sock, remaining_budget(deadline)?)?;
        stream
            .conn
            .complete_io(&mut stream.sock)
            .map_err(|source| {
                AtmError::daemon_unavailable_with_cause(
                    "HTTPS peer mutual-TLS server handshake failed",
                    source,
                )
            })?;
    }
    Ok(())
}

fn apply_deadline(stream: &TcpStream, deadline: Duration) -> Result<(), AtmError> {
    stream.set_read_timeout(Some(deadline)).map_err(|source| {
        AtmError::daemon_unavailable_with_cause("failed to set HTTPS peer read deadline", source)
    })?;
    stream.set_write_timeout(Some(deadline)).map_err(|source| {
        AtmError::daemon_unavailable_with_cause("failed to set HTTPS peer write deadline", source)
    })
}

#[cfg(test)]
fn complete_handshake(
    stream: &mut StreamOwned<ClientConnection, TcpStream>,
) -> Result<(), AtmError> {
    complete_handshake_with_deadline(stream, RequestDeadline::after(HTTPS_TIMEOUT))
}

fn complete_handshake_with_deadline(
    stream: &mut StreamOwned<ClientConnection, TcpStream>,
    deadline: RequestDeadline,
) -> Result<(), AtmError> {
    while stream.conn.is_handshaking() {
        apply_deadline(&stream.sock, remaining_budget(deadline)?)?;
        stream
            .conn
            .complete_io(&mut stream.sock)
            .map_err(|source| {
                AtmError::remote_delivery_unconfirmed(format!(
                    "HTTPS peer mutual-TLS handshake failed: {source}"
                ))
            })?;
    }
    Ok(())
}

fn remaining_budget(deadline: RequestDeadline) -> Result<Duration, AtmError> {
    deadline
        .remaining()
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| {
            AtmError::remote_delivery_unconfirmed(
                "local persistence completed before the peer accepted the write deadline expired",
            )
        })
}

fn resolve_peer_address(host: &str, port: u16, timeout: Duration) -> Result<SocketAddr, AtmError> {
    let peer = TrustedPeer {
        host: host.parse().map_err(|source| {
            AtmError::daemon_unavailable_with_cause("invalid configured HTTPS peer host", source)
        })?,
        fingerprint: "resolver-only".parse().map_err(|source| {
            AtmError::daemon_unavailable_with_cause("invalid resolver authority", source)
        })?,
        enabled: true,
        https_port: std::num::NonZeroU16::new(port)
            .ok_or_else(|| AtmError::validation("configured HTTPS peer port was zero"))?,
    };
    crate::peer_resolution::resolve_peer_socket_addresses(&peer, timeout)?
        .into_iter()
        .next()
        .map(|ip| SocketAddr::new(ip, port))
        .ok_or_else(|| {
            AtmError::validation_with_recovery(
                "HTTPS peer resolved to no addresses",
                "verify the registered hostname has a current A or AAAA record, then retry",
            )
        })
}

fn client_config(identity: &TlsIdentity, peer: &TrustedPeer) -> Result<ClientConfig, AtmError> {
    install_tls_provider();
    let verifier = Arc::new(PinnedServerVerifier::new(peer.fingerprint.to_string()));
    ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_client_auth_cert(
            identity.certificates.clone(),
            identity.private_key.clone_key(),
        )
        .map_err(|source| {
            AtmError::validation("configured TLS certificate/key pair is invalid")
                .with_cause(source)
        })
}

fn server_config(
    identity: &TlsIdentity,
    peer_verifier: Arc<PinnedClientVerifier>,
) -> Result<ServerConfig, AtmError> {
    install_tls_provider();
    ServerConfig::builder()
        .with_client_cert_verifier(peer_verifier)
        .with_single_cert(
            identity.certificates.clone(),
            identity.private_key.clone_key(),
        )
        .map_err(|source| {
            AtmError::validation("configured TLS certificate/key pair is invalid")
                .with_cause(source)
        })
}

fn install_tls_provider() {
    // Other workspace dependencies enable aws-lc as well as ring. ATM selects
    // ring explicitly once so HTTPS behavior is deterministic in every binary.
    let _ = rustls::crypto::ring::default_provider().install_default();
}

#[derive(Debug)]
struct PinnedServerVerifier {
    fingerprint: String,
    algorithms: rustls::crypto::WebPkiSupportedAlgorithms,
}

impl PinnedServerVerifier {
    fn new(fingerprint: String) -> Self {
        Self {
            fingerprint: normalize_fingerprint(&fingerprint),
            algorithms: rustls::crypto::ring::default_provider().signature_verification_algorithms,
        }
    }
}

impl ServerCertVerifier for PinnedServerVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        if certificate_fingerprint(end_entity) == self.fingerprint {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::CertificateError::UnknownIssuer.into())
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

#[derive(Debug)]
struct PinnedClientVerifier {
    peers: RwLock<Vec<TrustedPeer>>,
    algorithms: rustls::crypto::WebPkiSupportedAlgorithms,
}

impl PinnedClientVerifier {
    fn new(peers: Vec<TrustedPeer>) -> Self {
        Self {
            peers: RwLock::new(peers.into_iter().filter(|peer| peer.enabled).collect()),
            algorithms: rustls::crypto::ring::default_provider().signature_verification_algorithms,
        }
    }
}

impl ClientCertVerifier for PinnedClientVerifier {
    fn root_hint_subjects(&self) -> &[rustls::DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, Error> {
        if self.host_for_certificate(end_entity).is_some() {
            Ok(ClientCertVerified::assertion())
        } else {
            Err(rustls::CertificateError::UnknownIssuer.into())
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

impl PinnedClientVerifier {
    fn replace(&self, peers: Vec<TrustedPeer>) -> Result<(), AtmError> {
        let mut current = self.peers.write().map_err(|source| {
            AtmError::daemon_unavailable("HTTPS peer verifier lock poisoned").with_cause(source)
        })?;
        *current = peers.into_iter().filter(|peer| peer.enabled).collect();
        Ok(())
    }
    fn authenticated_host(&self, connection: &ServerConnection) -> Result<HostName, AtmError> {
        let certificate = connection
            .peer_certificates()
            .and_then(|certificates| certificates.first())
            .ok_or_else(|| {
                AtmError::daemon_unavailable("HTTPS peer provided no client certificate")
            })?;
        self.host_for_certificate(certificate).ok_or_else(|| {
            AtmError::daemon_unavailable("HTTPS peer certificate is not a configured trusted peer")
        })
    }

    fn host_for_certificate(&self, certificate: &CertificateDer<'_>) -> Option<HostName> {
        let fingerprint = certificate_fingerprint(certificate);
        self.peers
            .read()
            .ok()?
            .iter()
            .find(|peer| normalize_fingerprint(peer.fingerprint.as_str()) == fingerprint)
            .map(|peer| peer.host.clone())
    }
}

fn certificate_fingerprint(certificate: &CertificateDer<'_>) -> String {
    format!("{:x}", Sha256::digest(certificate.as_ref()))
}

fn normalize_fingerprint(value: &str) -> String {
    value
        .chars()
        .filter(char::is_ascii_hexdigit)
        .collect::<String>()
        .to_ascii_lowercase()
}

fn panic_payload_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, UdpSocket};
    use std::str::FromStr as _;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use crate::active_connection_registry::ActiveConnectionRegistry;

    use super::clear_remote_activity_observation;
    use atm_core::api::{
        ApiRequest, ApiResponse, ApiRouter, AuthenticatedIngress, MessageCollectionRequest,
        PEER_SOURCE_HOST_HEADER, RequestDeadline, read_http_response, write_http_request,
        write_http_request_with_headers,
    };
    use atm_core::boundary::RosterHarness;
    use atm_core::caller_context::ActivityObservation;
    use atm_core::doctor::DoctorQuery;
    use atm_core::error::AtmError;
    use atm_core::graft::{
        GraftPostSendResponse, GraftReceiverListener, graft_receiver_record_path_from_home,
    };
    use atm_core::protocol::{
        HeartbeatActivity, RequestEnvelope, ResponseEnvelope, TeamMemberHeartbeatRequest,
    };
    use atm_core::read::ReadQuery;
    use atm_core::schema::{AgentMember, AtmMessageId};
    use atm_core::send::{SendMessageSource, SendRequest, WriteRequest};
    use atm_core::test_support::{EnvGuard, ROLE_TEAM_LEAD};
    use atm_core::types::{AgentName, HostName, IsoTimestamp, ReadSelection, SessionId, TeamName};
    use atm_runtime_test_support::{SQLITE_RUNTIME_PATH_ENV, open_sqlite_boundary};
    use atm_storage::{
        CertificateFingerprint, HttpsInterface, LocalCertificate, PrivateKeyRef, TrustedPeer,
    };

    fn trusted(host: &str) -> TrustedPeer {
        TrustedPeer {
            host: host.parse().expect("host"),
            fingerprint: "00".repeat(32).parse().expect("fingerprint"),
            enabled: true,
            https_port: std::num::NonZeroU16::new(43101).expect("non-zero"),
        }
    }

    /// Resolve an address owned by this host without relying on a fixed
    /// workstation subnet. UDP connect selects the route's local address but
    /// does not send a packet, making this suitable for an in-process proof.
    fn non_loopback_interface() -> (IpAddr, HostName) {
        let probe = UdpSocket::bind(("0.0.0.0", 0)).expect("bind route probe");
        probe
            .connect(("192.0.2.1", 9))
            .expect("select a non-loopback route");
        let address = probe.local_addr().expect("inspect route probe").ip();
        assert!(
            !address.is_loopback() && !address.is_unspecified(),
            "the advertised-IP proof requires a non-loopback interface, got {address}"
        );
        let host = address.to_string().parse().expect("route host");
        (address, host)
    }

    use rustls::pki_types::ServerName;
    use rustls::pki_types::{CertificateDer, pem::PemObject};
    use rustls::{ClientConnection, StreamOwned};

    use crate::runtime_health::{DaemonRequestDispatcher, RuntimeStatusCache};

    use super::{
        HttpsListenerSet, MAX_PEER_HTTP_CONNECTIONS, PeerConnectionAdmission, TlsIdentity,
        client_config, complete_handshake, normalize_fingerprint, peer_connection_admission,
    };

    #[derive(Default)]
    struct RecordingRouter {
        routed: AtomicBool,
        requests: Mutex<Vec<ApiRequest>>,
        ingress: Mutex<Option<AuthenticatedIngress>>,
    }

    impl atm_core::boundary::sealed::Sealed for RecordingRouter {}

    impl ApiRouter for RecordingRouter {
        fn route(
            &self,
            _request: ApiRequest,
            ingress: AuthenticatedIngress,
            _deadline: RequestDeadline,
        ) -> Result<ApiResponse, AtmError> {
            self.routed.store(true, Ordering::SeqCst);
            self.requests
                .lock()
                .expect("recorded requests")
                .push(_request);
            *self.ingress.lock().expect("recorded ingress") = Some(ingress);
            Ok(ApiResponse::new(ResponseEnvelope::Error(
                AtmError::validation("test router response"),
            )))
        }
    }

    #[test]
    fn fingerprints_are_exact_after_presentation_normalization() {
        assert_eq!(normalize_fingerprint("AA:bb-CC"), "aabbcc");
        assert_ne!(normalize_fingerprint("aabbcd"), "aabbcc");
    }

    #[test]
    fn peer_deadline_is_one_absolute_budget() {
        let deadline = RequestDeadline::after(Duration::from_secs(5));
        let remaining = super::remaining_budget(deadline).expect("fresh deadline has budget");
        assert!(remaining <= Duration::from_secs(5));
        assert!(remaining > Duration::ZERO);
    }

    #[test]
    fn expired_peer_budget_reports_remote_delivery_unconfirmed() {
        let error = super::remaining_budget(RequestDeadline::after(Duration::ZERO))
            .expect_err("an expired shared request budget must fail before peer delivery");
        assert_eq!(
            error.code(),
            atm_core::error_codes::AtmErrorCode::RemoteDeliveryUnconfirmed
        );
    }

    #[test]
    fn stop_accepting_flips_the_shared_listener_gate_before_shutdown_join() {
        let router = Arc::new(RecordingRouter::default());
        let listener = HttpsListenerSet::bind_plaintext_test(
            &[HttpsInterface {
                bind_addr: "127.0.0.1:0".parse().expect("bind address"),
                advertise_host: "localhost".parse().expect("host"),
                enabled: true,
            }],
            router,
        )
        .expect("start plaintext test listener");

        assert!(!listener.stop.load(Ordering::SeqCst));
        listener.stop_accepting();
        assert!(
            listener.stop.load(Ordering::SeqCst),
            "draining must stop HTTPS admission before joining existing work"
        );
        listener.shutdown().expect("shutdown listener");
    }

    #[test]
    fn invalid_tls_server_name_preserves_the_parser_cause() {
        let result: Result<ServerName<'static>, AtmError> =
            ServerName::try_from("-invalid".to_string()).map_err(|source| {
                AtmError::validation("configured HTTPS peer host is not a valid TLS server name")
                    .with_cause(source)
            });
        let error = result.expect_err("invalid DNS label must fail validation");
        assert_eq!(
            error.code(),
            atm_core::error_codes::AtmErrorCode::MessageValidationFailed
        );
        assert!(
            error.cause().is_some(),
            "TLS parser cause must be preserved"
        );
    }

    #[test]
    fn https_address_resolution_uses_the_shared_bounded_helper() {
        let address = super::resolve_peer_address("localhost", 43101, Duration::from_secs(1))
            .expect("localhost must resolve through the shared helper");
        assert_eq!(address.port(), 43101);
    }

    #[test]
    fn exact_pinned_mtls_peer_reaches_the_shared_router() {
        let certificate = test_certificate();
        let identity = TlsIdentity::load(&certificate).expect("load test identity");
        let router = Arc::new(RecordingRouter::default());
        let listener = HttpsListenerSet::bind_enabled(
            &[HttpsInterface {
                bind_addr: "127.0.0.1:0".parse().expect("bind address"),
                advertise_host: "localhost".parse().expect("host"),
                enabled: true,
            }],
            &certificate,
            vec![TrustedPeer {
                host: "localhost".parse().expect("host"),
                fingerprint: certificate.fingerprint.clone(),
                enabled: true,
                https_port: std::num::NonZeroU16::new(43101).expect("non-zero"),
            }],
            router.clone(),
        )
        .expect("start listener");
        let address = listener.listeners[0].address;
        let stream = TcpStream::connect(address).expect("connect");
        let config = client_config(
            &identity,
            &TrustedPeer {
                host: "localhost".parse().expect("host"),
                fingerprint: certificate.fingerprint.clone(),
                enabled: true,
                https_port: std::num::NonZeroU16::new(43101).expect("non-zero"),
            },
        )
        .expect("client config");
        let connection = ClientConnection::new(
            Arc::new(config),
            ServerName::try_from("localhost".to_string()).expect("server name"),
        )
        .expect("client connection");
        let mut tls = StreamOwned::new(connection, stream);
        complete_handshake(&mut tls).expect("mutual TLS handshake");
        let request = RequestEnvelope::Doctor(DoctorQuery::default());
        write_http_request(&mut tls, &request).expect("write shared request");
        let _ = read_http_response(&mut tls, &request).expect("shared router response");
        assert!(router.routed.load(Ordering::SeqCst));
        listener.shutdown().expect("shutdown listener");
    }

    #[test]
    fn plaintext_test_http_reaches_the_same_router_without_tls() {
        let router = Arc::new(RecordingRouter::default());
        let listener = HttpsListenerSet::bind_plaintext_test(
            &[HttpsInterface {
                bind_addr: "127.0.0.1:0".parse().expect("bind address"),
                advertise_host: "localhost".parse().expect("host"),
                enabled: true,
            }],
            router.clone(),
        )
        .expect("start plaintext test listener");
        let mut stream = TcpStream::connect(listener.listeners[0].address).expect("connect");
        let request = RequestEnvelope::Doctor(DoctorQuery::default());
        write_http_request(&mut stream, &request).expect("write plain shared request");
        let _ = read_http_response(&mut stream, &request).expect("shared router response");
        assert!(router.routed.load(Ordering::SeqCst));
        assert!(matches!(
            router.ingress.lock().expect("recorded ingress").as_ref(),
            Some(AuthenticatedIngress::AnonymousSmoke)
        ));
        listener.shutdown().expect("shutdown listener");
    }

    #[test]
    fn plaintext_test_write_uses_untrusted_smoke_provenance_at_shared_router() {
        let router = Arc::new(RecordingRouter::default());
        let listener = HttpsListenerSet::bind_plaintext_test(
            &[HttpsInterface {
                bind_addr: "127.0.0.1:0".parse().expect("bind address"),
                advertise_host: "localhost".parse().expect("host"),
                enabled: true,
            }],
            router.clone(),
        )
        .expect("start plaintext test listener");
        let mut write = WriteRequest::new(
            std::env::temp_dir(),
            std::env::temp_dir(),
            "sender".parse().expect("sender"),
            "recipient@test-team.example.invalid",
            "test-team".parse().expect("team"),
            SendMessageSource::Inline("message".to_string()),
            None,
            true,
            None,
            false,
        )
        .expect("write request");
        let origin_message_id = atm_core::schema::AtmMessageId::new();
        write.origin_message_id = Some(origin_message_id);
        write.origin_timestamp = Some(atm_core::types::IsoTimestamp::now());
        write.authenticated_source_host = Some("spoofed.invalid".parse().expect("host"));
        let request = RequestEnvelope::Write(Box::new(write));
        let mut stream = TcpStream::connect(listener.listeners[0].address).expect("connect");
        write_http_request_with_headers(
            &mut stream,
            &request,
            &[(PEER_SOURCE_HOST_HEADER, "smoke-peer.invalid")],
        )
        .expect("write plain peer request");
        let _ = read_http_response(&mut stream, &request).expect("shared router response");
        let requests = router.requests.lock().expect("recorded requests");
        let ApiRequest::Write(write) = &requests[0] else {
            panic!("expected canonical write request");
        };
        assert!(write.authenticated_source_host.is_none());
        assert_eq!(write.origin_message_id, Some(origin_message_id));
        assert_eq!(
            write.to.as_ref().expect("destination").host(),
            Some(&"example.invalid".parse().expect("destination host"))
        );
        assert!(matches!(
            router.ingress.lock().expect("recorded ingress").as_ref(),
            Some(AuthenticatedIngress::UntrustedSmoke(_))
        ));
        listener.shutdown().expect("shutdown listener");
    }

    #[test]
    fn remote_ingress_clears_forged_activity_observation_for_write_and_receive() {
        let observation = ActivityObservation {
            team: "test-team".parse().expect("team"),
            member: "sender".parse().expect("member"),
            session_id: Some(SessionId::new("forged").expect("session")),
            pid: Some(7),
        };
        let write = WriteRequest::new(
            std::env::temp_dir(),
            std::env::temp_dir(),
            "sender".parse().expect("sender"),
            "recipient@test-team",
            "test-team".parse().expect("team"),
            SendMessageSource::Inline("message".to_string()),
            None,
            false,
            None,
            false,
        )
        .expect("write")
        .with_activity_observation(Some(observation.clone()));
        let mut write = ApiRequest::Write(Box::new(write));
        clear_remote_activity_observation(&mut write);
        assert!(
            matches!(write, ApiRequest::Write(ref request) if request.activity_observation.is_none())
        );
        let query = ReadQuery::new(
            std::env::temp_dir(),
            std::env::temp_dir(),
            "sender".parse().expect("sender"),
            None,
            "test-team".parse().expect("team"),
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
        .expect("query")
        .with_activity_observation(Some(observation));
        let mut receive = ApiRequest::Messages(Box::new(MessageCollectionRequest::Receive(query)));
        clear_remote_activity_observation(&mut receive);
        assert!(
            matches!(receive, ApiRequest::Messages(ref request) if matches!(request.as_ref(), MessageCollectionRequest::Receive(query) if query.activity_observation.is_none()))
        );
        let mut heartbeat = ApiRequest::Heartbeat(TeamMemberHeartbeatRequest {
            team: "test-team".parse().expect("team"),
            member: "sender".parse().expect("member"),
            pid: 7,
            observed_at: IsoTimestamp::now(),
            activity: HeartbeatActivity::ActiveToolUse,
            session_id: Some(SessionId::new("forged-heartbeat").expect("session")),
        });
        clear_remote_activity_observation(&mut heartbeat);
        assert!(matches!(
            heartbeat,
            ApiRequest::Heartbeat(request) if request.session_id.is_none()
        ));
    }

    #[test]
    fn peer_wire_strips_forged_activity_observation_before_routing() {
        let router = Arc::new(RecordingRouter::default());
        let listener = HttpsListenerSet::bind_plaintext_test(
            &[HttpsInterface {
                bind_addr: "127.0.0.1:0".parse().expect("bind address"),
                advertise_host: "localhost".parse().expect("host"),
                enabled: true,
            }],
            router.clone(),
        )
        .expect("start plaintext test listener");
        let observation = ActivityObservation {
            team: "test-team".parse().expect("team"),
            member: "sender".parse().expect("member"),
            session_id: Some(SessionId::new("forged").expect("session")),
            pid: Some(7),
        };
        let write = WriteRequest::new(
            std::env::temp_dir(),
            std::env::temp_dir(),
            "sender".parse().expect("sender"),
            "recipient@test-team.example.invalid",
            "test-team".parse().expect("team"),
            SendMessageSource::Inline("message".to_string()),
            None,
            false,
            None,
            false,
        )
        .expect("write")
        .with_activity_observation(Some(observation.clone()));
        let write = RequestEnvelope::Write(Box::new(write));
        let mut stream = TcpStream::connect(listener.listeners[0].address).expect("connect");
        write_http_request_with_headers(
            &mut stream,
            &write,
            &[(PEER_SOURCE_HOST_HEADER, "smoke-peer.invalid")],
        )
        .expect("write peer request");
        let _ = read_http_response(&mut stream, &write).expect("write response");

        let query = ReadQuery::new(
            std::env::temp_dir(),
            std::env::temp_dir(),
            "sender".parse().expect("sender"),
            None,
            "test-team".parse().expect("team"),
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
        .expect("query")
        .with_activity_observation(Some(observation));
        let receive = RequestEnvelope::Receive(query);
        let mut stream = TcpStream::connect(listener.listeners[0].address).expect("connect");
        write_http_request(&mut stream, &receive).expect("write receive request");
        let _ = read_http_response(&mut stream, &receive).expect("receive response");

        let requests = router.requests.lock().expect("recorded requests");
        assert!(matches!(
            requests.as_slice(),
            [ApiRequest::Write(write), ApiRequest::Messages(messages)]
                if write.activity_observation.is_none()
                    && matches!(messages.as_ref(), MessageCollectionRequest::Receive(query) if query.activity_observation.is_none())
        ));
        listener.shutdown().expect("shutdown listener");
    }

    #[test]
    #[serial_test::serial(env)]
    fn advertised_ip_peer_write_uses_real_dispatcher_persists_and_nudges() {
        crate::tests::install_retained_runtime_factory();
        let (advertised_ip, advertised_host) = non_loopback_interface();
        let tempdir = tempfile::TempDir::new().expect("tempdir");
        let atm_home = tempdir.path().join("atm-home");
        let workspace_dir = tempdir.path().join("workspace");
        let db_path = tempdir.path().join("mail.db");
        std::fs::create_dir_all(&atm_home).expect("atm home dir");
        std::fs::create_dir_all(&workspace_dir).expect("workspace dir");
        crate::tests::write_team_config(&atm_home, &[ROLE_TEAM_LEAD, "qa-a"]);
        std::fs::write(
            workspace_dir.join(".atm.toml"),
            "[atm.graft]\nenabled = true\n",
        )
        .expect("write graft configuration");

        let team: TeamName = crate::tests::TEST_TEAM.parse().expect("team");
        let roster = [ROLE_TEAM_LEAD, "qa-a"]
            .iter()
            .map(|name| {
                let mut member = AgentMember::with_name((*name).parse().expect("member"));
                member.home_dir = workspace_dir.clone().into();
                let mut record = atm_core::boundary::roster_member_record_from_claude_code_member(
                    team.clone(),
                    member,
                );
                record.harness = RosterHarness::CodexCli;
                record
            })
            .collect::<Vec<_>>();
        open_sqlite_boundary(&db_path)
            .expect("sqlite boundary")
            .roster_store_arc()
            .replace_roster(&team, &roster)
            .expect("install roster");

        let recipient: AgentName = "qa-a".parse().expect("recipient");
        let receiver_path = graft_receiver_record_path_from_home(&workspace_dir, &team, &recipient);
        let graft_listener =
            GraftReceiverListener::bind(&receiver_path, None).expect("bind fake graft receiver");
        let (nudge_tx, nudge_rx) = std::sync::mpsc::sync_channel(1);
        let graft_thread = std::thread::spawn(move || {
            let mut stream = loop {
                if let Some(stream) = graft_listener.poll_accept().expect("poll graft receiver") {
                    break stream;
                }
                std::thread::yield_now();
            };
            let request = graft_listener
                .read_request(&mut stream, Duration::from_secs(5))
                .expect("read graft nudge");
            nudge_tx.send(request.event).expect("capture graft nudge");
            graft_listener
                .write_response(&mut stream, &GraftPostSendResponse::Delivered)
                .expect("ack graft nudge");
        });

        let _env = EnvGuard::set_many([
            ("ATM_HOME", Some(atm_home.to_str().expect("utf8 atm home"))),
            (
                "ATM_CONFIG_HOME",
                Some(tempdir.path().to_str().expect("utf8 config home")),
            ),
            (
                SQLITE_RUNTIME_PATH_ENV,
                Some(db_path.to_str().expect("utf8 db path")),
            ),
            ("HOME", Some(atm_home.to_str().expect("utf8 atm home"))),
            ("USERPROFILE", None),
        ]);
        let dispatcher = Arc::new(DaemonRequestDispatcher::new_for_test(
            atm_home.clone(),
            RuntimeStatusCache::new(),
            db_path,
        ));
        let certificate = test_certificate();
        let peer = TrustedPeer {
            host: advertised_host.clone(),
            fingerprint: certificate.fingerprint.clone(),
            enabled: true,
            https_port: std::num::NonZeroU16::new(43101).expect("peer port"),
        };
        let listener = HttpsListenerSet::bind_enabled(
            &[HttpsInterface {
                bind_addr: SocketAddr::new(advertised_ip, 0),
                advertise_host: peer.host.clone(),
                enabled: true,
            }],
            &certificate,
            vec![peer.clone()],
            dispatcher.clone(),
        )
        .expect("start real HTTPS peer listener");
        let address = listener.listeners[0].address;
        let identity = TlsIdentity::load(&certificate).expect("load client identity");
        let config = client_config(&identity, &peer).expect("client config");
        let connection = ClientConnection::new(
            Arc::new(config),
            ServerName::try_from(peer.host.to_string()).expect("server name"),
        )
        .expect("client connection");
        let stream = TcpStream::connect(address).expect("connect advertised IP listener");
        let mut tls = StreamOwned::new(connection, stream);
        complete_handshake(&mut tls).expect("mutual TLS handshake");
        let origin_id = AtmMessageId::new();
        let request = RequestEnvelope::Write(Box::new(
            SendRequest::new(
                atm_home.clone(),
                workspace_dir.clone(),
                ROLE_TEAM_LEAD.parse().expect("sender"),
                &format!("qa-a@test-team.{advertised_host}"),
                team.clone(),
                SendMessageSource::Inline("advertised-IP real peer write".to_string()),
                None,
                false,
                None,
                false,
            )
            .expect("peer write")
            .with_origin_metadata(origin_id, IsoTimestamp::now()),
        ));
        write_http_request(&mut tls, &request).expect("write peer request");
        let response = read_http_response(&mut tls, &request).expect("read peer response");
        assert!(matches!(response, ResponseEnvelope::Send(_)));

        let nudge = nudge_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("peer receipt must nudge after persistence");
        assert_eq!(nudge.recipient, recipient);
        assert_eq!(nudge.description, "advertised-IP real peer write");
        let origin_id_filter = origin_id.to_string();
        let response = dispatcher
            .dispatch(RequestEnvelope::Receive(
                ReadQuery::new(
                    atm_home,
                    workspace_dir,
                    recipient,
                    None,
                    team,
                    ReadSelection::All,
                    false,
                    false,
                    Some(&origin_id_filter),
                    None,
                    None,
                    None,
                    None,
                    None,
                )
                .expect("recipient read query"),
            ))
            .expect("read persisted recipient record");
        let ResponseEnvelope::Receive(outcome) = response else {
            panic!("expected recipient inbox read response");
        };
        assert_eq!(
            outcome.count, 1,
            "recipient can read the persisted peer write"
        );
        graft_thread.join().expect("join graft receiver");
        listener.shutdown().expect("shutdown listener");
    }

    #[test]
    fn live_trust_refresh_keeps_one_listener_reachable() {
        let certificate = test_certificate();
        let identity = TlsIdentity::load(&certificate).expect("load test identity");
        let router = Arc::new(RecordingRouter::default());
        let listener = HttpsListenerSet::bind_enabled(
            &[HttpsInterface {
                bind_addr: "127.0.0.1:0".parse().expect("bind"),
                advertise_host: "localhost".parse().expect("host"),
                enabled: true,
            }],
            &certificate,
            vec![trusted("localhost")],
            router.clone(),
        )
        .expect("one listener");
        let address = listener.listeners[0].address;
        listener
            .refresh_trusted_peers(vec![TrustedPeer {
                host: "localhost".parse().expect("host"),
                fingerprint: certificate.fingerprint.clone(),
                enabled: true,
                https_port: std::num::NonZeroU16::new(43101).expect("port"),
            }])
            .expect("refresh verifier");
        assert_eq!(
            listener.listeners[0].address, address,
            "refresh must retain the one daemon listener"
        );
        let config = client_config(
            &identity,
            &TrustedPeer {
                host: "localhost".parse().expect("host"),
                fingerprint: certificate.fingerprint.clone(),
                enabled: true,
                https_port: std::num::NonZeroU16::new(43101).expect("port"),
            },
        )
        .expect("client config");
        let stream = TcpStream::connect(address).expect("same listener reachable");
        let connection = ClientConnection::new(
            Arc::new(config),
            ServerName::try_from("localhost".to_string()).expect("server name"),
        )
        .expect("connection");
        let mut tls = StreamOwned::new(connection, stream);
        complete_handshake(&mut tls).expect("refreshed trust handshake");
        let request = RequestEnvelope::Doctor(DoctorQuery::default());
        write_http_request(&mut tls, &request).expect("write request");
        let _ = read_http_response(&mut tls, &request).expect("shared response");
        assert!(router.routed.load(Ordering::SeqCst));
        listener.shutdown().expect("shutdown");
    }

    #[test]
    fn untrusted_mtls_peer_is_rejected_before_router() {
        let certificate = test_certificate();
        let identity = TlsIdentity::load(&certificate).expect("load test identity");
        let router = Arc::new(RecordingRouter::default());
        let listener = HttpsListenerSet::bind_enabled(
            &[HttpsInterface {
                bind_addr: "127.0.0.1:0".parse().expect("bind address"),
                advertise_host: "localhost".parse().expect("host"),
                enabled: true,
            }],
            &certificate,
            vec![TrustedPeer {
                host: "localhost".parse().expect("host"),
                fingerprint: CertificateFingerprint::from_str(&"00".repeat(32))
                    .expect("fingerprint"),
                enabled: true,
                https_port: std::num::NonZeroU16::new(43101).expect("non-zero"),
            }],
            router.clone(),
        )
        .expect("start listener");
        let stream = TcpStream::connect(listener.listeners[0].address).expect("connect");
        let config = client_config(
            &identity,
            &TrustedPeer {
                host: "localhost".parse::<HostName>().expect("host"),
                fingerprint: certificate.fingerprint.clone(),
                enabled: true,
                https_port: std::num::NonZeroU16::new(43101).expect("non-zero"),
            },
        )
        .expect("client config");
        let connection = ClientConnection::new(
            Arc::new(config),
            ServerName::try_from("localhost".to_string()).expect("server name"),
        )
        .expect("client connection");
        let mut tls = StreamOwned::new(connection, stream);
        complete_handshake(&mut tls).expect("client completes its handshake flight");
        let request = RequestEnvelope::Doctor(DoctorQuery::default());
        if write_http_request(&mut tls, &request).is_ok() {
            assert!(read_http_response(&mut tls, &request).is_err());
        }
        assert!(!router.routed.load(Ordering::SeqCst));
        listener.shutdown().expect("shutdown listener");
    }

    #[test]
    fn advertised_ip_peer_send_and_ack_reach_the_shared_router() {
        let (advertised_ip, advertised_host) = non_loopback_interface();
        let certificate = test_certificate();
        let identity = TlsIdentity::load(&certificate).expect("load test identity");
        let router = Arc::new(RecordingRouter::default());
        let peer = TrustedPeer {
            host: advertised_host.clone(),
            fingerprint: certificate.fingerprint.clone(),
            enabled: true,
            https_port: std::num::NonZeroU16::new(43101).expect("non-zero"),
        };
        let listener = HttpsListenerSet::bind_enabled(
            &[HttpsInterface {
                bind_addr: SocketAddr::new(advertised_ip, 0),
                advertise_host: peer.host.clone(),
                enabled: true,
            }],
            &certificate,
            vec![peer.clone()],
            router.clone(),
        )
        .expect("start listener");
        let stream = TcpStream::connect(listener.listeners[0].address).expect("connect");
        let config = client_config(&identity, &peer).expect("client config");
        let connection = ClientConnection::new(
            Arc::new(config),
            ServerName::try_from(peer.host.to_string()).expect("server name"),
        )
        .expect("client connection");
        let mut tls = StreamOwned::new(connection, stream);
        complete_handshake(&mut tls).expect("mutual TLS handshake");
        let mut write = WriteRequest::new(
            std::env::temp_dir(),
            std::env::temp_dir(),
            "sender".parse().expect("sender"),
            &format!("recipient@test-team.{advertised_host}"),
            "test-team".parse().expect("team"),
            SendMessageSource::Inline("message".to_string()),
            None,
            true,
            None,
            false,
        )
        .expect("write request");
        let origin_message_id = atm_core::schema::AtmMessageId::new();
        write.origin_message_id = Some(origin_message_id);
        write.authenticated_source_host = Some("spoofed.invalid".parse().expect("host"));
        let request = RequestEnvelope::Write(Box::new(write));
        write_http_request(&mut tls, &request).expect("write shared request");
        let _ = read_http_response(&mut tls, &request).expect("shared router response");
        let ack_source_id = AtmMessageId::new();
        let mut ack = crate::test_support::test_ack_write_request(
            std::env::temp_dir(),
            std::env::temp_dir(),
            "sender".parse().expect("sender"),
            "test-team".parse().expect("team"),
            ack_source_id,
            "acknowledged",
        );
        let ack_origin_id = AtmMessageId::new();
        ack.origin_message_id = Some(ack_origin_id);
        let ack_request = RequestEnvelope::Write(Box::new(ack));
        let stream = TcpStream::connect(listener.listeners[0].address).expect("connect");
        let config = client_config(&identity, &peer).expect("client config");
        let connection = ClientConnection::new(
            Arc::new(config),
            ServerName::try_from("localhost".to_string()).expect("server name"),
        )
        .expect("client connection");
        let mut tls = StreamOwned::new(connection, stream);
        complete_handshake(&mut tls).expect("mutual TLS handshake");
        write_http_request(&mut tls, &ack_request).expect("write canonical ack request");
        let _ = read_http_response(&mut tls, &ack_request).expect("shared router response");

        let requests = router.requests.lock().expect("recorded requests");
        assert_eq!(requests.len(), 2);
        let ApiRequest::Write(write) = &requests[0] else {
            panic!("expected canonical write request");
        };
        assert_eq!(write.authenticated_source_host, Some(peer.host.clone()));
        assert_eq!(write.origin_message_id, Some(origin_message_id));
        assert_eq!(
            write.to.as_ref().expect("destination").host(),
            Some(&advertised_host)
        );
        assert!(write.acknowledges_message_id.is_none());
        let ApiRequest::Write(ack) = &requests[1] else {
            panic!("expected canonical acknowledgement write request");
        };
        assert_eq!(ack.authenticated_source_host, Some(peer.host));
        assert_eq!(ack.origin_message_id, Some(ack_origin_id));
        assert_eq!(ack.acknowledges_message_id, Some(ack_source_id));
        assert_eq!(
            router.ingress.lock().expect("recorded ingress").as_ref(),
            Some(&AuthenticatedIngress::Peer),
            "the peer TCP listener must present the advertised-IP write to the one shared router as peer ingress"
        );
        listener.shutdown().expect("shutdown listener");
    }

    #[test]
    fn invalid_enabled_interface_leaves_no_partial_listener() {
        let certificate = test_certificate();
        let first = TcpListener::bind("127.0.0.1:0")
            .expect("reserve first address")
            .local_addr()
            .expect("first address");
        let blocker = TcpListener::bind("127.0.0.1:0").expect("reserve blocking address");
        let blocked = blocker.local_addr().expect("blocking address");
        drop(TcpListener::bind(first).expect("first address remains available"));
        let result = HttpsListenerSet::bind_enabled(
            &[
                HttpsInterface {
                    bind_addr: first,
                    advertise_host: "localhost".parse().expect("host"),
                    enabled: true,
                },
                HttpsInterface {
                    bind_addr: blocked,
                    advertise_host: "localhost".parse().expect("host"),
                    enabled: true,
                },
            ],
            &certificate,
            vec![],
            Arc::new(RecordingRouter::default()),
        );
        assert!(
            result.is_err(),
            "occupied enabled interface must reject startup"
        );
        TcpListener::bind(first).expect("failed startup must release earlier bound interface");
    }

    #[test]
    fn peer_connection_admission_is_deterministic_and_reserves_capacity_atomically() {
        assert_eq!(
            peer_connection_admission(true),
            PeerConnectionAdmission::Stop,
            "the shutdown wake-up is never admitted as a peer request"
        );
        assert_eq!(
            peer_connection_admission(false),
            PeerConnectionAdmission::Admit
        );

        let registry = Arc::new(ActiveConnectionRegistry::default());
        let reservations: Vec<_> = (0..MAX_PEER_HTTP_CONNECTIONS)
            .map(|_| {
                registry
                    .try_register(MAX_PEER_HTTP_CONNECTIONS)
                    .expect("slot")
            })
            .collect();
        assert!(
            registry.try_register(MAX_PEER_HTTP_CONNECTIONS).is_none(),
            "the connection cap is enforced before a worker is spawned"
        );
        drop(reservations);
        assert!(registry.try_register(MAX_PEER_HTTP_CONNECTIONS).is_some());
    }

    fn test_certificate() -> LocalCertificate {
        let certificate = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
            .expect("generate certificate");
        let bundle = format!(
            "{}{}",
            certificate.cert.pem(),
            certificate.key_pair.serialize_pem()
        );
        let path = tempfile::NamedTempFile::new().expect("temporary PEM");
        std::fs::write(path.path(), bundle).expect("write temporary PEM");
        let path = path.into_temp_path().keep().expect("retain temporary PEM");
        let parsed = TlsIdentity::load(&LocalCertificate {
            fingerprint: CertificateFingerprint::from_str(&"0".repeat(64))
                .expect("placeholder fingerprint"),
            private_key_ref: PrivateKeyRef::from_str(&path.display().to_string())
                .expect("private key reference"),
        });
        let fingerprint = match parsed {
            Ok(_) => unreachable!("placeholder fingerprint must not match"),
            Err(_) => {
                let pem = std::fs::read(&path).expect("read temporary PEM");
                let certificate_der = CertificateDer::pem_slice_iter(&pem)
                    .next()
                    .expect("certificate result")
                    .expect("certificate");
                super::certificate_fingerprint(&certificate_der)
            }
        };
        LocalCertificate {
            fingerprint: CertificateFingerprint::from_str(&fingerprint).expect("fingerprint"),
            private_key_ref: PrivateKeyRef::from_str(&path.display().to_string())
                .expect("private key reference"),
        }
    }
}
