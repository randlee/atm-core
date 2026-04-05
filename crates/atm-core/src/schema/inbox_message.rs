use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use ulid::Ulid;
use uuid::Uuid;

use crate::types::IsoTimestamp;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
/// UUID-based compatibility identifier for legacy top-level ATM `message_id`.
pub struct LegacyMessageId(Uuid);

impl LegacyMessageId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn into_uuid(self) -> Uuid {
        self.0
    }
}

impl Default for LegacyMessageId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<Uuid> for LegacyMessageId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl From<LegacyMessageId> for Uuid {
    fn from(value: LegacyMessageId) -> Self {
        value.0
    }
}

impl std::str::FromStr for LegacyMessageId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(s).map(Self)
    }
}

impl fmt::Display for LegacyMessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
/// ULID-based forward ATM identifier for `metadata.atm.messageId`.
pub struct AtmMessageId(Ulid);

impl AtmMessageId {
    pub fn new() -> Self {
        Self(Ulid::new())
    }

    pub fn into_ulid(self) -> Ulid {
        self.0
    }

    pub fn timestamp(self) -> IsoTimestamp {
        let datetime: DateTime<Utc> = self.0.datetime().into();
        IsoTimestamp::from_datetime(datetime)
    }

    pub fn new_with_timestamp() -> (Self, IsoTimestamp) {
        let message_id = Self::new();
        let timestamp = message_id.timestamp();
        (message_id, timestamp)
    }
}

impl Default for AtmMessageId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<Ulid> for AtmMessageId {
    fn from(value: Ulid) -> Self {
        Self(value)
    }
}

impl From<AtmMessageId> for Ulid {
    fn from(value: AtmMessageId) -> Self {
        value.0
    }
}

impl std::str::FromStr for AtmMessageId {
    type Err = ulid::DecodeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ulid::from_string(s).map(Self)
    }
}

impl fmt::Display for AtmMessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
/// ATM-owned machine metadata planned for the forward `metadata.atm` namespace.
pub struct AtmMetadataFields {
    #[serde(rename = "messageId", skip_serializing_if = "Option::is_none")]
    pub message_id: Option<AtmMessageId>,

    #[serde(rename = "sourceTeam", skip_serializing_if = "Option::is_none")]
    pub source_team: Option<String>,

    #[serde(rename = "pendingAckAt", skip_serializing_if = "Option::is_none")]
    pub pending_ack_at: Option<IsoTimestamp>,

    #[serde(rename = "acknowledgedAt", skip_serializing_if = "Option::is_none")]
    pub acknowledged_at: Option<IsoTimestamp>,

    #[serde(
        rename = "acknowledgesMessageId",
        skip_serializing_if = "Option::is_none"
    )]
    pub acknowledges_message_id: Option<AtmMessageId>,

    #[serde(rename = "alertKind", skip_serializing_if = "Option::is_none")]
    pub alert_kind: Option<String>,

    #[serde(rename = "missingConfigPath", skip_serializing_if = "Option::is_none")]
    pub missing_config_path: Option<String>,

    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
/// Top-level metadata container preserving ATM-owned and foreign metadata keys.
pub struct MessageMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub atm: Option<AtmMetadataFields>,

    // Preserve unknown producer-owned fields so ATM does not accidentally
    // redefine external schemas by dropping or rewriting them.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Minimal forward-schema projection used to validate metadata/timestamp rules.
pub struct ForwardMetadataEnvelope {
    pub timestamp: IsoTimestamp,
    pub metadata: MessageMetadata,
}

/// Persisted inbox superset used by ATM.
///
/// Native Claude Code message shape is owned externally and documented in
/// `docs/claude-code-message-schema.md`. Do not repurpose or rename Claude-owned
/// fields in this struct. Historical top-level ATM additions are documented in
/// `docs/legacy-atm-message-schema.md`, and forward ATM machine metadata is
/// documented in `docs/atm-message-schema.md`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MessageEnvelope {
    // Claude Code-native fields. Do not change these as if ATM owned the
    // native schema; update the owning schema docs first if the external
    // contract changes.
    pub from: String,
    pub text: String,
    pub timestamp: IsoTimestamp,
    pub read: bool,

    // Legacy ATM additive fields layered on top of the native Claude Code
    // message schema. Historical provenance analysis in this design sprint
    // confirmed these persisted fields are ATM-added rather than Claude-native.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_team: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<LegacyMessageId>,

    #[serde(rename = "pendingAckAt", skip_serializing_if = "Option::is_none")]
    pub pending_ack_at: Option<IsoTimestamp>,

    #[serde(rename = "acknowledgedAt", skip_serializing_if = "Option::is_none")]
    pub acknowledged_at: Option<IsoTimestamp>,

    #[serde(
        rename = "acknowledgesMessageId",
        skip_serializing_if = "Option::is_none"
    )]
    pub acknowledges_message_id: Option<LegacyMessageId>,

    #[serde(rename = "taskId", skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,

    // Preserve unknown producer-owned fields so ATM does not accidentally
    // redefine external schemas by dropping or rewriting them.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingAck {
    pub message_id: LegacyMessageId,
    pub from: String,
    pub acked: bool,
    pub acked_at: Option<IsoTimestamp>,
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde_json::{Map, json};

    use chrono::Utc;

    use super::{
        AtmMessageId, AtmMetadataFields, ForwardMetadataEnvelope, IsoTimestamp, LegacyMessageId,
        MessageEnvelope, MessageMetadata, PendingAck,
    };

    #[test]
    fn message_envelope_round_trips_with_current_inbox_shape() {
        // Validates the current ATM superset storage shape, not the
        // Claude-native schema. Ownership is documented in
        // docs/legacy-atm-message-schema.md and docs/atm-message-schema.md.
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
            message_id: Some(LegacyMessageId::new()),
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
        // Preserving producer-owned fields prevents ATM from silently
        // redefining external schemas documented in
        // docs/claude-code-message-schema.md.
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
            message_id: LegacyMessageId::new(),
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

    #[test]
    fn forward_metadata_envelope_uses_atm_message_id() {
        let (message_id, timestamp) = AtmMessageId::new_with_timestamp();
        let envelope = ForwardMetadataEnvelope {
            timestamp,
            metadata: MessageMetadata {
                atm: Some(AtmMetadataFields {
                    message_id: Some(message_id),
                    source_team: Some("atm-dev".into()),
                    pending_ack_at: None,
                    acknowledged_at: None,
                    acknowledges_message_id: None,
                    alert_kind: None,
                    missing_config_path: None,
                    extra: Map::new(),
                }),
                extra: Map::new(),
            },
        };

        let encoded = serde_json::to_string(&envelope).expect("encode");
        let decoded: ForwardMetadataEnvelope = serde_json::from_str(&encoded).expect("decode");
        assert_eq!(decoded, envelope);
    }

    #[test]
    fn atm_message_id_timestamp_matches_derived_timestamp() {
        let (message_id, timestamp) = AtmMessageId::new_with_timestamp();
        assert_eq!(message_id.timestamp(), timestamp);
    }

    #[test]
    fn legacy_message_id_parses_from_uuid_string() {
        let parsed: LegacyMessageId = "11111111-1111-4111-8111-111111111111"
            .parse()
            .expect("parse legacy id");
        assert_eq!(parsed.to_string(), "11111111-1111-4111-8111-111111111111");
    }

    #[test]
    fn atm_message_id_parses_from_ulid_string() {
        let (message_id, _) = AtmMessageId::new_with_timestamp();
        let parsed: AtmMessageId = message_id.to_string().parse().expect("parse atm id");
        assert_eq!(parsed, message_id);
    }
}
