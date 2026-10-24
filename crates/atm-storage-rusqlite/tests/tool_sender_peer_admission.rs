use std::str::FromStr;

use atm_storage::{
    AtmMessageId, HostName, IsoTimestamp, MessageDigest, PluginSenderId, ToolNonce,
    ToolSenderAttestation,
};
use atm_storage_rusqlite::{
    SqliteStorageBackend, ToolSenderPeerAdmissionConfig, TrustedToolOriginKey,
};
use base64::Engine as _;
use ring::signature::{Ed25519KeyPair, KeyPair};
use tempfile::tempdir;

fn setup(path: &std::path::Path, key_pair: &Ed25519KeyPair) -> SqliteStorageBackend {
    let sender = PluginSenderId::parse("plugin").expect("sender");
    let public_key = base64::engine::general_purpose::STANDARD.encode(key_pair.public_key());
    let origin = TrustedToolOriginKey::new(
        HostName::from_str("origin.example").expect("host"),
        &public_key,
        [sender],
    )
    .expect("origin key");
    let config = ToolSenderPeerAdmissionConfig::new([origin]).expect("peer policy");
    SqliteStorageBackend::new_with_tool_sender_configs(path, None, Some(config))
        .expect("configured SQLite backend")
}

fn signed_attestation(
    key_pair: &Ed25519KeyPair,
    message_id: AtmMessageId,
    digest: MessageDigest,
    nonce: &str,
) -> ToolSenderAttestation {
    let mut attestation = ToolSenderAttestation {
        sender: PluginSenderId::parse("plugin").expect("sender"),
        registration_generation: 7,
        host_id: HostName::from_str("origin.example").expect("host"),
        message_id,
        message_digest: digest,
        nonce: ToolNonce::parse(nonce).expect("nonce"),
        signature: String::new(),
    };
    attestation.signature = base64::engine::general_purpose::STANDARD.encode(
        key_pair.sign(
            &attestation
                .canonical_signing_bytes()
                .expect("canonical bytes"),
        ),
    );
    attestation
}

fn key_pair() -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed_unchecked(&[41; 32]).expect("deterministic test key")
}

#[test]
fn exact_attestation_replay_survives_backend_restart() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("peer-admission.sqlite");
    let key = key_pair();
    let message_id = AtmMessageId::new();
    let digest = MessageDigest::for_message_body("canonical body");
    let attestation = signed_attestation(&key, message_id, digest.clone(), "nonce-1");
    let source_host = HostName::from_str("origin.example").expect("host");

    let first = setup(&path, &key);
    let admitted = first
        .tool_sender_peer_admission_store()
        .admit_attestation(
            attestation.clone(),
            message_id,
            digest.clone(),
            &source_host,
            IsoTimestamp::now(),
        )
        .expect("first admission");
    assert_eq!(admitted.sender().address().to_string(), "plugin@tool");
    assert_eq!(admitted.message_id(), message_id);
    assert_eq!(admitted.message_digest(), &digest);

    let exact_retry = first
        .tool_sender_peer_admission_store()
        .admit_attestation(
            attestation.clone(),
            message_id,
            digest.clone(),
            &source_host,
            IsoTimestamp::now(),
        )
        .expect("same transaction replay is idempotent");
    assert_eq!(exact_retry.message_id(), admitted.message_id());
    drop(first);

    let reopened = setup(&path, &key);
    let after_restart = reopened
        .tool_sender_peer_admission_store()
        .admit_attestation(
            attestation,
            message_id,
            digest.clone(),
            &source_host,
            IsoTimestamp::now(),
        )
        .expect("durable exact replay after restart");
    assert_eq!(after_restart.message_id(), admitted.message_id());
    assert_eq!(after_restart.message_digest(), &digest);
}

#[test]
fn changed_binding_and_foreign_authenticated_host_fail_closed() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("peer-admission.sqlite");
    let key = key_pair();
    let backend = setup(&path, &key);
    let store = backend.tool_sender_peer_admission_store();
    let message_id = AtmMessageId::new();
    let digest = MessageDigest::for_message_body("canonical body");
    let source_host = HostName::from_str("origin.example").expect("host");
    let attestation = signed_attestation(&key, message_id, digest.clone(), "nonce-2");

    let foreign = HostName::from_str("other.example").expect("foreign host");
    let foreign_error = store
        .admit_attestation(
            attestation.clone(),
            message_id,
            digest.clone(),
            &foreign,
            IsoTimestamp::now(),
        )
        .expect_err("TLS source must match signed origin host");
    assert_eq!(
        foreign_error.code(),
        atm_storage::AtmErrorCode::ToolHostMismatch
    );

    let changed_id = AtmMessageId::new();
    let binding_error = store
        .admit_attestation(
            attestation.clone(),
            changed_id,
            digest.clone(),
            &source_host,
            IsoTimestamp::now(),
        )
        .expect_err("signed message id cannot be substituted");
    assert_eq!(
        binding_error.code(),
        atm_storage::AtmErrorCode::ToolSignatureInvalid
    );

    store
        .admit_attestation(
            attestation,
            message_id,
            digest.clone(),
            &source_host,
            IsoTimestamp::now(),
        )
        .expect("valid binding first");
    let changed_digest = MessageDigest::for_message_body("tampered body");
    let changed = signed_attestation(&key, message_id, changed_digest.clone(), "nonce-2");
    let replay_error = store
        .admit_attestation(
            changed,
            message_id,
            changed_digest,
            &source_host,
            IsoTimestamp::now(),
        )
        .expect_err("nonce cannot be rebound to another signed digest");
    assert_eq!(
        replay_error.code(),
        atm_storage::AtmErrorCode::ToolNonceReplay
    );
}

#[test]
fn missing_peer_policy_fails_closed() {
    let directory = tempdir().expect("temporary directory");
    let backend = SqliteStorageBackend::new(directory.path().join("no-policy.sqlite"))
        .expect("SQLite backend");
    let key = key_pair();
    let message_id = AtmMessageId::new();
    let digest = MessageDigest::for_message_body("body");
    let attestation = signed_attestation(&key, message_id, digest.clone(), "nonce-3");
    let error = backend
        .tool_sender_peer_admission_store()
        .admit_attestation(
            attestation,
            message_id,
            digest,
            &HostName::from_str("origin.example").expect("host"),
            IsoTimestamp::now(),
        )
        .expect_err("unconfigured adapter cannot admit peer tools");
    assert_eq!(
        error.code(),
        atm_storage::AtmErrorCode::ToolSenderUnauthorized
    );
}
