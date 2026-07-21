//! HTTPS peer adapter.
//!
//! This module is intentionally only a transport adapter: it authenticates a
//! TCP/TLS peer, translates the shared HTTP envelope, and calls `ApiRouter`.
//! It has no mailbox, roster, acknowledgement, nudge, receipt, retry, or
//! replay state.

use std::fmt;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use atm_core::api::{
    ApiRouter, AuthenticatedIngress, RequestDeadline, decode_request, read_http_request,
    read_http_response, write_http_request, write_http_response,
};
use atm_core::error::AtmError;
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};
use atm_core::send::WriteRequest;
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

/// The independently bounded network stages for one peer request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HttpsRequestDeadline {
    pub(crate) connect: Duration,
    pub(crate) handshake: Duration,
    pub(crate) request: Duration,
}

impl Default for HttpsRequestDeadline {
    fn default() -> Self {
        Self {
            connect: HTTPS_TIMEOUT,
            handshake: HTTPS_TIMEOUT,
            request: HTTPS_TIMEOUT,
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
        deadline: HttpsRequestDeadline,
    ) -> Result<ResponseEnvelope, AtmError>;
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
        let pem = std::fs::read(path).map_err(|_source| {
            AtmError::daemon_unavailable("failed to open configured TLS certificate/key PEM bundle")
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
        deadline: HttpsRequestDeadline,
    ) -> Result<ResponseEnvelope, AtmError> {
        if !peer.enabled {
            return Err(AtmError::validation("configured HTTPS peer is disabled"));
        }
        let host = peer.host.to_string();
        let address = resolve_peer_address(&host)?;
        let stream = TcpStream::connect_timeout(&address, deadline.connect).map_err(|_source| {
            AtmError::daemon_unavailable(format!("failed to connect to HTTPS peer {host}"))
        })?;
        apply_deadline(&stream, deadline.handshake)?;
        let config = client_config(&self.identity, peer)?;
        let server_name = ServerName::try_from(host.clone()).map_err(|_source| {
            AtmError::validation("configured HTTPS peer host is not a valid TLS server name")
        })?;
        let connection =
            ClientConnection::new(Arc::new(config), server_name).map_err(|_source| {
                AtmError::daemon_unavailable("failed to initialize HTTPS peer TLS client")
            })?;
        let mut tls = StreamOwned::new(connection, stream);
        complete_handshake(&mut tls)?;
        write_http_request(&mut tls, &RequestEnvelope::Write(Box::new(request)))?;
        apply_deadline(tls.get_ref(), deadline.request)?;
        read_http_response(&mut tls)
    }
}

/// Starts enabled peer listeners. The caller owns lifecycle shutdown through
/// `HttpsListenerSet::shutdown`; the listener has no daemon state of its own.
pub(crate) struct HttpsListenerSet {
    stop: Arc<std::sync::atomic::AtomicBool>,
    listeners: Vec<HttpsListener>,
    requests: Arc<Mutex<Vec<std::thread::JoinHandle<()>>>>,
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
        let server_config = Arc::new(server_config(&identity, peers)?);
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
            let thread = std::thread::Builder::new()
                .name("atm-https-peer-listener".to_string())
                .spawn(move || {
                    accept_loop(
                        listener,
                        thread_stop,
                        thread_config,
                        thread_router,
                        thread_requests,
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
        })
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
) {
    while !stop.load(std::sync::atomic::Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Err(error) = stream.set_nonblocking(false) {
                    tracing::warn!(
                        subsystem = "https_transport",
                        action = "configure_connection",
                        outcome = "failed",
                        %error,
                        "HTTPS peer listener could not configure an accepted connection"
                    );
                    continue;
                }
                let config = Arc::clone(&config);
                let router = Arc::clone(&router);
                let request = std::thread::Builder::new()
                    .name("atm-https-peer-request".to_string())
                    .spawn(move || {
                        if let Err(error) = handle_peer_connection(stream, config, router) {
                            tracing::warn!(
                                subsystem = "https_transport",
                                action = "request",
                                outcome = "rejected",
                                error_code = %error.code(),
                                error_message = %error.message(),
                                "HTTPS peer request was rejected before or during shared API routing"
                            );
                        }
                    })
                    .map_err(|_source| {
                        AtmError::daemon_unavailable("failed to start HTTPS request worker")
                    });
                match request {
                    Ok(request) => match requests.lock() {
                        Ok(mut active) => active.push(request),
                        Err(_) => tracing::error!(
                            subsystem = "https_transport",
                            action = "track_connection",
                            outcome = "failed",
                            "HTTPS request worker could not be tracked for shutdown"
                        ),
                    },
                    Err(error) => tracing::warn!(
                        subsystem = "https_transport",
                        action = "start_request",
                        outcome = "failed",
                        error_code = %error.code(),
                        "HTTPS listener rejected connection because request worker startup failed"
                    ),
                }
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

fn handle_peer_connection(
    stream: TcpStream,
    config: Arc<ServerConfig>,
    router: Arc<dyn ApiRouter + Send + Sync>,
) -> Result<(), AtmError> {
    apply_deadline(&stream, HTTPS_TIMEOUT)?;
    let connection = ServerConnection::new(config).map_err(|_source| {
        AtmError::daemon_unavailable("failed to initialize HTTPS peer TLS server")
    })?;
    let mut tls = StreamOwned::new(connection, stream);
    complete_server_handshake(&mut tls)?;
    let request = match read_http_request(&mut tls)? {
        Some(request) => request,
        None => return Ok(()),
    };
    // The custom client verifier completed during the read above. Routing is
    // intentionally impossible before that mTLS + exact fingerprint check.
    let request = decode_request(request)?;
    let response = router
        .route(
            request,
            AuthenticatedIngress::Peer,
            RequestDeadline::after(HTTPS_TIMEOUT),
        )
        .map(|response| response.into_inner())
        .unwrap_or_else(ResponseEnvelope::Error);
    write_http_response(&mut tls, &response)
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

fn complete_handshake(
    stream: &mut StreamOwned<ClientConnection, TcpStream>,
) -> Result<(), AtmError> {
    while stream.conn.is_handshaking() {
        stream
            .conn
            .complete_io(&mut stream.sock)
            .map_err(|_source| {
                AtmError::daemon_unavailable("HTTPS peer mutual-TLS handshake failed")
            })?;
    }
    Ok(())
}

fn resolve_peer_address(host: &str) -> Result<SocketAddr, AtmError> {
    use std::net::ToSocketAddrs;
    let mut addresses = format!("{host}:43101")
        .to_socket_addrs()
        .map_err(|_source| {
            AtmError::daemon_unavailable(format!("failed to resolve HTTPS peer {host}"))
        })?;
    addresses.next().ok_or_else(|| {
        AtmError::daemon_unavailable(format!("HTTPS peer {host} resolved to no addresses"))
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
        .map_err(|_source| AtmError::validation("configured TLS certificate/key pair is invalid"))
}

fn server_config(
    identity: &TlsIdentity,
    peers: Vec<TrustedPeer>,
) -> Result<ServerConfig, AtmError> {
    install_tls_provider();
    let verifier = Arc::new(PinnedClientVerifier::new(peers));
    ServerConfig::builder()
        .with_client_cert_verifier(verifier)
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
    fingerprints: Vec<String>,
    algorithms: rustls::crypto::WebPkiSupportedAlgorithms,
}

impl PinnedClientVerifier {
    fn new(peers: Vec<TrustedPeer>) -> Self {
        Self {
            fingerprints: peers
                .into_iter()
                .filter(|peer| peer.enabled)
                .map(|peer| normalize_fingerprint(peer.fingerprint.as_str()))
                .collect(),
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
        if self
            .fingerprints
            .iter()
            .any(|fingerprint| fingerprint == &certificate_fingerprint(end_entity))
        {
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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use atm_core::api::{
        ApiRequest, ApiResponse, ApiRouter, AuthenticatedIngress, RequestDeadline,
        read_http_response, write_http_request,
    };
    use atm_core::doctor::DoctorQuery;
    use atm_core::error::AtmError;
    use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};
    use atm_core::types::HostName;
    use atm_storage::{
        CertificateFingerprint, HttpsInterface, LocalCertificate, PrivateKeyRef, TrustedPeer,
    };
    use rustls::pki_types::ServerName;
    use rustls::pki_types::{CertificateDer, pem::PemObject};
    use rustls::{ClientConnection, StreamOwned};

    use super::{
        HttpsListenerSet, HttpsRequestDeadline, TlsIdentity, client_config, complete_handshake,
        normalize_fingerprint,
    };

    #[derive(Default)]
    struct RecordingRouter(AtomicBool);

    impl atm_core::boundary::sealed::Sealed for RecordingRouter {}

    impl ApiRouter for RecordingRouter {
        fn route(
            &self,
            _request: ApiRequest,
            ingress: AuthenticatedIngress,
            _deadline: RequestDeadline,
        ) -> Result<ApiResponse, AtmError> {
            assert_eq!(ingress, AuthenticatedIngress::Peer);
            self.0.store(true, Ordering::SeqCst);
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
    fn peer_deadline_defaults_bound_every_network_leg() {
        let deadline = HttpsRequestDeadline::default();
        assert_eq!(deadline.connect.as_secs(), 5);
        assert_eq!(deadline.handshake.as_secs(), 5);
        assert_eq!(deadline.request.as_secs(), 5);
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
        write_http_request(&mut tls, &RequestEnvelope::Doctor(DoctorQuery::default()))
            .expect("write shared request");
        let _ = read_http_response(&mut tls).expect("shared router response");
        assert!(router.0.load(Ordering::SeqCst));
        listener.shutdown().expect("shutdown listener");
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
        write_http_request(&mut tls, &RequestEnvelope::Doctor(DoctorQuery::default()))
            .expect("client writes request before server alert");
        assert!(read_http_response(&mut tls).is_err());
        assert!(!router.0.load(Ordering::SeqCst));
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
