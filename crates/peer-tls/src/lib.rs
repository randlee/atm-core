//! The sole concrete mutual-TLS byte-stream adapter for ATM peer transport.
//!
//! This crate intentionally knows only configured peer identity and byte
//! streams. It owns neither HTTP nor any ATM request, route, persistence,
//! acknowledgement, retry, hook, daemon lifecycle, or plaintext fallback.

use std::{fmt, num::NonZeroU16, sync::Arc, time::Duration};

use atm_storage::{
    AtmError, HostName, PeerConfigStore, PinnedClientVerifier, TlsIdentity, TrustedPeer,
    TrustedPeerCatalogAudit, certificate_fingerprint, certificate_valid_now, install_tls_provider,
    normalize_fingerprint,
};
use rustls::{
    CertificateError, ClientConfig, DigitallySignedStruct, Error, ServerConfig, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
    time::timeout,
};
use tokio_rustls::{TlsAcceptor, TlsConnector, TlsStream};

const DEFAULT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// Name of the explicit, testing/benchmarking-only opt-out that downgrades a
/// legacy literal-IP enabled trusted-peer row from a fail-closed startup
/// error to a skipped-with-warning row. Exact value `"1"` selects
/// [`LegacyLiteralIpPolicy::SkipWithWarning`]; any other value, or the
/// variable being unset, keeps the default
/// [`LegacyLiteralIpPolicy::FailClosed`]. This must never be treated as a
/// wire-mode selector: it narrows only which trusted-peer rows are admitted
/// into an already-selected mTLS configuration.
pub const LEGACY_LITERAL_IP_SKIP_ENV_VAR: &str = "ATM_PEER_TRUST_SKIP_LEGACY_LITERAL_IP";

/// Startup policy for legacy literal-IP trusted-peer rows discovered while
/// composing an mTLS adapter. The default is fail-closed: a live literal-IP
/// authority blocks daemon startup with exact migration guidance rather than
/// silently trusting or silently dropping it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LegacyLiteralIpPolicy {
    /// Refuse to start when any enabled trusted peer uses a literal-IP
    /// authority. This is the safe default for every normal daemon launch.
    #[default]
    FailClosed,
    /// Skip enabled literal-IP rows (emitting a `tracing::warn!` naming each
    /// host and this opt-out) instead of failing startup. Scoped to
    /// testing/benchmarking; never enabled implicitly.
    SkipWithWarning,
}

/// Concrete mTLS facade assembled only after the daemon has selected mTLS.
///
/// The adapter snapshots durable interface, local identity, and trust data at
/// composition time. It cannot infer or select a peer-wire mode itself.
pub struct MtlsPeerStreamAdapter {
    client_configs: Vec<PeerClientConfig>,
    client_verifier: Arc<PinnedClientVerifier>,
    server_config: Arc<ServerConfig>,
    handshake_timeout: Duration,
}

struct PeerClientConfig {
    authority: HostName,
    https_port: NonZeroU16,
    config: Arc<ClientConfig>,
}

impl fmt::Debug for MtlsPeerStreamAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MtlsPeerStreamAdapter")
            .finish_non_exhaustive()
    }
}

impl MtlsPeerStreamAdapter {
    /// Snapshot valid mTLS configuration from the storage-neutral control
    /// plane, failing closed on any legacy literal-IP enabled trusted peer.
    /// This is the convenience default used by every normal daemon launch;
    /// see [`Self::from_peer_config_with_policy`] for the explicit,
    /// testing/benchmarking-only skip opt-out.
    pub fn from_peer_config(store: &(dyn PeerConfigStore + Send + Sync)) -> Result<Self, AtmError> {
        Self::from_peer_config_with_policy(store, LegacyLiteralIpPolicy::FailClosed)
    }

    /// Snapshot valid mTLS configuration from the storage-neutral control
    /// plane under an explicit legacy literal-IP admission `policy`.
    ///
    /// # Errors
    /// Returns [`AtmError`] when the daemon has no enabled peer interface, no
    /// valid local identity, or (under [`LegacyLiteralIpPolicy::FailClosed`])
    /// any enabled trusted peer uses a literal-IP authority. The error names
    /// every offending host and carries the exact `atm peer trust migrate`
    /// or `atm peer trust revoke` remediation commands.
    pub fn from_peer_config_with_policy(
        store: &(dyn PeerConfigStore + Send + Sync),
        policy: LegacyLiteralIpPolicy,
    ) -> Result<Self, AtmError> {
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
        let trusted_peers = Self::admit_trusted_peers(store.list_trusted_peers()?, policy)?;
        install_tls_provider();
        let client_configs = trusted_peers
            .iter()
            .filter(|peer| peer.enabled)
            .map(|peer| {
                Ok(PeerClientConfig {
                    authority: peer.host.clone(),
                    https_port: peer.https_port,
                    config: Arc::new(Self::build_client_config(&identity, peer)?),
                })
            })
            .collect::<Result<Vec<_>, AtmError>>()?;
        let client_verifier = Arc::new(PinnedClientVerifier::new(trusted_peers));
        let server_config = Arc::new(Self::build_server_config(
            &identity,
            Arc::clone(&client_verifier),
        )?);
        Ok(Self {
            client_configs,
            client_verifier,
            server_config,
            handshake_timeout: DEFAULT_HANDSHAKE_TIMEOUT,
        })
    }

    /// Applies the legacy literal-IP admission `policy` to `trusted_peers`,
    /// warning about (but never blocking on) disabled legacy rows and either
    /// failing closed on, or skipping with a warning, enabled legacy rows.
    /// [`PinnedServerVerifier::new`] retains its own durable-hostname check
    /// as a defence-in-depth backstop after this admission step.
    fn admit_trusted_peers(
        trusted_peers: Vec<TrustedPeer>,
        policy: LegacyLiteralIpPolicy,
    ) -> Result<Vec<TrustedPeer>, AtmError> {
        let audit = TrustedPeerCatalogAudit::from_peers(&trusted_peers);
        warn_disabled_legacy_literal_ip_rows(&audit);
        if audit.legacy_literal_enabled_hosts().is_empty() {
            return Ok(trusted_peers);
        }
        match policy {
            LegacyLiteralIpPolicy::FailClosed => Err(legacy_literal_ip_fail_closed_error(&audit)),
            LegacyLiteralIpPolicy::SkipWithWarning => {
                warn_skipped_legacy_literal_ip_rows(&audit);
                Ok(trusted_peers
                    .into_iter()
                    .filter(|peer| peer.host.is_durable_hostname() || !peer.enabled)
                    .collect())
            }
        }
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
    ) -> Result<impl AsyncRead + AsyncWrite + Send + Unpin + use<>, AtmError> {
        let port = tcp
            .peer_addr()
            .map_err(|error| {
                AtmError::peer_authentication("peer stream address unavailable")
                    .with_cause(error.to_string())
            })?
            .port();
        let port = NonZeroU16::new(port)
            .ok_or_else(|| AtmError::peer_authentication("peer stream has invalid port"))?;
        let config = self.client_config_for(peer, port)?;
        let server_name = ServerName::try_from(peer.as_str().to_owned()).map_err(|_| {
            AtmError::peer_authentication("configured peer hostname is not valid for mTLS")
        })?;
        let connector = TlsConnector::from(Arc::clone(config));
        timeout(self.handshake_timeout, connector.connect(server_name, tcp))
            .await
            .map_err(|_| AtmError::transport_timeout("mTLS peer handshake deadline expired"))?
            .map(TlsStream::Client)
            .map_err(outbound_handshake_error)
    }

    /// Authenticate an inbound TCP byte stream against the configured client pins.
    pub async fn accept(
        &self,
        tcp: TcpStream,
    ) -> Result<impl AsyncRead + AsyncWrite + Send + Unpin + use<>, AtmError> {
        self.accept_server(tcp).await.map(TlsStream::Server)
    }

    async fn accept_server(
        &self,
        tcp: TcpStream,
    ) -> Result<tokio_rustls::server::TlsStream<TcpStream>, AtmError> {
        let acceptor = TlsAcceptor::from(Arc::clone(&self.server_config));
        timeout(self.handshake_timeout, acceptor.accept(tcp))
            .await
            .map_err(|_| AtmError::transport_timeout("mTLS peer handshake deadline expired"))?
            .map_err(inbound_handshake_error)
    }

    /// Authenticate an inbound TCP stream and return the configured identity
    /// proven by its client certificate.  The caller receives no certificate
    /// bytes, trust record, or Rustls connection state.
    pub async fn accept_with_peer(
        &self,
        tcp: TcpStream,
    ) -> Result<(impl AsyncRead + AsyncWrite + Send + Unpin + use<>, HostName), AtmError> {
        let stream = self.accept_server(tcp).await?;
        let source_host = self
            .client_verifier
            .authenticated_host(stream.get_ref().1)?;
        Ok((TlsStream::Server(stream), source_host))
    }

    fn client_config_for(
        &self,
        peer: &HostName,
        port: NonZeroU16,
    ) -> Result<&Arc<ClientConfig>, AtmError> {
        self.client_configs
            .iter()
            .find(|candidate| candidate.authority == *peer && candidate.https_port == port)
            .or_else(|| {
                let mut matches = self
                    .client_configs
                    .iter()
                    .filter(|candidate| candidate.authority == *peer);
                let first = matches.next()?;
                if matches.next().is_none() {
                    Some(first)
                } else {
                    None
                }
            })
            .map(|candidate| &candidate.config)
            .ok_or_else(|| {
                AtmError::peer_authentication(
                    "peer is not an enabled exact mTLS authority; plaintext fallback is forbidden",
                )
            })
    }

    fn build_client_config(
        identity: &TlsIdentity,
        peer: &TrustedPeer,
    ) -> Result<ClientConfig, AtmError> {
        ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(PinnedServerVerifier::new(peer)?))
            .with_client_auth_cert(
                identity.certificates().to_vec(),
                identity.private_key().clone_key(),
            )
            .map_err(|source| {
                AtmError::certificate_operation(
                    "configured mTLS identity cannot authenticate clients",
                )
                .with_cause(source)
            })
    }

    fn build_server_config(
        identity: &TlsIdentity,
        client_verifier: Arc<PinnedClientVerifier>,
    ) -> Result<ServerConfig, AtmError> {
        ServerConfig::builder()
            .with_client_cert_verifier(client_verifier)
            .with_single_cert(
                identity.certificates().to_vec(),
                identity.private_key().clone_key(),
            )
            .map_err(|source| {
                AtmError::certificate_operation(
                    "configured mTLS identity cannot authenticate peers",
                )
                .with_cause(source)
            })
    }
}

/// Builds the fail-closed startup error for one or more enabled legacy
/// literal-IP trusted-peer rows. The message names every offending host and
/// the cause carries the exact migrate/revoke remediation commands, so an
/// operator never needs a manual SQLite edit to recover.
fn legacy_literal_ip_fail_closed_error(audit: &TrustedPeerCatalogAudit) -> AtmError {
    let hosts = audit
        .legacy_literal_enabled_hosts()
        .iter()
        .map(HostName::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    AtmError::peer_config_validation(format!(
        "mTLS peer authority must use a durable hostname rather than a literal IP; \
         enabled legacy literal-IP trusted peer(s) block startup: {hosts}. Migrate or \
         revoke each row (see cause), or set {LEGACY_LITERAL_IP_SKIP_ENV_VAR}=1 to skip \
         them for testing/benchmarking only."
    ))
    .with_cause(audit.remediation_text())
}

/// Emits one `tracing::warn!` naming every disabled legacy literal-IP row.
/// Disabled rows are historical only and must never block startup, but an
/// operator should still be told they exist and how to prune them.
fn warn_disabled_legacy_literal_ip_rows(audit: &TrustedPeerCatalogAudit) {
    if audit.legacy_literal_disabled_hosts().is_empty() {
        return;
    }
    let hosts = audit
        .legacy_literal_disabled_hosts()
        .iter()
        .map(HostName::to_string)
        .collect::<Vec<_>>();
    tracing::warn!(
        hosts = ?hosts,
        remediation = %audit.remediation_text(),
        "disabled legacy literal-IP trusted peer row(s) present; they do not block mTLS \
         startup but should be pruned"
    );
}

/// Emits one `tracing::warn!` naming every enabled legacy literal-IP row
/// skipped under the explicit testing/benchmarking opt-out.
fn warn_skipped_legacy_literal_ip_rows(audit: &TrustedPeerCatalogAudit) {
    let hosts = audit
        .legacy_literal_enabled_hosts()
        .iter()
        .map(HostName::to_string)
        .collect::<Vec<_>>();
    tracing::warn!(
        hosts = ?hosts,
        env_var = LEGACY_LITERAL_IP_SKIP_ENV_VAR,
        remediation = %audit.remediation_text(),
        "skipping enabled legacy literal-IP trusted peer row(s) for outbound/inbound mTLS \
         due to explicit testing/benchmarking opt-out; these rows are not authenticated"
    );
}

fn outbound_handshake_error(source: std::io::Error) -> AtmError {
    if is_certificate_authentication_error(&source) {
        AtmError::peer_authentication(
            "mTLS peer certificate did not satisfy the configured hostname or pin",
        )
    } else {
        AtmError::transport_protocol("mTLS peer handshake failed before application processing")
            .with_cause(source)
    }
}

fn inbound_handshake_error(source: std::io::Error) -> AtmError {
    if is_certificate_authentication_error(&source) {
        AtmError::peer_authentication(
            "mTLS client certificate did not satisfy the configured trust record",
        )
    } else {
        AtmError::transport_protocol("mTLS client handshake failed before application processing")
            .with_cause(source)
    }
}

fn is_certificate_authentication_error(source: &std::io::Error) -> bool {
    source
        .get_ref()
        .and_then(|cause| cause.downcast_ref::<Error>())
        .is_some_and(|cause| matches!(cause, Error::InvalidCertificate(_)))
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
            fingerprint: normalize_fingerprint(peer.fingerprint.as_str()),
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
        peer_with_host(certificate, "localhost", enabled)
    }

    fn peer_with_host(certificate: &LocalCertificate, host: &str, enabled: bool) -> TrustedPeer {
        TrustedPeer {
            host: host.parse().expect("host"),
            fingerprint: certificate.fingerprint.clone(),
            enabled,
            https_port: NonZeroU16::new(443).expect("port"),
        }
    }

    #[tokio::test]
    async fn configured_pair_exchanges_opaque_bytes_over_mutual_tls_and_returns_pinned_identity() {
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
            let (mut tls, peer) = server.accept_with_peer(tcp).await.expect("accept tls");
            assert_eq!(
                peer.as_str(),
                "localhost",
                "server reports the configured pinned peer"
            );
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
    fn disabled_peer_fails_before_tcp_or_http_work() {
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
        let error = adapter
            .client_config_for(&host, NonZeroU16::new(443).unwrap())
            .expect_err("disabled peer");
        assert_eq!(error.code().as_str(), "ATM_PEER_AUTHENTICATION_FAILED");
    }

    #[test]
    fn hostname_mismatch_fails_before_tcp_or_http_work() {
        let directory = tempfile::tempdir().expect("directory");
        let identity = identity(&directory, "local");
        let adapter = MtlsPeerStreamAdapter::from_peer_config(&TestStore {
            interfaces: vec![interface()],
            certificate: Some(identity.clone()),
            peers: vec![peer(&identity, true)],
        })
        .expect("adapter");
        let mismatched_host: HostName = "other-peer.example".parse().expect("host");
        let error = adapter
            .client_config_for(&mismatched_host, NonZeroU16::new(443).unwrap())
            .expect_err("mismatched hostname");
        assert_eq!(error.code().as_str(), "ATM_PEER_AUTHENTICATION_FAILED");
    }

    #[test]
    fn configured_tls_configs_are_cached_at_adapter_composition() {
        let directory = tempfile::tempdir().expect("directory");
        let local = identity(&directory, "local");
        let peer_identity = identity(&directory, "peer");
        let adapter = MtlsPeerStreamAdapter::from_peer_config(&TestStore {
            interfaces: vec![interface()],
            certificate: Some(local),
            peers: vec![peer(&peer_identity, true)],
        })
        .expect("adapter");
        let host: HostName = "localhost".parse().expect("host");
        let first = adapter
            .client_config_for(&host, NonZeroU16::new(443).unwrap())
            .expect("cached config");
        let second = adapter
            .client_config_for(&host, NonZeroU16::new(443).unwrap())
            .expect("cached config");
        assert!(Arc::ptr_eq(first, second));
        assert_eq!(adapter.client_configs.len(), 1);
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

    #[test]
    fn server_verifier_uses_canonical_fingerprint_normalization() {
        let directory = tempfile::tempdir().expect("directory");
        let identity = identity(&directory, "peer");
        let mut configured = peer(&identity, true);
        configured.fingerprint = CertificateFingerprint::try_from(
            identity
                .fingerprint
                .as_str()
                .as_bytes()
                .chunks(2)
                .map(|chunk| {
                    std::str::from_utf8(chunk)
                        .expect("hex")
                        .to_ascii_uppercase()
                })
                .collect::<Vec<_>>()
                .join(":"),
        )
        .expect("fingerprint");
        let verifier = PinnedServerVerifier::new(&configured).expect("verifier");
        assert_eq!(verifier.fingerprint, identity.fingerprint.as_str());
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
        let server_error = match server_task.await.expect("server task") {
            Err(error) => error,
            Ok(_) => panic!("untrusted client must not complete mTLS"),
        };
        assert_eq!(
            server_error.code().as_str(),
            "ATM_PEER_AUTHENTICATION_FAILED"
        );
        // TLS 1.3 can let the client receive the server Finished before the
        // server observes and rejects its certificate. The authoritative
        // inbound decision above nevertheless fails before any stream is
        // handed to application protocol code, so no opaque bytes can flow.
    }

    #[tokio::test(start_paused = true)]
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
        let (connected_tx, connected_rx) = tokio::sync::oneshot::channel();
        let raw_client = tokio::spawn(async move {
            let _tcp = TcpStream::connect(address).await.expect("connect tcp");
            connected_tx.send(()).expect("signal connected client");
            std::future::pending::<()>().await;
        });
        let (tcp, _) = listener.accept().await.expect("accept tcp");
        connected_rx
            .await
            .expect("client connects before handshake");
        let server_accept = tokio::spawn(async move { server.accept(tcp).await });
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_millis(50)).await;
        let error = match server_accept.await.expect("server accept task joins") {
            Err(error) => error,
            Ok(_) => panic!("handshake deadline must fail"),
        };
        assert_eq!(error.code().as_str(), "ATM_TRANSPORT_TIMEOUT");
        raw_client.abort();
    }

    #[tokio::test]
    async fn malformed_handshake_fails_with_a_transport_protocol_code() {
        let directory = tempfile::tempdir().expect("directory");
        let server_identity = identity(&directory, "server");
        let client_identity = identity(&directory, "client");
        let server = MtlsPeerStreamAdapter::from_peer_config(&TestStore {
            interfaces: vec![interface()],
            certificate: Some(server_identity),
            peers: vec![peer(&client_identity, true)],
        })
        .expect("server adapter");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        let raw_client = tokio::spawn(async move {
            let mut tcp = TcpStream::connect(address).await.expect("connect tcp");
            tcp.write_all(b"not a TLS client hello")
                .await
                .expect("write junk");
        });
        let (tcp, _) = listener.accept().await.expect("accept tcp");
        let error = match server.accept(tcp).await {
            Err(error) => error,
            Ok(_) => panic!("malformed handshake must fail"),
        };
        assert_eq!(error.code().as_str(), "ATM_TRANSPORT_PROTOCOL_FAILED");
        raw_client.await.expect("raw client");
    }

    #[test]
    fn mixed_catalog_enabled_literal_ip_fails_closed_and_names_the_host() {
        let directory = tempfile::tempdir().expect("directory");
        let local = identity(&directory, "local");
        let durable = identity(&directory, "durable-peer");
        let legacy = identity(&directory, "legacy-peer");
        let error = MtlsPeerStreamAdapter::from_peer_config(&TestStore {
            interfaces: vec![interface()],
            certificate: Some(local),
            peers: vec![
                peer_with_host(&durable, "rand-m5.local", true),
                peer_with_host(&legacy, "192.168.128.29", true),
            ],
        })
        .expect_err("enabled legacy literal-IP peer must fail closed");
        assert_eq!(error.code().as_str(), "ATM_PEER_CONFIG_VALIDATION_FAILED");
        assert!(error.message().contains("192.168.128.29"));
        assert!(
            error
                .cause()
                .is_some_and(|cause| cause.contains("192.168.128.29"))
        );
    }

    #[test]
    fn mixed_catalog_disabled_literal_ip_never_blocks_a_valid_hostname_configuration() {
        let directory = tempfile::tempdir().expect("directory");
        let local = identity(&directory, "local");
        let durable = identity(&directory, "durable-peer");
        let legacy = identity(&directory, "legacy-peer");
        let adapter = MtlsPeerStreamAdapter::from_peer_config(&TestStore {
            interfaces: vec![interface()],
            certificate: Some(local),
            peers: vec![
                peer_with_host(&durable, "rand-m5.local", true),
                peer_with_host(&legacy, "192.168.128.29", false),
            ],
        })
        .expect("disabled legacy literal-IP row must not block startup");
        assert_eq!(adapter.client_configs.len(), 1);
        let hostname: HostName = "rand-m5.local".parse().expect("host");
        assert!(
            adapter
                .client_config_for(&hostname, NonZeroU16::new(443).unwrap())
                .is_ok()
        );
    }

    #[test]
    fn skip_with_warning_policy_admits_the_hostname_peer_without_trusting_the_ip() {
        let directory = tempfile::tempdir().expect("directory");
        let local = identity(&directory, "local");
        let durable = identity(&directory, "durable-peer");
        let legacy = identity(&directory, "legacy-peer");
        let adapter = MtlsPeerStreamAdapter::from_peer_config_with_policy(
            &TestStore {
                interfaces: vec![interface()],
                certificate: Some(local),
                peers: vec![
                    peer_with_host(&durable, "rand-m5.local", true),
                    peer_with_host(&legacy, "192.168.128.29", true),
                ],
            },
            LegacyLiteralIpPolicy::SkipWithWarning,
        )
        .expect("skip policy must not fail closed");
        assert_eq!(adapter.client_configs.len(), 1);
        let hostname: HostName = "rand-m5.local".parse().expect("host");
        assert!(
            adapter
                .client_config_for(&hostname, NonZeroU16::new(443).unwrap())
                .is_ok()
        );
        let literal_ip: HostName = "192.168.128.29".parse().expect("host");
        let error = adapter
            .client_config_for(&literal_ip, NonZeroU16::new(443).unwrap())
            .expect_err("skipped literal-IP row must not be an authority");
        assert_eq!(error.code().as_str(), "ATM_PEER_AUTHENTICATION_FAILED");
    }
}
