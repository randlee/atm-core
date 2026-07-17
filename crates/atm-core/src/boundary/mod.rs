//! Phase R boundary skeleton contracts.

use crate::error::AtmError;
use crate::protocol::{FramePayload, RequestEnvelope, RequestId, ResponseEnvelope};
pub use crate::protocol::{NotificationEvent, RuntimeStatusSnapshot};
use crate::schema::AtmMessageId;
use crate::types::{AgentName, PaneId, TaskId, TeamName};
pub use atm_storage::contract::{AckTransition, Message, MessageKey, TaskState};
pub use atm_storage::{
    BuiltInNudgeTemplateKind, NudgeTemplateOverrideStore, TeamNudgeTemplateOverrideMode,
    TeamNudgeTemplateOverrideRow,
};

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
    ///
    /// `peer_origin` carries the presented remote host (for example the
    /// connecting peer's IP address) when the request arrived over the
    /// cross-host peer transport, and is `None` for same-host requests
    /// dispatched over the local IPC transport.
    fn dispatch(
        &self,
        request: RequestEnvelope,
        peer_origin: Option<&str>,
    ) -> Result<ResponseEnvelope, AtmError>;
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

pub fn built_in_nudge_template_kind_from_post_send_event(
    event: &PostSendHookEvent,
) -> BuiltInNudgeTemplateKind {
    match (event.is_ack, event.task_id.is_some(), event.requires_ack) {
        (true, true, _) => BuiltInNudgeTemplateKind::AcknowledgeTask,
        (true, false, _) => BuiltInNudgeTemplateKind::Acknowledge,
        (false, true, true) => BuiltInNudgeTemplateKind::DeliveryTaskAck,
        (false, true, false) => BuiltInNudgeTemplateKind::DeliveryTask,
        (false, false, true) => BuiltInNudgeTemplateKind::DeliveryAck,
        (false, false, false) => BuiltInNudgeTemplateKind::Delivery,
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ResolvedBuiltInNudgeTemplate {
    pub kind: BuiltInNudgeTemplateKind,
    pub body: Option<String>,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BuiltInNudgeSinkTarget {
    Tmux,
    Graft,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct InternalNudgeEnvelope {
    pub event: PostSendHookEvent,
    pub sink_target: BuiltInNudgeSinkTarget,
    pub template: ResolvedBuiltInNudgeTemplate,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct LocalTmuxNudgeTarget {
    pub pane_id: PaneId,
    pub rendered_nudge: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct GraftNudgeTarget {
    pub recipient: AgentName,
    pub recipient_team: TeamName,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum PostSendBuiltInTarget {
    LocalTmux(LocalTmuxNudgeTarget),
    Graft(GraftNudgeTarget),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct BuiltInPostSendDispatch {
    pub event: PostSendHookEvent,
    pub target: PostSendBuiltInTarget,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PostSendEmissionPath {
    ExternalHook,
    LocalTmux,
    GraftPort,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct HookExecutionSummary {
    matched_rules: usize,
    succeeded_rules: usize,
    failed_rules: usize,
}

impl HookExecutionSummary {
    pub fn new(
        matched_rules: usize,
        succeeded_rules: usize,
        failed_rules: usize,
    ) -> Result<Self, AtmError> {
        if succeeded_rules + failed_rules > matched_rules {
            return Err(AtmError::validation(format!(
                "invalid post-send hook execution summary: succeeded ({succeeded_rules}) + failed ({failed_rules}) exceeds matched ({matched_rules})"
            ))
            .with_recovery(
                "Count each matching post-send hook rule exactly once before constructing hook execution summary state.",
            ));
        }
        Ok(Self {
            matched_rules,
            succeeded_rules,
            failed_rules,
        })
    }

    pub const fn matched_rules(&self) -> usize {
        self.matched_rules
    }

    pub const fn succeeded_rules(&self) -> usize {
        self.succeeded_rules
    }

    pub const fn failed_rules(&self) -> usize {
        self.failed_rules
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum PostSendEmissionOutcome {
    NoCapability {
        hook_summary: HookExecutionSummary,
    },
    Delivered {
        path: PostSendEmissionPath,
        hook_summary: HookExecutionSummary,
    },
    Failed {
        hook_summary: HookExecutionSummary,
        warning: crate::send::WarningEntry,
    },
}
/// BOUNDARY-PostSendHookEmitter — see docs/atm-core/boundaries.md.
pub trait PostSendHookEmitter: sealed::Sealed + Send + Sync {
    /// # Errors
    ///
    /// Returns `AtmError` when one direct post-send emission attempt fails
    /// after durable message persistence has already succeeded.
    fn emit_post_send(
        &self,
        dispatch: &BuiltInPostSendDispatch,
    ) -> Result<PostSendEmissionPath, AtmError>;
}

/// BOUNDARY-AckReplyDeliveryPort — see docs/atm-core/boundaries.md.
pub trait AckReplyDeliveryPort: sealed::Sealed + Send + Sync {
    /// # Errors
    ///
    /// Returns `AtmError` when one cross-host acknowledgement reply cannot be
    /// routed back to the original remote sender's host after local
    /// acknowledgement state has already persisted.
    fn deliver_reply(&self, reply: crate::send::SendRequest) -> Result<(), AtmError>;
}

/// BOUNDARY-GraftPostSendPort — see docs/atm-core/boundaries.md.
pub trait GraftPostSendPort: sealed::Sealed + Send + Sync {
    /// # Errors
    ///
    /// Returns `AtmError` when one graft-backed post-send emission attempt
    /// fails after durable message persistence has already succeeded.
    fn deliver_post_send(
        &self,
        event: &PostSendHookEvent,
        target: &GraftNudgeTarget,
    ) -> Result<(), AtmError>;
}
