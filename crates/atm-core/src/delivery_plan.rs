use std::fmt;
use std::path::Path;

use crate::delivery_policy::DeliveryRecipientSnapshot;
use crate::schema::{AtmMessageId, InboxMessage};
use crate::send::{
    DeliveryPersistenceDisposition, DeliveryPersistenceResult, ResolvedRecipient, WarningEntry,
};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeliveryPlanDisposition {
    Persisted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeliveryPlanKind {
    Send,
}

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
    let messages = vec![LogicalMessage::new(
        persistence.original_message.clone(),
        requires_ack,
        is_ack,
    )?];
    Ok(messages)
}

pub(crate) fn delivery_plan_disposition(
    disposition: DeliveryPersistenceDisposition,
) -> DeliveryPlanDisposition {
    let DeliveryPersistenceDisposition::Persisted = disposition;
    DeliveryPlanDisposition::Persisted
}

pub(crate) fn delivery_target_for_snapshot(
    _inbox_path: &Path,
    delivery_snapshot: &DeliveryRecipientSnapshot,
) -> DeliveryTarget {
    DeliveryTarget::NonClaude {
        recipient: delivery_snapshot.clone(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DeliveryTarget {
    NonClaude {
        recipient: DeliveryRecipientSnapshot,
    },
}

impl DeliveryTarget {
    pub(crate) fn harness_path(&self) -> crate::delivery_policy::DeliveryHarnessPath {
        match self {
            Self::NonClaude { .. } => crate::delivery_policy::DeliveryHarnessPath::NonClaude,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DeliveryPlan {
    pub(crate) kind: DeliveryPlanKind,
    pub(crate) disposition: DeliveryPlanDisposition,
    pub(crate) delivery_target: DeliveryTarget,
    pub(crate) recipient: ResolvedRecipient,
    pub(crate) messages: Vec<LogicalMessage>,
    pub(crate) warnings: Vec<WarningEntry>,
}

impl DeliveryPlan {
    pub(crate) fn new(
        kind: DeliveryPlanKind,
        disposition: DeliveryPlanDisposition,
        delivery_target: DeliveryTarget,
        recipient: ResolvedRecipient,
        messages: Vec<LogicalMessage>,
        warnings: Vec<WarningEntry>,
    ) -> Self {
        Self {
            kind,
            disposition,
            delivery_target,
            recipient,
            messages,
            warnings,
        }
    }
}
