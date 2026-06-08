use atm_core::error::AtmError;
use atm_core::protocol::{
    ATM_FRAME_FLAGS_V1, FramePayload, MessageKind, RequestEnvelope, RequestId, ResponseEnvelope,
    next_request_id, request_from_frame_payload, request_to_frame_payload,
    response_from_frame_payload, response_to_frame_payload,
};
use bytes::Bytes;
use serde::Serialize;
use serde::de::DeserializeOwned;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RpcHeader {
    pub request_id: RequestId,
    pub message_kind: MessageKind,
    pub flags: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcEnvelope {
    pub header: RpcHeader,
    pub body: Bytes,
}

impl RpcEnvelope {
    pub fn new(header: RpcHeader, body: Bytes) -> Self {
        Self { header, body }
    }

    pub fn from_frame_payload(frame: FramePayload) -> Self {
        Self {
            header: RpcHeader {
                request_id: frame.request_id,
                message_kind: frame.message_kind,
                flags: frame.flags,
            },
            body: Bytes::from(frame.bytes),
        }
    }

    pub fn into_frame_payload(self) -> FramePayload {
        FramePayload {
            request_id: self.header.request_id,
            message_kind: self.header.message_kind,
            flags: self.header.flags,
            bytes: self.body.to_vec(),
        }
    }

    pub fn encode_body<T>(header: RpcHeader, body: &T) -> Result<Self, AtmError>
    where
        T: Serialize,
    {
        let bytes = serde_json::to_vec(body).map_err(AtmError::from)?;
        Ok(Self::new(header, Bytes::from(bytes)))
    }

    pub fn decode_body<T>(&self) -> Result<T, AtmError>
    where
        T: DeserializeOwned,
    {
        serde_json::from_slice(self.body.as_ref()).map_err(AtmError::from)
    }

    pub fn encode_request(request: RequestEnvelope) -> Result<Self, AtmError> {
        let frame = request_to_frame_payload(next_request_id(), request)?;
        Ok(Self::from_frame_payload(frame))
    }

    pub fn decode_request(self) -> Result<(RequestId, RequestEnvelope), AtmError> {
        request_from_frame_payload(self.into_frame_payload())
    }

    pub fn encode_response(
        request_id: RequestId,
        response: ResponseEnvelope,
    ) -> Result<Self, AtmError> {
        let frame = response_to_frame_payload(request_id, response)?;
        Ok(Self::from_frame_payload(frame))
    }

    pub fn decode_response(self) -> Result<(RequestId, ResponseEnvelope), AtmError> {
        response_from_frame_payload(self.into_frame_payload())
    }
}

impl Default for RpcHeader {
    fn default() -> Self {
        Self {
            request_id: next_request_id(),
            message_kind: MessageKind::ErrorResponse,
            flags: ATM_FRAME_FLAGS_V1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{RpcEnvelope, RpcHeader};
    use atm_core::protocol::{
        MessageKind, RequestEnvelope, ResponseEnvelope, SendRequestEnvelope, SendResponseEnvelope,
    };
    use atm_core::send::{SendCommandOutcome, SendMessageSource, SendOutcome, SendRequest};
    use atm_storage::{
        AtmMessageId, IsoTimestamp, Message, MessageEnvelope, MessageKey, ModelName, RosterHarness,
        RosterMember, RosterMemberKind,
    };
    use std::path::PathBuf;

    #[test]
    fn rpc_envelope_round_trips_canonical_message_body() {
        let header = RpcHeader {
            request_id: atm_core::protocol::next_request_id(),
            message_kind: MessageKind::SendComposeRequest,
            flags: atm_core::protocol::ATM_FRAME_FLAGS_V1,
        };
        let message = Message {
            team: "atm-dev".parse().expect("team"),
            agent: "quality-mgr".parse().expect("agent"),
            message_key: MessageKey::from(AtmMessageId::new()),
            envelope: MessageEnvelope {
                from: "team-lead".parse().expect("from"),
                text: "body".to_string(),
                timestamp: IsoTimestamp::now(),
                read: false,
                source_team: Some("atm-dev".parse().expect("source team")),
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
        let header = RpcHeader {
            request_id: atm_core::protocol::next_request_id(),
            message_kind: MessageKind::HeartbeatRequest,
            flags: atm_core::protocol::ATM_FRAME_FLAGS_V1,
        };
        let roster = RosterMember {
            team_name: "atm-dev".parse().expect("team"),
            agent_name: "arch-ctm".parse().expect("agent"),
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
        let request = RequestEnvelope::Send(SendRequestEnvelope::Compose(SendRequest {
            home_dir: PathBuf::from("/tmp/home"),
            current_dir: PathBuf::from("/tmp/cwd"),
            sender_override: Some("arch-ctm".parse().expect("sender")),
            to: "quality-mgr@atm-dev".parse().expect("address"),
            team_override: Some("atm-dev".parse().expect("team")),
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
                assert_eq!(decoded.home_dir, PathBuf::from("/tmp/home"));
                assert_eq!(decoded.current_dir, PathBuf::from("/tmp/cwd"));
                assert_eq!(decoded.summary_override.as_deref(), Some("body"));
                assert_eq!(decoded.to.to_string(), "quality-mgr@atm-dev");
            }
            other => panic!("unexpected request envelope: {other:?}"),
        }
    }

    #[test]
    fn rpc_envelope_round_trips_response_envelopes() {
        let response = ResponseEnvelope::Send(SendResponseEnvelope::Sent(SendOutcome {
            action: atm_core::types::CommandAction::Send,
            team: "atm-dev".parse().expect("team"),
            agent: "quality-mgr".parse().expect("agent"),
            sender: "team-lead".parse().expect("sender"),
            outcome: SendCommandOutcome::Sent,
            message_id: AtmMessageId::new(),
            requires_ack: false,
            task_id: None,
            summary: Some("body".to_string()),
            message: Some("body".to_string()),
            warnings: Vec::new(),
            dry_run: false,
        }));

        let request_id = atm_core::protocol::next_request_id();
        let envelope = RpcEnvelope::encode_response(request_id, response.clone()).expect("encode");
        let (decoded_request_id, decoded) = envelope.decode_response().expect("decode");

        assert_eq!(decoded_request_id, request_id);
        match decoded {
            ResponseEnvelope::Send(SendResponseEnvelope::Sent(decoded)) => {
                assert_eq!(decoded.team.to_string(), "atm-dev");
                assert_eq!(decoded.agent.to_string(), "quality-mgr");
                assert_eq!(decoded.sender.to_string(), "team-lead");
                assert_eq!(decoded.summary.as_deref(), Some("body"));
                assert_eq!(decoded.message.as_deref(), Some("body"));
            }
            other => panic!("unexpected response envelope: {other:?}"),
        }
    }
}
