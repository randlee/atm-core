//! Phase R boundary skeleton contracts.

use crate::address::AgentAddress;
use crate::error::AtmError;
pub use crate::protocol::{NotificationEvent, RuntimeStatusSnapshot};
use crate::schema::AtmMessageId;
use crate::types::{AgentName, ChatId, HostName, PaneId, TaskId, TeamName};
pub use atm_storage::contract::{AckTransition, Message, MessageKey, TaskState};
pub use atm_storage::{
    BuiltInNudgeTemplateKind, NudgeTemplateOverrideStore, TeamNudgeTemplateOverrideMode,
    TeamNudgeTemplateOverrideRow,
};

/// Durable at-most-once delivery state for deferred (`atm queue`) nudges.
///
/// Re-exported from `atm_storage::contract`; see that crate for the
/// canonical trait, error, and claim-tracking documentation.
#[doc(inline)]
pub use atm_storage::contract::{MAX_NUDGE_ATTEMPTS, NudgeClaim, PendingNudgeStore};
/// The canonical durable-mailbox member key for nudge and queue surfaces.
///
/// Re-exported from `atm_storage::types`; see that crate for the canonical
/// definition.
#[doc(inline)]
pub use atm_storage::types::MemberKey;

/// Which kind of recipient nudge a committed dispatch represents.
///
/// `Steer` is the historical immediate best-effort receiver nudge (tmux
/// `send-keys`, Graft loopback injection). `Queue` marks a message for
/// deferred, durable, at-most-once delivery via [`PendingNudgeStore`]
/// instead of an immediate emission attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NudgeKind {
    Steer,
    Queue,
}

/// The retained tmux receiver nudge confirms its literal payload with two
/// `send-keys Enter` events separated by this bounded delay. The active Tokio
/// runtime and CLI command share this contract; frozen legacy daemon source
/// remains reference-only until Phase AM removes it.
pub const TMUX_DOUBLE_ENTER_DELAY: std::time::Duration = std::time::Duration::from_millis(275);

/// Literal tmux key used for each confirmation in the shared nudge sequence.
pub const TMUX_NUDGE_CONFIRM_KEY: &str = "Enter";

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
mod template_composer;

// Intentional re-export façade: the boundary module is the stable public import
// surface for Phase R/AA contracts, so callers should not need to know whether
// an item lives in `mail` or `store`.
pub use atm_storage::TemplateOutputFormat;
pub use mail::*;
pub use message_received_hook_emitter::{
    AsyncMessageReceivedHookEmitter, MessageReceivedHookEmitter, MessageReceivedHookSelector,
};
pub use store::*;
pub use template_composer::{
    RenderedBody, SourceSpan, TemplateComposer, TemplateInspection, TemplateReference,
    TemplateReferenceKind, TemplateRoot, TemplateSource,
};

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
    /// Host authenticated by peer ingress for a cross-host sender.
    ///
    /// Local sends intentionally leave this empty. The value is transport
    /// provenance, never caller-provided nudge data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_host: Option<HostName>,
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
            self.sender_host.clone(),
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

/// Target metadata for a Herdr prompt. The live agent name is carried by the
/// dispatch event; only the optional per-member session is persisted.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct HerdrNudgeTarget {
    pub session: Option<String>,
}

/// Backend-specific payload for a local steer.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum LocalSteerTarget {
    Tmux(LocalTmuxNudgeTarget),
    Herdr(HerdrNudgeTarget),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct GraftNudgeTarget {
    pub recipient: AgentName,
    pub recipient_team: TeamName,
    /// Canonical database-resolved `<atm …>` nudge text for the receiver.
    pub rendered_nudge: String,
    /// Immutable body of the message that triggered this nudge.
    ///
    /// Unlike `description`, this preserves the complete admitted content so
    /// an embedded host never needs to perform a follow-up mailbox read just
    /// to inject the message it was notified about.
    pub message_body: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum PostSendBuiltInTarget {
    /// Local steer through the explicitly selected backend.
    LocalSteer(LocalSteerTarget),
    Graft(GraftNudgeTarget),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct BuiltInPostSendDispatch {
    pub event: PostSendHookEvent,
    pub target: PostSendBuiltInTarget,
    /// Whether this dispatch is an immediate steer or a deferred queue
    /// marker. See [`NudgeKind`].
    pub kind: NudgeKind,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PostSendEmissionPath {
    ExternalHook,
    LocalTmux,
    LocalHerdr,
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
