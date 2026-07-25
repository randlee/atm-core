//! HTTPS peer adapter.
//!
//! This module is intentionally only a transport adapter: it authenticates a
//! TCP/TLS peer, translates the shared HTTP envelope, and calls `ApiRouter`.
//! It has no mailbox, roster, acknowledgement, nudge, receipt, retry, or
//! replay state.

use std::fmt;
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use atm_core::api::{
    ApiRequest, ApiRouter, AuthenticatedIngress, RequestDeadline, decode_request,
    read_http_request, read_http_response, write_http_request, write_http_response,
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

const HTTPS_TIMEOUT: Duration = Duration::from_secs(5);

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

/// Resolves a delivery target to one configured hostname authority. Literal
/// addresses are only aliases of exactly one fresh forward-DNS result; they
/// never become durable peer records and reverse DNS is deliberately absent.
pub(crate) fn resolve_peer_authority(
    target: &atm_core::types::HostName,
    peers: &[TrustedPeer],
) -> Result<TrustedPeer, AtmError> {
    if let Some(peer) = peers
        .iter()
        .find(|peer| peer.enabled && peer.host == *target)
    {
        return Ok(peer.clone());
    }
    let ip: IpAddr = target.as_str().parse().map_err(|_| {
        AtmError::daemon_unavailable(format!("no trusted HTTPS peer is configured for {target}"))
    })?;
    let matches = peers
        .iter()
        .filter(|peer| peer.enabled)
        .filter(|peer| {
            resolve_peer_addresses(peer, HTTPS_TIMEOUT)
                .is_ok_and(|addresses| addresses.contains(&ip))
        })
        .cloned()
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [peer] => Ok(peer.clone()),
        [] => Err(AtmError::daemon_unavailable(format!(
            "literal peer IP {target} matches no trusted hostname"
        ))),
        _ => Err(AtmError::validation(format!(
            "literal peer IP {target} matches multiple trusted hostnames"
        ))),
    }
}

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
            .map_err(|_source| AtmError::validation("configured TLS certificate PEM is invalid"))?;
        let private_key = PrivateKeyDer::from_pem_slice(&pem)
            .map_err(|_source| AtmError::validation("configured TLS private key PEM is invalid"))?;
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
    identity: TlsIdentity,
}

impl HttpsTransport {
    pub(crate) fn from_local_certificate(certificate: &LocalCertificate) -> Result<Self, AtmError> {
        Ok(Self {
            identity: TlsIdentity::load(certificate)?,
        })
    }
}

impl HttpsMessageTransport for HttpsTransport {
    fn deliver(
        &self,
        request: WriteRequest,
        peer: &TrustedPeer,
        deadline: RequestDeadline,
    ) -> Result<ResponseEnvelope, AtmError> {
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
                    "failed to connect to HTTPS peer {host}: {source}"
                ))
            },
        )?;
        apply_deadline(&stream, remaining_budget(deadline)?)?;
        let config = client_config(&self.identity, peer)?;
        let server_name = ServerName::try_from(host.clone()).map_err(|source| {
            AtmError::daemon_unavailable_with_cause(
                "configured HTTPS peer host is not a valid TLS server name",
                source,
            )
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
        let request = RequestEnvelope::Write(Box::new(request));
        apply_deadline(tls.get_ref(), remaining_budget(deadline)?)?;
        write_http_request(&mut tls, &request)?;
        apply_deadline(tls.get_ref(), remaining_budget(deadline)?)?;
        read_http_response(&mut tls, &request)
    }
}

/// Starts enabled peer listeners. The caller owns lifecycle shutdown through
/// `HttpsListenerSet::shutdown`; the listener has no daemon state of its own.
pub(crate) struct HttpsListenerSet {
    stop: Arc<std::sync::atomic::AtomicBool>,
    listeners: Vec<HttpsListener>,
    requests: Arc<Mutex<Vec<std::thread::JoinHandle<()>>>>,
    peer_verifier: Arc<PinnedClientVerifier>,
}

struct HttpsListener {
    address: SocketAddr,
    thread: Option<std::thread::JoinHandle<()>>,
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
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        // Bind every enabled endpoint before any accept loop starts. A bad
        // later row therefore cannot leave an earlier listener partially live.
        let bound = interfaces
            .iter()
            .filter(|interface| interface.enabled)
            .map(|interface| {
                let listener = TcpListener::bind(interface.bind_addr).map_err(|_source| {
                    AtmError::daemon_unavailable(format!(
                        "failed to bind configured HTTPS listener {}",
                        interface.bind_addr
                    ))
                })?;
                listener.set_nonblocking(true).map_err(|_source| {
                    AtmError::daemon_unavailable("failed to configure HTTPS listener")
                })?;
                let address = listener.local_addr().map_err(|_source| {
                    AtmError::daemon_unavailable("failed to inspect configured HTTPS listener")
                })?;
                Ok((listener, address))
            })
            .collect::<Result<Vec<_>, AtmError>>()?;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let mut listeners = Vec::new();
        for (listener, address) in bound {
            let thread_stop = Arc::clone(&stop);
            let thread_router = Arc::clone(&router);
            let thread_config = Arc::clone(&server_config);
            let thread_requests = Arc::clone(&requests);
            let thread_peer_verifier = Arc::clone(&peer_verifier);
            let thread = std::thread::Builder::new()
                .name("atm-https-peer-listener".to_string())
                .spawn(move || {
                    accept_loop(
                        listener,
                        thread_stop,
                        thread_config,
                        thread_router,
                        thread_requests,
                        thread_peer_verifier,
                    )
                })
                .map_err(|_source| {
                    AtmError::daemon_unavailable("failed to start HTTPS listener")
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
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        for listener in &self.listeners {
            let _ = TcpStream::connect_timeout(&listener.address, Duration::from_millis(100));
        }
        for listener in &mut self.listeners {
            if let Some(thread) = listener.thread.take() {
                thread.join().map_err(|_| {
                    AtmError::daemon_unavailable("HTTPS listener worker panicked during shutdown")
                })?;
            }
        }
        let requests =
            std::mem::take(&mut *self.requests.lock().map_err(|_| {
                AtmError::daemon_unavailable("HTTPS request registry lock poisoned")
            })?);
        for request in requests {
            request.join().map_err(|_| {
                AtmError::daemon_unavailable("HTTPS request worker panicked during shutdown")
            })?;
        }
        Ok(())
    }
}

fn accept_loop(
    listener: TcpListener,
    stop: Arc<std::sync::atomic::AtomicBool>,
    config: Arc<ServerConfig>,
    router: Arc<dyn ApiRouter + Send + Sync>,
    requests: Arc<Mutex<Vec<std::thread::JoinHandle<()>>>>,
    peer_verifier: Arc<PinnedClientVerifier>,
) {
    while !stop.load(std::sync::atomic::Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                spawn_request_worker(stream, &config, &router, &requests, &peer_verifier);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => {
                tracing::warn!(
                    subsystem = "https_transport",
                    action = "accept",
                    outcome = "failed",
                    %error,
                    "HTTPS peer listener accept failed"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

fn spawn_request_worker(
    stream: TcpStream,
    config: &Arc<ServerConfig>,
    router: &Arc<dyn ApiRouter + Send + Sync>,
    requests: &Arc<Mutex<Vec<std::thread::JoinHandle<()>>>>,
    peer_verifier: &Arc<PinnedClientVerifier>,
) {
    if let Err(error) = stream.set_nonblocking(false) {
        tracing::warn!(
            subsystem = "https_transport",
            action = "configure_connection",
            outcome = "failed",
            %error,
            "HTTPS peer listener could not configure an accepted connection"
        );
        return;
    }
    let request = std::thread::Builder::new()
        .name("atm-https-peer-request".to_string())
        .spawn({
            let config = Arc::clone(config);
            let router = Arc::clone(router);
            let peer_verifier = Arc::clone(peer_verifier);
            move || {
                log_peer_request_result(handle_peer_connection(
                    stream,
                    config,
                    router,
                    peer_verifier,
                ))
            }
        })
        .map_err(|_source| AtmError::daemon_unavailable("failed to start HTTPS request worker"));
    match request {
        Ok(request) => track_request_worker(requests, request),
        Err(error) => tracing::warn!(
            subsystem = "https_transport",
            action = "start_request",
            outcome = "failed",
            error_code = %error.code(),
            "HTTPS listener rejected connection because request worker startup failed"
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
    requests: &Mutex<Vec<std::thread::JoinHandle<()>>>,
    request: std::thread::JoinHandle<()>,
) {
    match requests.lock() {
        Ok(mut active) => active.push(request),
        Err(_) => tracing::error!(
            subsystem = "https_transport",
            action = "track_connection",
            outcome = "failed",
            "HTTPS request worker could not be tracked for shutdown"
        ),
    }
}

fn handle_peer_connection(
    stream: TcpStream,
    config: Arc<ServerConfig>,
    router: Arc<dyn ApiRouter + Send + Sync>,
    peer_verifier: Arc<PinnedClientVerifier>,
) -> Result<(), AtmError> {
    let deadline = RequestDeadline::after(HTTPS_TIMEOUT);
    apply_deadline(&stream, HTTPS_TIMEOUT)?;
    let connection = ServerConnection::new(config).map_err(|_source| {
        AtmError::daemon_unavailable("failed to initialize HTTPS peer TLS server")
    })?;
    let mut tls = StreamOwned::new(connection, stream);
    complete_server_handshake(&mut tls)?;
    let authenticated_source_host = peer_verifier.authenticated_host(&tls.conn)?;
    let request = match read_http_request(&mut tls)? {
        Some(request) => request,
        None => return Ok(()),
    };
    // The custom client verifier completed during the read above. Routing is
    // intentionally impossible before that mTLS + exact fingerprint check.
    let mut request = decode_request(request)?;
    normalize_peer_write_for_local_delivery(&mut request, authenticated_source_host);
    let response = router
        .route(request, AuthenticatedIngress::Peer, deadline)
        .map(|response| response.into_inner())
        .unwrap_or_else(ResponseEnvelope::Error);
    write_http_response(&mut tls, &response)
}

/// The authenticated HTTPS adapter has already selected this daemon. It drops
/// only the transport destination before submitting the same canonical write
/// request used by local UDS and graft clients.
fn normalize_peer_write_for_local_delivery(request: &mut ApiRequest, source_host: HostName) {
    if let ApiRequest::Write(write) = request {
        if let Some(destination) = write.to.as_mut() {
            destination.host = None;
        }
        write.authenticated_source_host = Some(source_host);
    }
}

fn complete_server_handshake(
    stream: &mut StreamOwned<ServerConnection, TcpStream>,
) -> Result<(), AtmError> {
    while stream.conn.is_handshaking() {
        stream
            .conn
            .complete_io(&mut stream.sock)
            .map_err(|source| {
                AtmError::daemon_unavailable(format!(
                    "HTTPS peer mutual-TLS server handshake failed: {source}"
                ))
            })?;
    }
    Ok(())
}

fn apply_deadline(stream: &TcpStream, deadline: Duration) -> Result<(), AtmError> {
    stream.set_read_timeout(Some(deadline)).map_err(|_source| {
        AtmError::daemon_unavailable("failed to set HTTPS peer read deadline")
    })?;
    stream
        .set_write_timeout(Some(deadline))
        .map_err(|_source| AtmError::daemon_unavailable("failed to set HTTPS peer write deadline"))
}

fn complete_handshake_with_deadline(
    stream: &mut StreamOwned<ClientConnection, TcpStream>,
    deadline: RequestDeadline,
) -> Result<(), AtmError> {
    while stream.conn.is_handshaking() {
        apply_deadline(stream.get_ref(), remaining_budget(deadline)?)?;
        stream
            .conn
            .complete_io(&mut stream.sock)
            .map_err(|source| {
                AtmError::remote_delivery_unconfirmed(format!(
                    "HTTPS peer mutual-TLS handshake was not confirmed before the shared deadline: {source}"
                ))
            })?;
    }
    Ok(())
}

#[cfg(test)]
fn complete_handshake(
    stream: &mut StreamOwned<ClientConnection, TcpStream>,
) -> Result<(), AtmError> {
    complete_handshake_with_deadline(stream, RequestDeadline::after(HTTPS_TIMEOUT))
}

fn remaining_budget(deadline: RequestDeadline) -> Result<Duration, AtmError> {
    deadline.remaining().ok_or_else(|| {
        AtmError::remote_delivery_unconfirmed(
            "local persistence completed before the peer accepted the write deadline expired",
        )
    })
}

fn resolve_peer_addresses(peer: &TrustedPeer, timeout: Duration) -> Result<Vec<IpAddr>, AtmError> {
    let authority = format!("{}:{}", peer.host, peer.https_port);
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("atm-peer-dns".to_string())
        .spawn(move || {
            let _ = sender.send(
                authority
                    .to_socket_addrs()
                    .map(|a| a.map(|address| address.ip()).collect::<Vec<_>>()),
            );
        })
        .map_err(|source| {
            AtmError::daemon_unavailable_with_cause(
                "failed to start bounded HTTPS DNS resolution",
                source,
            )
        })?;
    receiver
        .recv_timeout(timeout)
        .map_err(|source| {
            AtmError::daemon_unavailable_with_cause(
                "HTTPS DNS resolution timed out; verify peer forward DNS or retry",
                source,
            )
        })?
        .map_err(|source| {
            AtmError::daemon_unavailable_with_cause(
                "failed to resolve configured HTTPS peer; verify forward DNS",
                source,
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
    resolve_peer_addresses(&peer, timeout)?
        .into_iter()
        .next()
        .map(|ip| SocketAddr::new(ip, port))
        .ok_or_else(|| AtmError::daemon_unavailable("HTTPS peer resolved to no addresses"))
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
        .map_err(|_source| AtmError::validation("configured TLS certificate/key pair is invalid"))
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
        .map_err(|_source| AtmError::validation("configured TLS certificate/key pair is invalid"))
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
        let mut current = self
            .peers
            .write()
            .map_err(|_| AtmError::daemon_unavailable("HTTPS peer verifier lock poisoned"))?;
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

#[cfg(test)]
mod tests {
    use std::net::{TcpListener, TcpStream};
    use std::str::FromStr as _;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use atm_core::api::{
        ApiRequest, ApiResponse, ApiRouter, AuthenticatedIngress, RequestDeadline,
        read_http_response, write_http_request,
    };
    use atm_core::doctor::DoctorQuery;
    use atm_core::error::AtmError;
    use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};
    use atm_core::send::{SendMessageSource, WriteRequest};
    use atm_core::types::HostName;
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

    #[test]
    fn literal_ip_selects_its_single_forward_dns_authority() {
        let target = "127.0.0.1".parse().expect("target");
        assert_eq!(
            super::resolve_peer_authority(&target, &[trusted("localhost")])
                .expect("authority")
                .host
                .as_str(),
            "localhost"
        );
    }

    #[test]
    fn literal_ip_without_authority_fails_closed() {
        let target = "192.0.2.1".parse().expect("target");
        assert!(super::resolve_peer_authority(&target, &[trusted("localhost")]).is_err());
    }

    #[test]
    fn literal_ip_with_ambiguous_authority_fails_closed() {
        let target = "127.0.0.1".parse().expect("target");
        assert!(
            super::resolve_peer_authority(&target, &[trusted("localhost"), trusted("localhost")])
                .is_err()
        );
    }
    use rustls::pki_types::ServerName;
    use rustls::pki_types::{CertificateDer, pem::PemObject};
    use rustls::{ClientConnection, StreamOwned};

    use super::{
        HttpsListenerSet, TlsIdentity, client_config, complete_handshake, normalize_fingerprint,
    };

    #[derive(Default)]
    struct RecordingRouter {
        routed: AtomicBool,
        request: Mutex<Option<ApiRequest>>,
    }

    impl atm_core::boundary::sealed::Sealed for RecordingRouter {}

    impl ApiRouter for RecordingRouter {
        fn route(
            &self,
            _request: ApiRequest,
            ingress: AuthenticatedIngress,
            _deadline: RequestDeadline,
        ) -> Result<ApiResponse, AtmError> {
            assert_eq!(ingress, AuthenticatedIngress::Peer);
            self.routed.store(true, Ordering::SeqCst);
            *self.request.lock().expect("recorded request") = Some(_request);
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
    fn request_deadline_is_one_absolute_budget() {
        let deadline = RequestDeadline::after(Duration::from_secs(5));
        assert!(deadline.remaining().is_some());
    }

    #[test]
    fn expired_peer_budget_reports_remote_delivery_unconfirmed() {
        let error = super::remaining_budget(RequestDeadline::after(Duration::ZERO))
            .expect_err("an expired shared request budget must fail before peer delivery");
        assert_eq!(error.code().as_str(), "REMOTE_DELIVERY_UNCONFIRMED");
    }

    #[test]
    fn stalled_tls_handshake_reports_remote_delivery_unconfirmed() {
        let certificate = test_certificate();
        let identity = TlsIdentity::load(&certificate).expect("load test identity");
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind stalled peer");
        let address = listener.local_addr().expect("listener address");
        let server = std::thread::spawn(move || {
            let (_stream, _) = listener.accept().expect("accept client hello");
            std::thread::sleep(Duration::from_millis(100));
        });
        let peer = TrustedPeer {
            host: "localhost".parse().expect("host"),
            fingerprint: certificate.fingerprint.clone(),
            enabled: true,
            https_port: std::num::NonZeroU16::new(43101).expect("port"),
        };
        let config = client_config(&identity, &peer).expect("client config");
        let connection = ClientConnection::new(
            Arc::new(config),
            ServerName::try_from("localhost".to_string()).expect("server name"),
        )
        .expect("client connection");
        let stream = TcpStream::connect(address).expect("connect stalled peer");
        let mut tls = StreamOwned::new(connection, stream);

        let error = super::complete_handshake_with_deadline(
            &mut tls,
            RequestDeadline::after(Duration::from_millis(25)),
        )
        .expect_err("stalled peer TLS handshake must time out");
        assert_eq!(error.code().as_str(), "REMOTE_DELIVERY_UNCONFIRMED");
        server.join().expect("stalled peer exits");
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
    fn shutdown_joins_an_incomplete_peer_request_within_its_deadline() {
        let certificate = test_certificate();
        let listener = HttpsListenerSet::bind_enabled(
            &[HttpsInterface {
                bind_addr: "127.0.0.1:0".parse().expect("bind address"),
                advertise_host: "localhost".parse().expect("host"),
                enabled: true,
            }],
            &certificate,
            vec![],
            Arc::new(RecordingRouter::default()),
        )
        .expect("start listener");
        let _incomplete = TcpStream::connect(listener.listeners[0].address)
            .expect("open incomplete peer connection");
        let wait_started = Instant::now();
        while listener
            .requests
            .lock()
            .expect("request registry")
            .is_empty()
        {
            assert!(
                wait_started.elapsed() < Duration::from_secs(1),
                "listener must retain each accepted request before shutdown"
            );
            std::thread::yield_now();
        }

        let started = Instant::now();
        listener.shutdown().expect("shutdown listener");
        assert!(
            started.elapsed() < Duration::from_secs(6),
            "shutdown must join the bounded peer I/O worker"
        );
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
    fn authenticated_peer_host_overwrites_wire_source_host_on_shared_write() {
        let certificate = test_certificate();
        let identity = TlsIdentity::load(&certificate).expect("load test identity");
        let router = Arc::new(RecordingRouter::default());
        let peer = TrustedPeer {
            host: "localhost".parse().expect("host"),
            fingerprint: certificate.fingerprint.clone(),
            enabled: true,
            https_port: std::num::NonZeroU16::new(43101).expect("non-zero"),
        };
        let listener = HttpsListenerSet::bind_enabled(
            &[HttpsInterface {
                bind_addr: "127.0.0.1:0".parse().expect("bind address"),
                advertise_host: "localhost".parse().expect("host"),
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
            ServerName::try_from("localhost".to_string()).expect("server name"),
        )
        .expect("client connection");
        let mut tls = StreamOwned::new(connection, stream);
        complete_handshake(&mut tls).expect("mutual TLS handshake");
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
        write.authenticated_source_host = Some("spoofed.invalid".parse().expect("host"));
        let request = RequestEnvelope::Write(Box::new(write));
        write_http_request(&mut tls, &request).expect("write shared request");
        let _ = read_http_response(&mut tls, &request).expect("shared router response");
        let request = router
            .request
            .lock()
            .expect("recorded request")
            .clone()
            .expect("router request");
        let ApiRequest::Write(write) = request else {
            panic!("expected canonical write request");
        };
        assert_eq!(write.authenticated_source_host, Some(peer.host));
        assert_eq!(write.origin_message_id, Some(origin_message_id));
        assert!(write.to.expect("destination").host.is_none());
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
    fn shutdown_bounds_an_incomplete_peer_handshake() {
        let certificate = test_certificate();
        let listener = HttpsListenerSet::bind_enabled(
            &[HttpsInterface {
                bind_addr: "127.0.0.1:0".parse().expect("bind address"),
                advertise_host: "localhost".parse().expect("host"),
                enabled: true,
            }],
            &certificate,
            vec![],
            Arc::new(RecordingRouter::default()),
        )
        .expect("start listener");
        let _incomplete = TcpStream::connect(listener.listeners[0].address)
            .expect("open incomplete peer connection");
        let wait_started = Instant::now();
        while listener
            .requests
            .lock()
            .expect("request registry")
            .is_empty()
        {
            assert!(
                wait_started.elapsed() < Duration::from_secs(1),
                "listener must retain the accepted request before shutdown"
            );
            std::thread::yield_now();
        }

        let started = Instant::now();
        listener.shutdown().expect("shutdown listener");
        assert!(
            started.elapsed() < Duration::from_secs(6),
            "shutdown must not wait beyond the bounded peer I/O deadline"
        );
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
