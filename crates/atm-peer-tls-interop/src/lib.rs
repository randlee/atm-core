//! Provisioning-only TLS material and a bounded curl mTLS receiver fixture.
//!
//! This crate deliberately has no production caller.  It preserves the
//! certificate/key and client-fingerprint proof needed to verify curl mTLS
//! interoperability while active ATM delivery moves to ordinary HTTP.  The
//! fixture binds one socket, accepts one connection, and exits; it has no
//! sender, route, retry, resolver, worker, or daemon lifecycle API.

use std::io::Write;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use atm_core::api::RequestDeadline;
use atm_core::error::AtmError;
use atm_storage::{LocalCertificate, TrustedPeer};
use rustls::client::danger::HandshakeSignatureValid;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, UnixTime, pem::PemObject};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{
    DigitallySignedStruct, Error, ServerConfig, ServerConnection, SignatureScheme, StreamOwned,
};
use sha2::{Digest, Sha256};

const CURL_HTTP_RESPONSE: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

/// TLS provisioning values retained for the isolated interoperability proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsInteropConfig {
    local_certificate: LocalCertificate,
    trusted_peers: Vec<TrustedPeer>,
}

impl TlsInteropConfig {
    /// Validate and retain only durable certificate/trusted-peer configuration.
    pub fn from_provisioning(
        local_certificate: LocalCertificate,
        trusted_peers: Vec<TrustedPeer>,
    ) -> Result<Self, AtmError> {
        TlsIdentity::load(&local_certificate)?;
        Ok(Self {
            local_certificate,
            trusted_peers,
        })
    }
}

/// Result of one bounded curl mTLS receiver exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurlMtlsFixtureOutcome {
    Accepted,
    RejectedClientCertificate,
}

/// A synchronous, one-shot receiver used only by the curl mTLS fixture.
pub struct CurlMtlsReceiverFixture {
    bind_addr: SocketAddr,
    config: TlsInteropConfig,
}

impl CurlMtlsReceiverFixture {
    /// Construct a fixture.  The constructor is crate-private so no production
    /// daemon, CLI, graft, or sender can register this receiver.
    #[allow(
        dead_code,
        reason = "the fixture is constructed by in-crate proof harnesses"
    )]
    pub(crate) fn new(bind_addr: SocketAddr, config: TlsInteropConfig) -> Self {
        Self { bind_addr, config }
    }

    /// Accept exactly one connection and prove configured client-certificate
    /// acceptance or rejection before returning.
    pub fn serve_one(&self, deadline: RequestDeadline) -> Result<CurlMtlsFixtureOutcome, AtmError> {
        let listener = TcpListener::bind(self.bind_addr).map_err(|source| {
            AtmError::daemon_unavailable_with_cause(
                "failed to bind the curl mTLS interoperability fixture",
                source,
            )
        })?;
        listener.set_nonblocking(true).map_err(|source| {
            AtmError::daemon_unavailable_with_cause(
                "failed to configure the curl mTLS interoperability fixture",
                source,
            )
        })?;
        let stream = accept_with_deadline(&listener, deadline)?;
        stream.set_nonblocking(false).map_err(|source| {
            AtmError::daemon_unavailable_with_cause(
                "failed to configure the curl mTLS fixture connection",
                source,
            )
        })?;
        apply_deadline(&stream, remaining_budget(deadline)?)?;

        let identity = TlsIdentity::load(&self.config.local_certificate)?;
        let server_config = server_config(&identity, &self.config.trusted_peers)?;
        let connection = ServerConnection::new(Arc::new(server_config)).map_err(|source| {
            AtmError::daemon_unavailable_with_cause(
                "failed to initialize the curl mTLS interoperability fixture",
                source,
            )
        })?;
        let mut tls = StreamOwned::new(connection, stream);

        match complete_server_handshake(&mut tls, deadline) {
            Ok(()) => {
                tls.write_all(CURL_HTTP_RESPONSE).map_err(|source| {
                    AtmError::daemon_unavailable_with_cause(
                        "failed to write the curl mTLS fixture response",
                        source,
                    )
                })?;
                tls.flush().map_err(|source| {
                    AtmError::daemon_unavailable_with_cause(
                        "failed to flush the curl mTLS fixture response",
                        source,
                    )
                })?;
                tls.conn.send_close_notify();
                tls.flush().map_err(|source| {
                    AtmError::daemon_unavailable_with_cause(
                        "failed to flush the curl mTLS fixture close notification",
                        source,
                    )
                })?;
                Ok(CurlMtlsFixtureOutcome::Accepted)
            }
            Err(HandshakeFailure::Deadline) => Err(AtmError::daemon_unavailable(
                "curl mTLS interoperability fixture deadline expired",
            )),
            Err(HandshakeFailure::RejectedClientCertificate) => {
                Ok(CurlMtlsFixtureOutcome::RejectedClientCertificate)
            }
        }
    }
}

struct TlsIdentity {
    certificates: Vec<CertificateDer<'static>>,
    private_key: PrivateKeyDer<'static>,
}

impl TlsIdentity {
    fn load(certificate: &LocalCertificate) -> Result<Self, AtmError> {
        let path = Path::new(certificate.private_key_ref.as_str());
        let pem = std::fs::read(path).map_err(|source| {
            AtmError::daemon_unavailable_with_cause(
                "failed to open the configured TLS certificate/key PEM bundle",
                source,
            )
        })?;
        let certificates = CertificateDer::pem_slice_iter(&pem)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|source| {
                AtmError::validation("configured TLS certificate PEM is invalid").with_cause(source)
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
        let private_key = PrivateKeyDer::from_pem_slice(&pem).map_err(|source| {
            AtmError::validation("configured TLS private key PEM is invalid").with_cause(source)
        })?;
        Ok(Self {
            certificates,
            private_key,
        })
    }
}

fn server_config(
    identity: &TlsIdentity,
    trusted_peers: &[TrustedPeer],
) -> Result<ServerConfig, AtmError> {
    install_tls_provider();
    ServerConfig::builder()
        .with_client_cert_verifier(Arc::new(PinnedClientVerifier::new(trusted_peers)))
        .with_single_cert(
            identity.certificates.clone(),
            identity.private_key.clone_key(),
        )
        .map_err(|source| {
            AtmError::validation("configured TLS certificate/key pair is invalid")
                .with_cause(source)
        })
}

#[derive(Debug)]
struct PinnedClientVerifier {
    fingerprints: Vec<String>,
    algorithms: rustls::crypto::WebPkiSupportedAlgorithms,
}

impl PinnedClientVerifier {
    fn new(peers: &[TrustedPeer]) -> Self {
        Self {
            fingerprints: peers
                .iter()
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

fn accept_with_deadline(
    listener: &TcpListener,
    deadline: RequestDeadline,
) -> Result<TcpStream, AtmError> {
    loop {
        match listener.accept() {
            Ok((stream, _)) => return Ok(stream),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if deadline.expired() {
                    return Err(AtmError::daemon_unavailable(
                        "curl mTLS interoperability fixture accept deadline expired",
                    ));
                }
                std::thread::yield_now();
            }
            Err(source) => {
                return Err(AtmError::daemon_unavailable_with_cause(
                    "curl mTLS interoperability fixture accept failed",
                    source,
                ));
            }
        }
    }
}

enum HandshakeFailure {
    Deadline,
    RejectedClientCertificate,
}

fn complete_server_handshake(
    stream: &mut StreamOwned<ServerConnection, TcpStream>,
    deadline: RequestDeadline,
) -> Result<(), HandshakeFailure> {
    while stream.conn.is_handshaking() {
        let remaining = deadline.remaining().ok_or(HandshakeFailure::Deadline)?;
        apply_deadline(stream.get_ref(), remaining).map_err(|_| HandshakeFailure::Deadline)?;
        if let Err(_error) = stream.conn.complete_io(&mut stream.sock) {
            if deadline.expired() {
                return Err(HandshakeFailure::Deadline);
            }
            return Err(HandshakeFailure::RejectedClientCertificate);
        }
    }
    Ok(())
}

fn apply_deadline(stream: &TcpStream, deadline: Duration) -> Result<(), AtmError> {
    stream.set_read_timeout(Some(deadline)).map_err(|source| {
        AtmError::daemon_unavailable_with_cause(
            "failed to set curl mTLS fixture read deadline",
            source,
        )
    })?;
    stream.set_write_timeout(Some(deadline)).map_err(|source| {
        AtmError::daemon_unavailable_with_cause(
            "failed to set curl mTLS fixture write deadline",
            source,
        )
    })
}

fn remaining_budget(deadline: RequestDeadline) -> Result<Duration, AtmError> {
    deadline
        .remaining()
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| AtmError::daemon_unavailable("curl mTLS fixture deadline expired"))
}

fn install_tls_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
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
    use super::*;
    use rcgen::generate_simple_self_signed;
    use rustls::pki_types::ServerName;
    use rustls::{ClientConfig, ClientConnection, RootCertStore};
    use std::io::Read;
    use std::path::PathBuf;
    use std::thread;
    use tempfile::TempDir;

    struct TestIdentity {
        certificate: LocalCertificate,
        tls: TlsIdentity,
        certificate_pem: PathBuf,
        key_pem: PathBuf,
    }

    fn write_identity(dir: &TempDir, name: &str) -> TestIdentity {
        let generated = generate_simple_self_signed(vec!["localhost".to_string()])
            .expect("generate test certificate");
        let pem = format!(
            "{}{}",
            generated.cert.pem(),
            generated.key_pair.serialize_pem()
        );
        let path = dir.path().join(format!("{name}.pem"));
        let certificate_pem = dir.path().join(format!("{name}.cert.pem"));
        let key_pem = dir.path().join(format!("{name}.key.pem"));
        std::fs::write(&path, pem).expect("write test PEM");
        std::fs::write(&certificate_pem, generated.cert.pem()).expect("write test certificate");
        std::fs::write(&key_pem, generated.key_pair.serialize_pem()).expect("write test key");
        let certificate_der = generated.cert.der();
        let fingerprint = certificate_fingerprint(certificate_der);
        let certificate = LocalCertificate {
            fingerprint: fingerprint.parse().expect("fingerprint"),
            private_key_ref: path.display().to_string().parse().expect("key ref"),
        };
        let tls = TlsIdentity::load(&certificate).expect("load test identity");
        TestIdentity {
            certificate,
            tls,
            certificate_pem,
            key_pem,
        }
    }

    fn trusted_peer(fingerprint: &str) -> TrustedPeer {
        TrustedPeer {
            host: "localhost".parse().expect("host"),
            fingerprint: fingerprint.parse().expect("fingerprint"),
            enabled: true,
            https_port: std::num::NonZeroU16::new(43101).expect("port"),
        }
    }

    fn available_addr() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").expect("reserve test port");
        listener.local_addr().expect("inspect test port")
    }

    fn client_config(
        identity: &TlsIdentity,
        server_certificate: &CertificateDer<'_>,
    ) -> ClientConfig {
        install_tls_provider();
        let mut roots = RootCertStore::empty();
        roots
            .add(server_certificate.clone())
            .expect("add server root");
        ClientConfig::builder()
            .with_root_certificates(roots)
            .with_client_auth_cert(
                identity.certificates.clone(),
                identity.private_key.clone_key(),
            )
            .expect("client certificate config")
    }

    fn connect_with_retry(addr: SocketAddr) -> TcpStream {
        for _ in 0..500 {
            if let Ok(stream) = TcpStream::connect(addr) {
                return stream;
            }
            thread::yield_now();
        }
        panic!("fixture did not bind at {addr}");
    }

    fn complete_client_handshake(
        tls: &mut StreamOwned<ClientConnection, TcpStream>,
    ) -> Result<(), std::io::Error> {
        while tls.conn.is_handshaking() {
            tls.conn.complete_io(&mut tls.sock)?;
        }
        Ok(())
    }

    #[test]
    fn configured_client_certificate_is_accepted_by_one_shot_fixture() {
        let dir = TempDir::new().expect("tempdir");
        let server = write_identity(&dir, "server");
        let client = write_identity(&dir, "client");
        let config = TlsInteropConfig::from_provisioning(
            server.certificate.clone(),
            vec![trusted_peer(client.certificate.fingerprint.as_str())],
        )
        .expect("interop config");
        let addr = available_addr();
        let fixture = CurlMtlsReceiverFixture::new(addr, config);
        let server_certificate = server.tls.certificates[0].clone();
        let handle = thread::spawn(move || {
            fixture.serve_one(RequestDeadline::after(Duration::from_secs(5)))
        });
        let stream = connect_with_retry(addr);
        let config = client_config(&client.tls, &server_certificate);
        let connection = ClientConnection::new(
            Arc::new(config),
            ServerName::try_from("localhost".to_string()).expect("server name"),
        )
        .expect("client connection");
        let mut tls = StreamOwned::new(connection, stream);
        complete_client_handshake(&mut tls).expect("mTLS handshake");
        assert_eq!(
            handle
                .join()
                .expect("fixture thread")
                .expect("fixture result"),
            CurlMtlsFixtureOutcome::Accepted
        );
        let mut response = Vec::new();
        let mut chunk = [0_u8; 128];
        while response.len() < CURL_HTTP_RESPONSE.len() {
            match tls.read(&mut chunk) {
                Ok(0) => break,
                Ok(size) => response.extend_from_slice(&chunk[..size]),
                Err(error) if response.len() >= CURL_HTTP_RESPONSE.len() => {
                    let _ = error;
                    break;
                }
                Err(error) => panic!("fixture response: {error}"),
            }
        }
        assert_eq!(&response[..CURL_HTTP_RESPONSE.len()], CURL_HTTP_RESPONSE);
    }

    #[test]
    fn mismatched_client_certificate_is_rejected_before_response() {
        let dir = TempDir::new().expect("tempdir");
        let server = write_identity(&dir, "server");
        let client = write_identity(&dir, "client");
        let config = TlsInteropConfig::from_provisioning(
            server.certificate.clone(),
            vec![trusted_peer(&"00".repeat(32))],
        )
        .expect("interop config");
        let addr = available_addr();
        let fixture = CurlMtlsReceiverFixture::new(addr, config);
        let server_certificate = server.tls.certificates[0].clone();
        let handle = thread::spawn(move || {
            fixture.serve_one(RequestDeadline::after(Duration::from_secs(5)))
        });
        let stream = connect_with_retry(addr);
        let config = client_config(&client.tls, &server_certificate);
        let connection = ClientConnection::new(
            Arc::new(config),
            ServerName::try_from("localhost".to_string()).expect("server name"),
        )
        .expect("client connection");
        let mut tls = StreamOwned::new(connection, stream);
        let handshake = complete_client_handshake(&mut tls);
        let mut alert = [0_u8; 1];
        let alert_read = tls.read(&mut alert);
        assert!(
            handshake.is_err() || alert_read.is_err() || matches!(alert_read, Ok(0)),
            "mismatched client certificate must fail during handshake or alert processing"
        );
        assert_eq!(
            handle
                .join()
                .expect("fixture thread")
                .expect("fixture result"),
            CurlMtlsFixtureOutcome::RejectedClientCertificate
        );
    }

    #[test]
    fn curl_accepts_a_configured_client_certificate() {
        if std::process::Command::new("curl")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("curl is unavailable; skipping external mTLS fixture proof");
            return;
        }

        let dir = TempDir::new().expect("tempdir");
        let server = write_identity(&dir, "curl-server");
        let client = write_identity(&dir, "curl-client");
        let config = TlsInteropConfig::from_provisioning(
            server.certificate.clone(),
            vec![trusted_peer(client.certificate.fingerprint.as_str())],
        )
        .expect("interop config");
        let addr = available_addr();
        let fixture = CurlMtlsReceiverFixture::new(addr, config);
        let handle = thread::spawn(move || {
            fixture.serve_one(RequestDeadline::after(Duration::from_secs(5)))
        });
        let url = format!("https://localhost:{}/fixture", addr.port());
        let output = std::process::Command::new("curl")
            .args([
                "--silent",
                "--show-error",
                "--fail",
                "--retry",
                "5",
                "--retry-connrefused",
                "--retry-delay",
                "0",
                "--cacert",
                server.certificate_pem.to_str().expect("server cert path"),
                "--cert",
                client.certificate_pem.to_str().expect("client cert path"),
                "--key",
                client.key_pem.to_str().expect("client key path"),
                &url,
            ])
            .output()
            .expect("run curl mTLS fixture proof");
        assert!(
            output.status.success(),
            "curl mTLS proof failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            handle
                .join()
                .expect("fixture thread")
                .expect("fixture result"),
            CurlMtlsFixtureOutcome::Accepted
        );
    }

    #[test]
    fn fingerprint_normalization_is_stable_for_provisioned_values() {
        assert_eq!(normalize_fingerprint("AA:bb-CC"), "aabbcc");
        assert_ne!(normalize_fingerprint("aabbcd"), "aabbcc");
    }
}
