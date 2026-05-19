use std::fmt;
use std::path::{Path, PathBuf};

use crate::delivery_policy::DeliveryRecipientSnapshot;
use crate::schema::{AtmMessageId, MessageEnvelope};
use crate::send::{
    DeliveryPersistenceDisposition, DeliveryPersistenceResult, ResolvedRecipient, WarningEntry,
};
use crate::types::{AgentName, TaskId, TeamName};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeliveryPlanDisposition {
    Persisted,
    SqliteFailedRecovered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeliveryPlanKind {
    Send,
    Reply,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LogicalMessage {
    pub(crate) envelope: MessageEnvelope,
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
        envelope: MessageEnvelope,
        requires_ack: bool,
        is_ack: bool,
    ) -> Result<Self, LogicalMessageError> {
        if envelope.message_id.is_none() {
            return Err(LogicalMessageError::MissingMessageId);
        }
        Ok(Self {
            envelope,
            requires_ack,
            is_ack,
        })
    }

    pub(crate) fn message_id(&self) -> AtmMessageId {
        // Invariant: `LogicalMessage::new` rejects envelopes without a message
        // id, and every constructor path in this module goes through that
        // validation before a message ever reaches a delivery plan.
        self.envelope
            .message_id
            .expect("validated logical delivery messages always have a message id")
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

pub(crate) fn delivery_plan_disposition(
    disposition: DeliveryPersistenceDisposition,
) -> DeliveryPlanDisposition {
    match disposition {
        DeliveryPersistenceDisposition::Persisted => DeliveryPlanDisposition::Persisted,
        DeliveryPersistenceDisposition::SqliteFailedRecovered => {
            DeliveryPlanDisposition::SqliteFailedRecovered
        }
    }
}

pub(crate) fn delivery_target_for_snapshot(
    inbox_path: &Path,
    delivery_snapshot: &DeliveryRecipientSnapshot,
) -> DeliveryTarget {
    match delivery_snapshot.harness {
        crate::delivery_policy::DeliveryHarnessPath::ClaudeCode => DeliveryTarget::ClaudeCode {
            inbox_path: inbox_path.to_path_buf(),
            recipient: delivery_snapshot.clone(),
        },
        crate::delivery_policy::DeliveryHarnessPath::NonClaude => DeliveryTarget::NonClaude {
            recipient: delivery_snapshot.clone(),
        },
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

impl DeliveryTarget {
    pub(crate) fn harness_path(&self) -> crate::delivery_policy::DeliveryHarnessPath {
        match self {
            Self::ClaudeCode { recipient, .. } | Self::NonClaude { recipient } => recipient.harness,
        }
    }

    pub(crate) fn recipient_snapshot(&self) -> &DeliveryRecipientSnapshot {
        match self {
            Self::ClaudeCode { recipient, .. } | Self::NonClaude { recipient } => recipient,
        }
    }
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
    pub(crate) kind: DeliveryPlanKind,
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
        kind: DeliveryPlanKind,
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
            kind,
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
