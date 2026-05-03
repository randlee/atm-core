//! Shared inbox compatibility schema for Claude-native envelopes plus ATM metadata.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fmt;
use tracing::warn;
use ulid::Ulid;
use uuid::Uuid;

use crate::error::AtmError;
use crate::types::{AgentName, IsoTimestamp, TaskId, TeamName};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
/// UUID-based compatibility identifier for legacy top-level ATM `message_id`.
pub struct LegacyMessageId(Uuid);

impl LegacyMessageId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn from_atm_message_id(value: AtmMessageId) -> Self {
        Self(Uuid::from_bytes(value.into_ulid().to_bytes()))
    }

    pub fn into_uuid(self) -> Uuid {
        self.0
    }

    /// Reinterpret the raw UUID bytes as an ATM message ULID without mutation.
    pub fn into_atm_message_id(self) -> AtmMessageId {
        AtmMessageId::from(Ulid::from_bytes(self.0.into_bytes()))
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
/// ATM-owned semantic discriminator for alert-class metadata.
pub struct AlertKind(String);

impl AlertKind {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
/// ATM-owned machine metadata planned for the forward `metadata.atm` namespace.
pub struct AtmMetadataFields {
    #[serde(rename = "messageId", skip_serializing_if = "Option::is_none")]
    pub message_id: Option<AtmMessageId>,

    #[serde(rename = "sourceTeam", skip_serializing_if = "Option::is_none")]
    pub source_team: Option<TeamName>,

    #[serde(rename = "fromIdentity", skip_serializing_if = "Option::is_none")]
    pub from_identity: Option<AgentName>,

    #[serde(rename = "pendingAckAt", skip_serializing_if = "Option::is_none")]
    pub pending_ack_at: Option<IsoTimestamp>,

    #[serde(rename = "acknowledgedAt", skip_serializing_if = "Option::is_none")]
    pub acknowledged_at: Option<IsoTimestamp>,

    #[serde(
        rename = "acknowledgesMessageId",
        skip_serializing_if = "Option::is_none"
    )]
    pub acknowledges_message_id: Option<AtmMessageId>,

    #[serde(rename = "taskId", skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,

    #[serde(rename = "alertKind", skip_serializing_if = "Option::is_none")]
    pub alert_kind: Option<AlertKind>,

    // This advisory diagnostic field preserves platform-native path encoding
    // (including backslashes on Windows) rather than normalizing JSON output to
    // forward-slash-only form.
    #[serde(rename = "missingConfigPath", skip_serializing_if = "Option::is_none")]
    pub missing_config_path: Option<std::path::PathBuf>,

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
    pub task_id: Option<TaskId>,

    // Preserve unknown producer-owned fields so ATM does not accidentally
    // redefine external schemas by dropping or rewriting them.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingAck {
    pub message_id: LegacyMessageId,
    pub from: AgentName,
    pub acked: bool,
    pub acked_at: Option<IsoTimestamp>,
}

fn ensure_object<'a>(parent: &'a mut Map<String, Value>, key: &str) -> &'a mut Map<String, Value> {
    let entry = parent
        .entry(key.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    let Some(entry) = entry.as_object_mut() else {
        unreachable!("entry was just normalized into an object")
    };
    entry
}

pub(crate) fn to_shared_inbox_value(message: &MessageEnvelope) -> Result<Value, AtmError> {
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
        })?;
    // The legacy UUID `message_id` is stripped here but deliberately not
    // forwarded. Forwarded ATM message ids must remain ULID-authored per
    // architecture §5.2 rather than being derived from compatibility UUIDs.
    let _ = object.remove("message_id");
    let source_team = object.remove("source_team");
    let pending_ack_at = object.remove("pendingAckAt");
    let acknowledged_at = object.remove("acknowledgedAt");
    let acknowledges_message_id =
        object
            .remove("acknowledgesMessageId")
            .and_then(|value| match value {
                Value::String(_) => message
                    .acknowledges_message_id
                    // This forwarding path preserves the legacy UUID bytes
                    // exactly, but the resulting shared-inbox value is still a
                    // compatibility reinterpretation of those bytes as a ULID.
                    .map(LegacyMessageId::into_atm_message_id)
                    .map(|message_id| Value::String(message_id.to_string())),
                _ => None,
            });
    let task_id = object.remove("taskId");

    let metadata = ensure_object(object, "metadata");
    let atm = ensure_object(metadata, "atm");

    if let Some(value) = source_team {
        atm.entry("sourceTeam".to_string()).or_insert(value);
    }
    if let Some(value) = pending_ack_at {
        atm.entry("pendingAckAt".to_string()).or_insert(value);
    }
    if let Some(value) = acknowledged_at {
        atm.entry("acknowledgedAt".to_string()).or_insert(value);
    }
    if let Some(value) = acknowledges_message_id {
        atm.entry("acknowledgesMessageId".to_string())
            .or_insert(value);
    }
    if let Some(value) = task_id {
        atm.entry("taskId".to_string()).or_insert(value);
    }
    Ok(value)
}

impl MessageEnvelope {
    pub fn atm_message_id(&self) -> Option<AtmMessageId> {
        self.extra
            .get("metadata")
            .and_then(Value::as_object)
            .and_then(|metadata| metadata.get("atm"))
            .and_then(Value::as_object)
            .and_then(|atm| atm.get("messageId"))
            .and_then(Value::as_str)
            .and_then(|value| value.parse().ok())
    }
}

pub fn hydrate_legacy_fields_from_metadata(value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    let Some(atm) = object
        .get("metadata")
        .and_then(Value::as_object)
        .and_then(|metadata| metadata.get("atm"))
        .and_then(Value::as_object)
    else {
        return;
    };

    let message_id = if object.contains_key("message_id") {
        None
    } else if let Some(raw) = atm.get("messageId").and_then(Value::as_str) {
        match raw.parse::<AtmMessageId>() {
            Ok(message_id) => Some(Value::String(
                LegacyMessageId::from_atm_message_id(message_id).to_string(),
            )),
            Err(error) => {
                warn!(%error, raw, "ignoring malformed metadata.atm.messageId");
                None
            }
        }
    } else {
        None
    };

    let source_team = (!object.contains_key("source_team"))
        .then(|| atm.get("sourceTeam").cloned())
        .flatten();
    let pending_ack_at = (!object.contains_key("pendingAckAt"))
        .then(|| atm.get("pendingAckAt").cloned())
        .flatten();
    let acknowledged_at = (!object.contains_key("acknowledgedAt"))
        .then(|| atm.get("acknowledgedAt").cloned())
        .flatten();
    let acknowledges_message_id = if object.contains_key("acknowledgesMessageId") {
        None
    } else if let Some(raw) = atm.get("acknowledgesMessageId").and_then(Value::as_str) {
        match raw.parse::<AtmMessageId>() {
            Ok(message_id) => Some(Value::String(
                LegacyMessageId::from_atm_message_id(message_id).to_string(),
            )),
            Err(error) => {
                warn!(
                    %error,
                    raw,
                    "ignoring malformed metadata.atm.acknowledgesMessageId"
                );
                None
            }
        }
    } else {
        None
    };
    let task_id = (!object.contains_key("taskId"))
        .then(|| atm.get("taskId").cloned())
        .flatten();

    if let Some(value) = message_id {
        object.insert("message_id".to_string(), value);
    }
    if let Some(value) = source_team {
        object.insert("source_team".to_string(), value);
    }
    if let Some(value) = pending_ack_at {
        object.insert("pendingAckAt".to_string(), value);
    }
    if let Some(value) = acknowledged_at {
        object.insert("acknowledgedAt".to_string(), value);
    }
    if let Some(value) = acknowledges_message_id {
        object.insert("acknowledgesMessageId".to_string(), value);
    }
    if let Some(value) = task_id {
        object.insert("taskId".to_string(), value);
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde_json::{Map, Value, json};

    use chrono::Utc;

    use super::{
        AtmMessageId, AtmMetadataFields, ForwardMetadataEnvelope, IsoTimestamp, LegacyMessageId,
        MessageEnvelope, MessageMetadata, PendingAck, hydrate_legacy_fields_from_metadata,
        to_shared_inbox_value,
    };

    #[test]
    fn message_envelope_round_trips_with_current_inbox_shape() {
        // Validates the current ATM superset storage shape, not the
        // Claude-native schema. Ownership is documented in
        // docs/legacy-atm-message-schema.md and docs/atm-message-schema.md.
        let envelope = MessageEnvelope {
            from: "arch-ctm".parse().expect("agent"),
            text: "hello".into(),
            timestamp: IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
                    .single()
                    .expect("timestamp"),
            ),
            read: false,
            source_team: Some("atm-dev".parse().expect("team")),
            summary: Some("hello".into()),
            message_id: Some(LegacyMessageId::new()),
            pending_ack_at: Some(IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 1)
                    .single()
                    .expect("timestamp"),
            )),
            acknowledged_at: None,
            acknowledges_message_id: None,
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
    fn blank_task_id_is_rejected() {
        let json = json!({
            "from": "team-lead",
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
            message_id: LegacyMessageId::new(),
            from: "team-lead".parse().expect("agent"),
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
                    source_team: Some("atm-dev".parse().expect("team name")),
                    from_identity: None,
                    pending_ack_at: None,
                    acknowledged_at: None,
                    acknowledges_message_id: None,
                    task_id: None,
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
    fn forward_metadata_source_team_rejects_blank_team_name() {
        let json = json!({
            "timestamp": "2026-03-30T00:00:00Z",
            "metadata": {
                "atm": {
                    "sourceTeam": "   "
                }
            }
        });

        let error =
            serde_json::from_value::<ForwardMetadataEnvelope>(json).expect_err("blank sourceTeam");

        assert!(error.to_string().contains("team"));
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

    #[test]
    fn shared_inbox_write_shape_moves_machine_fields_into_metadata() {
        let envelope = MessageEnvelope {
            from: "arch-ctm".parse().expect("agent"),
            text: "hello".into(),
            timestamp: IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
                    .single()
                    .expect("timestamp"),
            ),
            read: false,
            source_team: Some("atm-dev".parse().expect("team")),
            summary: Some("hello".into()),
            message_id: Some(LegacyMessageId::new()),
            pending_ack_at: Some(IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 1)
                    .single()
                    .expect("timestamp"),
            )),
            acknowledged_at: None,
            acknowledges_message_id: None,
            task_id: Some("TASK-123".parse().expect("task id")),
            extra: Map::new(),
        };

        let encoded = to_shared_inbox_value(&envelope).expect("encode");
        let object = encoded.as_object().expect("object");
        assert!(!object.contains_key("message_id"));
        assert!(!object.contains_key("source_team"));
        assert!(!object.contains_key("pendingAckAt"));
        assert!(!object.contains_key("taskId"));

        let atm = object
            .get("metadata")
            .and_then(Value::as_object)
            .and_then(|metadata| metadata.get("atm"))
            .and_then(Value::as_object)
            .expect("metadata.atm");
        assert!(!atm.contains_key("messageId"));
        assert_eq!(atm.get("sourceTeam"), Some(&json!("atm-dev")));
        assert_eq!(atm.get("taskId"), Some(&json!("TASK-123")));
    }

    #[test]
    fn shared_inbox_write_shape_moves_ack_machine_fields_into_metadata() {
        let acknowledged_at = IsoTimestamp::from_datetime(
            Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 2)
                .single()
                .expect("timestamp"),
        );
        let envelope = MessageEnvelope {
            from: "arch-ctm".parse().expect("agent"),
            text: "ack reply".into(),
            timestamp: IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
                    .single()
                    .expect("timestamp"),
            ),
            read: false,
            source_team: Some("atm-dev".parse().expect("team")),
            summary: Some("ack reply".into()),
            message_id: Some(LegacyMessageId::new()),
            pending_ack_at: None,
            acknowledged_at: Some(acknowledged_at),
            acknowledges_message_id: Some(LegacyMessageId::new()),
            task_id: None,
            extra: Map::new(),
        };

        let encoded = to_shared_inbox_value(&envelope).expect("encode");
        let object = encoded.as_object().expect("object");
        assert!(!object.contains_key("acknowledgedAt"));
        assert!(!object.contains_key("acknowledgesMessageId"));

        let atm = object
            .get("metadata")
            .and_then(Value::as_object)
            .and_then(|metadata| metadata.get("atm"))
            .and_then(Value::as_object)
            .expect("metadata.atm");
        assert_eq!(
            atm.get("acknowledgedAt"),
            Some(&json!("2026-03-30T00:00:02Z"))
        );
        assert!(atm["acknowledgesMessageId"].as_str().is_some());
    }

    #[test]
    fn metadata_fields_hydrate_legacy_internal_shape() {
        let mut value = json!({
            "from": "arch-ctm",
            "text": "hello",
            "timestamp": "2026-03-30T00:00:00Z",
            "read": false,
            "summary": "hello",
            "metadata": {
                "atm": {
                    "messageId": "01JQYVB6W51Q2E7E6T3Y4Q9N2M",
                    "sourceTeam": "atm-dev",
                    "pendingAckAt": "2026-03-30T00:00:01Z",
                    "taskId": "TASK-123"
                }
            }
        });

        hydrate_legacy_fields_from_metadata(&mut value);
        let object = value.as_object().expect("object");
        assert!(object.contains_key("message_id"));
        assert_eq!(object.get("source_team"), Some(&json!("atm-dev")));
        assert_eq!(object.get("taskId"), Some(&json!("TASK-123")));
    }

    #[test]
    fn metadata_fields_hydrate_legacy_ack_fields() {
        let mut value = json!({
            "from": "arch-ctm",
            "text": "ack reply",
            "timestamp": "2026-03-30T00:00:00Z",
            "read": false,
            "metadata": {
                "atm": {
                    "acknowledgedAt": "2026-03-30T00:00:02Z",
                    "acknowledgesMessageId": "01JQYVB6W51Q2E7E6T3Y4Q9N2M"
                }
            }
        });

        hydrate_legacy_fields_from_metadata(&mut value);
        let object = value.as_object().expect("object");
        assert_eq!(
            object.get("acknowledgedAt"),
            Some(&json!("2026-03-30T00:00:02Z"))
        );
        assert!(object["acknowledgesMessageId"].as_str().is_some());
    }

    #[test]
    fn hydrate_legacy_fields_ignores_malformed_metadata_without_panic() {
        let mut value = json!({
            "from": "arch-ctm",
            "text": "hello",
            "timestamp": "2026-03-30T00:00:00Z",
            "read": false,
            "metadata": {
                "atm": {
                    "messageId": "not-a-ulid",
                    "acknowledgesMessageId": "also-not-a-ulid"
                }
            }
        });

        hydrate_legacy_fields_from_metadata(&mut value);
        let object = value.as_object().expect("object");
        assert!(!object.contains_key("message_id"));
        assert!(!object.contains_key("acknowledgesMessageId"));
    }

    #[test]
    fn hydrate_legacy_fields_handles_partially_migrated_envelope() {
        let mut value = json!({
            "from": "arch-ctm",
            "text": "hello",
            "timestamp": "2026-03-30T00:00:00Z",
            "read": false,
            "source_team": "legacy-team",
            "metadata": {
                "atm": {
                    "messageId": "01JQYVB6W51Q2E7E6T3Y4Q9N2M",
                    "pendingAckAt": "2026-03-30T00:00:01Z"
                }
            }
        });

        hydrate_legacy_fields_from_metadata(&mut value);
        let object = value.as_object().expect("object");
        assert_eq!(object.get("source_team"), Some(&json!("legacy-team")));
        assert!(object["message_id"].as_str().is_some());
        assert_eq!(
            object.get("pendingAckAt"),
            Some(&json!("2026-03-30T00:00:01Z"))
        );
    }
}
