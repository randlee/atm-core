//! The sole concrete mutual-TLS byte-stream adapter for ATM peer transport.
//!
//! This crate intentionally knows only configured peer identity and byte
//! streams. It owns neither HTTP nor any ATM request, route, persistence,
//! acknowledgement, retry, hook, daemon lifecycle, or plaintext fallback.

use std::{fmt, sync::Arc, time::Duration};

use atm_storage::{
    AtmError, HostName, PeerConfigStore, PinnedClientVerifier, TlsIdentity, TrustedPeer,
    certificate_fingerprint, certificate_valid_now, install_tls_provider,
};
use rustls::{
    CertificateError, ClientConfig, DigitallySignedStruct, Error, ServerConfig, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use tokio::{net::TcpStream, time::timeout};
use tokio_rustls::{TlsAcceptor, TlsConnector, TlsStream};

const DEFAULT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// Concrete mTLS facade assembled only after the daemon has selected mTLS.
///
/// The adapter snapshots durable interface, local identity, and trust data at
/// composition time. It cannot infer or select a peer-wire mode itself.
pub struct MtlsPeerStreamAdapter {
    identity: TlsIdentity,
    trusted_peers: Vec<TrustedPeer>,
    handshake_timeout: Duration,
}

impl fmt::Debug for MtlsPeerStreamAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MtlsPeerStreamAdapter")
            .finish_non_exhaustive()
    }
}

impl MtlsPeerStreamAdapter {
    /// Snapshot valid mTLS configuration from the storage-neutral control plane.
    pub fn from_peer_config(store: &(dyn PeerConfigStore + Send + Sync)) -> Result<Self, AtmError> {
        let has_enabled_interface = store
            .list_interfaces()?
            .into_iter()
            .any(|interface| interface.enabled);
        if !has_enabled_interface {
            return Err(AtmError::peer_config_validation(
                "mTLS requires one enabled peer interface",
            ));
        }
        let certificate = store.local_certificate()?.ok_or_else(|| {
            AtmError::peer_config_validation("mTLS requires a configured local identity")
        })?;
        let identity = TlsIdentity::load(&certificate).map_err(|error| {
            AtmError::certificate_operation("configured mTLS identity is invalid")
                .with_cause(error.detail())
        })?;
        Ok(Self {
            identity,
            trusted_peers: store.list_trusted_peers()?,
            handshake_timeout: DEFAULT_HANDSHAKE_TIMEOUT,
        })
    }

    /// Narrow test/operations hook for a bounded handshake deadline.
    #[must_use]
    pub fn with_handshake_timeout(mut self, handshake_timeout: Duration) -> Self {
        self.handshake_timeout = handshake_timeout;
        self
    }

    /// Authenticate an outbound TCP byte stream against the exact configured authority and pin.
    pub async fn connect(
        &self,
        tcp: TcpStream,
        peer: &HostName,
    ) -> Result<TlsStream<TcpStream>, AtmError> {
        let configured = self.trusted_peer(peer)?;
        let server_name = ServerName::try_from(peer.as_str().to_owned()).map_err(|_| {
            AtmError::peer_authentication("configured peer hostname is not valid for mTLS")
        })?;
        let connector = TlsConnector::from(Arc::new(self.client_config(configured)?));
        timeout(self.handshake_timeout, connector.connect(server_name, tcp))
            .await
            .map_err(|_| AtmError::peer_authentication("mTLS peer handshake deadline expired"))?
            .map(|stream| stream.into())
            .map_err(|source| {
                AtmError::peer_authentication(
                    "mTLS peer handshake failed before application processing",
                )
                .with_cause(source)
            })
    }

    /// Authenticate an inbound TCP byte stream against the configured client pins.
    pub async fn accept(&self, tcp: TcpStream) -> Result<TlsStream<TcpStream>, AtmError> {
        let acceptor = TlsAcceptor::from(Arc::new(self.server_config()?));
        timeout(self.handshake_timeout, acceptor.accept(tcp))
            .await
            .map_err(|_| AtmError::peer_authentication("mTLS peer handshake deadline expired"))?
            .map(|stream| stream.into())
            .map_err(|source| {
                AtmError::peer_authentication(
                    "mTLS client certificate was rejected before application processing",
                )
                .with_cause(source)
            })
    }

    fn trusted_peer(&self, peer: &HostName) -> Result<&TrustedPeer, AtmError> {
        self.trusted_peers
            .iter()
            .find(|candidate| candidate.enabled && candidate.host == *peer)
            .ok_or_else(|| {
                AtmError::peer_authentication(
                    "peer is not an enabled exact mTLS authority; plaintext fallback is forbidden",
                )
            })
    }

    fn client_config(&self, peer: &TrustedPeer) -> Result<ClientConfig, AtmError> {
        install_tls_provider();
        ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(PinnedServerVerifier::new(peer)?))
            .with_client_auth_cert(
                self.identity.certificates().to_vec(),
                self.identity.private_key().clone_key(),
            )
            .map_err(|source| {
                AtmError::certificate_operation(
                    "configured mTLS identity cannot authenticate clients",
                )
                .with_cause(source)
            })
    }

    fn server_config(&self) -> Result<ServerConfig, AtmError> {
        install_tls_provider();
        ServerConfig::builder()
            .with_client_cert_verifier(Arc::new(PinnedClientVerifier::new(
                self.trusted_peers.clone(),
            )))
            .with_single_cert(
                self.identity.certificates().to_vec(),
                self.identity.private_key().clone_key(),
            )
            .map_err(|source| {
                AtmError::certificate_operation(
                    "configured mTLS identity cannot authenticate peers",
                )
                .with_cause(source)
            })
    }
}

/// Exact fingerprint verifier for the selected outbound peer. The registered
/// hostname is checked before connection construction; the custom verifier
/// binds the subsequently presented certificate to that authority's pin.
#[derive(Debug)]
struct PinnedServerVerifier {
    hostname: String,
    fingerprint: String,
    algorithms: rustls::crypto::WebPkiSupportedAlgorithms,
}

impl PinnedServerVerifier {
    fn new(peer: &TrustedPeer) -> Result<Self, AtmError> {
        if !peer.host.is_durable_hostname() {
            return Err(AtmError::peer_authentication(
                "mTLS peer authority must use a durable hostname rather than a literal IP",
            ));
        }
        Ok(Self {
            hostname: peer.host.as_str().to_owned(),
            fingerprint: peer.fingerprint.as_str().to_ascii_lowercase(),
            algorithms: rustls::crypto::ring::default_provider().signature_verification_algorithms,
        })
    }
}

impl ServerCertVerifier for PinnedServerVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        let presented_name = server_name.to_str();
        if certificate_valid_now(end_entity).is_err()
            || presented_name != self.hostname
            || certificate_fingerprint(end_entity) != self.fingerprint
        {
            return Err(CertificateError::UnknownIssuer.into());
        }
        Ok(ServerCertVerified::assertion())
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

#[cfg(test)]
mod tests {
    use super::*;
    use atm_storage::{CertificateFingerprint, HttpsInterface, LocalCertificate, PrivateKeyRef};
    use std::{
        net::{IpAddr, Ipv4Addr, SocketAddr},
        num::NonZeroU16,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[derive(Default)]
    struct TestStore {
        interfaces: Vec<HttpsInterface>,
        certificate: Option<LocalCertificate>,
        peers: Vec<TrustedPeer>,
    }
    impl atm_storage::contract::sealed::Sealed for TestStore {}
    impl PeerConfigStore for TestStore {
        fn list_interfaces(&self) -> Result<Vec<HttpsInterface>, AtmError> {
            Ok(self.interfaces.clone())
        }
        fn save_interface(&self, _: &HttpsInterface) -> Result<(), AtmError> {
            unreachable!()
        }
        fn remove_interface(&self, _: SocketAddr) -> Result<bool, AtmError> {
            unreachable!()
        }
        fn local_certificate(&self) -> Result<Option<LocalCertificate>, AtmError> {
            Ok(self.certificate.clone())
        }
        fn save_local_certificate(&self, _: &LocalCertificate) -> Result<(), AtmError> {
            unreachable!()
        }
        fn list_trusted_peers(&self) -> Result<Vec<TrustedPeer>, AtmError> {
            Ok(self.peers.clone())
        }
        fn trusted_peer(&self, _: &HostName) -> Result<Option<TrustedPeer>, AtmError> {
            unreachable!()
        }
        fn save_trusted_peer(&self, _: &TrustedPeer) -> Result<(), AtmError> {
            unreachable!()
        }
        fn remove_trusted_peer(&self, _: &HostName) -> Result<bool, AtmError> {
            unreachable!()
        }
    }

    fn identity(directory: &tempfile::TempDir, name: &str) -> LocalCertificate {
        let generated =
            rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).expect("certificate");
        let path = directory.path().join(format!("{name}.pem"));
        std::fs::write(
            &path,
            format!(
                "{}{}",
                generated.cert.pem(),
                generated.key_pair.serialize_pem()
            ),
        )
        .expect("PEM");
        LocalCertificate {
            fingerprint: CertificateFingerprint::try_from(certificate_fingerprint(
                generated.cert.der(),
            ))
            .expect("fingerprint"),
            private_key_ref: PrivateKeyRef::try_from(path.display().to_string())
                .expect("key reference"),
        }
    }

    fn expired_identity(directory: &tempfile::TempDir) -> LocalCertificate {
        let mut parameters =
            rcgen::CertificateParams::new(vec!["localhost".to_owned()]).expect("parameters");
        parameters.not_before = rcgen::date_time_ymd(2020, 1, 1);
        parameters.not_after = rcgen::date_time_ymd(2020, 1, 2);
        let key = rcgen::KeyPair::generate().expect("key");
        let certificate = parameters.self_signed(&key).expect("certificate");
        let path = directory.path().join("expired.pem");
        std::fs::write(
            &path,
            format!("{}{}", certificate.pem(), key.serialize_pem()),
        )
        .expect("PEM");
        LocalCertificate {
            fingerprint: CertificateFingerprint::try_from(certificate_fingerprint(
                certificate.der(),
            ))
            .expect("fingerprint"),
            private_key_ref: PrivateKeyRef::try_from(path.display().to_string())
                .expect("key reference"),
        }
    }

    fn interface() -> HttpsInterface {
        HttpsInterface {
            bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            advertise_host: "localhost".parse().expect("host"),
            enabled: true,
        }
    }

    fn peer(certificate: &LocalCertificate, enabled: bool) -> TrustedPeer {
        TrustedPeer {
            host: "localhost".parse().expect("host"),
            fingerprint: certificate.fingerprint.clone(),
            enabled,
            https_port: NonZeroU16::new(443).expect("port"),
        }
    }

    #[tokio::test]
    async fn configured_pair_exchanges_opaque_bytes_over_mutual_tls() {
        let directory = tempfile::tempdir().expect("directory");
        let server_identity = identity(&directory, "server");
        let client_identity = identity(&directory, "client");
        let server = MtlsPeerStreamAdapter::from_peer_config(&TestStore {
            interfaces: vec![interface()],
            certificate: Some(server_identity.clone()),
            peers: vec![peer(&client_identity, true)],
        })
        .expect("server adapter");
        let client = MtlsPeerStreamAdapter::from_peer_config(&TestStore {
            interfaces: vec![interface()],
            certificate: Some(client_identity),
            peers: vec![peer(&server_identity, true)],
        })
        .expect("client adapter");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        let server_task = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.expect("accept tcp");
            let mut tls = server.accept(tcp).await.expect("accept tls");
            let mut received = [0_u8; 5];
            tls.read_exact(&mut received).await.expect("read bytes");
            assert_eq!(&received, b"bytes");
            tls.write_all(b"reply").await.expect("write bytes");
        });
        let tcp = TcpStream::connect(address).await.expect("connect tcp");
        let host: HostName = "localhost".parse().expect("host");
        let mut tls = client.connect(tcp, &host).await.expect("connect tls");
        tls.write_all(b"bytes").await.expect("write bytes");
        let mut reply = [0_u8; 5];
        tls.read_exact(&mut reply).await.expect("read bytes");
        assert_eq!(&reply, b"reply");
        server_task.await.expect("server task");
    }

    #[test]
    fn missing_or_disabled_interface_fails_before_any_tcp_or_http_work() {
        let directory = tempfile::tempdir().expect("directory");
        let identity = identity(&directory, "local");
        let error = MtlsPeerStreamAdapter::from_peer_config(&TestStore {
            interfaces: vec![],
            certificate: Some(identity),
            peers: vec![],
        })
        .expect_err("missing interface");
        assert_eq!(error.code().as_str(), "ATM_PEER_CONFIG_VALIDATION_FAILED");
    }

    #[test]
    fn missing_local_identity_fails_before_any_tcp_or_http_work() {
        let error = MtlsPeerStreamAdapter::from_peer_config(&TestStore {
            interfaces: vec![interface()],
            certificate: None,
            peers: vec![],
        })
        .expect_err("missing identity");
        assert_eq!(error.code().as_str(), "ATM_PEER_CONFIG_VALIDATION_FAILED");
    }

    #[test]
    fn disabled_or_hostname_mismatched_peer_fails_before_tcp_or_http_work() {
        let directory = tempfile::tempdir().expect("directory");
        let identity = identity(&directory, "local");
        let target = peer(&identity, false);
        let adapter = MtlsPeerStreamAdapter::from_peer_config(&TestStore {
            interfaces: vec![interface()],
            certificate: Some(identity),
            peers: vec![target],
        })
        .expect("adapter");
        let host: HostName = "localhost".parse().expect("host");
        let error = adapter.trusted_peer(&host).expect_err("disabled peer");
        assert_eq!(error.code().as_str(), "ATM_PEER_AUTHENTICATION_FAILED");
    }

    #[test]
    fn bad_key_is_rejected_as_a_non_secret_certificate_error() {
        let directory = tempfile::tempdir().expect("directory");
        let identity = identity(&directory, "local");
        std::fs::write(identity.private_key_ref.as_str(), "not a PEM bundle").expect("bad PEM");
        let error = MtlsPeerStreamAdapter::from_peer_config(&TestStore {
            interfaces: vec![interface()],
            certificate: Some(identity),
            peers: vec![],
        })
        .expect_err("bad key");
        assert_eq!(error.code().as_str(), "ATM_CERTIFICATE_OPERATION_FAILED");
        assert!(!error.message().contains("not a PEM bundle"));
    }

    #[test]
    fn expired_local_certificate_is_rejected_before_any_tcp_or_http_work() {
        let directory = tempfile::tempdir().expect("directory");
        let error = MtlsPeerStreamAdapter::from_peer_config(&TestStore {
            interfaces: vec![interface()],
            certificate: Some(expired_identity(&directory)),
            peers: vec![],
        })
        .expect_err("expired certificate");
        assert_eq!(error.code().as_str(), "ATM_CERTIFICATE_OPERATION_FAILED");
    }

    #[tokio::test]
    async fn pin_mismatch_fails_during_tls_before_opaque_bytes_can_flow() {
        let directory = tempfile::tempdir().expect("directory");
        let server_identity = identity(&directory, "server");
        let client_identity = identity(&directory, "client");
        let server = MtlsPeerStreamAdapter::from_peer_config(&TestStore {
            interfaces: vec![interface()],
            certificate: Some(server_identity.clone()),
            peers: vec![peer(&client_identity, true)],
        })
        .expect("server adapter");
        let mut wrong_pin = peer(&server_identity, true);
        wrong_pin.fingerprint = CertificateFingerprint::try_from("00".repeat(32)).expect("pin");
        let client = MtlsPeerStreamAdapter::from_peer_config(&TestStore {
            interfaces: vec![interface()],
            certificate: Some(client_identity),
            peers: vec![wrong_pin],
        })
        .expect("client adapter");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        let server_task = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.expect("accept tcp");
            server.accept(tcp).await
        });
        let host: HostName = "localhost".parse().expect("host");
        let client_result = client
            .connect(
                TcpStream::connect(address).await.expect("connect tcp"),
                &host,
            )
            .await;
        assert!(
            client_result.is_err(),
            "pin mismatch must fail the TLS handshake"
        );
        let _ = server_task.await.expect("server task");
    }

    #[tokio::test]
    async fn untrusted_client_certificate_fails_before_opaque_bytes_can_flow() {
        let directory = tempfile::tempdir().expect("directory");
        let server_identity = identity(&directory, "server");
        let client_identity = identity(&directory, "client");
        let untrusted_identity = identity(&directory, "untrusted");
        let server = MtlsPeerStreamAdapter::from_peer_config(&TestStore {
            interfaces: vec![interface()],
            certificate: Some(server_identity.clone()),
            peers: vec![peer(&untrusted_identity, true)],
        })
        .expect("server adapter");
        let client = MtlsPeerStreamAdapter::from_peer_config(&TestStore {
            interfaces: vec![interface()],
            certificate: Some(client_identity),
            peers: vec![peer(&server_identity, true)],
        })
        .expect("client adapter");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        let server_task = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.expect("accept tcp");
            server.accept(tcp).await
        });
        let host: HostName = "localhost".parse().expect("host");
        let _client_result = client
            .connect(
                TcpStream::connect(address).await.expect("connect tcp"),
                &host,
            )
            .await;
        let server_error = server_task
            .await
            .expect("server task")
            .expect_err("untrusted client must not complete mTLS");
        assert_eq!(
            server_error.code().as_str(),
            "ATM_PEER_AUTHENTICATION_FAILED"
        );
        // TLS 1.3 can let the client receive the server Finished before the
        // server observes and rejects its certificate. The authoritative
        // inbound decision above nevertheless fails before any stream is
        // handed to application protocol code, so no opaque bytes can flow.
    }

    #[tokio::test]
    async fn handshake_deadline_fails_before_any_application_protocol_can_start() {
        let directory = tempfile::tempdir().expect("directory");
        let server_identity = identity(&directory, "server");
        let client_identity = identity(&directory, "client");
        let server = MtlsPeerStreamAdapter::from_peer_config(&TestStore {
            interfaces: vec![interface()],
            certificate: Some(server_identity),
            peers: vec![peer(&client_identity, true)],
        })
        .expect("server adapter")
        .with_handshake_timeout(Duration::from_millis(10));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        let raw_client = tokio::spawn(async move {
            let _tcp = TcpStream::connect(address).await.expect("connect tcp");
            tokio::time::sleep(Duration::from_millis(50)).await;
        });
        let (tcp, _) = listener.accept().await.expect("accept tcp");
        let error = server.accept(tcp).await.expect_err("deadline");
        assert_eq!(error.code().as_str(), "ATM_PEER_AUTHENTICATION_FAILED");
        raw_client.await.expect("raw client");
    }
}
