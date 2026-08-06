//! Phase R boundary skeleton contracts.

use crate::address::AgentAddress;
use crate::error::AtmError;
pub use crate::protocol::{NotificationEvent, RuntimeStatusSnapshot};
use crate::schema::AtmMessageId;
use crate::types::{AgentName, ChatId, PaneId, TaskId, TeamName};
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
mod message_received_hook_emitter;
mod store;

// Intentional re-export façade: the boundary module is the stable public import
// surface for Phase R/AA contracts, so callers should not need to know whether
// an item lives in `mail` or `store`.
pub use mail::*;
pub use message_received_hook_emitter::{
    AsyncMessageReceivedHookEmitter, MessageReceivedHookEmitter,
};
pub use store::*;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_chat_id: Option<ChatId>,
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

impl PostSendHookEvent {
    /// The canonical source address carried by every post-write nudge.
    pub fn source_address(&self) -> AgentAddress {
        AgentAddress::new(
            self.sender.clone(),
            self.sender_chat_id.clone(),
            Some(self.sender_team.clone()),
            None,
        )
        .expect("post-send event sender always has a team")
    }
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
            )));
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
// `PostSendHookEmitter` deliberately has no compatibility alias. Any use is
// a compiler failure and must migrate to `MessageReceivedHookEmitter`.
