//! SQLite replay protection for origin-host tool sender attestations.
//!
//! The immutable policy is supplied by protected daemon composition. This
//! adapter never treats an authenticated TLS peer identity as a signature.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use atm_storage::contract::sealed;
use atm_storage::{
    AgentAddress, AtmError, AtmErrorCode, AtmMessageId, HostName, IsoTimestamp, MessageDigest,
    PluginSenderId, RegisteredToolSenderIssuer, ToolPeerAdmission, ToolPeerAdmissionIssuer,
    ToolPrincipal, ToolSenderAttestation, ToolSenderFailure, ToolSenderPeerAdmissionStore,
};
use base64::Engine as _;
use ring::signature::{ED25519, UnparsedPublicKey};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::shared_db::SharedDb;

#[derive(Clone, Debug)]
pub struct TrustedToolOriginKey {
    host_id: HostName,
    public_key: [u8; 32],
    allowed_senders: BTreeSet<PluginSenderId>,
}

impl TrustedToolOriginKey {
    pub fn new(
        host_id: HostName,
        public_key_base64: &str,
        allowed_senders: impl IntoIterator<Item = PluginSenderId>,
    ) -> Result<Self, AtmError> {
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(public_key_base64)
            .map_err(|error| config_error(format!("invalid trusted origin public key: {error}")))?;
        let public_key: [u8; 32] = decoded.try_into().map_err(|_| {
            config_error("trusted origin Ed25519 public key must contain exactly 32 bytes")
        })?;
        let allowed_senders: BTreeSet<_> = allowed_senders.into_iter().collect();
        if allowed_senders.is_empty() {
            return Err(config_error(
                "trusted origin policy must allow at least one tool sender",
            ));
        }
        Ok(Self {
            host_id,
            public_key,
            allowed_senders,
        })
    }
}

/// Immutable adapter policy keyed by the host identity authenticated by HTTP.
#[derive(Clone, Debug)]
pub struct ToolSenderPeerAdmissionConfig {
    origins: BTreeMap<HostName, TrustedToolOriginKey>,
}

impl ToolSenderPeerAdmissionConfig {
    pub fn new(
        trusted_origins: impl IntoIterator<Item = TrustedToolOriginKey>,
    ) -> Result<Self, AtmError> {
        let mut origins = BTreeMap::new();
        for origin in trusted_origins {
            let host = origin.host_id.clone();
            if origins.insert(host, origin).is_some() {
                return Err(config_error("duplicate trusted tool origin host"));
            }
        }
        if origins.is_empty() {
            return Err(config_error("trusted tool origin policy is empty"));
        }
        Ok(Self { origins })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
struct PeerBindingRecord {
    origin_host: String,
    sender: String,
    registration_generation: u64,
    message_id: String,
    message_digest: String,
    signature: String,
    admitted_at: String,
}

struct SqliteToolPeerAdmissionIssuer {
    principal: ToolPrincipal,
    sender: PluginSenderId,
    address: AgentAddress,
    attestation: ToolSenderAttestation,
    authenticated_source_host: HostName,
    message_id: AtmMessageId,
    digest: MessageDigest,
}

impl sealed::Sealed for SqliteToolPeerAdmissionIssuer {}

impl RegisteredToolSenderIssuer for SqliteToolPeerAdmissionIssuer {
    fn principal(&self) -> &ToolPrincipal {
        &self.principal
    }

    fn address(&self) -> &AgentAddress {
        &self.address
    }
}

impl ToolPeerAdmissionIssuer for SqliteToolPeerAdmissionIssuer {
    fn attestation(&self) -> &ToolSenderAttestation {
        &self.attestation
    }

    fn authenticated_source_host(&self) -> &HostName {
        &self.authenticated_source_host
    }

    fn sender_id(&self) -> &PluginSenderId {
        &self.sender
    }

    fn registration_generation(&self) -> u64 {
        self.attestation.registration_generation
    }

    fn message_id(&self) -> AtmMessageId {
        self.message_id
    }

    fn message_digest(&self) -> &MessageDigest {
        &self.digest
    }
}

struct SqliteToolSenderPeerAdmissionStore {
    db: Arc<SharedDb>,
    config: Option<ToolSenderPeerAdmissionConfig>,
}

pub(crate) fn tool_sender_peer_admission_store(
    db: Arc<SharedDb>,
    config: Option<ToolSenderPeerAdmissionConfig>,
) -> Arc<dyn ToolSenderPeerAdmissionStore + Send + Sync> {
    Arc::new(SqliteToolSenderPeerAdmissionStore { db, config })
}

impl sealed::Sealed for SqliteToolSenderPeerAdmissionStore {}

impl ToolSenderPeerAdmissionStore for SqliteToolSenderPeerAdmissionStore {
    fn admit_attestation(
        &self,
        attestation: ToolSenderAttestation,
        message_id: AtmMessageId,
        digest: MessageDigest,
        authenticated_source_host: &HostName,
        now: IsoTimestamp,
    ) -> Result<ToolPeerAdmission, AtmError> {
        let config = self.config.as_ref().ok_or_else(missing_config)?;
        if attestation.host_id != *authenticated_source_host {
            return Err(ToolSenderFailure::HostMismatch
                .into_error("tool attestation host does not match authenticated peer"));
        }
        if attestation.message_id != message_id || attestation.message_digest != digest {
            return Err(ToolSenderFailure::SignatureInvalid
                .into_error("tool attestation does not bind this canonical message"));
        }
        let origin = config
            .origins
            .get(authenticated_source_host)
            .ok_or_else(|| {
                ToolSenderFailure::SenderUnauthorized
                    .into_error("authenticated origin host is not enrolled for tool senders")
            })?;
        if !origin.allowed_senders.contains(&attestation.sender) {
            return Err(ToolSenderFailure::SenderUnauthorized
                .into_error("origin host is not authorized for this tool sender"));
        }
        verify_signature(
            &origin.public_key,
            &attestation.canonical_signing_bytes()?,
            &attestation.signature,
        )?;

        let sender_id = attestation.sender.clone();
        let source_host = authenticated_source_host.clone();
        let replay_key = (
            source_host.to_string(),
            sender_id.to_string(),
            attestation.nonce.to_string(),
        );
        let binding = PeerBindingRecord {
            origin_host: replay_key.0.clone(),
            sender: replay_key.1.clone(),
            registration_generation: attestation.registration_generation,
            message_id: message_id.to_string(),
            message_digest: digest.to_string(),
            signature: attestation.signature.clone(),
            admitted_at: now.to_string(),
        };
        let committed = self.db.with_transaction(|transaction| {
            let prior: Option<PeerBindingRecord> = transaction
                .query_row(
                    "SELECT origin_host, sender, registration_generation, message_id, message_digest, signature, admitted_at FROM tool_sender_peer_binding WHERE origin_host=?1 AND sender=?2 AND nonce=?3",
                    params![replay_key.0, replay_key.1, replay_key.2],
                    |row| Ok(PeerBindingRecord {
                        origin_host: row.get(0)?,
                        sender: row.get(1)?,
                        registration_generation: row.get(2)?,
                        message_id: row.get(3)?,
                        message_digest: row.get(4)?,
                        signature: row.get(5)?,
                        admitted_at: row.get(6)?,
                    }),
                )
                .optional()
                .map_err(|error| db_error("failed to inspect peer tool replay", error))?;
            if let Some(prior) = prior {
                if prior.origin_host != binding.origin_host
                    || prior.sender != binding.sender
                    || prior.registration_generation != binding.registration_generation
                    || prior.message_id != binding.message_id
                    || prior.message_digest != binding.message_digest
                    || prior.signature != binding.signature
                {
                    return Err(ToolSenderFailure::NonceReplay
                        .into_error("peer tool nonce was already bound to different message provenance"));
                }
                return Ok(prior);
            }
            transaction
                .execute(
                    "INSERT INTO tool_sender_peer_binding (origin_host, sender, nonce, registration_generation, message_id, message_digest, signature, admitted_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        binding.origin_host,
                        binding.sender,
                        replay_key.2,
                        binding.registration_generation,
                        binding.message_id,
                        binding.message_digest,
                        binding.signature,
                        binding.admitted_at,
                    ],
                )
                .map_err(|error| db_error("failed to persist peer tool replay binding", error))?;
            Ok(binding.clone())
        })?;

        // Capabilities are projected only after the replay transaction commits.
        if committed.message_id != message_id.to_string()
            || committed.message_digest != digest.as_str()
            || committed.origin_host != source_host.as_str()
            || committed.sender != sender_id.as_str()
        {
            return Err(ToolSenderFailure::NonceReplay
                .into_error("durable peer tool binding differs from the requested message"));
        }
        let address: AgentAddress =
            format!("{}@tool", sender_id.as_str())
                .parse()
                .map_err(|error| {
                    ToolSenderFailure::SenderUnauthorized.into_error(format!(
                        "registered tool sender address is invalid: {error}"
                    ))
                })?;
        ToolPeerAdmission::from_issuer(&SqliteToolPeerAdmissionIssuer {
            principal: ToolPrincipal::RegisteredPlugin {
                sender: sender_id.clone(),
            },
            sender: sender_id.clone(),
            address,
            attestation,
            authenticated_source_host: source_host,
            message_id,
            digest,
        })
    }
}

fn verify_signature(
    public_key: &[u8; 32],
    canonical: &[u8],
    encoded: &str,
) -> Result<(), AtmError> {
    let signature = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| {
            ToolSenderFailure::SignatureInvalid.into_error(format!(
                "origin tool signature is not strict standard base64: {error}"
            ))
        })?;
    if base64::engine::general_purpose::STANDARD.encode(&signature) != encoded
        || signature.len() != 64
    {
        return Err(ToolSenderFailure::SignatureInvalid
            .into_error("origin tool signature must be canonical padded base64 for 64 bytes"));
    }
    UnparsedPublicKey::new(&ED25519, public_key)
        .verify(canonical, &signature)
        .map_err(|_| {
            ToolSenderFailure::SignatureInvalid
                .into_error("origin tool attestation signature is invalid")
        })
}

fn missing_config() -> AtmError {
    AtmError::new(
        AtmErrorCode::ToolSenderUnauthorized,
        "peer tool admission is unavailable because no origin-host policy is configured",
    )
}

fn config_error(message: impl Into<String>) -> AtmError {
    AtmError::validation(format!(
        "invalid peer tool admission config: {}",
        message.into()
    ))
}

fn db_error(operation: &str, error: rusqlite::Error) -> AtmError {
    AtmError::new(AtmErrorCode::MailboxWriteFailed, operation).with_cause(error.to_string())
}

#[cfg(test)]
mod tests {
    // Adapter behavior is covered through the public SQLite backend integration
    // tests so replay durability is exercised across a reopened database.
}
