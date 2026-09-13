//! Phase R boundary skeleton contracts.

use crate::address::AgentAddress;
use crate::error::AtmError;
pub use crate::protocol::{NotificationEvent, RuntimeStatusSnapshot};
use crate::schema::AtmMessageId;
use crate::types::{AgentName, ChatId, HostName, PaneId, TaskId, TeamName};
/// Durable roster store used by replacement-runtime maintenance projections.
#[doc(inline)]
pub use atm_storage::contract::RosterStore as DurableRosterStore;
pub use atm_storage::contract::{
    AckTransition, AsyncMailboxReader, MailboxScope, Message, MessageKey, MessageQuery,
    ReadLaneError,
};
pub use atm_storage::{
    AsyncTaskLedgerReader, BuiltInNudgeTemplateKind, DAEMON_ACTOR_NAME, EscalationScope,
    MAX_ESCALATION_RECIPIENTS, NudgeTemplateOverrideStore, PromptHandoff, PromptTrigger,
    ReadDeadline, ReminderOutcome, StaleNudgeTemplateOverrideKind,
    TASK_CONSECUTIVE_REFUSAL_THRESHOLD, TASK_REMINDER_INTERVAL_MS, TASK_STALLED_REMINDER_THRESHOLD,
    TaskCloseOutcome, TaskClosedOutcome, TaskEventKind, TaskEventRow, TaskRow, TaskStore,
    TaskTransition, TeamNudgeTemplateOverrideMode, TeamNudgeTemplateOverrideRow, next_reminder_due,
};
pub use atm_storage::{TaskOp, TaskState};

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

impl NudgeKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Steer => "steer",
            Self::Queue => "queue",
        }
    }
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

mod herdr_breaker;
mod herdr_endpoint;
mod mail;
mod message_received_hook_emitter;
mod store;
mod template_composer;

// Intentional re-export façade: the boundary module is the stable public import
// surface for Phase R/AA contracts, so callers should not need to know whether
// an item lives in `mail` or `store`.
pub use atm_storage::TemplateOutputFormat;
pub use herdr_breaker::HerdrBreakerDoctor;
pub use herdr_endpoint::HerdrEndpointDoctor;
pub use mail::*;
#[cfg(any(test, feature = "test-utils"))]
pub use message_received_hook_emitter::NoopMessageReceivedHookSelector;
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_transition: Option<TaskTransition>,
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
    delivery_kind: NudgeKind,
) -> BuiltInNudgeTemplateKind {
    use BuiltInNudgeTemplateKind as K;
    match (
        event.is_ack,
        event.task_transition,
        event.requires_ack,
        delivery_kind,
    ) {
        (true, _, _, _) => K::Acknowledge,
        (false, Some(TaskTransition::Queued { .. }), _, _) => K::TaskQueued,
        (false, Some(TaskTransition::Ready), _, _) => K::TaskReady,
        (false, Some(TaskTransition::Reminder { .. }), _, _) => K::TaskReminder,
        (false, Some(TaskTransition::Started), _, _) => K::TaskStarted,
        (false, Some(TaskTransition::Complete { .. }), _, _) => K::TaskComplete,
        (false, Some(TaskTransition::Closed { .. }), _, _) => K::TaskClosed,
        (false, None, false, NudgeKind::Steer) => K::Delivery,
        (false, None, true, NudgeKind::Steer) => K::DeliveryAck,
        (false, None, false, NudgeKind::Queue) => K::Queue,
        (false, None, true, NudgeKind::Queue) => K::QueueAck,
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

/// Target metadata for a Herdr prompt, resolved from durable roster data
/// before the dispatch crosses into the process adapter.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct HerdrNudgeTarget {
    pub agent: crate::HerdrAgentName,
    pub session: Option<crate::HerdrSession>,
    pub rendered_nudge: String,
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
}

/// Target for a bare-CLI queue handoff into the daemon-owned RAM FIFO.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct QueuePullTarget {
    pub team: TeamName,
    pub agent: AgentName,
    pub kind: NudgeKind,
    pub msg_id: AtmMessageId,
    pub body: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum PostSendBuiltInTarget {
    /// Local steer through the explicitly selected backend.
    LocalSteer(LocalSteerTarget),
    Graft(GraftNudgeTarget),
    QueuePull(QueuePullTarget),
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
    QueuePull,
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

#[cfg(test)]
mod tests {
    use super::{
        BuiltInNudgeTemplateKind as K, NudgeKind, PostSendHookEvent, TaskCloseOutcome,
        TaskClosedOutcome, TaskTransition, built_in_nudge_template_kind_from_post_send_event,
    };
    use crate::schema::AtmMessageId;
    use crate::types::{AgentName, TeamName};

    #[derive(serde::Deserialize, serde::Serialize)]
    struct FrozenPostSendHookEventV17 {
        sender: AgentName,
        sender_chat_id: Option<crate::types::ChatId>,
        sender_team: TeamName,
        sender_host: Option<crate::types::HostName>,
        recipient: AgentName,
        recipient_team: TeamName,
        message_id: AtmMessageId,
        description: String,
        requires_ack: bool,
        is_ack: bool,
        task_id: Option<crate::types::TaskId>,
        recipient_pane_id: Option<crate::types::PaneId>,
    }

    fn event() -> PostSendHookEvent {
        PostSendHookEvent {
            sender: AgentName::from_validated("sender"),
            sender_chat_id: None,
            sender_team: TeamName::from_validated("team"),
            sender_host: None,
            recipient: AgentName::from_validated("recipient"),
            recipient_team: TeamName::from_validated("team"),
            message_id: AtmMessageId::new(),
            description: "test".to_owned(),
            requires_ack: false,
            is_ack: false,
            task_id: None,
            task_transition: None,
            recipient_pane_id: None,
        }
    }

    #[test]
    fn kind_decision_covers_every_transition() {
        let transitions = [
            (TaskTransition::Queued { position: 2 }, K::TaskQueued),
            (TaskTransition::Ready, K::TaskReady),
            (TaskTransition::Reminder { attempt: 1 }, K::TaskReminder),
            (TaskTransition::Started, K::TaskStarted),
            (
                TaskTransition::Complete {
                    outcome: TaskCloseOutcome::Completed,
                },
                K::TaskComplete,
            ),
            (
                TaskTransition::Closed {
                    outcome: TaskClosedOutcome::Cancelled,
                },
                K::TaskClosed,
            ),
        ];
        for (transition, expected) in transitions {
            let mut value = event();
            value.task_transition = Some(transition);
            assert_eq!(
                built_in_nudge_template_kind_from_post_send_event(&value, NudgeKind::Steer),
                expected
            );
        }

        for (is_ack, requires_ack, kind, expected) in [
            (true, false, NudgeKind::Steer, K::Acknowledge),
            (true, false, NudgeKind::Queue, K::Acknowledge),
            (false, false, NudgeKind::Steer, K::Delivery),
            (false, true, NudgeKind::Steer, K::DeliveryAck),
            (false, false, NudgeKind::Queue, K::Queue),
            (false, true, NudgeKind::Queue, K::QueueAck),
        ] {
            let mut value = event();
            value.is_ack = is_ack;
            value.requires_ack = requires_ack;
            assert_eq!(
                built_in_nudge_template_kind_from_post_send_event(&value, kind),
                expected
            );
        }
    }

    #[test]
    fn frozen_1_7_event_shape_decodes_1_8_payload_with_task_transition() {
        let fixture = serde_json::json!({
            "sender": "sender",
            "sender_chat_id": null,
            "sender_team": "team",
            "recipient": "recipient",
            "recipient_team": "team",
            "message_id": "01KX1TEST00000000000000000",
            "description": "transition payload",
            "requires_ack": false,
            "is_ack": false,
            "task_id": "BB.1",
            "task_transition": { "transition": "started" },
            "recipient_pane_id": null
        });
        let frozen: FrozenPostSendHookEventV17 =
            serde_json::from_value(fixture.clone()).expect("1.7 consumer ignores additive field");
        assert!(
            serde_json::to_value(&frozen)
                .expect("serialize frozen shape")
                .get("task_transition")
                .is_none()
        );
        let current: PostSendHookEvent =
            serde_json::from_value(fixture).expect("1.8 event decodes");
        assert_eq!(current.task_transition, Some(TaskTransition::Started));
    }

    #[test]
    fn task_linked_event_without_transition_renders_non_task_kind() {
        let mut value = event();
        value.task_id = Some("BB.1".parse().expect("task id"));
        assert_eq!(
            built_in_nudge_template_kind_from_post_send_event(&value, NudgeKind::Steer),
            K::Delivery,
            "D3/P14 require a task-linked event without a transition to fall through"
        );
        assert_eq!(
            built_in_nudge_template_kind_from_post_send_event(&value, NudgeKind::Queue),
            K::Queue,
            "the fallback preserves the requested delivery family"
        );
    }
}
// `PostSendHookEmitter` deliberately has no compatibility alias. Any use is
// a compiler failure and must migrate to `MessageReceivedHookEmitter`.
