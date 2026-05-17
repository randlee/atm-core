use std::path::PathBuf;

use crate::delivery_policy::DeliveryRecipientSnapshot;
use crate::schema::{AtmMessageId, MessageEnvelope};
use crate::send::{ResolvedRecipient, WarningEntry};
use crate::types::{AgentName, TaskId, TeamName};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeliveryPlanDisposition {
    Persisted,
    SqliteFailedRecovered,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LogicalMessage {
    pub(crate) envelope: MessageEnvelope,
    pub(crate) requires_ack: bool,
    pub(crate) is_ack: bool,
}

impl LogicalMessage {
    pub(crate) fn new(
        envelope: MessageEnvelope,
        requires_ack: bool,
        is_ack: bool,
    ) -> Result<Self, &'static str> {
        if envelope.message_id.is_none() {
            return Err("logical delivery messages must carry a message id");
        }
        Ok(Self {
            envelope,
            requires_ack,
            is_ack,
        })
    }

    pub(crate) fn message_id(&self) -> AtmMessageId {
        self.envelope
            .message_id
            .expect("validated logical delivery messages always have a message id")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DeliveryTarget {
    ClaudeCode {
        inbox_path: PathBuf,
        recipient: DeliveryRecipientSnapshot,
    },
    NonClaude {
        recipient: DeliveryRecipientSnapshot,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NotificationTarget {
    pub(crate) sender: AgentName,
    pub(crate) sender_team: Option<TeamName>,
    pub(crate) message_id: AtmMessageId,
    pub(crate) requires_ack: bool,
    pub(crate) is_ack: bool,
    pub(crate) task_id: Option<TaskId>,
}

impl NotificationTarget {
    pub(crate) fn from_logical_message(message: &LogicalMessage) -> Self {
        Self {
            sender: message.envelope.from.clone(),
            sender_team: message.envelope.source_team.clone(),
            message_id: message.message_id(),
            requires_ack: message.requires_ack,
            is_ack: message.is_ack,
            task_id: message.envelope.task_id.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DeliveryPlan {
    pub(crate) disposition: DeliveryPlanDisposition,
    pub(crate) delivery_target: DeliveryTarget,
    pub(crate) recipient: ResolvedRecipient,
    pub(crate) recipient_pane_id: Option<String>,
    pub(crate) messages: Vec<LogicalMessage>,
    pub(crate) notifications: Vec<NotificationTarget>,
    pub(crate) warnings: Vec<WarningEntry>,
}

impl DeliveryPlan {
    pub(crate) fn new(
        disposition: DeliveryPlanDisposition,
        delivery_target: DeliveryTarget,
        recipient: ResolvedRecipient,
        recipient_pane_id: Option<String>,
        messages: Vec<LogicalMessage>,
        warnings: Vec<WarningEntry>,
    ) -> Self {
        let notifications = messages
            .iter()
            .map(NotificationTarget::from_logical_message)
            .collect();
        Self {
            disposition,
            delivery_target,
            recipient,
            recipient_pane_id,
            messages,
            notifications,
            warnings,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReplyDeliveryPlan {
    pub(crate) disposition: DeliveryPlanDisposition,
    pub(crate) delivery_target: DeliveryTarget,
    pub(crate) recipient: ResolvedRecipient,
    pub(crate) recipient_pane_id: Option<String>,
    pub(crate) messages: Vec<LogicalMessage>,
    pub(crate) notifications: Vec<NotificationTarget>,
    pub(crate) warnings: Vec<WarningEntry>,
}

impl ReplyDeliveryPlan {
    pub(crate) fn new(
        disposition: DeliveryPlanDisposition,
        delivery_target: DeliveryTarget,
        recipient: ResolvedRecipient,
        recipient_pane_id: Option<String>,
        messages: Vec<LogicalMessage>,
        warnings: Vec<WarningEntry>,
    ) -> Self {
        let notifications = messages
            .iter()
            .map(NotificationTarget::from_logical_message)
            .collect();
        Self {
            disposition,
            delivery_target,
            recipient,
            recipient_pane_id,
            messages,
            notifications,
            warnings,
        }
    }
}
