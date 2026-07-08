//! Phase R boundary skeleton contracts.

use crate::error::AtmError;
use crate::protocol::{FramePayload, RequestEnvelope, RequestId, ResponseEnvelope};
pub use crate::protocol::{NotificationEvent, RuntimeStatusSnapshot};
use crate::schema::AtmMessageId;
use crate::types::{AgentName, PaneId, TaskId, TeamName};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostSendHookEvent {
    pub sender: AgentName,
    pub sender_team: TeamName,
    pub recipient: AgentName,
    pub recipient_team: TeamName,
    pub message_id: AtmMessageId,
    pub message: String,
    pub requires_ack: bool,
    pub is_ack: bool,
    pub task_id: Option<TaskId>,
    pub recipient_pane_id: Option<PaneId>,
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
