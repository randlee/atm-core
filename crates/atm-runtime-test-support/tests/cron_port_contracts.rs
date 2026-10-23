//! External consumers for the frozen BK-9 cron and sender ports.
//!
//! The calls here intentionally use the published test doubles through their
//! traits.  They are deterministic contract checks rather than implementation
//! tests for the doubles themselves.

use std::str::FromStr;

use atm_core::boundary::CronLifecycleNudger;
use atm_runtime_test_support::RecordingCronLifecycleNudger;
use atm_storage::testing::{
    InMemoryCommittedCronNoticeRecord, InMemoryCronTransitionStore, InMemoryToolSenderRegistry,
};
use atm_storage::{
    AgentName, AtmMessageId, CronCommitResult, CronDeliveryKey, CronDiagnosticDetail,
    CronGeneration, CronLifecycleEvent, CronName, CronNoticeIntent, CronNoticeKey,
    CronRunTransition, CronScheduleId, CronSuccess, CronTransitionStore, CronTransitionWrite,
    DeliveryReceipt, HostName, InstallerId, IsoTimestamp, MessageDigest, PluginSenderId, TeamName,
    ToolInstallerOperation, ToolInstallerRequest, ToolKeyId, ToolNonce, ToolRuntimeProof,
    ToolSenderRegistry,
};

fn fixed_time() -> IsoTimestamp {
    "2026-01-02T03:04:05Z".parse().expect("fixed timestamp")
}

fn notice() -> CronNoticeIntent {
    let record = InMemoryCommittedCronNoticeRecord::new(
        CronNoticeKey {
            schedule_id: CronScheduleId::new(7),
            generation: CronGeneration::FIRST,
            transition_seq: 3,
        },
        TeamName::from_validated("team"),
        AgentName::from_validated("creator"),
        CronName::parse("nightly").expect("fixed name"),
        CronLifecycleEvent::Stopped {
            message: CronDiagnosticDetail::parse("completed".into()).expect("fixed detail"),
        },
    );
    CronNoticeIntent::from_committed(&record)
}

#[tokio::test]
async fn lifecycle_consumer_delivers_the_persisted_notice_unchanged() {
    let nudger = RecordingCronLifecycleNudger::default();
    nudger
        .replies
        .lock()
        .expect("reply queue")
        .push_back(Ok(DeliveryReceipt::Accepted {
            receipt_id: "receipt-7".into(),
        }));
    let persisted = notice();

    let receipt = nudger.deliver(&persisted).await.expect("delivery");

    assert_eq!(
        receipt,
        DeliveryReceipt::Accepted {
            receipt_id: "receipt-7".into()
        }
    );
    assert_eq!(
        nudger
            .delivered
            .lock()
            .expect("recorded delivery")
            .as_slice(),
        &[persisted]
    );
}

#[test]
fn transition_store_consumer_commits_and_marks_the_same_lifecycle_key() {
    let store = InMemoryCronTransitionStore::default();
    let persisted = notice();
    let key = CronDeliveryKey::Lifecycle(persisted.key());
    store
        .commit_results
        .lock()
        .expect("commit queue")
        .push_back(Ok(CronCommitResult::Applied {
            intents: vec![atm_storage::CronDeliveryIntent::Lifecycle(
                persisted.clone(),
            )],
        }));

    let result = store
        .commit(CronTransitionWrite {
            id: persisted.key().schedule_id,
            generation: persisted.key().generation,
            at: fixed_time(),
            transition: CronRunTransition::Success {
                outcome: CronSuccess::Ok,
            },
            outputs: vec![],
        })
        .expect("commit result");
    assert_eq!(
        result,
        CronCommitResult::Applied {
            intents: vec![atm_storage::CronDeliveryIntent::Lifecycle(persisted)]
        }
    );
    assert!(
        store
            .mark_delivered(key, DeliveryReceipt::Suppressed)
            .expect("mark delivery")
    );
    assert_eq!(store.writes.lock().expect("writes").len(), 1);
    assert_eq!(store.delivered.lock().expect("deliveries").len(), 1);
}

fn installer_request(sender: PluginSenderId) -> ToolInstallerRequest {
    ToolInstallerRequest {
        schema_version: "1.0.0".into(),
        operation: ToolInstallerOperation::Register,
        installer_id: InstallerId::parse("installer").expect("fixed installer"),
        installer_key_id: ToolKeyId::parse("installer-key").expect("fixed key"),
        sender,
        package_digest: MessageDigest::parse("package-digest").expect("fixed digest"),
        registration_generation: 1,
        host_id: HostName::from_str("host.example").expect("fixed host"),
        runtime_key_id: ToolKeyId::parse("runtime-key").expect("fixed runtime key"),
        runtime_public_key: "public-key".into(),
        previous_runtime_key_id: None,
        nonce: ToolNonce::parse("installer-nonce").expect("fixed nonce"),
        issued_at: fixed_time(),
        expires_at: "2026-01-02T04:04:05Z".parse().expect("fixed expiry"),
        signature: "opaque-signature".into(),
    }
}

#[test]
fn sender_registry_consumer_requires_registered_proof_binding() {
    let registry = InMemoryToolSenderRegistry::default();
    let sender = PluginSenderId::parse("plugin").expect("fixed sender");
    registry
        .apply_signed(installer_request(sender.clone()), fixed_time())
        .expect("register sender");
    let message_id = AtmMessageId::from_str("01ARZ3NDEKTSV4RRFFQ69G5FAV").expect("fixed id");
    let digest = MessageDigest::parse("message-digest").expect("fixed digest");
    let admitted = registry
        .admit_signed_message(
            ToolRuntimeProof {
                runtime_key_id: ToolKeyId::parse("runtime-key").expect("fixed runtime key"),
                sender: sender.clone(),
                registration_generation: 1,
                host_id: HostName::from_str("host.example").expect("fixed host"),
                message_id,
                message_digest: digest.clone(),
                nonce: ToolNonce::parse("message-nonce").expect("fixed nonce"),
                signature: "opaque-signature".into(),
            },
            message_id,
            digest,
            fixed_time(),
        )
        .expect("matching proof is admitted");

    assert_eq!(admitted.address().to_string(), "plugin@tool");
    assert_eq!(
        registry.resolve(&sender).expect("registered sender"),
        admitted
    );
}
