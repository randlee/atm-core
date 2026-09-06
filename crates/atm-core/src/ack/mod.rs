use crate::error::AtmError;
use crate::observability::ObservabilityPort;
use crate::service_runtime::LocalServiceRuntime;
use crate::service_runtime_store::default_runtime;

// The acknowledgement request/outcome types and admission pipeline live in the
// canonical write module; `ack` re-exports the public API so external paths
// (including serde/persisted shapes) are unchanged.
use crate::write::WriteOutcome;
pub use crate::write::{AckOutcome, AckReplyDisposition, AckRequest, ReplyTarget};

#[cfg(test)]
pub(crate) use crate::write::{admit_acknowledgement_write, admit_acknowledgement_write_async};

/// Acknowledge one previously read pending-ack message and emit the documented
/// reply disposition.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::IdentityUnavailable`],
/// [`crate::error_codes::AtmErrorCode::TeamUnavailable`],
/// [`crate::error_codes::AtmErrorCode::TeamNotFound`],
/// [`crate::error_codes::AtmErrorCode::AgentNotFound`],
/// [`crate::error_codes::AtmErrorCode::AddressParseFailed`],
/// [`crate::error_codes::AtmErrorCode::MailboxReadFailed`],
/// [`crate::error_codes::AtmErrorCode::MailboxWriteFailed`],
/// [`crate::error_codes::AtmErrorCode::MailboxLockFailed`],
/// [`crate::error_codes::AtmErrorCode::MailboxLockTimeout`], or
/// [`crate::error_codes::AtmErrorCode::MessageValidationFailed`] when actor or
/// team resolution fails, the message is missing or no longer pending
/// acknowledgement, reply-target validation fails, or either the source or
/// reply inbox cannot be persisted.
pub fn ack_mail(
    request: AckRequest,
    observability: &dyn ObservabilityPort,
) -> Result<AckOutcome, AtmError> {
    let runtime = default_runtime()?;
    ack_mail_with_runtime(request, observability, &runtime)
}

pub fn ack_mail_with_runtime(
    request: AckRequest,
    observability: &dyn ObservabilityPort,
    runtime: &LocalServiceRuntime,
) -> Result<AckOutcome, AtmError> {
    match crate::write::write_mail_with_runtime(
        request.into_write_request(),
        observability,
        runtime,
    )? {
        WriteOutcome::Acknowledged(outcome) => Ok(outcome),
        WriteOutcome::Sent(_) => Err(AtmError::validation(
            "acknowledgement command produced a non-acknowledgement write outcome",
        )),
    }
}

#[cfg(test)]
mod admission_tests;

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::Map;

    use super::{AckRequest, ReplyTarget};
    use crate::boundary::{Message, MessageKey};
    use crate::caller_context::ActivityObservation;
    use crate::read::{MailboxQueryFilters, ReadQuery};
    use crate::schema::{
        AckIntentFields, AtmMessageId, InboxMessage, authenticated_source_host,
        set_authenticated_source_host, set_peer_delivery_target,
    };
    use crate::send::{SendMessageSource, WriteRequest};
    use crate::types::{
        AgentName, ChatId, HostName, IsoTimestamp, ReadSelection, SessionId, TeamName,
    };
    use crate::write::{
        build_atomic_acknowledgement, canonical_ack_write_request, reply_target_host,
    };

    #[test]
    fn request_json_omits_or_includes_activity_observation() {
        let observation = ActivityObservation {
            team: "local-team".parse().expect("team"),
            member: "local-agent".parse().expect("agent"),
            session_id: Some(SessionId::new("session-17").expect("session")),
            pid: Some(17),
        };
        let write = WriteRequest::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp"),
            "local-agent".parse().expect("agent"),
            "remote@remote-team",
            "local-team".parse().expect("team"),
            SendMessageSource::Inline("body".to_string()),
            None,
            false,
            None,
            false,
        )
        .expect("write");
        assert!(
            serde_json::to_value(&write)
                .expect("json")
                .get("activity_observation")
                .is_none()
        );
        assert_eq!(
            serde_json::to_value(write.with_activity_observation(Some(observation.clone())))
                .expect("json")["activity_observation"]["pid"],
            17
        );
        let query = ReadQuery::from_filters(
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp"),
            "local-agent".parse().expect("agent"),
            "local-team".parse().expect("team"),
            ReadSelection::All,
            false,
            false,
            MailboxQueryFilters::default(),
        )
        .expect("query");
        assert!(
            serde_json::to_value(&query)
                .expect("json")
                .get("activity_observation")
                .is_none()
        );
        assert_eq!(
            serde_json::to_value(query.with_activity_observation(Some(observation))).expect("json")
                ["activity_observation"]["session_id"],
            "session-17"
        );
    }

    #[test]
    fn acknowledgement_round_trip_preserves_activity_observation() {
        let observation = ActivityObservation {
            team: "local-team".parse().expect("team"),
            member: "local-agent".parse().expect("agent"),
            session_id: Some(SessionId::new("session-17").expect("session")),
            pid: Some(17),
        };
        let temp_dir = std::env::temp_dir();
        let request = AckRequest {
            home_dir: temp_dir.clone(),
            current_dir: temp_dir,
            caller_identity: "local-agent".parse().expect("agent"),
            caller_chat_id: None,
            caller_team: "local-team".parse().expect("team"),
            activity_observation: Some(observation.clone()),
            message_id: AtmMessageId::new(),
            reply_body: "ack".to_string(),
        };
        let write = request.clone().into_write_request();
        assert_eq!(write.activity_observation, Some(observation));
        assert_eq!(
            AckRequest::from_unresolved_write(write)
                .expect("round trip")
                .activity_observation,
            request.activity_observation
        );
    }

    #[test]
    fn remote_ack_is_the_canonical_host_qualified_write() {
        let message_id = AtmMessageId::new();
        let mut envelope = InboxMessage {
            from: "remote-agent".parse().expect("agent"),
            source_chat_id: None,
            text: "request acknowledgement".to_string(),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: Some("remote-team".parse().expect("team")),
            destination_chat_id: None,
            summary: None,
            message_id: Some(message_id),
            requires_ack: AckIntentFields::required_pending(IsoTimestamp::now()).requires_ack,
            pending_ack_at: Some(IsoTimestamp::now()),
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: None,
            task_complete: None,
            extra: Map::new(),
        };
        let host: HostName = "peer.example.test".parse().expect("host");
        set_authenticated_source_host(&mut envelope, Some(host.clone()));
        assert_eq!(
            authenticated_source_host(&envelope).expect("stored authenticated host"),
            Some(host.clone())
        );
        let source = Message {
            team: "local-team".parse().expect("team"),
            agent: "local-agent".parse().expect("agent"),
            message_key: MessageKey::new("ack-source").expect("key"),
            envelope,
        };
        let temp_dir = std::env::temp_dir();
        let request = AckRequest {
            home_dir: temp_dir.clone(),
            current_dir: temp_dir,
            caller_identity: "local-agent".parse().expect("agent"),
            caller_chat_id: Some("chat-42".parse::<ChatId>().expect("chat id")),
            caller_team: "local-team".parse().expect("team"),
            activity_observation: None,
            message_id,
            reply_body: "acknowledged".to_string(),
        };
        let target = ReplyTarget::new(
            "remote-agent".parse::<AgentName>().expect("agent"),
            "remote-team".parse::<TeamName>().expect("team"),
            Some(host.clone()),
        );

        let write = canonical_ack_write_request(
            &request,
            &request.caller_identity,
            &request.caller_team,
            &target,
            &source,
        )
        .expect("canonical ack write");
        assert_eq!(write.to.as_ref().expect("destination").host(), Some(&host));
        assert_eq!(write.acknowledges_message_id, Some(message_id));
        assert_eq!(
            write.caller_chat_id.as_ref().map(ChatId::as_str),
            Some("chat-42")
        );
        assert_eq!(
            target.to_string(),
            "remote-agent@remote-team.peer.example.test"
        );

        let acknowledged = build_atomic_acknowledgement(
            write,
            request.caller_identity.clone(),
            request.caller_team.clone(),
            target,
            request.reply_body.clone(),
            message_id,
            None,
        )
        .expect("acknowledgement write");
        let acknowledgement_id = acknowledged
            .reply
            .envelope
            .message_id
            .expect("acknowledgement ULID");
        assert_ne!(
            acknowledgement_id, message_id,
            "the acknowledgement is a new immutable write, not a replay of its send"
        );
        assert_eq!(
            acknowledged.reply.envelope.acknowledges_message_id,
            Some(message_id),
            "the acknowledgement response keeps the exact send ULID it causally acknowledges"
        );
        assert_eq!(
            acknowledged.reply.envelope.extra["peerOutbound"]["host"],
            host.to_string(),
            "the retained direct target is enough for the synchronous router"
        );
        assert!(
            acknowledged.reply.envelope.extra["peerOutbound"]
                .get("request")
                .is_none(),
            "acknowledgements retain no serialized replay payload"
        );
    }

    #[test]
    fn same_store_receipt_uses_retained_origin_host_for_ack_reply() {
        let host: HostName = "192.168.128.82".parse().expect("host");
        let mut envelope = InboxMessage {
            from: "remote-agent".parse().expect("agent"),
            source_chat_id: None,
            text: "request acknowledgement".to_string(),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: Some("remote-team".parse().expect("team")),
            destination_chat_id: None,
            summary: None,
            message_id: Some(AtmMessageId::new()),
            requires_ack: true,
            pending_ack_at: Some(IsoTimestamp::now()),
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: None,
            task_complete: None,
            extra: Map::new(),
        };
        set_peer_delivery_target(&mut envelope, &host);

        assert_eq!(
            reply_target_host(&envelope).expect("reply host"),
            Some(host)
        );
    }
}
