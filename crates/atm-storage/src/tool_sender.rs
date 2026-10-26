//! Storage-neutral contracts for authenticated virtual tool senders.
//!
//! These values describe signed installer and runtime provenance.  They are
//! deliberately not a substitute for the daemon-owned capability used to emit
//! mail: callers can deserialize a proof, but only a registry implementation
//! backed by the protected installer trust root can admit it.

use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::address::AgentAddress;
use crate::contract::sealed;
use crate::error::{AtmError, AtmErrorCode};
use crate::schema::AtmMessageId;
use crate::types::{HostName, IsoTimestamp};

/// Stable failure vocabulary for installer and runtime sender admission.
///
/// Registry implementations return the corresponding [`AtmErrorCode`] through
/// [`AtmError`]. These cases deliberately distinguish invalid claims from
/// unknown transaction outcomes so callers can choose safe recovery behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolSenderFailure {
    SenderUnregistered,
    SenderReserved,
    SenderUnauthorized,
    KeyUnknown,
    KeyRevoked,
    NonceReplay,
    HostMismatch,
    GenerationConflict,
    SignatureInvalid,
    RequestExpired,
    CommitUncertain,
}

impl ToolSenderFailure {
    #[must_use]
    pub const fn code(self) -> AtmErrorCode {
        match self {
            Self::SenderUnregistered => AtmErrorCode::ToolSenderUnregistered,
            Self::SenderReserved => AtmErrorCode::ToolSenderReserved,
            Self::SenderUnauthorized => AtmErrorCode::ToolSenderUnauthorized,
            Self::KeyUnknown => AtmErrorCode::ToolKeyUnknown,
            Self::KeyRevoked => AtmErrorCode::ToolKeyRevoked,
            Self::NonceReplay => AtmErrorCode::ToolNonceReplay,
            Self::HostMismatch => AtmErrorCode::ToolHostMismatch,
            Self::GenerationConflict => AtmErrorCode::ToolGenerationConflict,
            Self::SignatureInvalid => AtmErrorCode::ToolSignatureInvalid,
            Self::RequestExpired => AtmErrorCode::ToolRequestExpired,
            Self::CommitUncertain => AtmErrorCode::ToolCommitUncertain,
        }
    }

    /// Builds the public ATM error with the catalog recovery for this failure.
    #[must_use]
    pub fn into_error(self, detail: impl Into<String>) -> AtmError {
        AtmError::new(self.code(), detail)
    }
}

macro_rules! validated_text_id {
    ($name:ident, $subject:literal) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: &str) -> Result<Self, AtmError> {
                let value = value.trim();
                if value.is_empty() || value.len() > 256 {
                    return Err(AtmError::validation_with_recovery(
                        format!(concat!($subject, " must contain 1..=256 bytes")),
                        concat!("provide a stable ", $subject),
                    ));
                }
                Ok(Self(value.to_owned()))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = AtmError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::parse(&value)
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

validated_text_id!(PluginSenderId, "plugin sender id");
validated_text_id!(InstallerId, "installer id");
validated_text_id!(ToolNonce, "tool nonce");
validated_text_id!(MessageDigest, "message digest");
validated_text_id!(ToolKeyId, "tool key id");

impl MessageDigest {
    /// Computes the canonical SHA-256 digest for UTF-8 message body bytes.
    #[must_use]
    pub fn for_message_body(body: &str) -> Self {
        let digest = Sha256::digest(body.as_bytes());
        let mut encoded = String::with_capacity(7 + digest.len() * 2);
        encoded.push_str("sha256:");
        for byte in digest {
            use std::fmt::Write as _;
            let _ = write!(encoded, "{byte:02x}");
        }
        Self(encoded)
    }
}

/// The only virtual identities that can be admitted by trusted daemon code.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolPrincipal {
    Cron,
    RegisteredPlugin { sender: PluginSenderId },
}

/// A registry-admitted sender.  Its fields are private so callers cannot make
/// an arbitrary `@tool` address look registered.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegisteredToolSender {
    principal: ToolPrincipal,
    address: AgentAddress,
}

impl RegisteredToolSender {
    /// Projects registry-owned sender inspection state into its public value.
    ///
    /// This is deliberately not an emission credential. Only a registry-owned
    /// issuer record can implement [`RegisteredToolSenderIssuer`], and this
    /// projection does not create a [`ToolRuntimeAdmission`]. Runtime admission
    /// still requires a distinct issuer that binds verified proof to a message.
    pub fn from_issuer(issuer: &impl RegisteredToolSenderIssuer) -> Self {
        Self {
            principal: issuer.principal().clone(),
            address: issuer.address().clone(),
        }
    }

    #[must_use]
    pub fn principal(&self) -> &ToolPrincipal {
        &self.principal
    }

    #[must_use]
    pub fn address(&self) -> &AgentAddress {
        &self.address
    }
}

/// Read-only registry-owned state used to project a [`RegisteredToolSender`].
///
/// This ADR-001 workspace-convention seal is authorized only for the private
/// SQLite registry record and deterministic in-memory inspection record. Its
/// values are inspection data; implementing it does not grant runtime
/// admission or emission authority. That authority is issued separately by
/// [`ToolRuntimeAdmissionIssuer`] after proof/message binding.
pub trait RegisteredToolSenderIssuer: sealed::Sealed {
    /// Registered virtual principal selected by the registry.
    fn principal(&self) -> &ToolPrincipal;

    /// Canonical address stored by the registry for that principal.
    fn address(&self) -> &AgentAddress;
}

/// Opaque result of a successful, durable runtime admission.
///
/// The private marker is minted only in this module after a registry adapter
/// has checked immutable registration state and bound the runtime proof to a
/// message id and digest. It has no serde implementation and cannot be
/// reconstructed from a resolved sender or a peer/CLI payload.
#[derive(Debug)]
pub struct ToolRuntimeAdmission {
    sender: RegisteredToolSender,
    binding: VerifiedRuntimeBinding,
}

/// Immutable runtime proof binding retained inside an admitted sender.
///
/// This is intentionally private: it records the exact authenticated sender,
/// key, generation, host, message id and digest that the registry committed.
/// It is proof that a later consumer received an admission rather than an
/// inspection result from [`ToolSenderRegistry::resolve`].
#[allow(dead_code)]
#[derive(Debug)]
struct VerifiedRuntimeBinding {
    sender: PluginSenderId,
    runtime_key_id: ToolKeyId,
    registration_generation: u64,
    host_id: HostName,
    message_id: AtmMessageId,
    message_digest: MessageDigest,
    nonce: ToolNonce,
    signature: String,
}

impl ToolRuntimeAdmission {
    /// Projects an opaque runtime admission from an authorized issuer record.
    ///
    /// [`ToolRuntimeAdmissionIssuer`] uses ADR-001's workspace-convention
    /// seal.  The SQLite registry adapter creates its private issuer record
    /// only after its verification transaction commits; the deterministic
    /// in-memory registry owns the corresponding test-only record.  This
    /// cross-crate seam deliberately has no constructor accepting a resolved
    /// [`RegisteredToolSender`]: `ToolSenderRegistry::resolve` remains
    /// inspection-only and cannot mint an emission admission.
    pub fn from_issuer(issuer: &impl ToolRuntimeAdmissionIssuer) -> Result<Self, AtmError> {
        Self::verified(
            issuer.sender().clone(),
            issuer.proof(),
            issuer.message_id(),
            issuer.message_digest().clone(),
        )
    }

    /// Mints an opaque admission after the registry transaction commits.
    pub(crate) fn verified(
        sender: RegisteredToolSender,
        proof: &ToolRuntimeProof,
        message_id: AtmMessageId,
        message_digest: MessageDigest,
    ) -> Result<Self, AtmError> {
        if proof.message_id != message_id || proof.message_digest != message_digest {
            return Err(AtmError::validation_with_recovery(
                "tool runtime proof does not bind the requested message id and digest",
                "submit the original proof and canonical message unchanged",
            ));
        }
        Ok(Self {
            sender,
            binding: VerifiedRuntimeBinding {
                sender: proof.sender.clone(),
                runtime_key_id: proof.runtime_key_id.clone(),
                registration_generation: proof.registration_generation,
                host_id: proof.host_id.clone(),
                message_id,
                message_digest,
                nonce: proof.nonce.clone(),
                signature: proof.signature.clone(),
            },
        })
    }

    #[must_use]
    pub fn sender(&self) -> &RegisteredToolSender {
        let _ = &self.binding;
        &self.sender
    }

    #[must_use]
    pub fn message_id(&self) -> AtmMessageId {
        self.binding.message_id
    }

    #[must_use]
    pub fn message_digest(&self) -> &MessageDigest {
        &self.binding.message_digest
    }

    /// Confirms that the request carries the exact proof admitted by the registry.
    #[must_use]
    pub fn matches_proof(&self, proof: &ToolRuntimeProof) -> bool {
        self.binding.sender == proof.sender
            && self.binding.runtime_key_id == proof.runtime_key_id
            && self.binding.registration_generation == proof.registration_generation
            && self.binding.host_id == proof.host_id
            && self.binding.message_id == proof.message_id
            && self.binding.message_digest == proof.message_digest
            && self.binding.nonce == proof.nonce
            && self.binding.signature == proof.signature
    }
}

/// Read-only, adapter-produced provenance for one runtime admission.
///
/// This is an ADR-001 workspace-convention sealed trait.  Only the documented
/// SQLite registry adapter and the deterministic in-memory double may
/// implement it; the architecture test keeps that source allowlist exact.
/// The record carries the verified sender and the canonical proof/message
/// binding, so a `resolve` result by itself cannot become an admission.
pub trait ToolRuntimeAdmissionIssuer: sealed::Sealed {
    /// Sender selected by the adapter's verified registration state.
    fn sender(&self) -> &RegisteredToolSender;

    /// Runtime proof accepted by the adapter's verification transaction.
    fn proof(&self) -> &ToolRuntimeProof;

    /// Canonical durable message identity bound by the proof.
    fn message_id(&self) -> AtmMessageId;

    /// Canonical message digest bound by the proof.
    fn message_digest(&self) -> &MessageDigest;
}

const INSTALL_DOMAIN: &str = "ATM-TOOL-INSTALL-v1";
const MESSAGE_DOMAIN: &str = "ATM-TOOL-MESSAGE-v1";
const PEER_DOMAIN: &str = "ATM-TOOL-PEER-v1";

fn canonical_field(bytes: &mut Vec<u8>, value: &str) -> Result<(), AtmError> {
    let length = u32::try_from(value.len()).map_err(|_| {
        AtmError::validation("tool proof field exceeds the canonical framing limit")
    })?;
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(value.as_bytes());
    Ok(())
}

fn canonical_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn canonical_operation(operation: &ToolInstallerOperation) -> &'static str {
    match operation {
        ToolInstallerOperation::Register => "register",
        ToolInstallerOperation::Rotate => "rotate",
        ToolInstallerOperation::Unregister => "unregister",
    }
}

/// Signed operation accepted only after registry verification.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolInstallerOperation {
    Register,
    Rotate,
    Unregister,
}

/// Versioned installer envelope. `signature` is opaque base64 text; its
/// canonical signing bytes are defined by ADR-066 and exclude this field.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolInstallerRequest {
    pub schema_version: String,
    pub operation: ToolInstallerOperation,
    pub installer_id: InstallerId,
    pub installer_key_id: ToolKeyId,
    pub sender: PluginSenderId,
    pub package_digest: MessageDigest,
    pub registration_generation: u64,
    pub host_id: HostName,
    pub runtime_key_id: ToolKeyId,
    pub runtime_public_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_runtime_key_id: Option<ToolKeyId>,
    pub nonce: ToolNonce,
    pub issued_at: IsoTimestamp,
    pub expires_at: IsoTimestamp,
    pub signature: String,
}

impl ToolInstallerRequest {
    /// Returns the domain-separated, length-prefixed signing bytes.
    ///
    /// The opaque signature is deliberately excluded. Field order is fixed by
    /// this method and ADR-066 so installer verification cannot depend on a
    /// serializer's map ordering.
    pub fn canonical_signing_bytes(&self) -> Result<Vec<u8>, AtmError> {
        let mut bytes = Vec::new();
        for field in [
            INSTALL_DOMAIN,
            self.schema_version.as_str(),
            canonical_operation(&self.operation),
            self.installer_id.as_str(),
            self.installer_key_id.as_str(),
            self.sender.as_str(),
            self.package_digest.as_str(),
        ] {
            canonical_field(&mut bytes, field)?;
        }
        canonical_u64(&mut bytes, self.registration_generation);
        for field in [
            self.host_id.as_str(),
            self.runtime_key_id.as_str(),
            self.runtime_public_key.as_str(),
            self.previous_runtime_key_id
                .as_ref()
                .map_or("", ToolKeyId::as_str),
            self.nonce.as_str(),
            &self.issued_at.to_string(),
            &self.expires_at.to_string(),
        ] {
            canonical_field(&mut bytes, field)?;
        }
        Ok(bytes)
    }
}

/// Signed runtime provenance for one canonical message write.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolRuntimeProof {
    pub runtime_key_id: ToolKeyId,
    pub sender: PluginSenderId,
    pub registration_generation: u64,
    pub host_id: HostName,
    pub message_id: AtmMessageId,
    pub message_digest: MessageDigest,
    pub nonce: ToolNonce,
    pub signature: String,
}

impl ToolRuntimeProof {
    /// Returns domain-separated, length-prefixed bytes excluding `signature`.
    pub fn canonical_signing_bytes(&self) -> Result<Vec<u8>, AtmError> {
        let mut bytes = Vec::new();
        for field in [
            MESSAGE_DOMAIN,
            self.runtime_key_id.as_str(),
            self.sender.as_str(),
        ] {
            canonical_field(&mut bytes, field)?;
        }
        canonical_u64(&mut bytes, self.registration_generation);
        canonical_field(&mut bytes, self.host_id.as_str())?;
        canonical_field(&mut bytes, &self.message_id.to_string())?;
        canonical_field(&mut bytes, self.message_digest.as_str())?;
        canonical_field(&mut bytes, self.nonce.as_str())?;
        Ok(bytes)
    }
}

/// Origin-daemon assertion required when a virtual sender crosses a peer link.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSenderAttestation {
    pub sender: PluginSenderId,
    pub registration_generation: u64,
    pub host_id: HostName,
    pub message_id: AtmMessageId,
    pub message_digest: MessageDigest,
    pub nonce: ToolNonce,
    pub signature: String,
}

impl ToolSenderAttestation {
    /// Returns domain-separated, length-prefixed bytes excluding `signature`.
    pub fn canonical_signing_bytes(&self) -> Result<Vec<u8>, AtmError> {
        let mut bytes = Vec::new();
        for field in [PEER_DOMAIN, self.sender.as_str()] {
            canonical_field(&mut bytes, field)?;
        }
        canonical_u64(&mut bytes, self.registration_generation);
        canonical_field(&mut bytes, self.host_id.as_str())?;
        canonical_field(&mut bytes, &self.message_id.to_string())?;
        canonical_field(&mut bytes, self.message_digest.as_str())?;
        canonical_field(&mut bytes, self.nonce.as_str())?;
        Ok(bytes)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ToolInstallReceipt {
    pub sender: PluginSenderId,
    pub registration_generation: u64,
    pub applied_at: IsoTimestamp,
}

/// Sealed persistence/authentication boundary for virtual plug-in principals.
///
/// Implementations preserve the [`ToolSenderFailure`] distinctions in the
/// returned [`AtmError::code`]: installer admission may report unauthorized,
/// key, host, generation, signature, expiry, nonce, or uncertain-commit cases;
/// runtime admission may report sender, key, nonce, host, generation,
/// signature, or uncertain-commit cases. A commit with an unknown outcome is
/// never reported as an ordinary retryable storage failure.
pub trait ToolSenderRegistry: sealed::Sealed + Send + Sync {
    /// Verifies and applies one signed installer operation atomically.
    fn apply_signed(
        &self,
        request: ToolInstallerRequest,
        now: IsoTimestamp,
    ) -> Result<ToolInstallReceipt, AtmError>;

    /// Validates runtime provenance and durably binds its nonce to this message.
    fn admit_signed_message(
        &self,
        proof: ToolRuntimeProof,
        message_id: AtmMessageId,
        digest: MessageDigest,
        now: IsoTimestamp,
    ) -> Result<ToolRuntimeAdmission, AtmError>;

    /// Resolves current registry state for inspection only. It does not mint a
    /// daemon emission capability. An absent active registration returns
    /// [`ToolSenderFailure::SenderUnregistered`].
    fn resolve(&self, sender: &PluginSenderId) -> Result<RegisteredToolSender, AtmError>;
}

/// Opaque result of a durable origin-host attestation admission.
///
/// Its sender and message binding come from an adapter-owned committed issuer
/// record. It is deliberately non-serializable and cannot be reconstructed
/// from peer payload data.
#[derive(Debug)]
pub struct ToolPeerAdmission {
    sender: RegisteredToolSender,
    binding: VerifiedPeerBinding,
}

#[derive(Debug)]
struct VerifiedPeerBinding {
    authenticated_source_host: HostName,
    sender: PluginSenderId,
    registration_generation: u64,
    message_id: AtmMessageId,
    message_digest: MessageDigest,
    nonce: ToolNonce,
    signature: String,
}

impl ToolPeerAdmission {
    /// Projects an opaque admission only from an adapter-owned committed
    /// issuer record.
    pub fn from_issuer(issuer: &impl ToolPeerAdmissionIssuer) -> Result<Self, AtmError> {
        let attestation = issuer.attestation();
        let source_host = issuer.authenticated_source_host();
        let digest = issuer.message_digest();
        let message_id = issuer.message_id();
        if source_host != &attestation.host_id {
            return Err(ToolSenderFailure::HostMismatch
                .into_error("peer admission issuer host does not match the authenticated source"));
        }
        if attestation.sender != *issuer.sender_id()
            || attestation.registration_generation != issuer.registration_generation()
            || attestation.message_id != message_id
            || attestation.message_digest != *digest
        {
            return Err(ToolSenderFailure::SignatureInvalid
                .into_error("peer admission issuer does not match the verified message binding"));
        }
        let sender = RegisteredToolSender::from_issuer(issuer);
        Ok(Self {
            sender,
            binding: VerifiedPeerBinding {
                authenticated_source_host: source_host.clone(),
                sender: issuer.sender_id().clone(),
                registration_generation: issuer.registration_generation(),
                message_id,
                message_digest: digest.clone(),
                nonce: attestation.nonce.clone(),
                signature: attestation.signature.clone(),
            },
        })
    }

    #[must_use]
    pub fn sender(&self) -> &RegisteredToolSender {
        &self.sender
    }

    #[must_use]
    pub fn authenticated_source_host(&self) -> &HostName {
        &self.binding.authenticated_source_host
    }

    #[must_use]
    pub fn sender_id(&self) -> &PluginSenderId {
        &self.binding.sender
    }

    #[must_use]
    pub fn registration_generation(&self) -> u64 {
        self.binding.registration_generation
    }

    #[must_use]
    pub fn message_id(&self) -> AtmMessageId {
        self.binding.message_id
    }

    #[must_use]
    pub fn message_digest(&self) -> &MessageDigest {
        &self.binding.message_digest
    }

    #[must_use]
    pub fn nonce(&self) -> &ToolNonce {
        &self.binding.nonce
    }

    /// Confirms that the request carries the exact attestation admitted by the peer store.
    #[must_use]
    pub fn matches_attestation(&self, attestation: &ToolSenderAttestation) -> bool {
        self.binding.sender == attestation.sender
            && self.binding.registration_generation == attestation.registration_generation
            && self.binding.authenticated_source_host == attestation.host_id
            && self.binding.message_id == attestation.message_id
            && self.binding.message_digest == attestation.message_digest
            && self.binding.nonce == attestation.nonce
            && self.binding.signature == attestation.signature
    }
}

/// Private adapter record from which a committed peer admission is projected.
/// Implementations are restricted by ADR-001's workspace-convention seal.
pub trait ToolPeerAdmissionIssuer: RegisteredToolSenderIssuer {
    fn attestation(&self) -> &ToolSenderAttestation;
    fn authenticated_source_host(&self) -> &HostName;
    fn sender_id(&self) -> &PluginSenderId;
    fn registration_generation(&self) -> u64;
    fn message_id(&self) -> AtmMessageId;
    fn message_digest(&self) -> &MessageDigest;
}

/// Durable verification and replay boundary for origin-host attestations.
///
/// Implementations verify the enrolled origin key, sender-host policy,
/// transport-authenticated host, and exact message binding before committing
/// a replay record. Exact replay is idempotent; conflicting nonce reuse fails
/// before a new admission can be issued.
pub trait ToolSenderPeerAdmissionStore: sealed::Sealed + Send + Sync {
    fn admit_attestation(
        &self,
        attestation: ToolSenderAttestation,
        message_id: AtmMessageId,
        digest: MessageDigest,
        authenticated_source_host: &HostName,
        now: IsoTimestamp,
    ) -> Result<ToolPeerAdmission, AtmError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_message_digest_hashes_exact_utf8_body_bytes() {
        assert_eq!(
            MessageDigest::for_message_body("abc").as_str(),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_ne!(
            MessageDigest::for_message_body("abc"),
            MessageDigest::for_message_body("abc\n")
        );
    }

    #[test]
    fn identifiers_reject_blank_values() {
        assert!(PluginSenderId::parse(" ").is_err());
        assert_eq!(
            InstallerId::parse("installer").unwrap().as_str(),
            "installer"
        );
    }
}
