//! Bounded Tokio-Rustls stream wrapping for ATM's canonical peer HTTP path.
//!
//! This crate owns concrete Rustls configuration and handshakes only. Mailbox
//! routing, HTTP encoding, listener lifecycle, and retry policy remain outside
//! the adapter boundary.

use std::sync::Arc;
use std::time::Duration;

use atm_core::{BoxedPeerIo, PeerIoAdapter, RequestDeadline};
use atm_storage::{
    AtmError, HttpsInterface, PeerConfigStore, PinnedClientVerifier, PinnedServerVerifier,
    TlsIdentity, TrustedPeer, install_tls_provider,
};
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ServerConfig};
use tokio::net::TcpStream;
use tokio_rustls::{TlsAcceptor, TlsConnector};

/// The only production implementation of `atm_core::PeerIoAdapter`.
///
/// Configuration is read at each handshake so certificate and trust changes
/// become effective without exposing Rustls or storage configuration to the
/// HTTP runtime.
#[derive(Clone)]
pub struct PeerTlsAdapter {
    store: Arc<dyn PeerConfigStore>,
}

impl std::fmt::Debug for PeerTlsAdapter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PeerTlsAdapter")
            .finish_non_exhaustive()
    }
}

/// Construct the sealed production peer I/O adapter.
///
/// The constructor performs the non-secret local configuration validation
/// before it returns an adapter. Per-peer checks still occur immediately
/// before an outbound connection or inbound handshake.
pub fn mtls_adapter(store: Arc<dyn PeerConfigStore>) -> Result<Arc<dyn PeerIoAdapter>, AtmError> {
    Ok(Arc::new(PeerTlsAdapter::new(store)?))
}

impl PeerTlsAdapter {
    /// Validate the local enabled interface, identity, and trust snapshot.
    pub fn new(store: Arc<dyn PeerConfigStore>) -> Result<Self, AtmError> {
        let adapter = Self { store };
        adapter.local_snapshot()?;
        Ok(adapter)
    }

    fn local_snapshot(&self) -> Result<LocalTlsSnapshot, AtmError> {
        let interfaces = self.store.list_interfaces()?;
        let interface = exactly_one_enabled_interface(interfaces)?;
        let certificate = self.store.local_certificate()?.ok_or_else(|| {
            AtmError::validation(
                "peer TLS has no configured local certificate; configure a certificate before enabling mTLS",
            )
        })?;
        let identity = TlsIdentity::load(&certificate)?;
        let trusted_peers = self.store.list_trusted_peers()?;
        if !trusted_peers.iter().any(|peer| peer.enabled) {
            return Err(AtmError::validation(
                "peer TLS has no enabled trusted peer; configure an enabled peer before enabling mTLS",
            ));
        }
        Ok(LocalTlsSnapshot {
            interface,
            identity,
            trusted_peers,
        })
    }

    fn server_config(&self) -> Result<ServerConfig, AtmError> {
        let snapshot = self.local_snapshot()?;
        install_tls_provider();
        ServerConfig::builder()
            .with_client_cert_verifier(Arc::new(PinnedClientVerifier::new(snapshot.trusted_peers)))
            .with_single_cert(
                snapshot.identity.certificates().to_vec(),
                snapshot.identity.private_key().clone_key(),
            )
            .map_err(|source| {
                AtmError::validation("configured peer TLS certificate/key pair is invalid")
                    .with_cause(source)
            })
    }

    fn client_config(
        &self,
        peer: &atm_core::types::HostName,
    ) -> Result<(ClientConfig, u16), AtmError> {
        let snapshot = self.local_snapshot()?;
        let configured_peer = self.store.trusted_peer(peer)?.ok_or_else(|| {
            AtmError::validation(format!(
                "peer TLS has no configured trusted peer for host `{peer}`"
            ))
        })?;
        if !configured_peer.enabled {
            return Err(AtmError::validation(format!(
                "peer TLS trusted peer `{peer}` is disabled; enable it before connecting"
            )));
        }
        install_tls_provider();
        let config = ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(PinnedServerVerifier::new(
                configured_peer.clone(),
            )))
            .with_client_auth_cert(
                snapshot.identity.certificates().to_vec(),
                snapshot.identity.private_key().clone_key(),
            )
            .map_err(|source| {
                AtmError::validation("configured peer TLS certificate/key pair is invalid")
                    .with_cause(source)
            })?;
        Ok((config, configured_peer.https_port.get()))
    }

    async fn accept_inner(
        &self,
        stream: TcpStream,
        deadline: RequestDeadline,
    ) -> Result<BoxedPeerIo, AtmError> {
        let adapter = self.clone();
        let config = load_configuration_with_deadline(
            deadline,
            "peer TLS inbound configuration",
            move || adapter.server_config(),
        )
        .await?;
        let remaining = remaining_budget(deadline, "peer TLS inbound handshake")?;
        let stream = tokio::time::timeout(
            remaining,
            TlsAcceptor::from(Arc::new(config)).accept(stream),
        )
        .await
        .map_err(|_| deadline_error("peer TLS inbound handshake"))?
        .map_err(|source| {
            AtmError::daemon_unavailable_with_cause(
                "peer TLS rejected the inbound client certificate or handshake",
                source,
            )
        })?;
        Ok(Box::new(stream))
    }

    async fn connect_inner(
        &self,
        peer: atm_core::types::HostName,
        deadline: RequestDeadline,
    ) -> Result<BoxedPeerIo, AtmError> {
        let adapter = self.clone();
        let configured_peer = peer.clone();
        let (config, port) = load_configuration_with_deadline(
            deadline,
            "peer TLS outbound configuration",
            move || adapter.client_config(&configured_peer),
        )
        .await?;
        let remaining = remaining_budget(deadline, "peer TLS TCP connection")?;
        let tcp = tokio::time::timeout(remaining, TcpStream::connect((peer.as_str(), port)))
            .await
            .map_err(|_| deadline_error("peer TLS TCP connection"))?
            .map_err(|source| {
                AtmError::daemon_unavailable_with_cause(
                    "peer TLS could not connect to the configured trusted peer",
                    source,
                )
            })?;
        let server_name = ServerName::try_from(peer.as_str().to_owned()).map_err(|source| {
            AtmError::validation("configured peer TLS hostname is invalid").with_cause(source)
        })?;
        let remaining = remaining_budget(deadline, "peer TLS outbound handshake")?;
        let stream = tokio::time::timeout(
            remaining,
            TlsConnector::from(Arc::new(config)).connect(server_name, tcp),
        )
        .await
        .map_err(|_| deadline_error("peer TLS outbound handshake"))?
        .map_err(|source| {
            AtmError::daemon_unavailable_with_cause(
                "peer TLS server certificate, hostname, pin, or handshake verification failed",
                source,
            )
        })?;
        Ok(Box::new(stream))
    }
}

impl atm_core::boundary::sealed::Sealed for PeerTlsAdapter {}

impl PeerIoAdapter for PeerTlsAdapter {
    fn accept<'adapter>(
        &'adapter self,
        stream: TcpStream,
        deadline: RequestDeadline,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<BoxedPeerIo, AtmError>> + Send + 'adapter>,
    > {
        Box::pin(self.accept_inner(stream, deadline))
    }

    fn connect<'adapter>(
        &'adapter self,
        peer: atm_core::types::HostName,
        deadline: RequestDeadline,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<BoxedPeerIo, AtmError>> + Send + 'adapter>,
    > {
        Box::pin(self.connect_inner(peer, deadline))
    }
}

struct LocalTlsSnapshot {
    #[allow(
        dead_code,
        reason = "the selected interface is validated before every handshake"
    )]
    interface: HttpsInterface,
    identity: TlsIdentity,
    trusted_peers: Vec<TrustedPeer>,
}

fn exactly_one_enabled_interface(
    interfaces: Vec<HttpsInterface>,
) -> Result<HttpsInterface, AtmError> {
    let mut enabled = interfaces.into_iter().filter(|interface| interface.enabled);
    let interface = enabled.next().ok_or_else(|| {
        AtmError::validation(
            "peer TLS has no enabled interface; configure and enable one interface before using mTLS",
        )
    })?;
    if enabled.next().is_some() {
        return Err(AtmError::validation(
            "peer TLS has multiple enabled interfaces; configure exactly one before using mTLS",
        ));
    }
    Ok(interface)
}

fn remaining_budget(deadline: RequestDeadline, stage: &str) -> Result<Duration, AtmError> {
    deadline.remaining().ok_or_else(|| deadline_error(stage))
}

async fn load_configuration_with_deadline<T>(
    deadline: RequestDeadline,
    stage: &str,
    operation: impl FnOnce() -> Result<T, AtmError> + Send + 'static,
) -> Result<T, AtmError>
where
    T: Send + 'static,
{
    let remaining = remaining_budget(deadline, stage)?;
    tokio::time::timeout(remaining, tokio::task::spawn_blocking(operation))
        .await
        .map_err(|_| deadline_error(stage))?
        .map_err(|source| {
            AtmError::daemon_unavailable_with_cause("peer TLS configuration worker failed", source)
        })?
}

fn deadline_error(stage: &str) -> AtmError {
    AtmError::daemon_unavailable(format!("{stage} exceeded the request deadline"))
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::num::NonZeroU16;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use atm_core::PeerIoAdapter;
    use atm_storage::contract::sealed;
    use atm_storage::{LocalCertificate, certificate_fingerprint};
    use rcgen::generate_simple_self_signed;
    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    #[derive(Clone)]
    struct TestPeerConfigStore {
        interfaces: Vec<HttpsInterface>,
        local_certificate: Option<LocalCertificate>,
        trusted_peers: Vec<TrustedPeer>,
        configuration_delay: Arc<AtomicBool>,
    }

    impl TestPeerConfigStore {
        fn delay_if_requested(&self) {
            if self.configuration_delay.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(40));
            }
        }
    }

    impl sealed::Sealed for TestPeerConfigStore {}

    impl PeerConfigStore for TestPeerConfigStore {
        fn list_interfaces(&self) -> Result<Vec<HttpsInterface>, AtmError> {
            self.delay_if_requested();
            Ok(self.interfaces.clone())
        }

        fn save_interface(&self, _interface: &HttpsInterface) -> Result<(), AtmError> {
            Ok(())
        }

        fn remove_interface(&self, _bind_addr: SocketAddr) -> Result<bool, AtmError> {
            Ok(false)
        }

        fn local_certificate(&self) -> Result<Option<LocalCertificate>, AtmError> {
            self.delay_if_requested();
            Ok(self.local_certificate.clone())
        }

        fn save_local_certificate(&self, _certificate: &LocalCertificate) -> Result<(), AtmError> {
            Ok(())
        }

        fn list_trusted_peers(&self) -> Result<Vec<TrustedPeer>, AtmError> {
            self.delay_if_requested();
            Ok(self.trusted_peers.clone())
        }

        fn trusted_peer(
            &self,
            host: &atm_storage::HostName,
        ) -> Result<Option<TrustedPeer>, AtmError> {
            self.delay_if_requested();
            Ok(self
                .trusted_peers
                .iter()
                .find(|peer| &peer.host == host)
                .cloned())
        }

        fn save_trusted_peer(&self, _peer: &TrustedPeer) -> Result<(), AtmError> {
            Ok(())
        }

        fn remove_trusted_peer(&self, _host: &atm_storage::HostName) -> Result<bool, AtmError> {
            Ok(false)
        }
    }

    struct TestIdentity {
        certificate: LocalCertificate,
        fingerprint: String,
    }

    fn write_identity(directory: &TempDir, file_name: &str, hostname: &str) -> TestIdentity {
        let generated = generate_simple_self_signed(vec![hostname.to_owned()])
            .expect("generate test certificate");
        let bundle = format!(
            "{}{}",
            generated.cert.pem(),
            generated.key_pair.serialize_pem()
        );
        let pem_path = directory.path().join(format!("{file_name}.pem"));
        std::fs::write(&pem_path, bundle).expect("write test certificate bundle");
        let fingerprint = certificate_fingerprint(generated.cert.der());
        let certificate = LocalCertificate {
            fingerprint: fingerprint.parse().expect("valid fingerprint"),
            private_key_ref: pem_path
                .display()
                .to_string()
                .parse()
                .expect("valid private key reference"),
        };
        TestIdentity {
            certificate,
            fingerprint,
        }
    }

    fn enabled_interface() -> HttpsInterface {
        HttpsInterface {
            bind_addr: "127.0.0.1:43101".parse().expect("socket address"),
            advertise_host: "localhost".parse().expect("host"),
            enabled: true,
        }
    }

    fn trusted_peer(host: &str, fingerprint: &str, port: u16) -> TrustedPeer {
        TrustedPeer {
            host: host.parse().expect("host"),
            fingerprint: fingerprint.parse().expect("fingerprint"),
            enabled: true,
            https_port: NonZeroU16::new(port).expect("non-zero port"),
        }
    }

    fn store(
        certificate: LocalCertificate,
        trusted_peers: Vec<TrustedPeer>,
    ) -> Arc<dyn PeerConfigStore> {
        Arc::new(TestPeerConfigStore {
            interfaces: vec![enabled_interface()],
            local_certificate: Some(certificate),
            trusted_peers,
            configuration_delay: Arc::new(AtomicBool::new(false)),
        })
    }

    #[tokio::test]
    async fn mutual_tls_adapter_exchanges_opaque_bytes() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let server_identity = write_identity(&directory, "server", "localhost");
        let client_identity = write_identity(&directory, "client", "client.test");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let port = listener.local_addr().expect("listener address").port();

        let server = PeerTlsAdapter::new(store(
            server_identity.certificate.clone(),
            vec![trusted_peer(
                "client.test",
                &client_identity.fingerprint,
                port,
            )],
        ))
        .expect("server configuration");
        let client = PeerTlsAdapter::new(store(
            client_identity.certificate,
            vec![trusted_peer(
                "localhost",
                &server_identity.fingerprint,
                port,
            )],
        ))
        .expect("client configuration");

        let server_task = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.expect("accept TCP");
            let mut stream = server
                .accept(tcp, RequestDeadline::after(Duration::from_secs(1)))
                .await
                .expect("accept mTLS");
            let mut request = [0_u8; 4];
            stream
                .read_exact(&mut request)
                .await
                .expect("read opaque bytes");
            assert_eq!(&request, b"ping");
            stream.write_all(b"pong").await.expect("write opaque bytes");
        });

        let mut stream = client
            .connect(
                "localhost".parse().expect("peer host"),
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("connect mTLS");
        stream.write_all(b"ping").await.expect("write opaque bytes");
        let mut response = [0_u8; 4];
        stream
            .read_exact(&mut response)
            .await
            .expect("read opaque bytes");
        assert_eq!(&response, b"pong");
        server_task.await.expect("server task");
    }

    #[test]
    fn constructor_rejects_disabled_interface_before_handshake() {
        let store: Arc<dyn PeerConfigStore> = Arc::new(TestPeerConfigStore {
            interfaces: vec![HttpsInterface {
                enabled: false,
                ..enabled_interface()
            }],
            local_certificate: None,
            trusted_peers: vec![],
            configuration_delay: Arc::new(AtomicBool::new(false)),
        });
        let error = PeerTlsAdapter::new(store).expect_err("disabled interface must reject");
        assert!(error.message().contains("no enabled interface"));
    }

    #[test]
    fn constructor_rejects_multiple_enabled_interfaces_with_clear_guidance() {
        let store: Arc<dyn PeerConfigStore> = Arc::new(TestPeerConfigStore {
            interfaces: vec![enabled_interface(), enabled_interface()],
            local_certificate: None,
            trusted_peers: vec![],
            configuration_delay: Arc::new(AtomicBool::new(false)),
        });

        let error = PeerTlsAdapter::new(store).expect_err("multiple interfaces must reject");
        assert!(error.message().contains("configure exactly one"));
        assert!(!error.message().contains("select"));
    }

    #[test]
    fn constructor_rejects_missing_or_invalid_local_certificate_without_secret_output() {
        let missing: Arc<dyn PeerConfigStore> = Arc::new(TestPeerConfigStore {
            interfaces: vec![enabled_interface()],
            local_certificate: None,
            trusted_peers: vec![],
            configuration_delay: Arc::new(AtomicBool::new(false)),
        });
        let missing_error =
            PeerTlsAdapter::new(missing).expect_err("missing certificate must reject");
        assert!(
            missing_error
                .message()
                .contains("no configured local certificate")
        );

        let invalid: Arc<dyn PeerConfigStore> = Arc::new(TestPeerConfigStore {
            interfaces: vec![enabled_interface()],
            local_certificate: Some(LocalCertificate {
                fingerprint: "00".repeat(32).parse().expect("fingerprint"),
                private_key_ref: PathBuf::from("/not/a/real/private-key.pem")
                    .display()
                    .to_string()
                    .parse()
                    .expect("private key reference"),
            }),
            trusted_peers: vec![],
            configuration_delay: Arc::new(AtomicBool::new(false)),
        });
        let invalid_error =
            PeerTlsAdapter::new(invalid).expect_err("invalid certificate must reject");
        assert!(
            invalid_error
                .message()
                .contains("failed to open the configured TLS certificate")
        );
        assert!(!invalid_error.message().contains("private-key.pem"));

        let directory = tempfile::tempdir().expect("temporary directory");
        let identity = write_identity(&directory, "mismatched", "localhost");
        let mismatched: Arc<dyn PeerConfigStore> = Arc::new(TestPeerConfigStore {
            interfaces: vec![enabled_interface()],
            local_certificate: Some(LocalCertificate {
                fingerprint: "11".repeat(32).parse().expect("fingerprint"),
                private_key_ref: identity.certificate.private_key_ref,
            }),
            trusted_peers: vec![trusted_peer("localhost", &identity.fingerprint, 43101)],
            configuration_delay: Arc::new(AtomicBool::new(false)),
        });
        let mismatch_error = PeerTlsAdapter::new(mismatched)
            .expect_err("certificate fingerprint mismatch must reject");
        assert!(
            mismatch_error
                .message()
                .contains("fingerprint does not match the PEM bundle")
        );
    }

    #[tokio::test]
    async fn rejects_wrong_pinned_server_certificate_before_yielding_a_stream() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let expected_server = write_identity(&directory, "expected-server", "localhost");
        let actual_server = write_identity(&directory, "actual-server", "localhost");
        let client_identity = write_identity(&directory, "client", "client.test");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let port = listener.local_addr().expect("listener address").port();

        let server = PeerTlsAdapter::new(store(
            actual_server.certificate,
            vec![trusted_peer(
                "client.test",
                &client_identity.fingerprint,
                port,
            )],
        ))
        .expect("server configuration");
        let client = PeerTlsAdapter::new(store(
            client_identity.certificate,
            vec![trusted_peer(
                "localhost",
                &expected_server.fingerprint,
                port,
            )],
        ))
        .expect("client configuration");

        let server_task = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.expect("accept TCP");
            server
                .accept(tcp, RequestDeadline::after(Duration::from_secs(1)))
                .await
        });
        let result = client
            .connect(
                "localhost".parse().expect("peer host"),
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await;
        assert!(result.is_err(), "wrong pin must not yield a peer stream");
        let error = result.err().expect("wrong pin must return a typed error");
        assert!(
            error
                .message()
                .contains("certificate, hostname, pin, or handshake")
        );
        let _ = server_task.await.expect("server task");
    }

    #[tokio::test]
    async fn rejects_server_certificate_with_a_mismatched_hostname_before_yielding_a_stream() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let server_identity = write_identity(&directory, "server", "server.test");
        let client_identity = write_identity(&directory, "client", "client.test");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let port = listener.local_addr().expect("listener address").port();

        let server = PeerTlsAdapter::new(store(
            server_identity.certificate.clone(),
            vec![trusted_peer(
                "client.test",
                &client_identity.fingerprint,
                port,
            )],
        ))
        .expect("server configuration");
        let client = PeerTlsAdapter::new(store(
            client_identity.certificate,
            vec![trusted_peer(
                "localhost",
                &server_identity.fingerprint,
                port,
            )],
        ))
        .expect("client configuration");

        let server_task = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.expect("accept TCP");
            server
                .accept(tcp, RequestDeadline::after(Duration::from_secs(1)))
                .await
        });
        let result = client
            .connect(
                "localhost".parse().expect("peer host"),
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await;
        assert!(
            result.is_err(),
            "a pin alone must not bypass the configured hostname check"
        );
        let error = result
            .err()
            .expect("hostname mismatch must return typed error");
        assert!(
            error
                .message()
                .contains("certificate, hostname, pin, or handshake")
        );
        let _ = server_task.await.expect("server task");
    }

    #[tokio::test]
    async fn rejects_untrusted_client_certificate_before_yielding_a_stream() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let server_identity = write_identity(&directory, "server", "localhost");
        let trusted_client = write_identity(&directory, "trusted-client", "trusted-client.test");
        let actual_client = write_identity(&directory, "actual-client", "actual-client.test");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let port = listener.local_addr().expect("listener address").port();

        let server = PeerTlsAdapter::new(store(
            server_identity.certificate.clone(),
            vec![trusted_peer(
                "trusted-client.test",
                &trusted_client.fingerprint,
                port,
            )],
        ))
        .expect("server configuration");
        let client = PeerTlsAdapter::new(store(
            actual_client.certificate,
            vec![trusted_peer(
                "localhost",
                &server_identity.fingerprint,
                port,
            )],
        ))
        .expect("client configuration");

        let server_task = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.expect("accept TCP");
            server
                .accept(tcp, RequestDeadline::after(Duration::from_secs(1)))
                .await
        });
        let _ = client
            .connect(
                "localhost".parse().expect("peer host"),
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await;
        let result = server_task.await.expect("server task");
        assert!(
            result.is_err(),
            "wrong client certificate must reject on the server"
        );
        let error = result
            .err()
            .expect("wrong client certificate must return a typed error");
        assert!(
            error
                .message()
                .contains("rejected the inbound client certificate")
        );
    }

    #[tokio::test]
    async fn rejects_missing_client_certificate_before_yielding_a_stream() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let server_identity = write_identity(&directory, "server", "localhost");
        let expected_client = write_identity(&directory, "expected-client", "client.test");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let port = listener.local_addr().expect("listener address").port();
        let server = PeerTlsAdapter::new(store(
            server_identity.certificate.clone(),
            vec![trusted_peer(
                "client.test",
                &expected_client.fingerprint,
                port,
            )],
        ))
        .expect("server configuration");

        install_tls_provider();
        let client_config =
            ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(PinnedServerVerifier::new(
                    trusted_peer("localhost", &server_identity.fingerprint, port),
                )))
                .with_no_client_auth();
        let server_task = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.expect("accept TCP");
            server
                .accept(tcp, RequestDeadline::after(Duration::from_secs(1)))
                .await
        });
        let tcp = TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("connect TCP");
        let _client_result = TlsConnector::from(Arc::new(client_config))
            .connect(
                ServerName::try_from("localhost".to_owned()).expect("server name"),
                tcp,
            )
            .await;
        let server_result = server_task.await.expect("server task");
        assert!(
            server_result.is_err(),
            "a missing client certificate must not yield a peer stream"
        );
    }

    #[tokio::test]
    async fn configuration_loading_is_deadline_bounded_without_blocking_the_runtime() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let server_identity = write_identity(&directory, "server", "localhost");
        let expected_client = write_identity(&directory, "client", "client.test");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let address = listener.local_addr().expect("listener address");
        let delay = Arc::new(AtomicBool::new(false));
        let store = Arc::new(TestPeerConfigStore {
            interfaces: vec![enabled_interface()],
            local_certificate: Some(server_identity.certificate),
            trusted_peers: vec![trusted_peer(
                "client.test",
                &expected_client.fingerprint,
                address.port(),
            )],
            configuration_delay: delay.clone(),
        });
        let server = PeerTlsAdapter::new(store).expect("server configuration");
        delay.store(true, Ordering::SeqCst);

        let raw_client = tokio::spawn(async move {
            let _stream = TcpStream::connect(address).await.expect("connect raw TCP");
            tokio::time::sleep(Duration::from_millis(60)).await;
        });
        let (tcp, _) = listener.accept().await.expect("accept TCP");
        let result = server
            .accept(tcp, RequestDeadline::after(Duration::from_millis(5)))
            .await;
        let error = match result {
            Ok(_) => panic!("slow configuration must exhaust the deadline"),
            Err(error) => error,
        };
        assert!(
            error
                .message()
                .contains("inbound configuration exceeded the request deadline")
        );
        raw_client.await.expect("raw client task");
    }

    #[tokio::test]
    async fn handshake_deadline_expires_without_yielding_a_stream() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let server_identity = write_identity(&directory, "server", "localhost");
        let client_identity = write_identity(&directory, "client", "client.test");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let address = listener.local_addr().expect("listener address");
        let server = PeerTlsAdapter::new(store(
            server_identity.certificate.clone(),
            vec![trusted_peer(
                "client.test",
                &client_identity.fingerprint,
                address.port(),
            )],
        ))
        .expect("server configuration");

        let raw_client = tokio::spawn(async move {
            let _stream = TcpStream::connect(address).await.expect("connect raw TCP");
            tokio::time::sleep(Duration::from_millis(100)).await;
        });
        let (tcp, _) = listener.accept().await.expect("accept TCP");
        let result = server
            .accept(tcp, RequestDeadline::after(Duration::from_millis(50)))
            .await;
        assert!(result.is_err(), "missing TLS handshake must time out");
        let error = result.err().expect("deadline must return a typed error");
        let message = error.message();
        assert!(
            message.contains("inbound configuration exceeded the request deadline")
                || message.contains("inbound handshake exceeded the request deadline"),
            "unexpected deadline stage: {message}"
        );
        raw_client.await.expect("raw client task");
    }
}
