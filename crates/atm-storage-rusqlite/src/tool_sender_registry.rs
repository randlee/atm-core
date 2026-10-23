//! SQLite persistence and authentication for registered virtual tool senders.
//!
//! Installer policy is supplied by the local composition root. Neither a
//! request nor ambient process/repository state can add a trusted key or sender.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use atm_storage::contract::sealed;
use atm_storage::{
    AgentAddress, AtmError, AtmErrorCode, AtmMessageId, HostName, InstallerId, IsoTimestamp,
    MessageDigest, PluginSenderId, RegisteredToolSender, RegisteredToolSenderIssuer,
    ToolInstallReceipt, ToolInstallerOperation, ToolInstallerRequest, ToolKeyId, ToolPrincipal,
    ToolRuntimeAdmission, ToolRuntimeAdmissionIssuer, ToolRuntimeProof, ToolSenderFailure,
    ToolSenderRegistry,
};
use base64::Engine as _;
use ring::signature::{ED25519, UnparsedPublicKey};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::shared_db::SharedDb;

/// One installer public key and the sender names it may operate.
///
/// The public key is strict standard-base64 Ed25519 material. Fields remain
/// private so malformed or partially trusted policies cannot be constructed.
#[derive(Clone, Debug)]
pub struct TrustedToolInstallerKey {
    installer_id: InstallerId,
    key_id: ToolKeyId,
    public_key: [u8; 32],
    allowed_senders: BTreeSet<PluginSenderId>,
}

impl TrustedToolInstallerKey {
    pub fn new(
        installer_id: InstallerId,
        key_id: ToolKeyId,
        public_key_base64: &str,
        allowed_senders: impl IntoIterator<Item = PluginSenderId>,
    ) -> Result<Self, AtmError> {
        let public_key = decode_fixed::<32>(public_key_base64, "installer public key")?;
        let allowed_senders: BTreeSet<_> = allowed_senders.into_iter().collect();
        if allowed_senders.is_empty() || allowed_senders.iter().any(is_reserved_sender) {
            return Err(config_error(
                "trusted installer policy must allow at least one non-reserved sender",
            ));
        }
        Ok(Self {
            installer_id,
            key_id,
            public_key,
            allowed_senders,
        })
    }
}

/// Private, local trust-root input for the SQLite registry.
///
/// A trusted installer key is bound to an explicit sender allowlist. The host
/// identity and this policy are never read from a signed request or environment.
#[derive(Clone, Debug)]
pub struct ToolSenderRegistryConfig {
    host_id: HostName,
    trusted_keys: BTreeMap<(InstallerId, ToolKeyId), TrustedToolInstallerKey>,
    sender_owners: BTreeMap<PluginSenderId, InstallerId>,
}

impl ToolSenderRegistryConfig {
    pub fn new(
        host_id: HostName,
        trusted_keys: impl IntoIterator<Item = TrustedToolInstallerKey>,
    ) -> Result<Self, AtmError> {
        let mut keys = BTreeMap::new();
        let mut owners = BTreeMap::new();
        for key in trusted_keys {
            for sender in &key.allowed_senders {
                if owners
                    .insert(sender.clone(), key.installer_id.clone())
                    .is_some_and(|owner| owner != key.installer_id)
                {
                    return Err(config_error(format!(
                        "sender {sender} is assigned to more than one trusted installer"
                    )));
                }
            }
            let identity = (key.installer_id.clone(), key.key_id.clone());
            if keys.insert(identity, key).is_some() {
                return Err(config_error("duplicate trusted installer key id"));
            }
        }
        if keys.is_empty() {
            return Err(config_error("trusted installer key policy is empty"));
        }
        Ok(Self {
            host_id,
            trusted_keys: keys,
            sender_owners: owners,
        })
    }

    fn trusted_key(
        &self,
        installer: &InstallerId,
        key_id: &ToolKeyId,
        sender: &PluginSenderId,
    ) -> Result<&TrustedToolInstallerKey, AtmError> {
        let Some(key) = self.trusted_keys.get(&(installer.clone(), key_id.clone())) else {
            return Err(ToolSenderFailure::KeyUnknown
                .into_error("installer key is not present in the local trust root"));
        };
        if !key.allowed_senders.contains(sender)
            || self.sender_owners.get(sender) != Some(installer)
        {
            return Err(ToolSenderFailure::SenderUnauthorized
                .into_error("installer key is not authorized for this sender"));
        }
        Ok(key)
    }
}

struct SqliteToolSenderRegistry {
    db: Arc<SharedDb>,
    config: Option<ToolSenderRegistryConfig>,
}

struct SqliteRegisteredToolSenderIssuer {
    principal: ToolPrincipal,
    address: AgentAddress,
}

impl sealed::Sealed for SqliteRegisteredToolSenderIssuer {}

impl RegisteredToolSenderIssuer for SqliteRegisteredToolSenderIssuer {
    fn principal(&self) -> &ToolPrincipal {
        &self.principal
    }

    fn address(&self) -> &AgentAddress {
        &self.address
    }
}

struct SqliteToolRuntimeAdmissionIssuer {
    sender: RegisteredToolSender,
    proof: ToolRuntimeProof,
    message_id: AtmMessageId,
    digest: MessageDigest,
}

impl sealed::Sealed for SqliteToolRuntimeAdmissionIssuer {}

impl ToolRuntimeAdmissionIssuer for SqliteToolRuntimeAdmissionIssuer {
    fn sender(&self) -> &RegisteredToolSender {
        &self.sender
    }

    fn proof(&self) -> &ToolRuntimeProof {
        &self.proof
    }

    fn message_id(&self) -> AtmMessageId {
        self.message_id
    }

    fn message_digest(&self) -> &MessageDigest {
        &self.digest
    }
}

pub(crate) fn tool_sender_registry(
    db: Arc<SharedDb>,
    config: Option<ToolSenderRegistryConfig>,
) -> Arc<dyn ToolSenderRegistry + Send + Sync> {
    Arc::new(SqliteToolSenderRegistry { db, config })
}

impl sealed::Sealed for SqliteToolSenderRegistry {}

impl ToolSenderRegistry for SqliteToolSenderRegistry {
    fn apply_signed(
        &self,
        request: ToolInstallerRequest,
        now: IsoTimestamp,
    ) -> Result<ToolInstallReceipt, AtmError> {
        let config = self.config.as_ref().ok_or_else(missing_config)?;
        validate_installer_request(config, &request, now)?;
        let operation_digest = digest_framed(
            &request.canonical_signing_bytes()?,
            request.signature.as_bytes(),
        );
        let now_text = now.to_string();
        self.transaction(|transaction| {
            if let Some(receipt) = replay_receipt::<ToolInstallReceipt>(
                transaction,
                request.nonce.as_str(),
                &operation_digest,
                "tool_sender_install_receipt",
            )? {
                return Ok(receipt);
            }

            let current = load_registration(transaction, request.sender.as_str())?;
            let (receipt, mutation) = installer_transition(current.as_ref(), &request, now)?;
            if let Some(mutation) = mutation {
                persist_installer_transition(transaction, &request, &now_text, mutation)?;
            }
            persist_nonce(
                transaction,
                request.nonce.as_str(),
                request.sender.as_str(),
                &operation_digest,
                &serde_json::to_string(&receipt).map_err(serialization_error)?,
                &now_text,
                "tool_sender_install_receipt",
            )?;
            Ok(receipt)
        })
    }

    fn admit_signed_message(
        &self,
        proof: ToolRuntimeProof,
        message_id: AtmMessageId,
        digest: MessageDigest,
        now: IsoTimestamp,
    ) -> Result<ToolRuntimeAdmission, AtmError> {
        let config = self.config.as_ref().ok_or_else(missing_config)?;
        if proof.host_id != config.host_id {
            return Err(ToolSenderFailure::HostMismatch
                .into_error("runtime proof is for a different local host"));
        }
        if proof.message_id != message_id || proof.message_digest != digest {
            return Err(ToolSenderFailure::SignatureInvalid
                .into_error("runtime proof does not bind the requested message"));
        }
        let operation_digest = digest_framed(
            &proof.canonical_signing_bytes()?,
            proof.signature.as_bytes(),
        );
        let proof_copy = proof.clone();
        let sender_id = proof.sender.clone();
        let issuer_record = self.transaction(|transaction| {
            if let Some(record) = replay_receipt::<RuntimeBindingRecord>(
                transaction,
                proof.nonce.as_str(),
                &operation_digest,
                "tool_sender_runtime_binding",
            )? {
                return Ok(record);
            }
            let registration = load_registration(transaction, sender_id.as_str())?
                .filter(|registration| registration.active)
                .ok_or_else(|| {
                    ToolSenderFailure::SenderUnregistered
                        .into_error(format!("tool sender {sender_id} is not registered"))
                })?;
            if registration.host_id != proof.host_id.as_str() {
                return Err(ToolSenderFailure::HostMismatch
                    .into_error("runtime proof host does not match the registration"));
            }
            if registration.generation != proof.registration_generation {
                return Err(ToolSenderFailure::GenerationConflict
                    .into_error("runtime proof uses a stale registration generation"));
            }
            if registration.runtime_key_id != proof.runtime_key_id.as_str() {
                return Err(ToolSenderFailure::KeyRevoked
                    .into_error("runtime proof uses a key that is no longer active"));
            }
            let public_key = decode_fixed::<32>(
                &registration.runtime_public_key,
                "registered runtime public key",
            )?;
            verify_signature(
                &public_key,
                &proof.canonical_signing_bytes()?,
                &proof.signature,
            )?;
            let record = RuntimeBindingRecord {
                sender: sender_id.to_string(),
                runtime_key_id: proof.runtime_key_id.to_string(),
                generation: proof.registration_generation,
                host_id: proof.host_id.to_string(),
                message_id: message_id.to_string(),
                digest: digest.to_string(),
                admitted_at: now.to_string(),
            };
            let json = serde_json::to_string(&record).map_err(serialization_error)?;
            persist_nonce(
                transaction,
                proof.nonce.as_str(),
                sender_id.as_str(),
                &operation_digest,
                &json,
                &now.to_string(),
                "tool_sender_runtime_binding",
            )?;
            Ok(record)
        })?;

        // The durable row is the authority on exact replay. Verify that the
        // decoded request still matches that row before minting the opaque port value.
        if issuer_record.sender != proof_copy.sender.as_str()
            || issuer_record.runtime_key_id != proof_copy.runtime_key_id.as_str()
            || issuer_record.generation != proof_copy.registration_generation
            || issuer_record.host_id != proof_copy.host_id.as_str()
            || issuer_record.message_id != message_id.to_string()
            || issuer_record.digest != digest.as_str()
        {
            return Err(ToolSenderFailure::NonceReplay
                .into_error("runtime nonce was previously bound to different message provenance"));
        }
        let sender = registered_sender(sender_id)?;
        ToolRuntimeAdmission::from_issuer(&SqliteToolRuntimeAdmissionIssuer {
            sender,
            proof: proof_copy,
            message_id,
            digest,
        })
    }

    fn resolve(&self, sender: &PluginSenderId) -> Result<RegisteredToolSender, AtmError> {
        let sender_id = sender.clone();
        self.db.with_connection(|connection| {
            let active: Option<i64> = connection
                .query_row(
                    "SELECT active FROM tool_sender_registry WHERE sender = ?1",
                    params![sender_id.as_str()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| db_error("failed to read tool sender registration", error))?;
            if active != Some(1) {
                return Err(ToolSenderFailure::SenderUnregistered
                    .into_error(format!("tool sender {sender_id} is not registered")));
            }
            registered_sender(sender_id)
        })
    }
}

#[derive(Clone, Debug)]
struct Registration {
    installer_id: String,
    installer_key_id: String,
    host_id: String,
    runtime_key_id: String,
    runtime_public_key: String,
    generation: u64,
    active: bool,
    installed_at: String,
}

#[derive(Clone, Debug)]
enum Mutation {
    Register,
    Rotate,
    Unregister,
}

#[derive(Serialize, Deserialize)]
struct RuntimeBindingRecord {
    sender: String,
    runtime_key_id: String,
    generation: u64,
    host_id: String,
    message_id: String,
    digest: String,
    admitted_at: String,
}

fn installer_transition(
    current: Option<&Registration>,
    request: &ToolInstallerRequest,
    now: IsoTimestamp,
) -> Result<(ToolInstallReceipt, Option<Mutation>), AtmError> {
    let generation = request.registration_generation;
    let same_registration = current.is_some_and(|current| {
        current.active
            && current.installer_id == request.installer_id.as_str()
            && current.installer_key_id == request.installer_key_id.as_str()
            && current.host_id == request.host_id.as_str()
            && current.runtime_key_id == request.runtime_key_id.as_str()
            && current.runtime_public_key == request.runtime_public_key
            && current.generation == generation
    });
    if request.operation == ToolInstallerOperation::Register && same_registration {
        let current = current.expect("same_registration requires a row");
        return Ok((
            receipt(request, current.generation, &current.installed_at)?,
            None,
        ));
    }

    let mutation = match (&request.operation, current) {
        (ToolInstallerOperation::Register, None) if generation == 1 => Mutation::Register,
        (ToolInstallerOperation::Register, Some(row))
            if !row.active
                && row.installer_id == request.installer_id.as_str()
                && generation == row.generation + 1
                && request
                    .previous_runtime_key_id
                    .as_ref()
                    .is_none_or(|previous| previous.as_str() == row.runtime_key_id) =>
        {
            Mutation::Register
        }
        (ToolInstallerOperation::Rotate, Some(row))
            if row.active
                && row.installer_id == request.installer_id.as_str()
                && request
                    .previous_runtime_key_id
                    .as_ref()
                    .is_some_and(|previous| previous.as_str() == row.runtime_key_id)
                && generation == row.generation + 1
                && request.runtime_key_id.as_str() != row.runtime_key_id =>
        {
            Mutation::Rotate
        }
        (ToolInstallerOperation::Unregister, Some(row))
            if row.active
                && row.installer_id == request.installer_id.as_str()
                && request
                    .previous_runtime_key_id
                    .as_ref()
                    .is_some_and(|previous| previous.as_str() == row.runtime_key_id)
                && request.runtime_key_id.as_str() == row.runtime_key_id
                && request.runtime_public_key == row.runtime_public_key
                && generation == row.generation + 1 =>
        {
            Mutation::Unregister
        }
        _ => {
            if current.is_some_and(|row| row.installer_id != request.installer_id.as_str()) {
                return Err(ToolSenderFailure::SenderUnauthorized
                    .into_error("only the authenticated installer owner may change this sender"));
            }
            return Err(ToolSenderFailure::GenerationConflict
                .into_error("installer operation does not match current registration state"));
        }
    };
    Ok((
        receipt(request, generation, &now.to_string())?,
        Some(mutation),
    ))
}

fn receipt(
    request: &ToolInstallerRequest,
    generation: u64,
    applied_at: &str,
) -> Result<ToolInstallReceipt, AtmError> {
    Ok(ToolInstallReceipt {
        sender: request.sender.clone(),
        registration_generation: generation,
        applied_at: applied_at.parse().map_err(|error| {
            config_error(format!("invalid stored registration timestamp: {error}"))
        })?,
    })
}

fn persist_installer_transition(
    transaction: &Transaction<'_>,
    request: &ToolInstallerRequest,
    now: &str,
    mutation: Mutation,
) -> Result<(), AtmError> {
    match mutation {
        Mutation::Register => {
            transaction.execute(
                "INSERT INTO tool_sender_registry (sender, installer_id, installer_key_id, host_id, runtime_key_id, runtime_public_key, registration_generation, active, installed_at, revoked_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8, NULL) ON CONFLICT(sender) DO UPDATE SET installer_key_id=excluded.installer_key_id, host_id=excluded.host_id, runtime_key_id=excluded.runtime_key_id, runtime_public_key=excluded.runtime_public_key, registration_generation=excluded.registration_generation, active=1, installed_at=excluded.installed_at, revoked_at=NULL WHERE tool_sender_registry.installer_id=excluded.installer_id AND tool_sender_registry.active=0",
                params![request.sender.as_str(), request.installer_id.as_str(), request.installer_key_id.as_str(), request.host_id.as_str(), request.runtime_key_id.as_str(), request.runtime_public_key, request.registration_generation, now],
            ).map_err(|error| db_error("failed to persist tool sender registration", error))?;
        }
        Mutation::Rotate => {
            transaction.execute(
                "UPDATE tool_sender_registry SET installer_key_id=?2, host_id=?3, runtime_key_id=?4, runtime_public_key=?5, registration_generation=?6, installed_at=?7 WHERE sender=?1 AND active=1",
                params![request.sender.as_str(), request.installer_key_id.as_str(), request.host_id.as_str(), request.runtime_key_id.as_str(), request.runtime_public_key, request.registration_generation, now],
            ).map_err(|error| db_error("failed to rotate tool sender key", error))?;
        }
        Mutation::Unregister => {
            transaction.execute(
                "UPDATE tool_sender_registry SET installer_key_id=?2, registration_generation=?3, active=0, revoked_at=?4 WHERE sender=?1 AND active=1",
                params![request.sender.as_str(), request.installer_key_id.as_str(), request.registration_generation, now],
            ).map_err(|error| db_error("failed to uninstall tool sender", error))?;
        }
    }
    Ok(())
}

fn load_registration(
    transaction: &Transaction<'_>,
    sender: &str,
) -> Result<Option<Registration>, AtmError> {
    transaction
        .query_row(
            "SELECT installer_id, installer_key_id, host_id, runtime_key_id, runtime_public_key, registration_generation, active, installed_at FROM tool_sender_registry WHERE sender=?1",
            params![sender],
            |row| Ok(Registration {
                installer_id: row.get(0)?, installer_key_id: row.get(1)?, host_id: row.get(2)?,
                runtime_key_id: row.get(3)?, runtime_public_key: row.get(4)?, generation: row.get(5)?,
                active: row.get::<_, i64>(6)? == 1, installed_at: row.get(7)?,
            }),
        )
        .optional()
        .map_err(|error| db_error("failed to read tool sender registration", error))
}

fn persist_nonce(
    transaction: &Transaction<'_>,
    nonce: &str,
    sender: &str,
    operation_digest: &str,
    result_json: &str,
    now: &str,
    result_table: &str,
) -> Result<(), AtmError> {
    let result_digest = digest_text(result_json);
    transaction
        .execute(
            "INSERT INTO tool_sender_nonce_ledger (nonce, sender, operation_digest, result_digest, consumed_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![nonce, sender, operation_digest, result_digest, now],
        )
        .map_err(|error| db_error("failed to persist tool sender nonce", error))?;
    let sql = format!("INSERT INTO {result_table} (nonce, result_json) VALUES (?1, ?2)");
    transaction
        .execute(&sql, params![nonce, result_json])
        .map_err(|error| db_error("failed to persist tool sender operation result", error))?;
    Ok(())
}

fn replay_receipt<T: for<'de> Deserialize<'de>>(
    transaction: &Transaction<'_>,
    nonce: &str,
    operation_digest: &str,
    result_table: &str,
) -> Result<Option<T>, AtmError> {
    let prior: Option<(String, Option<String>)> = transaction
        .query_row(
            "SELECT operation_digest, result_digest FROM tool_sender_nonce_ledger WHERE nonce=?1",
            params![nonce],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| db_error("failed to inspect tool sender nonce", error))?;
    let Some((prior_digest, result_digest)) = prior else {
        return Ok(None);
    };
    if prior_digest != operation_digest {
        return Err(ToolSenderFailure::NonceReplay
            .into_error("tool sender nonce was already used for different signed bytes"));
    }
    let sql = format!("SELECT result_json FROM {result_table} WHERE nonce=?1");
    let result_json: Option<String> = transaction
        .query_row(&sql, params![nonce], |row| row.get(0))
        .optional()
        .map_err(|error| db_error("failed to read tool sender replay receipt", error))?;
    let result_json = result_json.ok_or_else(|| {
        ToolSenderFailure::CommitUncertain
            .into_error("nonce was committed without its durable operation result")
    })?;
    if result_digest.as_deref() != Some(digest_text(&result_json).as_str()) {
        return Err(ToolSenderFailure::CommitUncertain
            .into_error("tool sender replay receipt digest does not match its nonce ledger"));
    }
    serde_json::from_str(&result_json)
        .map(Some)
        .map_err(serialization_error)
}

fn validate_installer_request(
    config: &ToolSenderRegistryConfig,
    request: &ToolInstallerRequest,
    now: IsoTimestamp,
) -> Result<(), AtmError> {
    if is_reserved_sender(&request.sender) {
        return Err(ToolSenderFailure::SenderReserved
            .into_error("reserved daemon sender cannot be registered"));
    }
    let key = config.trusted_key(
        &request.installer_id,
        &request.installer_key_id,
        &request.sender,
    )?;
    if request.host_id != config.host_id {
        return Err(ToolSenderFailure::HostMismatch
            .into_error("installer request is for a different local host"));
    }
    if request.schema_version != "1" {
        return Err(ToolSenderFailure::SignatureInvalid
            .into_error("unsupported installer request schema version"));
    }
    if request.issued_at >= request.expires_at
        || now < request.issued_at
        || now >= request.expires_at
    {
        return Err(ToolSenderFailure::RequestExpired
            .into_error("installer request is outside its signed validity interval"));
    }
    let runtime_key = decode_fixed::<32>(&request.runtime_public_key, "runtime public key")?;
    if matches!(request.operation, ToolInstallerOperation::Rotate)
        && request.previous_runtime_key_id.is_none()
    {
        return Err(ToolSenderFailure::GenerationConflict
            .into_error("rotation must identify the prior runtime key"));
    }
    let _ = runtime_key;
    verify_signature(
        &key.public_key,
        &request.canonical_signing_bytes()?,
        &request.signature,
    )
}

fn verify_signature(
    public_key: &[u8; 32],
    message: &[u8],
    signature: &str,
) -> Result<(), AtmError> {
    let signature = decode_fixed::<64>(signature, "Ed25519 signature")?;
    UnparsedPublicKey::new(&ED25519, public_key)
        .verify(message, &signature)
        .map_err(|_| ToolSenderFailure::SignatureInvalid.into_error("Ed25519 signature is invalid"))
}

fn decode_fixed<const N: usize>(value: &str, subject: &str) -> Result<[u8; N], AtmError> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|_| {
            ToolSenderFailure::SignatureInvalid
                .into_error(format!("{subject} is not strict base64"))
        })?;
    if base64::engine::general_purpose::STANDARD.encode(&decoded) != value {
        return Err(ToolSenderFailure::SignatureInvalid
            .into_error(format!("{subject} is not canonical padded base64")));
    }
    decoded.try_into().map_err(|_| {
        ToolSenderFailure::SignatureInvalid
            .into_error(format!("{subject} must decode to exactly {N} bytes"))
    })
}

fn registered_sender(sender: PluginSenderId) -> Result<RegisteredToolSender, AtmError> {
    let address = format!("{}@tool", sender.as_str())
        .parse::<AgentAddress>()
        .map_err(|error| AtmError::new(AtmErrorCode::InternalError, error.to_string()))?;
    Ok(RegisteredToolSender::from_issuer(
        &SqliteRegisteredToolSenderIssuer {
            principal: ToolPrincipal::RegisteredPlugin { sender },
            address,
        },
    ))
}

fn is_reserved_sender(sender: &PluginSenderId) -> bool {
    sender.as_str() == "cron"
}

fn digest_framed(canonical: &[u8], signature: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(canonical);
    digest.update((signature.len() as u64).to_be_bytes());
    digest.update(signature);
    format!("sha256:{}", encode_hex(&digest.finalize()))
}

fn digest_text(value: &str) -> String {
    format!("sha256:{}", encode_hex(&Sha256::digest(value.as_bytes())))
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn missing_config() -> AtmError {
    config_error("signed tool sender operations require local trusted registry configuration")
}

fn config_error(message: impl Into<String>) -> AtmError {
    AtmError::new(AtmErrorCode::ConfigParseFailed, message)
}

fn db_error(message: &str, error: rusqlite::Error) -> AtmError {
    AtmError::new(
        AtmErrorCode::StoreQueryFailed,
        format!("{message}: {error}"),
    )
}

fn serialization_error(error: serde_json::Error) -> AtmError {
    AtmError::new(AtmErrorCode::SerializationFailed, error.to_string())
}

impl SqliteToolSenderRegistry {
    fn transaction<T>(
        &self,
        operation: impl FnOnce(&Transaction<'_>) -> Result<T, AtmError>,
    ) -> Result<T, AtmError> {
        self.db.with_connection(|connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|error| db_error("failed to begin tool sender transaction", error))?;
            let result = operation(&transaction)?;
            transaction.commit().map_err(|error| {
                ToolSenderFailure::CommitUncertain.into_error(format!(
                    "tool sender transaction commit outcome is uncertain: {error}"
                ))
            })?;
            Ok(result)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use atm_storage::{
        AtmErrorCode, AtmMessageId, HostName, InstallerId, IsoTimestamp, MessageDigest,
        PluginSenderId, ToolInstallerOperation, ToolInstallerRequest, ToolKeyId, ToolNonce,
        ToolRuntimeProof,
    };
    use base64::Engine as _;
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use tempfile::tempdir;

    use super::{ToolSenderRegistryConfig, TrustedToolInstallerKey, tool_sender_registry};
    use crate::SqliteStorageBackend;

    fn signing_key(seed: u8) -> Ed25519KeyPair {
        Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).expect("fixed Ed25519 test seed")
    }

    fn encoded_public_key(key: &Ed25519KeyPair) -> String {
        base64::engine::general_purpose::STANDARD.encode(key.public_key().as_ref())
    }

    fn config(installer_key: &Ed25519KeyPair) -> ToolSenderRegistryConfig {
        ToolSenderRegistryConfig::new(
            HostName::from_str("host.example").expect("host"),
            [TrustedToolInstallerKey::new(
                InstallerId::parse("installer").expect("installer id"),
                ToolKeyId::parse("installer-key").expect("installer key id"),
                &encoded_public_key(installer_key),
                [PluginSenderId::parse("nightly").expect("sender")],
            )
            .expect("trusted installer")],
        )
        .expect("registry config")
    }

    fn timestamp(value: &str) -> IsoTimestamp {
        IsoTimestamp::from_str(value).expect("timestamp")
    }

    fn installer_request(
        key: &Ed25519KeyPair,
        runtime_key: &Ed25519KeyPair,
        operation: ToolInstallerOperation,
        generation: u64,
        runtime_key_id: &str,
        previous_runtime_key_id: Option<&str>,
        nonce: &str,
    ) -> ToolInstallerRequest {
        let mut request = ToolInstallerRequest {
            schema_version: "1".to_owned(),
            operation,
            installer_id: InstallerId::parse("installer").expect("installer"),
            installer_key_id: ToolKeyId::parse("installer-key").expect("installer key"),
            sender: PluginSenderId::parse("nightly").expect("sender"),
            package_digest: MessageDigest::parse("sha256:package").expect("package digest"),
            registration_generation: generation,
            host_id: HostName::from_str("host.example").expect("host"),
            runtime_key_id: ToolKeyId::parse(runtime_key_id).expect("runtime key id"),
            runtime_public_key: encoded_public_key(runtime_key),
            previous_runtime_key_id: previous_runtime_key_id
                .map(|key_id| ToolKeyId::parse(key_id).expect("previous runtime key id")),
            nonce: ToolNonce::parse(nonce).expect("nonce"),
            issued_at: timestamp("2026-10-23T11:00:00Z"),
            expires_at: timestamp("2026-10-23T13:00:00Z"),
            signature: String::new(),
        };
        request.signature = base64::engine::general_purpose::STANDARD.encode(
            key.sign(&request.canonical_signing_bytes().expect("canonical bytes"))
                .as_ref(),
        );
        request
    }

    fn runtime_proof(
        key: &Ed25519KeyPair,
        runtime_key_id: &str,
        generation: u64,
        nonce: &str,
        message_id: AtmMessageId,
        digest: &str,
    ) -> ToolRuntimeProof {
        let mut proof = ToolRuntimeProof {
            runtime_key_id: ToolKeyId::parse(runtime_key_id).expect("runtime key id"),
            sender: PluginSenderId::parse("nightly").expect("sender"),
            registration_generation: generation,
            host_id: HostName::from_str("host.example").expect("host"),
            message_id,
            message_digest: MessageDigest::parse(digest).expect("digest"),
            nonce: ToolNonce::parse(nonce).expect("nonce"),
            signature: String::new(),
        };
        proof.signature = base64::engine::general_purpose::STANDARD.encode(
            key.sign(&proof.canonical_signing_bytes().expect("canonical bytes"))
                .as_ref(),
        );
        proof
    }

    #[test]
    fn signed_install_rotation_admission_and_replay_survive_reopen() {
        let root = tempdir().expect("tempdir");
        let path = root.path().join("mail.db");
        let installer_key = signing_key(1);
        let runtime_key_v1 = signing_key(2);
        let runtime_key_v2 = signing_key(3);
        let config = config(&installer_key);
        let backend = SqliteStorageBackend::new(&path).expect("backend");
        let registry = tool_sender_registry(backend.shared_db_for_test(), Some(config.clone()));

        let install = installer_request(
            &installer_key,
            &runtime_key_v1,
            ToolInstallerOperation::Register,
            1,
            "runtime-v1",
            None,
            "install-1",
        );
        let receipt = registry
            .apply_signed(install.clone(), timestamp("2026-10-23T12:00:00Z"))
            .expect("authenticated installation");
        assert_eq!(receipt.registration_generation, 1);
        assert_eq!(
            registry
                .apply_signed(install.clone(), timestamp("2026-10-23T12:01:00Z"))
                .expect("exact installer replay"),
            receipt
        );

        let reinstall = installer_request(
            &installer_key,
            &runtime_key_v1,
            ToolInstallerOperation::Register,
            1,
            "runtime-v1",
            None,
            "install-retry-new-nonce",
        );
        assert_eq!(
            registry
                .apply_signed(reinstall, timestamp("2026-10-23T12:02:00Z"))
                .expect("idempotent reinstall"),
            receipt
        );

        let rotate = installer_request(
            &installer_key,
            &runtime_key_v2,
            ToolInstallerOperation::Rotate,
            2,
            "runtime-v2",
            Some("runtime-v1"),
            "rotate-2",
        );
        assert_eq!(
            registry
                .apply_signed(rotate, timestamp("2026-10-23T12:03:00Z"))
                .expect("authenticated key rotation")
                .registration_generation,
            2
        );

        let message_id = AtmMessageId::new();
        let proof = runtime_proof(
            &runtime_key_v2,
            "runtime-v2",
            2,
            "runtime-message-1",
            message_id,
            "sha256:message",
        );
        let admission = registry
            .admit_signed_message(
                proof.clone(),
                message_id,
                MessageDigest::parse("sha256:message").expect("digest"),
                timestamp("2026-10-23T12:04:00Z"),
            )
            .expect("authenticated runtime admission");
        assert_eq!(admission.sender().address().to_string(), "nightly@tool");
        assert_eq!(
            registry
                .admit_signed_message(
                    proof.clone(),
                    message_id,
                    MessageDigest::parse("sha256:message").expect("digest"),
                    timestamp("2026-10-23T12:05:00Z"),
                )
                .expect("exact runtime replay")
                .sender()
                .address()
                .to_string(),
            "nightly@tool"
        );

        rusqlite::Connection::open(&path)
            .expect("mail history connection")
            .execute(
                "INSERT INTO mail_messages (team, agent, message_key, envelope_json, from_agent, message_text, message_at) VALUES ('dev', 'recipient', 'atm:historical-tool-mail', '{}', 'nightly@tool', 'retained', '2026-10-23T12:06:00Z')",
                [],
            )
            .expect("historical tool message");

        let altered_proof = runtime_proof(
            &runtime_key_v2,
            "runtime-v2",
            2,
            "runtime-message-1",
            AtmMessageId::new(),
            "sha256:altered",
        );
        let replay_error = registry
            .admit_signed_message(
                altered_proof.clone(),
                altered_proof.message_id,
                altered_proof.message_digest.clone(),
                timestamp("2026-10-23T12:06:00Z"),
            )
            .expect_err("same nonce with altered message is rejected");
        assert_eq!(replay_error.code(), AtmErrorCode::ToolNonceReplay);

        let uninstall = installer_request(
            &installer_key,
            &runtime_key_v2,
            ToolInstallerOperation::Unregister,
            3,
            "runtime-v2",
            Some("runtime-v2"),
            "uninstall-3",
        );
        registry
            .apply_signed(uninstall, timestamp("2026-10-23T12:06:30Z"))
            .expect("authenticated owner uninstall");
        let historical_mail_count: i64 = rusqlite::Connection::open(&path)
            .expect("mail history connection")
            .query_row(
                "SELECT COUNT(*) FROM mail_messages WHERE from_agent='nightly@tool' AND message_text='retained'",
                [],
                |row| row.get(0),
            )
            .expect("historical mail survives uninstall");
        assert_eq!(historical_mail_count, 1);
        assert_eq!(
            registry
                .resolve(&PluginSenderId::parse("nightly").expect("sender"))
                .expect_err("uninstalled sender is no longer inspectable")
                .code(),
            AtmErrorCode::ToolSenderUnregistered
        );

        drop(registry);
        drop(backend);
        let reopened = SqliteStorageBackend::new(&path).expect("reopened backend");
        let reopened_registry = tool_sender_registry(reopened.shared_db_for_test(), Some(config));
        assert_eq!(
            reopened_registry
                .apply_signed(install, timestamp("2026-10-23T12:07:00Z"))
                .expect("receipt survives reopen"),
            receipt
        );
        assert_eq!(
            reopened_registry
                .resolve(&PluginSenderId::parse("nightly").expect("sender"))
                .expect_err("uninstall survives reopen")
                .code(),
            AtmErrorCode::ToolSenderUnregistered
        );
        assert_eq!(
            reopened_registry
                .admit_signed_message(
                    proof,
                    message_id,
                    MessageDigest::parse("sha256:message").expect("digest"),
                    timestamp("2026-10-23T12:07:00Z"),
                )
                .expect("exact committed runtime retry retains its receipt")
                .sender()
                .address()
                .to_string(),
            "nightly@tool"
        );
    }

    #[test]
    fn registration_rejects_bad_signatures_wrong_owner_and_stale_generation() {
        let root = tempdir().expect("tempdir");
        let path = root.path().join("mail.db");
        let installer_key = signing_key(4);
        let other_key = signing_key(5);
        let runtime_key = signing_key(6);
        let backend = SqliteStorageBackend::new(&path).expect("backend");
        let registry_config = ToolSenderRegistryConfig::new(
            HostName::from_str("host.example").expect("host"),
            [
                TrustedToolInstallerKey::new(
                    InstallerId::parse("installer").expect("installer id"),
                    ToolKeyId::parse("installer-key").expect("installer key"),
                    &encoded_public_key(&installer_key),
                    [PluginSenderId::parse("nightly").expect("sender")],
                )
                .expect("trusted installer"),
                TrustedToolInstallerKey::new(
                    InstallerId::parse("other-installer").expect("other installer id"),
                    ToolKeyId::parse("other-key").expect("other installer key"),
                    &encoded_public_key(&other_key),
                    [PluginSenderId::parse("other-plugin").expect("other sender")],
                )
                .expect("other trusted installer"),
            ],
        )
        .expect("registry config");
        let registry = tool_sender_registry(backend.shared_db_for_test(), Some(registry_config));
        let mut bad_signature = installer_request(
            &installer_key,
            &runtime_key,
            ToolInstallerOperation::Register,
            1,
            "runtime-v1",
            None,
            "bad-signature",
        );
        bad_signature.signature = encoded_public_key(&installer_key);
        assert_eq!(
            registry
                .apply_signed(bad_signature, timestamp("2026-10-23T12:00:00Z"))
                .expect_err("invalid signature rejected")
                .code(),
            AtmErrorCode::ToolSignatureInvalid
        );

        let request = installer_request(
            &installer_key,
            &runtime_key,
            ToolInstallerOperation::Register,
            1,
            "runtime-v1",
            None,
            "install-owner",
        );
        registry
            .apply_signed(request, timestamp("2026-10-23T12:00:00Z"))
            .expect("owner registers sender");

        let mut wrong_owner = installer_request(
            &other_key,
            &runtime_key,
            ToolInstallerOperation::Unregister,
            2,
            "runtime-v1",
            Some("runtime-v1"),
            "wrong-owner",
        );
        wrong_owner.installer_id = InstallerId::parse("other-installer").expect("other owner");
        wrong_owner.installer_key_id = ToolKeyId::parse("other-key").expect("other key");
        wrong_owner.signature = base64::engine::general_purpose::STANDARD.encode(
            other_key
                .sign(&wrong_owner.canonical_signing_bytes().expect("bytes"))
                .as_ref(),
        );
        assert_eq!(
            registry
                .apply_signed(wrong_owner, timestamp("2026-10-23T12:01:00Z"))
                .expect_err("untrusted owner rejected")
                .code(),
            AtmErrorCode::ToolSenderUnauthorized
        );

        let stale = runtime_proof(
            &runtime_key,
            "runtime-v1",
            2,
            "stale-generation",
            AtmMessageId::new(),
            "sha256:message",
        );
        assert_eq!(
            registry
                .admit_signed_message(
                    stale.clone(),
                    stale.message_id,
                    stale.message_digest.clone(),
                    timestamp("2026-10-23T12:02:00Z"),
                )
                .expect_err("stale generation rejected")
                .code(),
            AtmErrorCode::ToolGenerationConflict
        );
        assert!(
            registry
                .resolve(&PluginSenderId::parse("nightly").expect("sender"))
                .is_ok()
        );
    }

    #[test]
    fn absent_trust_configuration_fails_closed_with_configuration_error() {
        let root = tempdir().expect("tempdir");
        let backend = SqliteStorageBackend::new(root.path().join("mail.db")).expect("backend");
        let registry = tool_sender_registry(backend.shared_db_for_test(), None);
        let error = registry
            .apply_signed(
                installer_request(
                    &signing_key(7),
                    &signing_key(8),
                    ToolInstallerOperation::Register,
                    1,
                    "runtime-v1",
                    None,
                    "no-config",
                ),
                timestamp("2026-10-23T12:00:00Z"),
            )
            .expect_err("missing local trust root fails closed");
        assert_eq!(error.code(), AtmErrorCode::ConfigParseFailed);
    }

    #[test]
    fn nonce_write_failure_rolls_back_registration_for_deterministic_retry() {
        let root = tempdir().expect("tempdir");
        let backend = SqliteStorageBackend::new(root.path().join("mail.db")).expect("backend");
        let db = backend.shared_db_for_test();
        let registry = tool_sender_registry(db.clone(), Some(config(&signing_key(9))));
        db.with_connection(|connection| {
            connection
                .execute_batch(
                    "CREATE TRIGGER reject_nonce BEFORE INSERT ON tool_sender_nonce_ledger BEGIN SELECT RAISE(ABORT, 'injected nonce failure'); END;",
                )
                .expect("install deterministic failure trigger");
            Ok(())
        })
        .expect("create failpoint");

        let installer_key = signing_key(9);
        let runtime_key = signing_key(10);
        let request = installer_request(
            &installer_key,
            &runtime_key,
            ToolInstallerOperation::Register,
            1,
            "runtime-v1",
            None,
            "rollback-retry",
        );
        assert_eq!(
            registry
                .apply_signed(request.clone(), timestamp("2026-10-23T12:00:00Z"))
                .expect_err("nonce failure aborts the transaction")
                .code(),
            AtmErrorCode::StoreQueryFailed
        );
        assert_eq!(
            registry
                .resolve(&PluginSenderId::parse("nightly").expect("sender"))
                .expect_err("registration update rolled back")
                .code(),
            AtmErrorCode::ToolSenderUnregistered
        );

        db.with_connection(|connection| {
            connection
                .execute_batch("DROP TRIGGER reject_nonce;")
                .expect("remove deterministic failure trigger");
            Ok(())
        })
        .expect("remove failpoint");
        registry
            .apply_signed(request, timestamp("2026-10-23T12:01:00Z"))
            .expect("same signed operation succeeds after transaction rollback");
        assert!(
            registry
                .resolve(&PluginSenderId::parse("nightly").expect("sender"))
                .is_ok()
        );
    }

    #[test]
    fn pre_bk10_registry_schema_migrates_repeatably_and_preserves_rows() {
        let root = tempdir().expect("tempdir");
        let path = root.path().join("mail.db");
        let legacy = rusqlite::Connection::open(&path).expect("legacy database");
        legacy
            .execute_batch(
                "CREATE TABLE tool_sender_registry (sender TEXT PRIMARY KEY, installer_id TEXT NOT NULL, installer_key_id TEXT NOT NULL, host_id TEXT NOT NULL, runtime_key_id TEXT NOT NULL, runtime_public_key TEXT NOT NULL, registration_generation INTEGER NOT NULL CHECK(registration_generation >= 1), active INTEGER NOT NULL DEFAULT 1 CHECK(active IN (0, 1)), installed_at TEXT NOT NULL, revoked_at TEXT NULL); CREATE TABLE tool_sender_nonce_ledger (nonce TEXT PRIMARY KEY, sender TEXT NOT NULL, operation_digest TEXT NOT NULL, result_digest TEXT NULL, consumed_at TEXT NOT NULL); INSERT INTO tool_sender_registry (sender, installer_id, installer_key_id, host_id, runtime_key_id, runtime_public_key, registration_generation, active, installed_at) VALUES ('nightly', 'installer', 'installer-key', 'host.example', 'runtime-v1', 'cHVibGlj', 1, 1, '2026-10-23T12:00:00Z');",
            )
            .expect("old schema fixture");
        drop(legacy);

        let installer_key = signing_key(11);
        let backend = SqliteStorageBackend::new(&path).expect("first migration");
        let registry =
            tool_sender_registry(backend.shared_db_for_test(), Some(config(&installer_key)));
        assert!(
            registry
                .resolve(&PluginSenderId::parse("nightly").expect("sender"))
                .is_ok()
        );
        drop(registry);
        drop(backend);

        let reopened = SqliteStorageBackend::new(&path).expect("repeat migration");
        let registry =
            tool_sender_registry(reopened.shared_db_for_test(), Some(config(&installer_key)));
        assert!(
            registry
                .resolve(&PluginSenderId::parse("nightly").expect("sender"))
                .is_ok()
        );
        let receipt_tables: i64 = reopened
            .shared_db_for_test()
            .with_connection(|connection| {
                connection
                    .query_row(
                        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('tool_sender_install_receipt', 'tool_sender_runtime_binding')",
                        [],
                        |row| row.get(0),
                    )
                    .map_err(|error| super::db_error("count receipt tables", error))
            })
            .expect("receipt tables migrated");
        assert_eq!(receipt_tables, 2);
    }
}
