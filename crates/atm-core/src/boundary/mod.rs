//! Phase R boundary skeleton contracts.

use crate::error::AtmError;
use crate::protocol::{FramePayload, RequestEnvelope, RequestId, ResponseEnvelope};
pub use crate::protocol::{NotificationEvent, RuntimeStatusSnapshot};
use crate::schema::AtmMessageId;
use crate::types::{AgentName, IsoTimestamp, PaneId, TaskId, TeamName};
pub use atm_storage::contract::{AckTransition, Message, MessageKey, TaskState};

/// Workspace-convention seal only; not compiler-enforced outside this crate.
///
/// Only ATM workspace crates may implement boundary traits. Enforced by
/// boundary lint, forbidden-edge rules, and review gates; this is a documented
/// enforcement limitation until the trait surfaces move behind stricter crate
/// extraction or compiler-enforced sealing.
#[doc(hidden)]
pub mod sealed {
    pub trait Sealed {}
}

mod mail;
mod runtime;
mod store;

// Intentional re-export façade: the boundary module is the stable public import
// surface for Phase R/AA contracts, so callers should not need to know whether
// an item lives in `mail` or `store`.
pub use mail::*;
pub use runtime::*;
pub use store::*;

/// BOUNDARY-AtmProtocol — see docs/atm-core/boundaries.md.
pub trait AtmProtocol: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when a protocol request envelope cannot be converted
    /// into a frame payload.
    fn request_to_frame(
        &self,
        request_id: RequestId,
        request: RequestEnvelope,
    ) -> Result<FramePayload, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when a frame payload cannot be decoded into a
    /// protocol request envelope.
    fn request_from_frame(
        &self,
        frame: FramePayload,
    ) -> Result<(RequestId, RequestEnvelope), AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when a protocol response envelope cannot be
    /// converted into a frame payload.
    fn response_to_frame(
        &self,
        request_id: RequestId,
        response: ResponseEnvelope,
    ) -> Result<FramePayload, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when a frame payload cannot be decoded into a
    /// protocol response envelope.
    fn response_from_frame(
        &self,
        frame: FramePayload,
    ) -> Result<(RequestId, ResponseEnvelope), AtmError>;
}

/// BOUNDARY-ClientTransport — see docs/atm-core/boundaries.md.
pub trait ClientTransport: sealed::Sealed + Send + Sync {
    /// # Errors
    ///
    /// Returns `AtmError` when the framed request cannot be delivered or when
    /// the peer returns an unrecoverable protocol response.
    fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError>;
}

/// BOUNDARY-ServerTransport — see docs/atm-core/boundaries.md.
pub trait ServerTransport: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when framing, transport serving, or dispatch handoff
    /// cannot proceed reliably.
    fn serve(
        &self,
        dispatcher: std::sync::Arc<dyn RequestDispatcher + Send + Sync>,
    ) -> Result<(), AtmError>;
}

/// BOUNDARY-RequestDispatcher — see docs/atm-core/boundaries.md.
pub trait RequestDispatcher: sealed::Sealed + Send + Sync {
    /// # Errors
    ///
    /// Returns `AtmError` when protocol request routing or handler dispatch
    /// cannot produce a valid response.
    fn dispatch(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError>;
}

/// BOUNDARY-StatusSource — see docs/atm-core/boundaries.md.
pub trait StatusSource: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when a runtime status snapshot cannot be collected.
    fn snapshot(&self) -> Result<RuntimeStatusSnapshot, AtmError>;
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct PostSendHookEvent {
    pub sender: AgentName,
    pub sender_team: TeamName,
    pub recipient: AgentName,
    pub recipient_team: TeamName,
    pub message_id: AtmMessageId,
    pub description: String,
    pub requires_ack: bool,
    pub is_ack: bool,
    pub task_id: Option<TaskId>,
    pub recipient_pane_id: Option<PaneId>,
}

#[derive(
    Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "snake_case")]
pub enum BuiltInNudgeTemplateKind {
    Delivery,
    DeliveryAck,
    DeliveryTask,
    DeliveryTaskAck,
    Acknowledge,
    AcknowledgeTask,
}

impl BuiltInNudgeTemplateKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Delivery => "delivery",
            Self::DeliveryAck => "delivery_ack",
            Self::DeliveryTask => "delivery_task",
            Self::DeliveryTaskAck => "delivery_task_ack",
            Self::Acknowledge => "acknowledge",
            Self::AcknowledgeTask => "acknowledge_task",
        }
    }

    pub fn from_post_send_event(event: &PostSendHookEvent) -> Self {
        match (event.is_ack, event.task_id.is_some(), event.requires_ack) {
            (true, true, _) => Self::AcknowledgeTask,
            (true, false, _) => Self::Acknowledge,
            (false, true, true) => Self::DeliveryTaskAck,
            (false, true, false) => Self::DeliveryTask,
            (false, false, true) => Self::DeliveryAck,
            (false, false, false) => Self::Delivery,
        }
    }
}

impl std::fmt::Display for BuiltInNudgeTemplateKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for BuiltInNudgeTemplateKind {
    type Err = crate::error::AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "delivery" => Ok(Self::Delivery),
            "delivery_ack" => Ok(Self::DeliveryAck),
            "delivery_task" => Ok(Self::DeliveryTask),
            "delivery_task_ack" => Ok(Self::DeliveryTaskAck),
            "acknowledge" => Ok(Self::Acknowledge),
            "acknowledge_task" => Ok(Self::AcknowledgeTask),
            other => Err(crate::error::AtmError::validation(format!(
                "unsupported built-in nudge template kind `{other}`"
            ))
            .with_recovery(
                "Use one of delivery, delivery_ack, delivery_task, delivery_task_ack, acknowledge, or acknowledge_task.",
            )),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct TeamNudgeTemplateOverrideRow {
    pub team_name: TeamName,
    pub kind: BuiltInNudgeTemplateKind,
    pub template_body: String,
    pub updated_at: IsoTimestamp,
}

/// BOUNDARY-PostSendHookEmitter — see docs/atm-core/boundaries.md.
pub trait PostSendHookEmitter: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when one direct post-send emission attempt fails
    /// after durable message persistence has already succeeded.
    fn emit(&self, event: &PostSendHookEvent) -> Result<(), AtmError>;
}

/// BOUNDARY-GraftPostSendPort — see docs/atm-core/boundaries.md.
pub trait GraftPostSendPort: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when one graft-backed post-send emission attempt
    /// fails after durable message persistence has already succeeded.
    fn deliver_post_send(&self, event: &PostSendHookEvent) -> Result<(), AtmError>;
}
