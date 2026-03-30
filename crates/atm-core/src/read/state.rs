use std::marker::PhantomData;

use crate::schema::MessageEnvelope;
use crate::types::{
    AckState, AcknowledgedAckState, DisplayBucket, IsoTimestamp, MessageClass, NoAckState,
    PendingAckState, ReadReadState, ReadState, UnreadReadState,
};

#[derive(Debug, Clone)]
pub struct StoredMessage<R, A> {
    pub envelope: MessageEnvelope,
    // Phantom typestate markers keep legal transitions on the type without storing runtime data.
    _read: PhantomData<R>,
    _ack: PhantomData<A>,
}

impl StoredMessage<UnreadReadState, NoAckState> {
    pub(crate) fn unread_no_ack(envelope: MessageEnvelope) -> Self {
        Self {
            envelope,
            _read: PhantomData,
            _ack: PhantomData,
        }
    }

    pub fn display_without_ack(self) -> StoredMessage<ReadReadState, NoAckState> {
        self.mark_read()
    }

    pub fn display_and_require_ack(
        mut self,
        at: IsoTimestamp,
    ) -> StoredMessage<ReadReadState, PendingAckState> {
        self.envelope.read = true;
        self.envelope.pending_ack_at = Some(at);
        StoredMessage::read_pending_ack(self.envelope)
    }

    pub fn mark_read(mut self) -> StoredMessage<ReadReadState, NoAckState> {
        self.envelope.read = true;
        StoredMessage::read_no_ack(self.envelope)
    }
}

impl StoredMessage<UnreadReadState, PendingAckState> {
    pub(crate) fn unread_pending_ack(envelope: MessageEnvelope) -> Self {
        Self {
            envelope,
            _read: PhantomData,
            _ack: PhantomData,
        }
    }

    pub fn mark_read_pending_ack(mut self) -> StoredMessage<ReadReadState, PendingAckState> {
        self.envelope.read = true;
        StoredMessage::read_pending_ack(self.envelope)
    }
}

impl StoredMessage<ReadReadState, NoAckState> {
    pub(crate) fn read_no_ack(envelope: MessageEnvelope) -> Self {
        Self {
            envelope,
            _read: PhantomData,
            _ack: PhantomData,
        }
    }
}

impl StoredMessage<ReadReadState, PendingAckState> {
    pub(crate) fn read_pending_ack(envelope: MessageEnvelope) -> Self {
        Self {
            envelope,
            _read: PhantomData,
            _ack: PhantomData,
        }
    }

    pub fn acknowledge(
        mut self,
        at: IsoTimestamp,
    ) -> StoredMessage<ReadReadState, AcknowledgedAckState> {
        self.envelope.acknowledged_at = Some(at);
        self.envelope.pending_ack_at = None;
        StoredMessage::read_acknowledged(self.envelope)
    }
}

impl StoredMessage<ReadReadState, AcknowledgedAckState> {
    pub(crate) fn read_acknowledged(envelope: MessageEnvelope) -> Self {
        Self {
            envelope,
            _read: PhantomData,
            _ack: PhantomData,
        }
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

    debug_assert!(
        !matches!(
            (read_state, ack_state),
            (ReadState::Unread, AckState::Acknowledged)
        ),
        "inconsistent message state: unread message cannot already be acknowledged"
    );

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
