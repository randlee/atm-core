use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::types::IsoTimestamp;

/// Persisted inbox superset used by ATM.
///
/// Native Claude Code message shape is owned externally and documented in
/// `docs/claude-code-message-schema.md`. Do not repurpose or rename Claude-owned
/// fields in this struct. ATM may only add additive fields documented in
/// `docs/atm-message-schema.md`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MessageEnvelope {
    // Claude Code-native fields. Do not change these as if ATM owned the
    // native schema; update the owning schema docs first if the external
    // contract changes.
    pub from: String,
    pub text: String,
    pub timestamp: IsoTimestamp,
    pub read: bool,

    // ATM additive fields layered on top of the native Claude Code message
    // schema. Historical provenance analysis in this design sprint confirmed
    // these persisted fields are ATM-added rather than Claude-native.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_team: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<Uuid>,

    #[serde(rename = "pendingAckAt", skip_serializing_if = "Option::is_none")]
    pub pending_ack_at: Option<IsoTimestamp>,

    #[serde(rename = "acknowledgedAt", skip_serializing_if = "Option::is_none")]
    pub acknowledged_at: Option<IsoTimestamp>,

    #[serde(
        rename = "acknowledgesMessageId",
        skip_serializing_if = "Option::is_none"
    )]
    pub acknowledges_message_id: Option<Uuid>,

    #[serde(rename = "taskId", skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,

    // Preserve unknown producer-owned fields so ATM does not accidentally
    // redefine external schemas by dropping or rewriting them.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingAck {
    pub message_id: Uuid,
    pub from: String,
    pub acked: bool,
    pub acked_at: Option<IsoTimestamp>,
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde_json::{json, Map};

    use chrono::Utc;

    use super::{IsoTimestamp, MessageEnvelope, PendingAck, Uuid};

    #[test]
    fn message_envelope_round_trips_with_current_inbox_shape() {
        let envelope = MessageEnvelope {
            from: "arch-ctm".into(),
            text: "hello".into(),
            timestamp: IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
                    .single()
                    .expect("timestamp"),
            ),
            read: false,
            source_team: Some("atm-dev".into()),
            summary: Some("hello".into()),
            message_id: Some(Uuid::new_v4()),
            pending_ack_at: Some(IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 1)
                    .single()
                    .expect("timestamp"),
            )),
            acknowledged_at: None,
            acknowledges_message_id: None,
            task_id: Some("TASK-123".into()),
            extra: Map::new(),
        };

        let encoded = serde_json::to_string(&envelope).expect("encode");
        let decoded: MessageEnvelope = serde_json::from_str(&encoded).expect("decode");

        assert_eq!(decoded, envelope);
    }

    #[test]
    fn unknown_fields_are_preserved() {
        let json = json!({
            "from": "team-lead",
            "text": "hello",
            "timestamp": "2026-03-30T00:00:00Z",
            "read": false,
            "futureField": {"nested": true}
        });

        let decoded: MessageEnvelope = serde_json::from_value(json).expect("decode");
        assert_eq!(decoded.extra["futureField"], json!({"nested": true}));

        let reencoded = serde_json::to_value(&decoded).expect("encode");
        assert_eq!(reencoded["futureField"], json!({"nested": true}));
    }

    #[test]
    fn message_id_is_optional() {
        let json = json!({
            "from": "team-lead",
            "text": "hello",
            "timestamp": "2026-03-30T00:00:00Z",
            "read": false
        });

        let decoded: MessageEnvelope = serde_json::from_value(json).expect("decode");
        assert!(decoded.message_id.is_none());
        assert!(decoded.task_id.is_none());
    }

    #[test]
    fn pending_ack_round_trips() {
        let pending_ack = PendingAck {
            message_id: Uuid::new_v4(),
            from: "team-lead".into(),
            acked: true,
            acked_at: Some(IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 1)
                    .single()
                    .expect("timestamp"),
            )),
        };

        let encoded = serde_json::to_string(&pending_ack).expect("encode");
        let decoded: PendingAck = serde_json::from_str(&encoded).expect("decode");

        assert_eq!(decoded, pending_ack);
    }
}
