//! Shared inbox compatibility schema for Claude-native envelopes with ATM additive compatibility fields.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fmt;
use ulid::Ulid;
use uuid::Uuid;

use crate::config::types::{ByteCount, DEFAULT_CLAUDE_JSONL_BODY_EXPORT_MAX_BYTES};
use crate::error::AtmError;
use crate::types::{AgentName, IsoTimestamp, TaskId, TeamName};

#[derive(Debug, Clone)]
pub struct AtmMessageIdParseError(String);

impl fmt::Display for AtmMessageIdParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AtmMessageIdParseError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
/// ATM-owned logical message identifier.
pub struct AtmMessageId(Ulid);

impl AtmMessageId {
    pub fn new() -> Self {
        Self(Ulid::new())
    }

    pub fn from_uuid_wire(value: Uuid) -> Self {
        Self(Ulid::from_bytes(value.into_bytes()))
    }

    pub fn into_uuid_wire(self) -> Uuid {
        Uuid::from_bytes(self.0.to_bytes())
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

impl From<Uuid> for AtmMessageId {
    fn from(value: Uuid) -> Self {
        Self::from_uuid_wire(value)
    }
}

impl From<AtmMessageId> for Ulid {
    fn from(value: AtmMessageId) -> Self {
        value.0
    }
}

impl From<AtmMessageId> for Uuid {
    fn from(value: AtmMessageId) -> Self {
        value.into_uuid_wire()
    }
}

impl std::str::FromStr for AtmMessageId {
    type Err = AtmMessageIdParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ulid::from_string(s)
            .map(Self)
            .or_else(|_| Uuid::parse_str(s).map(Self::from_uuid_wire))
            .map_err(|error| AtmMessageIdParseError(error.to_string()))
    }
}

impl fmt::Display for AtmMessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

mod atm_message_id_uuid_wire {
    use super::AtmMessageId;
    use serde::{Deserialize, Deserializer, Serializer};
    use uuid::Uuid;

    pub fn serialize<S>(value: &AtmMessageId, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.into_uuid_wire().to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<AtmMessageId, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Uuid::parse_str(&raw)
            .map(AtmMessageId::from_uuid_wire)
            .map_err(serde::de::Error::custom)
    }
}

mod option_atm_message_id_uuid_wire {
    use super::AtmMessageId;
    use serde::{Deserialize, Deserializer, Serializer};
    use uuid::Uuid;

    pub fn serialize<S>(value: &Option<AtmMessageId>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(value) => serializer.serialize_some(&value.into_uuid_wire().to_string()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<AtmMessageId>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::<String>::deserialize(deserializer)?
            .map(|raw| {
                let uuid = Uuid::parse_str(&raw).map_err(serde::de::Error::custom)?;
                Ok(AtmMessageId::from_uuid_wire(uuid))
            })
            .transpose()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThreadMode {
    AddDetails,
    Supersede,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ATM-owned semantic discriminator for alert-class metadata.
pub enum AlertKind {
    MissingTeamConfig,
    Unknown(String),
}

impl AlertKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::MissingTeamConfig => "missing_team_config",
            Self::Unknown(value) => value,
        }
    }
}

impl From<String> for AlertKind {
    fn from(value: String) -> Self {
        match value.as_str() {
            "missing_team_config" => Self::MissingTeamConfig,
            _ => Self::Unknown(value),
        }
    }
}

impl From<AlertKind> for String {
    fn from(value: AlertKind) -> Self {
        match value {
            AlertKind::MissingTeamConfig => "missing_team_config".to_string(),
            AlertKind::Unknown(value) => value,
        }
    }
}

impl Serialize for AlertKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AlertKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Self::from(String::deserialize(deserializer)?))
    }
}

/// Persisted inbox superset used by ATM.
///
/// Native Claude Code message shape is owned externally and documented in
/// `docs/claude-code-message-schema.md`. Do not repurpose or rename Claude-owned
/// fields in this struct. Historical top-level ATM additions are documented in
/// `docs/legacy-atm-message-schema.md`, and the approved additive compatibility
/// fields are documented in `docs/atm-message-schema.md`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MessageEnvelope {
    // Claude Code-native fields. Do not change these as if ATM owned the
    // native schema; update the owning schema docs first if the external
    // contract changes.
    pub from: AgentName,
    pub text: String,
    pub timestamp: IsoTimestamp,
    pub read: bool,

    // Legacy ATM additive fields layered on top of the native Claude Code
    // message schema. Historical provenance analysis in this design sprint
    // confirmed these persisted fields are ATM-added rather than Claude-native.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_team: Option<TeamName>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(with = "option_atm_message_id_uuid_wire")]
    pub message_id: Option<AtmMessageId>,

    #[serde(rename = "pendingAckAt", skip_serializing_if = "Option::is_none")]
    pub pending_ack_at: Option<IsoTimestamp>,

    #[serde(rename = "acknowledgedAt", skip_serializing_if = "Option::is_none")]
    pub acknowledged_at: Option<IsoTimestamp>,

    #[serde(
        rename = "acknowledgesMessageId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[serde(with = "option_atm_message_id_uuid_wire")]
    pub acknowledges_message_id: Option<AtmMessageId>,

    #[serde(
        rename = "parentMessageId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[serde(with = "option_atm_message_id_uuid_wire")]
    pub parent_message_id: Option<AtmMessageId>,

    #[serde(rename = "threadMode", skip_serializing_if = "Option::is_none")]
    pub thread_mode: Option<ThreadMode>,

    #[serde(rename = "staleAt", skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<IsoTimestamp>,

    #[serde(rename = "taskId", skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,

    // Preserve unknown producer-owned fields so ATM does not accidentally
    // redefine external schemas by dropping or rewriting them.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingAck {
    #[serde(with = "atm_message_id_uuid_wire")]
    pub message_id: AtmMessageId,
    pub from: AgentName,
    pub acked: bool,
    pub acked_at: Option<IsoTimestamp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SharedInboxExportPolicy {
    pub(crate) atm_authored_body_export_max_bytes: ByteCount,
}

impl Default for SharedInboxExportPolicy {
    fn default() -> Self {
        Self {
            atm_authored_body_export_max_bytes: ByteCount::new(
                DEFAULT_CLAUDE_JSONL_BODY_EXPORT_MAX_BYTES,
            ),
        }
    }
}

#[cfg(test)]
pub(crate) fn to_shared_inbox_value(message: &MessageEnvelope) -> Result<Value, AtmError> {
    to_shared_inbox_value_with_policy(message, SharedInboxExportPolicy::default())
}

fn strip_metadata_atm_namespace(object: &mut Map<String, Value>) {
    let Some(metadata) = object.get_mut("metadata").and_then(Value::as_object_mut) else {
        return;
    };
    metadata.remove("atm");
    if metadata.is_empty() {
        object.remove("metadata");
    }
}

pub(crate) fn to_shared_inbox_value_with_policy(
    message: &MessageEnvelope,
    policy: SharedInboxExportPolicy,
) -> Result<Value, AtmError> {
    let mut value = serde_json::to_value(message).map_err(|error| {
        AtmError::mailbox_write(format!(
            "failed to serialize shared inbox envelope for {} at {:?}: {error}",
            message.from, message.timestamp
        ))
        .with_source(error)
    })?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| {
            AtmError::mailbox_write(format!(
                "failed to serialize shared inbox envelope for {} at {:?}: envelope did not encode as a JSON object",
                message.from, message.timestamp
            ))
            .with_recovery(
                "Preserve the ATM shared-inbox envelope shape so serialization produces one JSON object per message before retrying mailbox export.",
            )
        })?;
    strip_metadata_atm_namespace(object);
    if should_export_retrieval_stub(message, policy)? {
        let retrieval_stub = retrieval_stub_text(message)?;
        object.insert("text".to_string(), Value::String(retrieval_stub));
    }
    Ok(value)
}

fn should_export_retrieval_stub(
    message: &MessageEnvelope,
    policy: SharedInboxExportPolicy,
) -> Result<bool, AtmError> {
    let export_cap = policy
        .atm_authored_body_export_max_bytes
        .as_usize()
        .ok_or_else(|| {
            AtmError::mailbox_write(format!(
                "failed to compare ATM-authored export cap for {} at {:?}: configured byte cap does not fit into usize",
                message.from, message.timestamp
            ))
            .with_recovery(
                "Lower [atm].claude_jsonl_body_export_max_bytes to a bounded value before retrying mailbox export.",
            )
        })?;

    Ok(message.message_id.is_some()
        && (policy.atm_authored_body_export_max_bytes.is_zero() || message.text.len() > export_cap))
}

fn retrieval_stub_text(message: &MessageEnvelope) -> Result<String, AtmError> {
    let Some(message_id) = message.message_id else {
        return Err(AtmError::mailbox_write(format!(
            "failed to project shared inbox retrieval stub for {} at {:?}: ATM-authored message is missing message_id",
            message.from, message.timestamp
        ))
        .with_recovery(
            "Ensure ATM-authored messages retain message_id so the retrieval stub can reference the shared compatibility message id.",
        ));
    };
    Ok(format!("atm read --message-id {message_id}"))
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde_json::{Map, Value, json};

    use chrono::Utc;

    use super::{
        AlertKind, AtmMessageId, IsoTimestamp, MessageEnvelope, PendingAck,
        SharedInboxExportPolicy, to_shared_inbox_value, to_shared_inbox_value_with_policy,
    };
    use crate::config::types::ByteCount;
    use crate::roles::ROLE_TEAM_LEAD;
    use crate::test_support::{TEST_SENDER, TEST_TEAM};

    #[test]
    fn alert_kind_round_trips_known_and_unknown_wire_values() {
        let known: AlertKind =
            serde_json::from_str(r#""missing_team_config""#).expect("known alert kind");
        assert_eq!(known, AlertKind::MissingTeamConfig);
        assert_eq!(
            serde_json::to_string(&known).expect("encode known"),
            r#""missing_team_config""#
        );

        let unknown: AlertKind =
            serde_json::from_str(r#""future_alert_kind""#).expect("unknown alert kind");
        assert_eq!(unknown, AlertKind::Unknown("future_alert_kind".to_string()));
        assert_eq!(
            serde_json::to_string(&unknown).expect("encode unknown"),
            r#""future_alert_kind""#
        );
    }

    #[test]
    fn message_envelope_round_trips_with_current_inbox_shape() {
        // Validates the current ATM superset storage shape, not the
        // Claude-native schema. Ownership is documented in
        // docs/legacy-atm-message-schema.md and docs/atm-message-schema.md.
        let envelope = MessageEnvelope {
            from: TEST_SENDER.parse().expect("agent"),
            text: "hello".into(),
            timestamp: IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
                    .single()
                    .expect("timestamp"),
            ),
            read: false,
            source_team: Some(TEST_TEAM.parse().expect("team")),
            summary: Some("hello".into()),
            message_id: Some(AtmMessageId::new()),
            pending_ack_at: Some(IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 1)
                    .single()
                    .expect("timestamp"),
            )),
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: Some("TASK-123".parse().expect("task id")),
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
            "from": ROLE_TEAM_LEAD,
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
            "from": ROLE_TEAM_LEAD,
            "text": "hello",
            "timestamp": "2026-03-30T00:00:00Z",
            "read": false
        });

        let decoded: MessageEnvelope = serde_json::from_value(json).expect("decode");
        assert!(decoded.message_id.is_none());
        assert!(decoded.task_id.is_none());
    }

    #[test]
    fn blank_task_id_is_rejected() {
        let json = json!({
            "from": ROLE_TEAM_LEAD,
            "text": "hello",
            "timestamp": "2026-03-30T00:00:00Z",
            "read": false,
            "taskId": "   "
        });

        let error = serde_json::from_value::<MessageEnvelope>(json).expect_err("blank task id");

        assert!(error.to_string().contains("task id must not be blank"));
    }

    #[test]
    fn pending_ack_round_trips() {
        let pending_ack = PendingAck {
            message_id: AtmMessageId::new(),
            from: ROLE_TEAM_LEAD.parse().expect("agent"),
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
    fn atm_message_id_timestamp_matches_derived_timestamp() {
        let (message_id, timestamp) = AtmMessageId::new_with_timestamp();
        assert_eq!(message_id.timestamp(), timestamp);
    }

    #[test]
    fn atm_message_id_parses_from_uuid_wire_string() {
        let parsed: AtmMessageId = "11111111-1111-4111-8111-111111111111"
            .parse()
            .expect("parse uuid wire id");
        assert_eq!(
            parsed.into_uuid_wire().to_string(),
            "11111111-1111-4111-8111-111111111111"
        );
    }

    #[test]
    fn atm_message_id_parses_from_ulid_string() {
        let (message_id, _) = AtmMessageId::new_with_timestamp();
        let parsed: AtmMessageId = message_id.to_string().parse().expect("parse atm id");
        assert_eq!(parsed, message_id);
    }

    #[test]
    fn atm_message_id_uuid_wire_round_trip_preserves_identity() {
        let message_id: AtmMessageId = "01KRFK5QTF2R6NRS3Q0F8Z9K0S".parse().expect("parse atm id");

        let round_trip = AtmMessageId::from_uuid_wire(message_id.into_uuid_wire());

        assert_eq!(round_trip, message_id);
    }

    #[test]
    fn shared_inbox_write_keeps_machine_fields_top_level() {
        let envelope = MessageEnvelope {
            from: TEST_SENDER.parse().expect("agent"),
            text: "hello".into(),
            timestamp: IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
                    .single()
                    .expect("timestamp"),
            ),
            read: false,
            source_team: Some(TEST_TEAM.parse().expect("team")),
            summary: Some("hello".into()),
            message_id: Some(AtmMessageId::new()),
            pending_ack_at: Some(IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 1)
                    .single()
                    .expect("timestamp"),
            )),
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: Some("TASK-123".parse().expect("task id")),
            extra: Map::new(),
        };

        let encoded = to_shared_inbox_value(&envelope).expect("encode");
        let object = encoded.as_object().expect("object");
        assert!(object.contains_key("message_id"));
        assert!(object.contains_key("source_team"));
        assert!(object.contains_key("pendingAckAt"));
        assert!(object.contains_key("taskId"));
        assert!(
            object
                .get("metadata")
                .and_then(Value::as_object)
                .and_then(|metadata| metadata.get("atm"))
                .is_none()
        );
    }

    #[test]
    fn shared_inbox_write_keeps_ack_fields_top_level() {
        let acknowledged_at = IsoTimestamp::from_datetime(
            Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 2)
                .single()
                .expect("timestamp"),
        );
        let envelope = MessageEnvelope {
            from: TEST_SENDER.parse().expect("agent"),
            text: "ack reply".into(),
            timestamp: IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
                    .single()
                    .expect("timestamp"),
            ),
            read: false,
            source_team: Some(TEST_TEAM.parse().expect("team")),
            summary: Some("ack reply".into()),
            message_id: Some(AtmMessageId::new()),
            pending_ack_at: None,
            acknowledged_at: Some(acknowledged_at),
            acknowledges_message_id: Some(AtmMessageId::new()),
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: None,
            extra: Map::new(),
        };

        let encoded = to_shared_inbox_value(&envelope).expect("encode");
        let object = encoded.as_object().expect("object");
        assert_eq!(
            object.get("acknowledgedAt"),
            Some(&json!("2026-03-30T00:00:02Z"))
        );
        assert!(object["acknowledgesMessageId"].as_str().is_some());
    }

    #[test]
    fn shared_inbox_write_stubs_oversized_atm_authored_messages() {
        let message_id = AtmMessageId::new();
        let envelope = MessageEnvelope {
            from: TEST_SENDER.parse().expect("agent"),
            text: "x".repeat(32),
            timestamp: IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
                    .single()
                    .expect("timestamp"),
            ),
            read: false,
            source_team: Some(TEST_TEAM.parse().expect("team")),
            summary: Some("oversized".into()),
            message_id: Some(message_id),
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: None,
            extra: Map::new(),
        };

        let encoded = to_shared_inbox_value_with_policy(
            &envelope,
            SharedInboxExportPolicy {
                atm_authored_body_export_max_bytes: ByteCount::new(8),
            },
        )
        .expect("encode");

        assert_eq!(
            encoded["text"],
            json!(format!("atm read --message-id {message_id}"))
        );
        assert_eq!(encoded["summary"], json!("oversized"));
        assert_eq!(
            encoded["message_id"],
            json!(message_id.into_uuid_wire().to_string())
        );
    }

    #[test]
    fn shared_inbox_write_exports_full_body_at_exact_cap() {
        let message_id = AtmMessageId::new();
        let text = "x".repeat(32);
        let envelope = MessageEnvelope {
            from: TEST_SENDER.parse().expect("agent"),
            text: text.clone(),
            timestamp: IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
                    .single()
                    .expect("timestamp"),
            ),
            read: false,
            source_team: Some(TEST_TEAM.parse().expect("team")),
            summary: Some("exact-cap".into()),
            message_id: Some(message_id),
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: None,
            extra: Map::new(),
        };

        let encoded = to_shared_inbox_value_with_policy(
            &envelope,
            SharedInboxExportPolicy {
                atm_authored_body_export_max_bytes: ByteCount::new(text.len() as u64),
            },
        )
        .expect("encode");

        assert_eq!(encoded["text"], json!(text));
    }

    #[test]
    fn shared_inbox_write_exports_stub_above_cap() {
        let message_id = AtmMessageId::new();
        let text = "x".repeat(32);
        let envelope = MessageEnvelope {
            from: TEST_SENDER.parse().expect("agent"),
            text,
            timestamp: IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
                    .single()
                    .expect("timestamp"),
            ),
            read: false,
            source_team: Some(TEST_TEAM.parse().expect("team")),
            summary: Some("above-cap".into()),
            message_id: Some(message_id),
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: None,
            extra: Map::new(),
        };

        let encoded = to_shared_inbox_value_with_policy(
            &envelope,
            SharedInboxExportPolicy {
                atm_authored_body_export_max_bytes: ByteCount::new(31),
            },
        )
        .expect("encode");

        assert_eq!(
            encoded["text"],
            json!(format!("atm read --message-id {message_id}"))
        );
    }

    #[test]
    fn shared_inbox_write_keeps_full_body_for_claude_native_messages() {
        let envelope = MessageEnvelope {
            from: TEST_SENDER.parse().expect("agent"),
            text: "x".repeat(32),
            timestamp: IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
                    .single()
                    .expect("timestamp"),
            ),
            read: false,
            source_team: None,
            summary: Some("native".into()),
            message_id: None,
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: None,
            extra: Map::new(),
        };

        let encoded = to_shared_inbox_value_with_policy(
            &envelope,
            SharedInboxExportPolicy {
                atm_authored_body_export_max_bytes: ByteCount::new(8),
            },
        )
        .expect("encode");

        assert_eq!(encoded["text"], json!("x".repeat(32)));
    }

    #[test]
    fn shared_inbox_write_strips_metadata_atm_namespace() {
        let mut extra = Map::new();
        extra.insert(
            "metadata".to_string(),
            json!({
                "atm": { "messageId": "01JQYVB6W51Q2E7E6T3Y4Q9N2M" },
                "foreign": { "keep": true }
            }),
        );
        let envelope = MessageEnvelope {
            from: TEST_SENDER.parse().expect("agent"),
            text: "hello".into(),
            timestamp: IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
                    .single()
                    .expect("timestamp"),
            ),
            read: false,
            source_team: None,
            summary: None,
            message_id: Some(AtmMessageId::new()),
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: None,
            extra,
        };

        let encoded = to_shared_inbox_value(&envelope).expect("encode");
        assert!(
            encoded["metadata"]["foreign"]["keep"]
                .as_bool()
                .unwrap_or(false)
        );
        assert!(encoded["metadata"]["atm"].is_null());
    }
}
