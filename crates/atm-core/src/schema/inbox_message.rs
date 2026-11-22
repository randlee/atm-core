//! Shared inbox compatibility helpers layered on top of the canonical
//! `atm-storage` message envelope types.

pub use atm_storage::schema::{AlertKind, AtmMessageId, InboxMessage, PendingAck, ThreadMode};

use serde_json::{Map, Value};

use crate::error::AtmError;
use crate::types::{HostName, IsoTimestamp};

const AUTHENTICATED_SOURCE_HOST_KEY: &str = "sourceHost";
const PEER_OUTBOUND_KEY: &str = "peerOutbound";
const DAEMON_ESCALATION_PROVENANCE_KEY: &str = "daemonEscalationProvenance";

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

/// Returns the direct-peer destination retained on an origin message.
/// This is routing metadata, never peer provenance or replay state.
pub(crate) fn peer_delivery_target(message: &InboxMessage) -> Result<Option<HostName>, AtmError> {
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

/// Returns whether the canonical local writer stamped this record as a Herdr
/// daemon escalation. This durable marker is never admitted from a client
/// write request.
#[must_use]
pub fn has_daemon_escalation_provenance(message: &InboxMessage) -> bool {
    message
        .extra
        .get(DAEMON_ESCALATION_PROVENANCE_KEY)
        .is_some_and(|value| value.as_bool() == Some(true))
}

/// Applies the daemon-only marker while the canonical writer builds the
/// durable message envelope.
pub(crate) fn set_daemon_escalation_provenance(message: &mut InboxMessage, enabled: bool) {
    if enabled {
        message.extra.insert(
            DAEMON_ESCALATION_PROVENANCE_KEY.to_string(),
            Value::Bool(true),
        );
    } else {
        message.extra.remove(DAEMON_ESCALATION_PROVENANCE_KEY);
    }
}

/// Retains only the direct-peer destination alongside its canonical local message.
pub(crate) fn set_peer_delivery_target(message: &mut InboxMessage, host: &HostName) {
    let mut value = Map::new();
    value.insert("host".to_string(), Value::String(host.to_string()));
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
    #[cfg(test)]
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

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde_json::{Map, json};

    use chrono::Utc;

    use super::{AckIntentFields, AlertKind, AtmMessageId, InboxMessage, PendingAck};
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
            placement: None,
            task_op: None,
            task_complete: None,
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
}
