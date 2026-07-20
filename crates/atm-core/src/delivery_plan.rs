use crate::delivery_policy::DeliveryRecipientSnapshot;
use crate::schema::{AtmMessageId, InboxMessage};
use crate::send::{
    DeliveryPersistenceDisposition, DeliveryPersistenceResult, ResolvedRecipient, WarningEntry,
};
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LogicalMessage {
    pub(crate) message_id: AtmMessageId,
    pub(crate) envelope: InboxMessage,
    pub(crate) requires_ack: bool,
    pub(crate) is_ack: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LogicalMessageError {
    MissingMessageId,
}

impl fmt::Display for LogicalMessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingMessageId => {
                f.write_str("logical delivery messages must carry a message id")
            }
        }
    }
}

impl LogicalMessage {
    pub(crate) fn new(
        envelope: InboxMessage,
        requires_ack: bool,
        is_ack: bool,
    ) -> Result<Self, LogicalMessageError> {
        let message_id = envelope
            .message_id
            .ok_or(LogicalMessageError::MissingMessageId)?;
        Ok(Self {
            message_id,
            envelope,
            requires_ack,
            is_ack,
        })
    }

    pub(crate) fn message_id(&self) -> AtmMessageId {
        self.message_id
    }
}

pub(crate) fn logical_messages_from_persistence(
    persistence: &DeliveryPersistenceResult,
    requires_ack: bool,
    is_ack: bool,
) -> Result<Vec<LogicalMessage>, LogicalMessageError> {
    let mut messages = vec![LogicalMessage::new(
        persistence.original_message.clone(),
        requires_ack,
        is_ack,
    )?];
    if let Some(companion_message) = persistence.companion_message.clone() {
        messages.push(LogicalMessage::new(companion_message, false, is_ack)?);
    }
    Ok(messages)
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DeliveryPlan {
    pub(crate) disposition: DeliveryPersistenceDisposition,
    pub(crate) recipient_snapshot: DeliveryRecipientSnapshot,
    pub(crate) recipient: ResolvedRecipient,
    pub(crate) messages: Vec<LogicalMessage>,
    pub(crate) warnings: Vec<WarningEntry>,
}

impl DeliveryPlan {
    pub(crate) fn new(
        disposition: DeliveryPersistenceDisposition,
        recipient_snapshot: DeliveryRecipientSnapshot,
        recipient: ResolvedRecipient,
        messages: Vec<LogicalMessage>,
        warnings: Vec<WarningEntry>,
    ) -> Self {
        Self {
            disposition,
            recipient_snapshot,
            recipient,
            messages,
            warnings,
        }
    }
}
