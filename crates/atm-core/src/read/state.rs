use std::marker::PhantomData;

use chrono::{DateTime, Utc};

use crate::schema::MessageEnvelope;
use crate::types::{
    AckState, AcknowledgedAckState, DisplayBucket, MessageClass, NoAckState, PendingAckState,
    ReadReadState, ReadState, UnreadReadState,
};

#[derive(Debug, Clone)]
pub struct StoredMessage<R, A> {
    pub envelope: MessageEnvelope,
    _read: PhantomData<R>,
    _ack: PhantomData<A>,
}

impl<R, A> StoredMessage<R, A> {
    pub(crate) fn from_envelope(envelope: MessageEnvelope) -> Self {
        Self {
            envelope,
            _read: PhantomData,
            _ack: PhantomData,
        }
    }
}

impl StoredMessage<UnreadReadState, NoAckState> {
    pub fn display_without_ack(mut self) -> StoredMessage<ReadReadState, NoAckState> {
        self.envelope.read = true;
        StoredMessage::from_envelope(self.envelope)
    }

    pub fn display_and_require_ack(
        mut self,
        at: DateTime<Utc>,
    ) -> StoredMessage<ReadReadState, PendingAckState> {
        self.envelope.read = true;
        self.envelope.pending_ack_at = Some(at);
        StoredMessage::from_envelope(self.envelope)
    }
}

impl StoredMessage<UnreadReadState, PendingAckState> {
    pub fn mark_read(mut self) -> StoredMessage<ReadReadState, PendingAckState> {
        self.envelope.read = true;
        StoredMessage::from_envelope(self.envelope)
    }
}

impl StoredMessage<ReadReadState, PendingAckState> {
    pub fn acknowledge(
        mut self,
        at: DateTime<Utc>,
    ) -> StoredMessage<ReadReadState, AcknowledgedAckState> {
        self.envelope.acknowledged_at = Some(at);
        self.envelope.pending_ack_at = None;
        StoredMessage::from_envelope(self.envelope)
    }
}

#[derive(Debug, Clone)]
pub enum TransitionedMessage {
    ReadNoAck(StoredMessage<ReadReadState, NoAckState>),
    ReadPendingAck(StoredMessage<ReadReadState, PendingAckState>),
    Unchanged(MessageEnvelope),
}

impl TransitionedMessage {
    pub fn into_envelope(self) -> MessageEnvelope {
        match self {
            Self::ReadNoAck(message) => message.envelope,
            Self::ReadPendingAck(message) => message.envelope,
            Self::Unchanged(envelope) => envelope,
        }
    }
}

pub fn derive_read_state(message: &MessageEnvelope) -> ReadState {
    if message.read {
        ReadState::Read
    } else {
        ReadState::Unread
    }
}

pub fn derive_ack_state(message: &MessageEnvelope) -> AckState {
    if message.acknowledged_at.is_some() {
        AckState::Acknowledged
    } else if message.pending_ack_at.is_some() {
        AckState::PendingAck
    } else {
        AckState::NoAckRequired
    }
}

pub fn classify_message(message: &MessageEnvelope) -> MessageClass {
    let read_state = derive_read_state(message);
    let ack_state = derive_ack_state(message);

    match (read_state, ack_state) {
        (ReadState::Unread, AckState::NoAckRequired) => MessageClass::Unread,
        (ReadState::Unread, AckState::PendingAck) => MessageClass::PendingAck,
        (ReadState::Unread, AckState::Acknowledged) => MessageClass::Acknowledged,
        (ReadState::Read, AckState::NoAckRequired) => MessageClass::Read,
        (ReadState::Read, AckState::PendingAck) => MessageClass::PendingAck,
        (ReadState::Read, AckState::Acknowledged) => MessageClass::Acknowledged,
    }
}

pub fn display_bucket_for_class(class: MessageClass) -> DisplayBucket {
    match class {
        MessageClass::Unread => DisplayBucket::Unread,
        MessageClass::PendingAck => DisplayBucket::PendingAck,
        MessageClass::Acknowledged | MessageClass::Read => DisplayBucket::History,
    }
}
