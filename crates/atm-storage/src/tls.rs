//! Canonical certificate and peer-pinning helpers shared by transport users.
//!
//! These types own only certificate parsing and client-certificate admission.
//! They do not own listeners, senders, routes, retries, or daemon lifecycle.

use std::fmt;
use std::path::Path;
use std::sync::RwLock;

use rustls::pki_types::{CertificateDer, PrivateKeyDer, UnixTime, pem::PemObject};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{CertificateError, DigitallySignedStruct, Error, ServerConnection, SignatureScheme};
use sha2::{Digest, Sha256};
use x509_parser::prelude::parse_x509_certificate;
use zeroize::Zeroizing;

use crate::contract::{LocalCertificate, TrustedPeer};
use crate::error::AtmError;
use crate::types::HostName;

/// Install ATM's explicit rustls provider selection once for the process.
pub fn install_tls_provider() {
    // Other workspace dependencies enable aws-lc as well as ring. ATM selects
    // ring explicitly once so HTTPS behavior is deterministic in every binary.
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// Parsed certificate chain and private key retained for a TLS adapter.
pub struct TlsIdentity {
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
    /// Parse and validate a durable certificate/key bundle.
    pub fn load(certificate: &LocalCertificate) -> Result<Self, AtmError> {
        let path = Path::new(certificate.private_key_ref.as_str());
        // The certificate chain is copied into rustls-owned DER values and the
        // private key into `PrivateKeyDer`; the source PEM must not remain in
        // process memory after this parser scope exits.
        let pem = Zeroizing::new(std::fs::read(path).map_err(|source| {
            AtmError::daemon_unavailable_with_cause(
                "failed to open the configured TLS certificate/key PEM bundle",
                source,
            )
        })?);
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
        certificate_valid_now(first).map_err(|_| {
            AtmError::validation("configured TLS certificate is expired or not yet valid")
        })?;
        Ok(Self {
            certificates,
            private_key,
            fingerprint,
        })
    }

    /// Certificate chain for rustls client/server configuration.
    pub fn certificates(&self) -> &[CertificateDer<'static>] {
        &self.certificates
    }

    /// Private key for rustls client/server configuration.
    pub fn private_key(&self) -> &PrivateKeyDer<'static> {
        &self.private_key
    }
}

/// Canonical client-certificate verifier for configured trusted peers.
pub struct PinnedClientVerifier {
    peers: RwLock<Vec<TrustedPeer>>,
    algorithms: rustls::crypto::WebPkiSupportedAlgorithms,
}

impl fmt::Debug for PinnedClientVerifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PinnedClientVerifier")
            .finish_non_exhaustive()
    }
}

impl PinnedClientVerifier {
    /// Construct a verifier from the currently enabled trusted peers.
    pub fn new(peers: Vec<TrustedPeer>) -> Self {
        Self {
            peers: RwLock::new(peers.into_iter().filter(|peer| peer.enabled).collect()),
            algorithms: rustls::crypto::ring::default_provider().signature_verification_algorithms,
        }
    }

    /// Replace the enabled peer snapshot after a trust refresh.
    pub fn replace(&self, peers: Vec<TrustedPeer>) -> Result<(), AtmError> {
        let mut current = self.peers.write().map_err(|source| {
            AtmError::daemon_unavailable("HTTPS peer verifier lock poisoned").with_cause(source)
        })?;
        *current = peers.into_iter().filter(|peer| peer.enabled).collect();
        Ok(())
    }

    /// Resolve an authenticated certificate to its configured source host.
    pub fn authenticated_host(&self, connection: &ServerConnection) -> Result<HostName, AtmError> {
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
        if certificate_valid_now(end_entity).is_err() {
            Err(CertificateError::Expired.into())
        } else if self.host_for_certificate(end_entity).is_some() {
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
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

/// Reject a malformed, expired, or not-yet-valid certificate before a stream
/// wrapper can expose application bytes.
pub fn certificate_valid_now(certificate: &CertificateDer<'_>) -> Result<(), Error> {
    let (_, parsed) = parse_x509_certificate(certificate.as_ref())
        .map_err(|_| Error::InvalidCertificate(CertificateError::BadEncoding))?;
    if parsed.validity().is_valid() {
        Ok(())
    } else {
        Err(CertificateError::Expired.into())
    }
}

/// Return the normalized SHA-256 fingerprint for a DER certificate.
pub fn certificate_fingerprint(certificate: &CertificateDer<'_>) -> String {
    format!("{:x}", Sha256::digest(certificate.as_ref()))
}

/// Normalize a configured fingerprint by removing presentation separators.
pub fn normalize_fingerprint(value: &str) -> String {
    value
        .chars()
        .filter(char::is_ascii_hexdigit)
        .collect::<String>()
        .to_ascii_lowercase()
}
