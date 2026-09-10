//! Storage-neutral idle-attention scheduling contracts.
//!
//! Attention is deliberately an identifier-only projection over the separate
//! message queue and task ledger.  It must never become another persistence
//! home for rendered messages or task objective text.

use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::contract::{ReadDeadline, ReadLaneError, RosterStateRevision, sealed};
use crate::error::AtmError;
use crate::schema::AtmMessageId;
use crate::task_state::{AssignmentAttempt, TaskPriority};
use crate::types::{MemberKey, TaskId};

/// The two independently-owned sources of attention for one idle member.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AttentionLane {
    Ephemeral,
    PersistentTask,
}

impl AttentionLane {
    #[must_use]
    pub const fn other(self) -> Self {
        match self {
            Self::Ephemeral => Self::PersistentTask,
            Self::PersistentTask => Self::Ephemeral,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ephemeral => "ephemeral",
            Self::PersistentTask => "persistent_task",
        }
    }
}

/// Opaque identity minted from one committed idle roster revision.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct IdleOpportunityId(Ulid);

impl IdleOpportunityId {
    #[must_use]
    pub fn new() -> Self {
        Self(Ulid::new())
    }

    #[must_use]
    pub const fn as_ulid(self) -> Ulid {
        self.0
    }

    pub fn parse(value: &str) -> Result<Self, AtmError> {
        Ulid::from_string(value)
            .map(Self)
            .map_err(|_| AtmError::validation("idle opportunity id is invalid"))
    }
}

impl Default for IdleOpportunityId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for IdleOpportunityId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// One committed canonical idle observation, safe to carry across scheduler
/// and storage boundaries because it contains no delivery content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdleOpportunity {
    pub id: IdleOpportunityId,
    pub member: MemberKey,
    pub roster_state_revision: RosterStateRevision,
}

/// The first FIFO message candidate, owned and claimed by `PendingNudgeStore`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EphemeralMessageCandidate {
    pub message_id: AtmMessageId,
}

/// Bounded metadata sufficient to revalidate a task reminder selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistentTaskCandidate {
    pub task_id: TaskId,
    pub attempt: AssignmentAttempt,
    pub assignment_message_id: AtmMessageId,
    pub priority: TaskPriority,
}

/// Independent per-lane candidates for one idle opportunity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttentionCandidates {
    pub ephemeral: Option<EphemeralMessageCandidate>,
    pub persistent_task: Option<PersistentTaskCandidate>,
}

/// One selected identifier-only item. Rendered titles and bodies are loaded
/// only after the owning lane has been claimed and revalidated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttentionItem {
    EphemeralMessage {
        member: MemberKey,
        message_id: AtmMessageId,
    },
    PersistentTaskReminder {
        member: MemberKey,
        task_id: TaskId,
        attempt: AssignmentAttempt,
        assignment_message_id: AtmMessageId,
    },
}

impl AttentionItem {
    #[must_use]
    pub const fn lane(&self) -> AttentionLane {
        match self {
            Self::EphemeralMessage { .. } => AttentionLane::Ephemeral,
            Self::PersistentTaskReminder { .. } => AttentionLane::PersistentTask,
        }
    }
}

/// Pure selector result. The cursor advances only when this selection is
/// durably reserved with the opportunity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttentionSelection {
    pub item: Option<AttentionItem>,
    pub next_lane: AttentionLane,
}

/// Selects at most one candidate. A lone lane progresses without delay; when
/// both lanes are due the durable cursor decides fairly.
#[must_use]
pub fn select_attention_item(
    member: MemberKey,
    next_lane: AttentionLane,
    candidates: AttentionCandidates,
) -> AttentionSelection {
    let selected = match (candidates.ephemeral, candidates.persistent_task) {
        (None, None) => None,
        (Some(message), None) => Some(AttentionItem::EphemeralMessage {
            member,
            message_id: message.message_id,
        }),
        (None, Some(task)) => Some(AttentionItem::PersistentTaskReminder {
            member,
            task_id: task.task_id,
            attempt: task.attempt,
            assignment_message_id: task.assignment_message_id,
        }),
        (Some(message), Some(task)) => match next_lane {
            AttentionLane::Ephemeral => Some(AttentionItem::EphemeralMessage {
                member,
                message_id: message.message_id,
            }),
            AttentionLane::PersistentTask => Some(AttentionItem::PersistentTaskReminder {
                member,
                task_id: task.task_id,
                attempt: task.attempt,
                assignment_message_id: task.assignment_message_id,
            }),
        },
    };
    AttentionSelection {
        next_lane: selected
            .as_ref()
            .map_or(next_lane, |item| item.lane().other()),
        item: selected,
    }
}

/// Durable fair-lane cursor for one member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttentionCursor {
    pub next_lane: AttentionLane,
    pub revision: u64,
}

/// The finite result of one reservation. A permanent delivery failure never
/// terminalizes the referenced task or message.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AttentionReservationStatus {
    Reserved,
    Delivered,
    Stale,
    PermanentlyFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttentionFinalizeOutcome {
    Delivered,
    Stale,
    RetryableFailure,
}

/// The durable, replayable reservation for one opportunity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttentionReservation {
    pub opportunity: IdleOpportunity,
    pub item: AttentionItem,
    pub status: AttentionReservationStatus,
    pub failed_attempts: u32,
}

/// One optimistic reservation request. `expected_cursor_revision` prevents a
/// concurrent opportunity from silently skipping an alternation turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttentionReservationRequest {
    pub opportunity: IdleOpportunity,
    pub expected_cursor_revision: u64,
    pub item: AttentionItem,
}

/// One idempotent reservation finalization request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttentionFinalizeRequest {
    pub member: MemberKey,
    pub opportunity_id: IdleOpportunityId,
    pub outcome: AttentionFinalizeOutcome,
}

/// Storage-neutral synchronous scheduler metadata capability.
pub trait AttentionScheduleStore: sealed::Sealed + Send + Sync {
    fn load_cursor(&self, member: &MemberKey) -> Result<AttentionCursor, AtmError>;
    fn reserve(
        &self,
        request: AttentionReservationRequest,
    ) -> Result<AttentionReservation, AtmError>;
    fn finalize(&self, request: AttentionFinalizeRequest)
    -> Result<AttentionReservation, AtmError>;
}

/// Tokio-safe access to the same scheduler metadata capability.
#[async_trait::async_trait]
pub trait AsyncAttentionScheduleStore: sealed::Sealed + Send + Sync {
    async fn load_cursor(
        &self,
        member: MemberKey,
        deadline: ReadDeadline,
    ) -> Result<AttentionCursor, ReadLaneError>;
    async fn reserve(
        &self,
        request: AttentionReservationRequest,
        deadline: ReadDeadline,
    ) -> Result<AttentionReservation, AtmError>;
    async fn finalize(
        &self,
        request: AttentionFinalizeRequest,
        deadline: ReadDeadline,
    ) -> Result<AttentionReservation, AtmError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::AtmMessageId;
    use crate::types::{AgentName, TeamName};

    fn member() -> MemberKey {
        MemberKey::new(
            "team".parse::<TeamName>().expect("team"),
            "agent".parse::<AgentName>().expect("agent"),
        )
    }

    fn task() -> PersistentTaskCandidate {
        PersistentTaskCandidate {
            task_id: "task".parse().expect("task"),
            attempt: AssignmentAttempt::FIRST,
            assignment_message_id: AtmMessageId::new(),
            priority: TaskPriority::Normal,
        }
    }

    #[test]
    fn alternates_when_both_lanes_are_due() {
        let candidates = AttentionCandidates {
            ephemeral: Some(EphemeralMessageCandidate {
                message_id: AtmMessageId::new(),
            }),
            persistent_task: Some(task()),
        };
        let first = select_attention_item(member(), AttentionLane::Ephemeral, candidates.clone());
        let second = select_attention_item(member(), first.next_lane, candidates);
        assert_eq!(first.item.expect("first").lane(), AttentionLane::Ephemeral);
        assert_eq!(
            second.item.expect("second").lane(),
            AttentionLane::PersistentTask
        );
    }

    #[test]
    fn single_lane_progresses_without_advancing_the_other_lane() {
        let selection = select_attention_item(
            member(),
            AttentionLane::PersistentTask,
            AttentionCandidates {
                ephemeral: Some(EphemeralMessageCandidate {
                    message_id: AtmMessageId::new(),
                }),
                persistent_task: None,
            },
        );
        assert_eq!(
            selection.item.expect("message").lane(),
            AttentionLane::Ephemeral
        );
        assert_eq!(selection.next_lane, AttentionLane::PersistentTask);
    }

    #[test]
    fn cursor_breaks_a_cross_lane_tie_in_favor_of_the_requested_lane() {
        let member = member();
        let task = task();
        let task_id = task.task_id.clone();
        let selection = select_attention_item(
            member,
            AttentionLane::PersistentTask,
            AttentionCandidates {
                ephemeral: Some(EphemeralMessageCandidate {
                    message_id: AtmMessageId::new(),
                }),
                persistent_task: Some(task),
            },
        );

        assert!(matches!(
            selection.item,
            Some(AttentionItem::PersistentTaskReminder { task_id: selected, .. })
                if selected == task_id
        ));
        assert_eq!(selection.next_lane, AttentionLane::Ephemeral);
    }

    #[test]
    fn empty_candidates_emit_nothing_and_preserve_the_cursor() {
        let selection = select_attention_item(
            member(),
            AttentionLane::PersistentTask,
            AttentionCandidates::default(),
        );

        assert_eq!(selection.item, None);
        assert_eq!(selection.next_lane, AttentionLane::PersistentTask);
    }

    #[test]
    fn persistent_lane_progresses_when_ephemeral_lane_is_empty() {
        let task = task();
        let task_id = task.task_id.clone();
        let selection = select_attention_item(
            member(),
            AttentionLane::Ephemeral,
            AttentionCandidates {
                ephemeral: None,
                persistent_task: Some(task),
            },
        );

        assert!(matches!(
            selection.item,
            Some(AttentionItem::PersistentTaskReminder { task_id: selected, .. })
                if selected == task_id
        ));
        assert_eq!(selection.next_lane, AttentionLane::Ephemeral);
    }
}
