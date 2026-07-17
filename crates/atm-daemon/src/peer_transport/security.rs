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
        .with_custom_certificate_verifier(Arc::new(PermissiveServerVerifier { algorithms }))
        .with_client_auth_cert(vec![certificate_der(&identity)], private_key_der(&identity))
        .map_err(rustls_config_error)?;
    let server_name = ServerName::IpAddress(endpoint.ip().into());
    let connection =
        ClientConnection::new(Arc::new(client_config), server_name).map_err(rustls_config_error)?;
    let mut tls = StreamOwned::new(connection, stream);
    complete_client_tls_handshake(&mut tls).map_err(rustls_io_error)?;
    ensure_presented_peer_matches(
        tls.conn.peer_certificates(),
        &expected,
        "remote daemon certificate fingerprint did not match the approved trusted peer row",
    )?;
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
        .with_client_cert_verifier(Arc::new(PermissiveClientVerifier {
            algorithms,
            root_hints: Vec::new(),
        }))
        .with_single_cert(vec![certificate_der(&identity)], private_key_der(&identity))
        .map_err(rustls_config_error)?;
    let connection = ServerConnection::new(Arc::new(server_config)).map_err(rustls_config_error)?;
    let mut tls = StreamOwned::new(connection, stream);
    complete_server_tls_handshake(&mut tls).map_err(rustls_io_error)?;
    ensure_presented_peer_matches(
        tls.conn.peer_certificates(),
        &expected,
        "remote daemon client certificate fingerprint did not match the approved trusted peer row",
    )?;
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

fn ensure_presented_peer_matches(
    peer_certs: Option<&[CertificateDer<'static>]>,
    expected_fingerprint: &str,
    mismatch_message: &'static str,
) -> Result<(), AtmError> {
    let Some(peer_cert) = peer_certs.and_then(|certs| certs.first()) else {
        return Err(AtmError::validation(
            "remote daemon did not present a peer certificate during the secure transport handshake"
                .to_string(),
        )
        .with_recovery(
            "Ensure both daemons are running in secure-required mode with a generated local identity before retrying secure cross-host delivery.",
        ));
    };
    let presented = sha256_hex(peer_cert.as_ref());
    if presented != expected_fingerprint {
        return Err(AtmError::validation(format!(
            "{mismatch_message}; expected {expected_fingerprint}, received {presented}"
        ))
        .with_recovery(
            "Approve the correct trusted peer fingerprint or remove the stale trust row before retrying secure cross-host delivery.",
        ));
    }
    Ok(())
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
    AtmError::daemon_unavailable(format!(
        "secure daemon peer transport handshake failed: {error}"
    ))
    .with_recovery(
        "Confirm the remote daemon is reachable and presenting the expected peer certificate before retrying secure cross-host delivery.",
    )
}

#[derive(Debug)]
struct PermissiveServerVerifier {
    algorithms: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for PermissiveServerVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
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
struct PermissiveClientVerifier {
    algorithms: WebPkiSupportedAlgorithms,
    root_hints: Vec<rustls::DistinguishedName>,
}

impl ClientCertVerifier for PermissiveClientVerifier {
    fn root_hint_subjects(&self) -> &[rustls::DistinguishedName] {
        &self.root_hints
    }

    fn verify_client_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
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
