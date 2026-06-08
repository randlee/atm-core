use crate::wire::{ATM_FRAME_FLAGS_V1, FramePayload, MessageKind, RequestId};
#[cfg(test)]
use crate::wire::{RequestEnvelope, ResponseEnvelope};
use atm_storage::AtmError;
use bytes::Bytes;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Canonical RPC transport header for same-host ATM daemon-client traffic.
///
/// The header carries routing metadata only. Body semantics remain encoded in
/// canonical JSON bytes within [`RpcEnvelope::body`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RpcHeader {
    request_id: RequestId,
    message_kind: MessageKind,
    flags: u16,
}

impl RpcHeader {
    /// Build a protocol-v1 header with the only currently supported flag set.
    pub fn new(request_id: RequestId, message_kind: MessageKind) -> Self {
        Self {
            request_id,
            message_kind,
            flags: ATM_FRAME_FLAGS_V1,
        }
    }

    fn from_parts(request_id: RequestId, message_kind: MessageKind, flags: u16) -> Self {
        Self {
            request_id,
            message_kind,
            flags,
        }
    }

    /// Return the request id carried by this transport header.
    pub fn request_id(&self) -> RequestId {
        self.request_id
    }

    /// Return the protocol message kind carried by this transport header.
    pub fn message_kind(&self) -> MessageKind {
        self.message_kind
    }

    /// Return the transport flags from the protocol header.
    pub fn flags(&self) -> u16 {
        self.flags
    }
}

/// Generic same-host RPC envelope.
///
/// `body` stores canonical JSON bytes for shared domain payloads so the same
/// message and roster structs can cross both transport and storage boundaries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcEnvelope {
    pub header: RpcHeader,
    /// Canonical JSON-encoded body bytes for the shared domain payload.
    pub body: Bytes,
}

impl RpcEnvelope {
    /// Build a new transport envelope from an already prepared header and
    /// canonical JSON payload bytes.
    pub fn new(header: RpcHeader, body: Bytes) -> Self {
        Self { header, body }
    }

    /// Convert a framed payload from the daemon protocol into the generic RPC
    /// envelope surface used by same-host clients.
    pub fn from_frame_payload(frame: FramePayload) -> Self {
        Self {
            header: RpcHeader::from_parts(frame.request_id, frame.message_kind, frame.flags),
            body: Bytes::from(frame.bytes),
        }
    }

    /// Convert the generic RPC envelope back into the daemon frame payload.
    pub fn into_frame_payload(self) -> FramePayload {
        FramePayload {
            request_id: self.header.request_id,
            message_kind: self.header.message_kind,
            flags: self.header.flags,
            bytes: self.body.into(),
        }
    }

    /// Encode any serializable canonical domain body into JSON bytes under the
    /// supplied transport header.
    pub fn encode_body<T>(header: RpcHeader, body: &T) -> Result<Self, AtmError>
    where
        T: Serialize,
    {
        let bytes = serde_json::to_vec(body).map_err(AtmError::from)?;
        Ok(Self::new(header, Bytes::from(bytes)))
    }

    /// Decode the canonical JSON body bytes into the requested domain type.
    pub fn decode_body<T>(&self) -> Result<T, AtmError>
    where
        T: DeserializeOwned,
    {
        serde_json::from_slice(self.body.as_ref()).map_err(|error| {
            AtmError::from(error).with_recovery(format!(
                "Decode RpcEnvelope.body as {} from canonical JSON bytes.",
                std::any::type_name::<T>()
            ))
        })
    }

    /// Wrap a request envelope into the generic RPC transport surface.
    #[cfg(test)]
    pub fn encode_request(request: RequestEnvelope) -> Result<Self, AtmError> {
        use crate::wire::{next_request_id, request_to_frame_payload};
        let frame = request_to_frame_payload(next_request_id(), request)?;
        Ok(Self::from_frame_payload(frame))
    }

    /// Decode a generic RPC envelope into the request id plus request payload.
    #[cfg(test)]
    pub fn decode_request(self) -> Result<(RequestId, RequestEnvelope), AtmError> {
        use crate::wire::request_from_frame_payload;
        request_from_frame_payload(self.into_frame_payload())
    }

    /// Wrap a response envelope into the generic RPC transport surface.
    #[cfg(test)]
    pub fn encode_response(
        request_id: RequestId,
        response: ResponseEnvelope,
    ) -> Result<Self, AtmError> {
        use crate::wire::response_to_frame_payload;
        let frame = response_to_frame_payload(request_id, response)?;
        Ok(Self::from_frame_payload(frame))
    }

    /// Decode a generic RPC envelope into the response id plus response
    /// payload.
    #[cfg(test)]
    pub fn decode_response(self) -> Result<(RequestId, ResponseEnvelope), AtmError> {
        use crate::wire::response_from_frame_payload;
        response_from_frame_payload(self.into_frame_payload())
    }
}

#[cfg(test)]
mod tests {
    use super::{MessageKind, RpcEnvelope, RpcHeader};
    use crate::wire::{
        RequestEnvelope, ResponseEnvelope, SendRequestEnvelope, SendResponseEnvelope,
        next_request_id,
    };
    use atm_core::roles::ROLE_TEAM_LEAD;
    use atm_core::send::{SendCommandOutcome, SendMessageSource, SendOutcome, SendRequest};
    use atm_core::test_support::{TEST_QA, TEST_SENDER, TEST_TEAM};
    use atm_storage::{
        AtmMessageId, InboxMessage, IsoTimestamp, Message, MessageKey, ModelName, RosterHarness,
        RosterMember, RosterMemberKind,
    };
    use tempfile::tempdir;

    const TEAM_NAME: &str = TEST_TEAM;
    const ROLE_ARCH_CTM: &str = TEST_SENDER;
    const ROLE_QUALITY_MGR: &str = TEST_QA;

    #[test]
    fn rpc_envelope_round_trips_canonical_message_body() {
        let header = RpcHeader::new(next_request_id(), MessageKind::SendComposeRequest);
        let message = Message {
            team: TEAM_NAME.parse().expect("team"),
            agent: ROLE_QUALITY_MGR.parse().expect("agent"),
            message_key: MessageKey::from(AtmMessageId::new()),
            envelope: InboxMessage {
                from: ROLE_TEAM_LEAD.parse().expect("from"),
                text: "body".to_string(),
                timestamp: IsoTimestamp::now(),
                read: false,
                source_team: Some(TEAM_NAME.parse().expect("source team")),
                summary: Some("body".to_string()),
                message_id: None,
                pending_ack_at: None,
                acknowledged_at: None,
                acknowledges_message_id: None,
                parent_message_id: None,
                thread_mode: None,
                expires_at: None,
                task_id: None,
                extra: Default::default(),
            },
        };

        let envelope = RpcEnvelope::encode_body(header, &message).expect("encode");
        let decoded: Message = envelope.decode_body().expect("decode");

        assert_eq!(decoded, message);
    }

    #[test]
    fn rpc_envelope_round_trips_canonical_roster_member_body() {
        let header = RpcHeader::new(next_request_id(), MessageKind::HeartbeatRequest);
        let roster = RosterMember {
            team_name: TEAM_NAME.parse().expect("team"),
            agent_name: ROLE_ARCH_CTM.parse().expect("agent"),
            member_kind: RosterMemberKind::Permanent,
            harness: RosterHarness::ClaudeCode,
            agent_type: atm_storage::contract::AgentType::Worker,
            model: ModelName::new("gpt-5").expect("model"),
            recipient_pane_id: None,
            metadata_json: Default::default(),
        };

        let envelope = RpcEnvelope::encode_body(header, &roster).expect("encode");
        let decoded: RosterMember = envelope.decode_body().expect("decode");

        assert_eq!(decoded, roster);
    }

    #[test]
    fn rpc_envelope_round_trips_request_envelopes() {
        let temp = tempdir().expect("tempdir");
        let home_dir = temp.path().join("home");
        let current_dir = temp.path().join("cwd");
        let request = RequestEnvelope::Send(SendRequestEnvelope::Compose(SendRequest {
            home_dir: home_dir.clone(),
            current_dir: current_dir.clone(),
            sender_override: Some(ROLE_ARCH_CTM.parse().expect("sender")),
            to: format!("{ROLE_QUALITY_MGR}@{TEAM_NAME}")
                .parse()
                .expect("address"),
            team_override: Some(TEAM_NAME.parse().expect("team")),
            message_source: SendMessageSource::Inline("body".to_string()),
            summary_override: Some("body".to_string()),
            requires_ack: false,
            task_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            dry_run: false,
        }));

        let envelope = RpcEnvelope::encode_request(request.clone()).expect("encode request");
        let (_, decoded) = envelope.decode_request().expect("decode request");

        match decoded {
            RequestEnvelope::Send(SendRequestEnvelope::Compose(decoded)) => {
                assert_eq!(decoded.home_dir, home_dir);
                assert_eq!(decoded.current_dir, current_dir);
                assert_eq!(decoded.summary_override.as_deref(), Some("body"));
                assert_eq!(
                    decoded.to.to_string(),
                    format!("{ROLE_QUALITY_MGR}@{TEAM_NAME}")
                );
            }
            other => panic!("unexpected request envelope: {other:?}"),
        }
    }

    #[test]
    fn rpc_envelope_round_trips_response_envelopes() {
        let response = ResponseEnvelope::Send(SendResponseEnvelope::Sent(SendOutcome {
            action: atm_core::types::CommandAction::Send,
            team: TEAM_NAME.parse().expect("team"),
            agent: ROLE_QUALITY_MGR.parse().expect("agent"),
            sender: ROLE_TEAM_LEAD.parse().expect("sender"),
            outcome: SendCommandOutcome::Sent,
            message_id: AtmMessageId::new(),
            requires_ack: false,
            task_id: None,
            summary: Some("body".to_string()),
            message: Some("body".to_string()),
            warnings: Vec::new(),
            dry_run: false,
        }));

        let request_id = next_request_id();
        let envelope = RpcEnvelope::encode_response(request_id, response.clone()).expect("encode");
        let (decoded_request_id, decoded) = envelope.decode_response().expect("decode");

        assert_eq!(decoded_request_id, request_id);
        match decoded {
            ResponseEnvelope::Send(SendResponseEnvelope::Sent(decoded)) => {
                assert_eq!(decoded.team.to_string(), TEAM_NAME);
                assert_eq!(decoded.agent.to_string(), ROLE_QUALITY_MGR);
                assert_eq!(decoded.sender.to_string(), ROLE_TEAM_LEAD);
                assert_eq!(decoded.summary.as_deref(), Some("body"));
                assert_eq!(decoded.message.as_deref(), Some("body"));
            }
            other => panic!("unexpected response envelope: {other:?}"),
        }
    }
}
