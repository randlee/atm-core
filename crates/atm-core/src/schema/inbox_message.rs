use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageEnvelope {
    pub message_id: Uuid,
    pub from: String,
    pub team: String,
    pub body: String,
    pub requires_ack: bool,
    pub task_id: Option<String>,
    pub sent_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingAck {
    pub message_id: Uuid,
    pub from: String,
    pub acked: bool,
    pub acked_at: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    use super::{MessageEnvelope, PendingAck};

    #[test]
    fn message_envelope_round_trips_with_uuid_message_id() {
        let envelope = MessageEnvelope {
            message_id: Uuid::new_v4(),
            from: "arch-ctm".into(),
            team: "atm-dev".into(),
            body: "hello".into(),
            requires_ack: true,
            task_id: Some("TASK-1".into()),
            sent_at: Utc
                .with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
                .single()
                .expect("timestamp"),
        };

        let encoded = serde_json::to_string(&envelope).expect("encode");
        let decoded: MessageEnvelope = serde_json::from_str(&encoded).expect("decode");

        assert_eq!(decoded, envelope);
    }

    #[test]
    fn pending_ack_round_trips() {
        let pending_ack = PendingAck {
            message_id: Uuid::new_v4(),
            from: "team-lead".into(),
            acked: true,
            acked_at: Some(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 1)
                    .single()
                    .expect("timestamp"),
            ),
        };

        let encoded = serde_json::to_string(&pending_ack).expect("encode");
        let decoded: PendingAck = serde_json::from_str(&encoded).expect("decode");

        assert_eq!(decoded, pending_ack);
    }
}
