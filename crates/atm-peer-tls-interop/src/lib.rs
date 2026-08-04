//! Provisioning-only TLS material and a bounded curl mTLS receiver fixture.
//!
//! This crate preserves the certificate/key and client-fingerprint proof
//! needed to verify curl mTLS interoperability. The fixture binds one socket,
//! accepts one connection, and exits; it has no sender, route, retry, resolver,
//! worker, or daemon lifecycle API.

use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use atm_core::api::RequestDeadline;
use atm_core::error::AtmError;
use atm_storage::{LocalCertificate, PinnedClientVerifier, TlsIdentity, TrustedPeer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};

const CURL_HTTP_RESPONSE: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
const MAX_CURL_REQUEST_BYTES: usize = 16 * 1024;

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
    /// Construct a fixture. The constructor is crate-private so no production
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
                drain_request(&mut tls, deadline)?;
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
                let _ = tls.sock.shutdown(Shutdown::Both);
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

fn drain_request(
    stream: &mut StreamOwned<ServerConnection, TcpStream>,
    deadline: RequestDeadline,
) -> Result<(), AtmError> {
    let mut request = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 1024];
    while request.len() < MAX_CURL_REQUEST_BYTES {
        remaining_budget(deadline)?;
        let read = stream.read(&mut chunk).map_err(|source| {
            AtmError::daemon_unavailable_with_cause(
                "failed to read the curl mTLS fixture request",
                source,
            )
        })?;
        if read == 0 {
            break;
        }
        request.extend_from_slice(&chunk[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            return Ok(());
        }
    }
    if request.len() >= MAX_CURL_REQUEST_BYTES {
        return Err(AtmError::validation(
            "curl mTLS fixture request headers exceeded the bounded size",
        ));
    }
    Ok(())
}

fn server_config(
    identity: &TlsIdentity,
    trusted_peers: &[TrustedPeer],
) -> Result<ServerConfig, AtmError> {
    install_tls_provider();
    ServerConfig::builder()
        .with_client_cert_verifier(Arc::new(PinnedClientVerifier::new(trusted_peers.to_vec())))
        .with_single_cert(
            identity.certificates().to_vec(),
            identity.private_key().clone_key(),
        )
        .map_err(|source| {
            AtmError::validation("configured TLS certificate/key pair is invalid")
                .with_cause(source)
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use atm_storage::{certificate_fingerprint, normalize_fingerprint};
    use rcgen::generate_simple_self_signed;
    use rustls::pki_types::{CertificateDer, ServerName};
    use rustls::{ClientConfig, ClientConnection, RootCertStore};
    use std::io::Read;
    use std::path::PathBuf;
    use std::process::Command;
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
                identity.certificates().to_vec(),
                identity.private_key().clone_key(),
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

    const CURL_BINARY: &str = "curl";

    fn curl_command() -> Command {
        Command::new(CURL_BINARY)
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
        let server_certificate = server.tls.certificates()[0].clone();
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
        tls.write_all(b"GET /fixture HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .expect("fixture request");
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
        let server_certificate = server.tls.certificates()[0].clone();
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
        if cfg!(windows) {
            eprintln!(
                "Windows curl uses Schannel and cannot consume this PEM certificate/key fixture; \
                 the rustls mTLS acceptance test covers Windows"
            );
            return;
        }
        if curl_command().arg("--version").output().is_err() {
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
        let output = curl_command()
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
