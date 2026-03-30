use crate::schema::MessageEnvelope;
use crate::types::{AckState, DisplayBucket, MessageClass, ReadState};

pub fn classify_message(message: &MessageEnvelope) -> MessageClass {
    let read_state = if message.read {
        ReadState::Read
    } else {
        ReadState::Unread
    };

    let ack_state = if message.pending_ack_at.is_some() && message.acknowledged_at.is_none() {
        AckState::PendingAck
    } else if message.acknowledged_at.is_some() {
        AckState::Acknowledged
    } else {
        AckState::NoAckRequired
    };

    match (read_state, ack_state) {
        (ReadState::Unread, AckState::NoAckRequired) => MessageClass::Unread,
        (ReadState::Unread, AckState::PendingAck) => MessageClass::PendingAck,
        (ReadState::Read, AckState::PendingAck) => MessageClass::PendingAck,
        (ReadState::Read, AckState::Acknowledged) => MessageClass::Acknowledged,
        (ReadState::Read, AckState::NoAckRequired) => MessageClass::Read,
        _ => MessageClass::Read,
    }
}

pub fn display_bucket_for_class(class: MessageClass) -> DisplayBucket {
    match class {
        MessageClass::Unread => DisplayBucket::Unread,
        MessageClass::PendingAck => DisplayBucket::PendingAck,
        MessageClass::Acknowledged | MessageClass::Read => DisplayBucket::History,
    }
}
