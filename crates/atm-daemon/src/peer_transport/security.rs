use std::net::{IpAddr, SocketAddr, TcpStream};
use std::sync::Arc;

use atm_core::error::AtmError;
use atm_storage::{
    AllowedHostName, LocalPeerIdentityRow, PeerSecurityMode, PeerSecurityStore, sha256_hex,
};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{WebPkiSupportedAlgorithms, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{
    ClientConfig, ClientConnection, DigitallySignedStruct, ServerConfig, ServerConnection,
    SignatureScheme, StreamOwned,
};

pub(super) type ClientTlsStream = StreamOwned<ClientConnection, TcpStream>;
pub(super) type ServerTlsStream = StreamOwned<ServerConnection, TcpStream>;

pub(super) fn load_peer_security_mode(
    store: Option<&Arc<dyn PeerSecurityStore + Send + Sync>>,
) -> Result<PeerSecurityMode, AtmError> {
    match store {
        Some(store) => store.load_security_settings().map(|row| row.mode),
        None => Ok(PeerSecurityMode::InsecureAllowed),
    }
}

pub(super) fn open_client_tls_stream(
    stream: TcpStream,
    endpoint: SocketAddr,
    store: &Arc<dyn PeerSecurityStore + Send + Sync>,
) -> Result<ClientTlsStream, AtmError> {
    let identity = store.load_or_create_local_identity()?;
    let expected = trusted_peer_fingerprint(store, endpoint.ip())?;
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    let algorithms = provider.signature_verification_algorithms;
    let client_config = ClientConfig::builder_with_provider(Arc::new(provider))
        .with_safe_default_protocol_versions()
        .map_err(rustls_config_error)?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinnedFingerprintServerVerifier {
            algorithms,
            expected_fingerprint: expected.clone(),
        }))
        .with_client_auth_cert(vec![certificate_der(&identity)], private_key_der(&identity))
        .map_err(rustls_config_error)?;
    let server_name = ServerName::IpAddress(endpoint.ip().into());
    let connection =
        ClientConnection::new(Arc::new(client_config), server_name).map_err(rustls_config_error)?;
    let mut tls = StreamOwned::new(connection, stream);
    complete_client_tls_handshake(&mut tls).map_err(rustls_io_error)?;
    Ok(tls)
}

pub(super) fn open_server_tls_stream(
    stream: TcpStream,
    peer_addr: SocketAddr,
    store: &Arc<dyn PeerSecurityStore + Send + Sync>,
) -> Result<ServerTlsStream, AtmError> {
    let identity = store.load_or_create_local_identity()?;
    let expected = trusted_peer_fingerprint(store, peer_addr.ip())?;
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    let algorithms = provider.signature_verification_algorithms;
    let server_config = ServerConfig::builder_with_provider(Arc::new(provider))
        .with_safe_default_protocol_versions()
        .map_err(rustls_config_error)?
        .with_client_cert_verifier(Arc::new(PinnedFingerprintClientVerifier {
            algorithms,
            expected_fingerprint: expected.clone(),
            root_hints: Vec::new(),
        }))
        .with_single_cert(vec![certificate_der(&identity)], private_key_der(&identity))
        .map_err(rustls_config_error)?;
    let connection = ServerConnection::new(Arc::new(server_config)).map_err(rustls_config_error)?;
    let mut tls = StreamOwned::new(connection, stream);
    complete_server_tls_handshake(&mut tls).map_err(rustls_io_error)?;
    Ok(tls)
}

fn trusted_peer_fingerprint(
    store: &Arc<dyn PeerSecurityStore + Send + Sync>,
    ip: IpAddr,
) -> Result<String, AtmError> {
    let host_name = AllowedHostName::new(ip.to_string())?;
    let Some(row) = store.load_trusted_peer(&host_name)? else {
        return Err(AtmError::validation(format!(
            "no trusted peer fingerprint is configured for remote daemon host `{host_name}`"
        ))
        .with_recovery(format!(
            "Run `atm daemon security trust approve {host_name} --fingerprint <sha256>` before retrying secure cross-host delivery."
        )));
    };
    Ok(row.fingerprint_sha256().to_string())
}

fn verify_presented_peer_fingerprint(
    end_entity: &CertificateDer<'_>,
    expected_fingerprint: &str,
    mismatch_message: &'static str,
) -> Result<(), rustls::Error> {
    let presented = sha256_hex(end_entity.as_ref());
    if presented == expected_fingerprint {
        return Ok(());
    }
    Err(rustls::Error::General(format!(
        "{mismatch_message}; expected {expected_fingerprint}, received {presented}"
    )))
}

fn note_adr030_pinned_fingerprint_time_model(_now: UnixTime) {
    // ADR-030 governs this behavior. ATM cross-host TLS currently uses
    // host-scoped fingerprint pinning from SQLite rather than PKI chain/expiry
    // validation. rustls supplies a verification-time parameter; binding it
    // explicitly here prevents that design choice from remaining an accidental
    // silent discard inside the verifier callbacks.
}

fn certificate_der(identity: &LocalPeerIdentityRow) -> CertificateDer<'static> {
    CertificateDer::from(identity.certificate_der().to_vec())
}

fn private_key_der(identity: &LocalPeerIdentityRow) -> PrivateKeyDer<'static> {
    PrivateKeyDer::from(PrivatePkcs8KeyDer::from(
        identity.private_key_der().to_vec(),
    ))
}

fn complete_client_tls_handshake(tls: &mut ClientTlsStream) -> Result<(), std::io::Error> {
    while tls.conn.is_handshaking() || tls.conn.wants_write() {
        let _ = tls.conn.complete_io(&mut tls.sock)?;
    }
    Ok(())
}

fn complete_server_tls_handshake(tls: &mut ServerTlsStream) -> Result<(), std::io::Error> {
    while tls.conn.is_handshaking() || tls.conn.wants_write() {
        let _ = tls.conn.complete_io(&mut tls.sock)?;
    }
    Ok(())
}

fn rustls_config_error(error: rustls::Error) -> AtmError {
    AtmError::daemon_unavailable(format!(
        "failed to assemble secure daemon peer transport configuration: {error}"
    ))
    .with_recovery(
        "Repair the daemon local identity or trusted peer configuration before retrying secure cross-host delivery.",
    )
}

fn rustls_io_error(error: std::io::Error) -> AtmError {
    let message = error.to_string();
    if is_peer_certificate_validation_failure(&message) {
        return AtmError::validation(format!(
            "secure daemon peer transport handshake rejected the presented peer certificate: {message}"
        ))
        .with_recovery(
            "Approve the correct trusted peer fingerprint or repair the secure peer trust rows before retrying secure cross-host delivery.",
        );
    }
    AtmError::daemon_unavailable(format!(
        "secure daemon peer transport handshake failed: {message}"
    ))
    .with_recovery(
        "Confirm the remote daemon is reachable and presenting the expected peer certificate before retrying secure cross-host delivery.",
    )
}

fn is_peer_certificate_validation_failure(message: &str) -> bool {
    message.contains("fingerprint did not match the approved trusted peer row")
        || message.contains("invalid peer certificate")
}

#[derive(Debug)]
/// Deliberate trust model per ADR-030: same-host / cross-host ATM secure
/// transport uses a pinned SHA256 certificate fingerprint from SQLite, not a
/// PKI chain. The TLS handshake rejects any peer whose presented leaf
/// certificate fingerprint does not match the approved row for that host.
struct PinnedFingerprintServerVerifier {
    algorithms: WebPkiSupportedAlgorithms,
    expected_fingerprint: String,
}

impl ServerCertVerifier for PinnedFingerprintServerVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        note_adr030_pinned_fingerprint_time_model(now);
        verify_presented_peer_fingerprint(
            end_entity,
            &self.expected_fingerprint,
            "remote daemon certificate fingerprint did not match the approved trusted peer row",
        )?;
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

#[derive(Debug)]
/// Deliberate trust model per ADR-030: mutual TLS client authentication uses
/// the same pinned-fingerprint verification as the server side. A peer client
/// certificate is accepted only when its SHA256 fingerprint matches the
/// approved trust row for the connecting host.
struct PinnedFingerprintClientVerifier {
    algorithms: WebPkiSupportedAlgorithms,
    expected_fingerprint: String,
    root_hints: Vec<rustls::DistinguishedName>,
}

impl ClientCertVerifier for PinnedFingerprintClientVerifier {
    fn root_hint_subjects(&self) -> &[rustls::DistinguishedName] {
        &self.root_hints
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        note_adr030_pinned_fingerprint_time_model(now);
        verify_presented_peer_fingerprint(
            end_entity,
            &self.expected_fingerprint,
            "remote daemon client certificate fingerprint did not match the approved trusted peer row",
        )?;
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }

    fn client_auth_mandatory(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::verify_presented_peer_fingerprint;
    use atm_storage::sha256_hex;
    use rustls::pki_types::CertificateDer;

    #[test]
    fn fingerprint_verifier_accepts_matching_leaf_certificate() {
        let cert = CertificateDer::from(vec![1_u8, 2, 3, 4]);
        let expected = sha256_hex(cert.as_ref());

        let result =
            verify_presented_peer_fingerprint(&cert, &expected, "certificate fingerprint mismatch");

        assert!(result.is_ok());
    }

    #[test]
    fn fingerprint_verifier_rejects_mismatched_leaf_certificate_during_callback() {
        let cert = CertificateDer::from(vec![9_u8, 8, 7, 6]);

        let error = verify_presented_peer_fingerprint(
            &cert,
            &"00".repeat(32),
            "certificate fingerprint mismatch",
        )
        .expect_err("mismatch should fail in verifier");

        assert!(
            error
                .to_string()
                .contains("certificate fingerprint mismatch")
        );
    }
}
