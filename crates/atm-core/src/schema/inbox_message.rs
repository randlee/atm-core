//! Shared inbox compatibility helpers layered on top of the canonical
//! `atm-storage` message envelope types.

pub use atm_storage::schema::{AlertKind, AtmMessageId, InboxMessage, PendingAck, ThreadMode};

use serde_json::{Map, Value};

use crate::address::display_sender_identity;
use crate::config::types::{ByteCount, DEFAULT_CLAUDE_JSONL_BODY_EXPORT_MAX_BYTES};
use crate::error::AtmError;
use crate::types::{HostName, IsoTimestamp};

const AUTHENTICATED_SOURCE_HOST_KEY: &str = "sourceHost";
const PEER_OUTBOUND_KEY: &str = "peerOutbound";

/// Removes daemon-local transport bookkeeping that is not part of the
/// immutable user message. The same origin ULID may carry different local
/// delivery metadata on its origin and receiving hosts.
pub(crate) fn clear_transport_delivery_metadata(message: &mut InboxMessage) {
    message.extra.remove(AUTHENTICATED_SOURCE_HOST_KEY);
    message.extra.remove(PEER_OUTBOUND_KEY);
}

/// Returns the source host that the HTTPS adapter authenticated for this
/// immutable inbound message. Local messages intentionally have no value.
pub(crate) fn authenticated_source_host(
    message: &InboxMessage,
) -> Result<Option<HostName>, AtmError> {
    let Some(value) = message.extra.get(AUTHENTICATED_SOURCE_HOST_KEY) else {
        return Ok(None);
    };
    let value = value.as_str().ok_or_else(|| {
        AtmError::mailbox_read("persisted authenticated source host is not a string")
    })?;
    value
        .parse()
        .map(Some)
        .map_err(|_| AtmError::mailbox_read("persisted authenticated source host is invalid"))
}

/// Renders the immutable sender and authenticated peer provenance for human
/// output. A malformed legacy `sourceHost` remains non-fatal to mailbox reads.
#[must_use]
pub fn display_inbound_sender(message: &InboxMessage) -> String {
    let authenticated_source_host = authenticated_source_host(message).ok().flatten();
    display_sender_identity(
        &message.from,
        message.source_chat_id.as_ref(),
        message.source_team.as_ref(),
        authenticated_source_host.as_ref(),
    )
}

/// Returns the destination host retained on an origin message that awaits
/// ordinary peer delivery. This is routing metadata, never peer provenance.
pub(crate) fn peer_outbound_host(message: &InboxMessage) -> Result<Option<HostName>, AtmError> {
    let Some(value) = message.extra.get(PEER_OUTBOUND_KEY) else {
        return Ok(None);
    };
    let host = value
        .as_object()
        .and_then(|object| object.get("host"))
        .and_then(Value::as_str)
        .ok_or_else(|| AtmError::mailbox_read("persisted peer outbound host is invalid"))?;
    host.parse()
        .map(Some)
        .map_err(|_| AtmError::mailbox_read("persisted peer outbound host is invalid"))
}

/// Persists only adapter-authenticated source-host metadata; callers must not
/// copy this value from an untrusted request payload.
pub(crate) fn set_authenticated_source_host(message: &mut InboxMessage, host: Option<HostName>) {
    match host {
        Some(host) => {
            message.extra.insert(
                AUTHENTICATED_SOURCE_HOST_KEY.to_string(),
                Value::String(host.to_string()),
            );
        }
        None => {
            message.extra.remove(AUTHENTICATED_SOURCE_HOST_KEY);
        }
    }
}

/// Retains the immutable origin write alongside its canonical local message.
/// This is metadata on the message itself, not a separate outbox/replay row.
pub(crate) fn set_peer_outbound_write(
    message: &mut InboxMessage,
    host: &HostName,
    request_json: String,
) {
    let mut value = Map::new();
    value.insert("host".to_string(), Value::String(host.to_string()));
    value.insert("request".to_string(), Value::String(request_json));
    message
        .extra
        .insert(PEER_OUTBOUND_KEY.to_string(), Value::Object(value));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AckIntentFields {
    pub(crate) requires_ack: bool,
    pub(crate) pending_ack_at: Option<IsoTimestamp>,
    pub(crate) acknowledged_at: Option<IsoTimestamp>,
}

impl AckIntentFields {
    pub(crate) const fn not_required() -> Self {
        Self {
            requires_ack: false,
            pending_ack_at: None,
            acknowledged_at: None,
        }
    }

    pub(crate) fn from_requires_ack(requires_ack: bool, timestamp: IsoTimestamp) -> Self {
        Self {
            requires_ack,
            pending_ack_at: requires_ack.then_some(timestamp),
            acknowledged_at: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn required_pending(timestamp: IsoTimestamp) -> Self {
        Self::from_requires_ack(true, timestamp)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SharedAppendPolicy {
    pub(crate) atm_authored_body_export_max_bytes: ByteCount,
}

impl Default for SharedAppendPolicy {
    fn default() -> Self {
        Self {
            atm_authored_body_export_max_bytes: ByteCount::new(
                DEFAULT_CLAUDE_JSONL_BODY_EXPORT_MAX_BYTES,
            ),
        }
    }
}

#[cfg(test)]
pub(crate) fn to_shared_inbox_value(message: &InboxMessage) -> Result<Value, AtmError> {
    to_shared_inbox_value_with_policy(message, SharedAppendPolicy::default())
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

fn strip_removed_compatibility_fields(object: &mut Map<String, Value>) {
    for field in [
        "source_team",
        "pendingAckAt",
        "acknowledgedAt",
        "acknowledgesMessageId",
        "expiresAt",
    ] {
        object.remove(field);
    }
}

pub(crate) fn to_shared_inbox_value_with_policy(
    message: &InboxMessage,
    policy: SharedAppendPolicy,
) -> Result<Value, AtmError> {
    let mut value = serde_json::to_value(message).map_err(|error| {
        AtmError::mailbox_write(format!(
            "failed to serialize shared inbox envelope for {} at {:?}: {error}",
            message.from, message.timestamp
        ))
    })?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| {
            AtmError::mailbox_write(format!(
                "failed to serialize shared inbox envelope for {} at {:?}: envelope did not encode as a JSON object",
                message.from, message.timestamp
            ))

        })?;
    strip_metadata_atm_namespace(object);
    strip_removed_compatibility_fields(object);
    if should_export_retrieval_stub(message, policy)? {
        let message_id = message.message_id.ok_or_else(|| {
            AtmError::mailbox_write(
                "retrieval stub export requires an ATM-authored message_id on the source envelope",
            )
        })?;
        let retrieval_stub = retrieval_stub_text(message_id);
        object.insert("text".to_string(), Value::String(retrieval_stub));
    }
    Ok(value)
}

fn should_export_retrieval_stub(
    message: &InboxMessage,
    policy: SharedAppendPolicy,
) -> Result<bool, AtmError> {
    let export_cap = policy
        .atm_authored_body_export_max_bytes
        .as_usize()
        .ok_or_else(|| {
            AtmError::mailbox_write(format!(
                "failed to compare ATM-authored export cap for {} at {:?}: configured byte cap does not fit into usize",
                message.from, message.timestamp
            ))

        })?;

    Ok(message.message_id.is_some()
        && (policy.atm_authored_body_export_max_bytes.is_zero() || message.text.len() > export_cap))
}

fn retrieval_stub_text(message_id: AtmMessageId) -> String {
    format!("atm read --message-id {message_id}")
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde_json::{Map, Value, json};

    use chrono::Utc;

    use super::{
        AckIntentFields, AlertKind, AtmMessageId, InboxMessage, PendingAck, SharedAppendPolicy,
        ThreadMode, display_inbound_sender, to_shared_inbox_value,
        to_shared_inbox_value_with_policy,
    };
    use crate::config::types::ByteCount;
    use crate::roles::ROLE_TEAM_LEAD;
    use crate::test_support::{TEST_SENDER, TEST_TEAM};
    use crate::types::IsoTimestamp;

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
    fn ack_intent_fields_helpers_keep_the_three_field_invariant_together() {
        let timestamp = IsoTimestamp::from_datetime(Utc::now());

        assert_eq!(
            AckIntentFields::not_required(),
            AckIntentFields {
                requires_ack: false,
                pending_ack_at: None,
                acknowledged_at: None,
            }
        );
        assert_eq!(
            AckIntentFields::from_requires_ack(false, timestamp),
            AckIntentFields::not_required()
        );
        assert_eq!(
            AckIntentFields::required_pending(timestamp),
            AckIntentFields {
                requires_ack: true,
                pending_ack_at: Some(timestamp),
                acknowledged_at: None,
            }
        );
    }

    #[test]
    fn display_inbound_sender_includes_compact_authenticated_peer_host() {
        let mut message = InboxMessage {
            from: TEST_SENDER.parse().expect("sender"),
            source_chat_id: None,
            text: "hello".to_string(),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: Some(TEST_TEAM.parse().expect("team")),
            destination_chat_id: None,
            summary: None,
            message_id: Some(AtmMessageId::new()),
            requires_ack: false,
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: None,
            extra: Map::new(),
        };
        message.extra.insert(
            "sourceHost".to_string(),
            Value::String("rand-m5.local".to_string()),
        );

        assert_eq!(
            display_inbound_sender(&message),
            format!("{TEST_SENDER}@{TEST_TEAM}.rand-m5")
        );
    }

    #[test]
    fn message_envelope_round_trips_with_current_inbox_shape() {
        // Validates the current ATM superset storage shape, not the
        // Claude-native schema. Ownership is documented in
        // docs/legacy-atm-message-schema.md and docs/atm-message-schema.md.
        let envelope = InboxMessage {
            from: TEST_SENDER.parse().expect("agent"),
            source_chat_id: None,
            text: "hello".into(),
            timestamp: IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
                    .single()
                    .expect("timestamp"),
            ),
            read: false,
            source_team: Some(TEST_TEAM.parse().expect("team")),
            destination_chat_id: None,
            summary: Some("hello".into()),
            message_id: Some(AtmMessageId::new()),
            requires_ack: true,
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
        let decoded: InboxMessage = serde_json::from_str(&encoded).expect("decode");

        assert_eq!(decoded, envelope);
    }

    #[test]
    fn legacy_pending_ack_without_ack_reply_metadata_defaults_requires_ack_true() {
        let json = json!({
            "from": ROLE_TEAM_LEAD,
            "text": "hello",
            "timestamp": "2026-03-30T00:00:00Z",
            "read": true,
            "pendingAckAt": "2026-03-30T00:00:01Z"
        });

        let decoded: InboxMessage = serde_json::from_value(json).expect("decode");
        assert!(decoded.requires_ack);
    }

    #[test]
    fn legacy_ack_reply_metadata_prevents_requires_ack_requalification() {
        let acknowledged_message_id = AtmMessageId::new();
        let json = json!({
            "from": ROLE_TEAM_LEAD,
            "text": "ack",
            "timestamp": "2026-03-30T00:00:00Z",
            "read": true,
            "pendingAckAt": "2026-03-30T00:00:01Z",
            "acknowledgesMessageId": acknowledged_message_id,
        });

        let decoded: InboxMessage = serde_json::from_value(json).expect("decode");
        assert!(!decoded.requires_ack);
        assert_eq!(
            decoded.acknowledges_message_id,
            Some(acknowledged_message_id)
        );
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

        let decoded: InboxMessage = serde_json::from_value(json).expect("decode");
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

        let decoded: InboxMessage = serde_json::from_value(json).expect("decode");
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

        let error = serde_json::from_value::<InboxMessage>(json).expect_err("blank task id");

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
    fn atm_message_id_parses_from_ulid_string() {
        let (message_id, _) = AtmMessageId::new_with_timestamp();
        let parsed: AtmMessageId = message_id.to_string().parse().expect("parse atm id");
        assert_eq!(parsed, message_id);
    }

    #[test]
    fn shared_inbox_write_keeps_only_approved_immutable_fields() {
        let envelope = InboxMessage {
            from: TEST_SENDER.parse().expect("agent"),
            source_chat_id: None,
            text: "hello".into(),
            timestamp: IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
                    .single()
                    .expect("timestamp"),
            ),
            read: false,
            source_team: Some(TEST_TEAM.parse().expect("team")),
            destination_chat_id: None,
            summary: Some("hello".into()),
            message_id: Some(AtmMessageId::new()),
            requires_ack: true,
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
        assert!(!object.contains_key("source_team"));
        assert!(!object.contains_key("pendingAckAt"));
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
    fn shared_inbox_write_keeps_parent_message_id_and_thread_mode_when_set() {
        // parentMessageId and threadMode are approved immutable fields and must
        // survive to_shared_inbox_value when they carry non-None values.
        let envelope = InboxMessage {
            from: TEST_SENDER.parse().expect("agent"),
            source_chat_id: None,
            text: "threaded message".into(),
            timestamp: IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
                    .single()
                    .expect("timestamp"),
            ),
            read: false,
            source_team: None,
            destination_chat_id: None,
            summary: None,
            message_id: Some(AtmMessageId::new()),
            requires_ack: false,
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: Some(AtmMessageId::new()),
            thread_mode: Some(ThreadMode::AddDetails),
            expires_at: None,
            task_id: None,
            extra: Map::new(),
        };

        let encoded = to_shared_inbox_value(&envelope).expect("encode");
        let object = encoded.as_object().expect("object");
        assert!(
            object.contains_key("parentMessageId"),
            "parentMessageId must be kept in shared inbox export"
        );
        assert!(
            object.contains_key("threadMode"),
            "threadMode must be kept in shared inbox export"
        );
    }

    #[test]
    fn shared_inbox_write_strips_expires_at() {
        // expiresAt is a removed compatibility field and must not appear in the
        // shared inbox export even when the envelope carries a non-None value.
        let envelope = InboxMessage {
            from: TEST_SENDER.parse().expect("agent"),
            source_chat_id: None,
            text: "expiring message".into(),
            timestamp: IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
                    .single()
                    .expect("timestamp"),
            ),
            read: false,
            source_team: None,
            destination_chat_id: None,
            summary: None,
            message_id: Some(AtmMessageId::new()),
            requires_ack: false,
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: Some(IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 31, 0, 0, 0)
                    .single()
                    .expect("timestamp"),
            )),
            task_id: None,
            extra: Map::new(),
        };

        let encoded = to_shared_inbox_value(&envelope).expect("encode");
        let object = encoded.as_object().expect("object");
        assert!(
            !object.contains_key("expiresAt"),
            "expiresAt must be stripped from shared inbox export"
        );
    }

    #[test]
    fn shared_inbox_write_strips_ack_workflow_fields_from_export() {
        let acknowledged_at = IsoTimestamp::from_datetime(
            Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 2)
                .single()
                .expect("timestamp"),
        );
        let envelope = InboxMessage {
            from: TEST_SENDER.parse().expect("agent"),
            source_chat_id: None,
            text: "ack reply".into(),
            timestamp: IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
                    .single()
                    .expect("timestamp"),
            ),
            read: false,
            source_team: Some(TEST_TEAM.parse().expect("team")),
            destination_chat_id: None,
            summary: Some("ack reply".into()),
            message_id: Some(AtmMessageId::new()),
            requires_ack: false,
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
        assert!(!object.contains_key("acknowledgedAt"));
        assert!(!object.contains_key("acknowledgesMessageId"));
    }

    #[test]
    fn shared_inbox_write_stubs_oversized_atm_authored_messages() {
        let message_id = AtmMessageId::new();
        let envelope = InboxMessage {
            from: TEST_SENDER.parse().expect("agent"),
            source_chat_id: None,
            text: "x".repeat(32),
            timestamp: IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
                    .single()
                    .expect("timestamp"),
            ),
            read: false,
            source_team: Some(TEST_TEAM.parse().expect("team")),
            destination_chat_id: None,
            summary: Some("oversized".into()),
            message_id: Some(message_id),
            requires_ack: false,
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
            SharedAppendPolicy {
                atm_authored_body_export_max_bytes: ByteCount::new(8),
            },
        )
        .expect("encode");

        assert_eq!(
            encoded["text"],
            json!(format!("atm read --message-id {message_id}"))
        );
        assert_eq!(encoded["summary"], json!("oversized"));
        assert_eq!(encoded["message_id"], json!(message_id.to_string()));
    }

    #[test]
    fn shared_inbox_write_exports_full_body_at_exact_cap() {
        let message_id = AtmMessageId::new();
        let text = "x".repeat(32);
        let envelope = InboxMessage {
            from: TEST_SENDER.parse().expect("agent"),
            source_chat_id: None,
            text: text.clone(),
            timestamp: IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
                    .single()
                    .expect("timestamp"),
            ),
            read: false,
            source_team: Some(TEST_TEAM.parse().expect("team")),
            destination_chat_id: None,
            summary: Some("exact-cap".into()),
            message_id: Some(message_id),
            requires_ack: false,
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
            SharedAppendPolicy {
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
        let envelope = InboxMessage {
            from: TEST_SENDER.parse().expect("agent"),
            source_chat_id: None,
            text,
            timestamp: IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
                    .single()
                    .expect("timestamp"),
            ),
            read: false,
            source_team: Some(TEST_TEAM.parse().expect("team")),
            destination_chat_id: None,
            summary: Some("above-cap".into()),
            message_id: Some(message_id),
            requires_ack: false,
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
            SharedAppendPolicy {
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
        let envelope = InboxMessage {
            from: TEST_SENDER.parse().expect("agent"),
            source_chat_id: None,
            text: "x".repeat(32),
            timestamp: IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
                    .single()
                    .expect("timestamp"),
            ),
            read: false,
            source_team: None,
            destination_chat_id: None,
            summary: Some("native".into()),
            message_id: None,
            requires_ack: false,
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
            SharedAppendPolicy {
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
        let envelope = InboxMessage {
            from: TEST_SENDER.parse().expect("agent"),
            source_chat_id: None,
            text: "hello".into(),
            timestamp: IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
                    .single()
                    .expect("timestamp"),
            ),
            read: false,
            source_team: None,
            destination_chat_id: None,
            summary: None,
            message_id: Some(AtmMessageId::new()),
            requires_ack: false,
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
