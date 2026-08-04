use tempfile::tempdir;

use super::tests::{
    RecordingObservability, RecordingPostSendEmitter, TestRuntime, delivery_snapshot, message,
    outbound_message, send_request,
};
use super::{
    DeliveryExecutionMode, DuplicateWriteDisposition, SendMessageSource, WriteOutcome,
    persist_message, write_mail_with_runtime_impl, write_mail_with_runtime_impl_with_mode,
};
use crate::boundary::{MailStoreMailboxMetadataRow, Message, MessageKey};
use crate::caller_context::ActivityObservation;
use crate::delivery_policy::DeliveryHarnessPath;
use crate::error_codes::AtmErrorCode;
use crate::schema::{AtmMessageId, set_authenticated_source_host, set_peer_outbound_write};
use crate::test_support::{TEST_SENDER, TEST_TEAM};
use crate::types::{AgentName, HostName, IsoTimestamp, SessionId, TeamName};

#[test]
fn host_qualified_origin_write_persists_without_remote_roster_and_preserves_origin_ulid() {
    let tempdir = tempdir().expect("tempdir");
    let mut runtime = TestRuntime::new(None, DeliveryHarnessPath::ClaudeCode);
    runtime.roster_member_missing = true;
    let origin_id = AtmMessageId::new();
    let mut request = send_request(tempdir.path()).with_origin_message_id(origin_id);
    request.to = Some(
        "recipient@test-team.localhost"
            .parse()
            .expect("remote address"),
    );

    let outcome = super::send_mail_with_runtime_impl(
        request,
        &RecordingObservability::default(),
        &runtime,
        None,
    )
    .expect("host-qualified origin write must not query the remote roster");
    assert_eq!(outcome.message_id, origin_id);
    let records = runtime
        .persisted_records
        .lock()
        .expect("persisted records lock");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].message_key, MessageKey::from(origin_id));
}

#[test]
fn peer_outbound_payload_excludes_local_activity_observation() {
    let tempdir = tempdir().expect("tempdir");
    let runtime = TestRuntime::new(None, DeliveryHarnessPath::NonClaude);
    let observability = RecordingObservability::default();
    let mut request = send_request(tempdir.path());
    request.to = Some(
        "recipient@test-team.peer.example.test"
            .parse()
            .expect("remote address"),
    );
    request.activity_observation = Some(ActivityObservation {
        team: "test-team".parse().expect("team"),
        member: "sender".parse().expect("agent"),
        session_id: Some(SessionId::new("session-17").expect("session")),
        pid: Some(17),
    });

    write_mail_with_runtime_impl(request, &observability, &runtime).expect("origin write");

    let records = runtime.persisted_records.lock().expect("persisted records");
    let peer_outbound = records[0].envelope.extra["peerOutbound"]
        .as_object()
        .expect("peer outbound metadata");
    let payload = peer_outbound["request"].as_str().expect("request payload");
    let payload: serde_json::Value = serde_json::from_str(payload).expect("request JSON");
    assert!(
        payload.get("activity_observation").is_none(),
        "durable peer replay payload must not retain local session or PID metadata"
    );
}

#[test]
fn same_store_peer_receipt_skips_the_duplicate_row_but_continues_post_write() {
    let runtime = TestRuntime::new(None, DeliveryHarnessPath::NonClaude);
    let tempdir = tempdir().expect("tempdir");
    let inbox_path = tempdir.path().join("recipient.jsonl");
    let recipient = delivery_snapshot(DeliveryHarnessPath::NonClaude);
    let destination_host: HostName = "192.168.128.82".parse().expect("destination host");
    let source_host: HostName = "peer.example.test".parse().expect("source host");
    let mut origin = outbound_message();
    set_peer_outbound_write(&mut origin, &destination_host, "{}".to_string());
    persist_message(
        &runtime,
        tempdir.path(),
        &recipient,
        &inbox_path,
        &origin,
        false,
        None,
    )
    .expect("origin write");

    let mut receipt = origin.clone();
    receipt.extra.remove("peerOutbound");
    set_authenticated_source_host(&mut receipt, Some(source_host.clone()));
    let result = persist_message(
        &runtime,
        tempdir.path(),
        &recipient,
        &inbox_path,
        &receipt,
        false,
        Some((&source_host, &destination_host)),
    )
    .expect("same-store peer receipt");

    assert_eq!(
        result.duplicate_disposition,
        DuplicateWriteDisposition::SameStorePeerReceipt
    );
    assert!(result.requires_post_write());
    let records = runtime.persisted_records.lock().expect("persisted records");
    assert_eq!(records.len(), 1);
    assert!(records[0].envelope.extra.contains_key("peerOutbound"));
}

#[test]
fn canonical_writer_persists_before_router_owned_local_nudge() {
    let tempdir = tempdir().expect("tempdir");
    let runtime = TestRuntime::new(None, DeliveryHarnessPath::NonClaude);
    let observability = RecordingObservability::default();
    let emitter = RecordingPostSendEmitter::succeed();
    let mut prepared = write_mail_with_runtime_impl_with_mode(
        send_request(tempdir.path()),
        &observability,
        &runtime,
        DeliveryExecutionMode::Deferred,
    )
    .expect("canonical write must persist before routing");

    assert_eq!(
        runtime
            .persisted_records
            .lock()
            .expect("persisted records lock")
            .len(),
        1,
        "the durable message must exist before the router emits a nudge"
    );
    assert!(
        emitter.emitted().is_empty(),
        "the canonical writer must not emit a nudge before PostWriteRouter"
    );
    assert!(
        runtime
            .non_claude_deliveries
            .lock()
            .expect("non-Claude deliveries")
            .is_empty(),
        "durable admission must not synchronously invoke non-Claude outbound delivery"
    );

    prepared.emit_post_write_for_test(&runtime, &emitter);
    assert_eq!(
        emitter.emitted().len(),
        1,
        "the router emits one local nudge"
    );
    assert!(matches!(
        prepared
            .finish_with_runtime(&runtime, &observability)
            .expect("local route finish"),
        WriteOutcome::Sent(_)
    ));
}

#[test]
fn same_host_peer_duplicate_still_routes_one_local_nudge() {
    let tempdir = tempdir().expect("tempdir");
    let runtime = TestRuntime::new(None, DeliveryHarnessPath::NonClaude);
    let observability = RecordingObservability::default();
    let emitter = RecordingPostSendEmitter::succeed();
    let mut origin = send_request(tempdir.path());
    origin.to = Some(
        "recipient@test-team.localhost"
            .parse()
            .expect("same-store peer target"),
    );
    write_mail_with_runtime_impl(origin, &observability, &runtime).expect("origin write persists");
    let origin = {
        let records = runtime.persisted_records.lock().expect("persisted records");
        assert_eq!(records.len(), 1, "origin write persists one record");
        records[0].envelope.clone()
    };
    let message_id = origin.message_id.expect("origin ULID");
    let timestamp = origin.timestamp;

    let mut receipt = send_request(tempdir.path()).with_origin_metadata(message_id, timestamp);
    receipt.to = Some(
        "recipient@test-team.localhost"
            .parse()
            .expect("same-store peer target"),
    );
    receipt.authenticated_source_host = Some("localhost".parse().expect("host"));
    let mut prepared = write_mail_with_runtime_impl(receipt, &observability, &runtime)
        .expect("same-host receipt is an idempotent write");

    assert!(prepared.requires_post_write_route());
    prepared.emit_post_write_for_test(&runtime, &emitter);
    assert_eq!(emitter.emitted().len(), 1);
}

#[test]
fn authenticated_duplicate_peer_receipt_for_advertised_ip_uses_one_local_post_write() {
    let tempdir = tempdir().expect("tempdir");
    let runtime = TestRuntime::new(None, DeliveryHarnessPath::NonClaude);
    let observability = RecordingObservability::default();
    let origin_id = AtmMessageId::new();
    let timestamp = IsoTimestamp::now();
    let mut origin = send_request(tempdir.path()).with_origin_metadata(origin_id, timestamp);
    origin.to = Some(
        "recipient@test-team.192.168.128.82"
            .parse()
            .expect("advertised peer address"),
    );

    write_mail_with_runtime_impl(origin.clone(), &observability, &runtime).expect("origin write");
    set_peer_outbound_write(
        &mut runtime
            .persisted_records
            .lock()
            .expect("persisted records lock")[0]
            .envelope,
        &"192.168.128.82".parse().expect("advertised peer host"),
        "canonical peer request".to_string(),
    );

    let mut receipt = origin;
    receipt.authenticated_source_host = Some("127.0.0.1".parse().expect("peer alias"));
    let emitter = RecordingPostSendEmitter::succeed();
    super::send_mail_with_runtime_impl(receipt, &observability, &runtime, Some(&emitter))
        .expect("authenticated duplicate receipt");

    assert_eq!(
        runtime
            .persisted_records
            .lock()
            .expect("persisted records lock")
            .len(),
        1,
        "the receipt retains the origin ULID row"
    );
    assert_eq!(
        emitter.emitted().len(),
        1,
        "an authenticated receipt uses the normal local post-write route even when the certificate host is an alias"
    );
}

#[test]
fn duplicate_ulid_ignores_transport_only_peer_metadata() {
    let tempdir = tempdir().expect("tempdir");
    let runtime = TestRuntime::new(None, DeliveryHarnessPath::NonClaude);
    let observability = RecordingObservability::default();
    let message_id = AtmMessageId::new();
    let timestamp = IsoTimestamp::now();
    let mut origin = send_request(tempdir.path()).with_origin_metadata(message_id, timestamp);
    origin.to = Some(
        "recipient@test-team.localhost"
            .parse()
            .expect("peer target"),
    );
    write_mail_with_runtime_impl(origin, &observability, &runtime).expect("origin write");

    let mut receipt = send_request(tempdir.path()).with_origin_metadata(message_id, timestamp);
    receipt.authenticated_source_host = Some("localhost".parse().expect("host"));
    let prepared = write_mail_with_runtime_impl(receipt, &observability, &runtime)
        .expect("transport-only peer metadata must not conflict");

    assert!(!prepared.requires_post_write_route());
}

#[test]
fn authenticated_peer_ack_message_uses_the_shared_write_pipeline() {
    let tempdir = tempdir().expect("tempdir");
    let runtime = TestRuntime::new(None, DeliveryHarnessPath::NonClaude);
    let observability = RecordingObservability::default();
    let mut source = send_request(tempdir.path());
    source.requires_ack = true;
    write_mail_with_runtime_impl(source, &observability, &runtime)
        .expect("pending source message persists before its peer acknowledgement");
    let source = {
        let records = runtime.persisted_records.lock().expect("persisted records");
        assert_eq!(records.len(), 1, "one source message persists");
        records[0].clone()
    };
    let acknowledged_message_id = source.envelope.message_id.expect("source message ULID");
    runtime
        .mailbox_rows
        .lock()
        .expect("mailbox rows")
        .push(MailStoreMailboxMetadataRow {
            message_key: source.message_key,
            message_id: Some(acknowledged_message_id),
            parent_message_id: None,
            thread_mode: None,
            from_agent: source.envelope.from,
            source_chat_id: None,
            destination_chat_id: None,
            summary: None,
            message_at: source.envelope.timestamp,
            read: false,
            requires_ack: true,
            pending_ack: true,
            acknowledged_at: None,
            expires_at: None,
            task_id: source.envelope.task_id,
        });
    let mut request =
        send_request(tempdir.path()).with_origin_metadata(AtmMessageId::new(), IsoTimestamp::now());
    request.authenticated_source_host = Some("peer.example.test".parse().expect("host"));
    request.acknowledges_message_id = Some(acknowledged_message_id);

    let prepared = write_mail_with_runtime_impl(request, &observability, &runtime)
        .expect("authenticated peer acknowledgement is a canonical write, not a local command");

    assert!(prepared.requires_post_write_route());
}

#[test]
fn conflicting_origin_ulid_stops_before_post_write_or_outbound_delivery() {
    let tempdir = tempdir().expect("tempdir");
    let runtime = TestRuntime::new(None, DeliveryHarnessPath::NonClaude);
    let observability = RecordingObservability::default();
    let emitter = RecordingPostSendEmitter::succeed();
    let origin_id = AtmMessageId::new();

    super::send_mail_with_runtime_impl(
        send_request(tempdir.path()).with_origin_message_id(origin_id),
        &observability,
        &runtime,
        Some(&emitter),
    )
    .expect("initial origin write");
    let post_write_count = emitter.emitted().len();
    let outbound_delivery_count = runtime
        .non_claude_deliveries
        .lock()
        .expect("non-claude deliveries lock")
        .len();

    let mut conflicting = send_request(tempdir.path()).with_origin_message_id(origin_id);
    conflicting.message_source =
        SendMessageSource::Inline("different immutable payload".to_string());
    let error =
        super::send_mail_with_runtime_impl(conflicting, &observability, &runtime, Some(&emitter))
            .expect_err("conflicting origin ULID must fail before routing");

    assert_eq!(error.code(), AtmErrorCode::MessageIdConflict);
    assert_eq!(
        runtime
            .persisted_records
            .lock()
            .expect("persisted records lock")[0]
            .envelope
            .text,
        "hello",
        "the conflicting replay must retain the original row"
    );
    assert_eq!(emitter.emitted().len(), post_write_count);
    assert_eq!(
        runtime
            .non_claude_deliveries
            .lock()
            .expect("non-claude deliveries lock")
            .len(),
        outbound_delivery_count,
        "the conflict must not start another outbound delivery"
    );
}

#[test]
fn failed_peer_route_leaves_acknowledgement_source_pending() {
    let tempdir = tempdir().expect("tempdir");
    let runtime = TestRuntime::new(None, DeliveryHarnessPath::ClaudeCode);
    let source_id = AtmMessageId::new();
    let source_key = MessageKey::from(source_id);
    let mut source_envelope = message("remote-agent", source_id, None, None);
    source_envelope.source_team = Some("remote-team".parse().expect("remote team"));
    source_envelope.requires_ack = true;
    source_envelope.pending_ack_at = Some(IsoTimestamp::now());
    set_authenticated_source_host(
        &mut source_envelope,
        Some("peer.example.test".parse().expect("peer host")),
    );
    runtime
        .persisted_records
        .lock()
        .expect("persisted records lock")
        .push(Message {
            team: TeamName::from_validated(TEST_TEAM),
            agent: AgentName::from_validated(TEST_SENDER),
            message_key: source_key.clone(),
            envelope: source_envelope.clone(),
        });
    runtime
        .mailbox_rows
        .lock()
        .expect("mailbox rows lock")
        .push(MailStoreMailboxMetadataRow {
            message_key: source_key.clone(),
            message_id: Some(source_id),
            parent_message_id: None,
            thread_mode: None,
            from_agent: source_envelope.from.clone(),
            source_chat_id: None,
            destination_chat_id: None,
            summary: None,
            message_at: source_envelope.timestamp,
            read: false,
            requires_ack: true,
            pending_ack: true,
            acknowledged_at: None,
            expires_at: None,
            task_id: None,
        });

    let write = crate::ack::AckRequest {
        home_dir: tempdir.path().to_path_buf(),
        current_dir: tempdir.path().to_path_buf(),
        caller_identity: AgentName::from_validated(TEST_SENDER),
        caller_chat_id: None,
        caller_team: TeamName::from_validated(TEST_TEAM),
        activity_observation: None,
        message_id: source_id,
        reply_body: "acknowledged".to_string(),
    }
    .into_write_request();
    let prepared =
        write_mail_with_runtime_impl(write, &RecordingObservability::default(), &runtime)
            .expect("ack reply must persist before the peer route");

    drop(prepared); // Models a failed PostWriteRouter peer delivery before finish().
    assert!(
        runtime
            .persisted_states
            .lock()
            .expect("persisted states lock")
            .iter()
            .all(|state| state.message_key != source_key),
        "a failed peer route must not mark the source acknowledgement state"
    );
}
