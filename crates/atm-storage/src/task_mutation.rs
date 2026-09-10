//! Storage-owned mutation request contract for the v2 logical task ledger.
//!
//! Callers prepare canonical messages before crossing this boundary; they do
//! not receive a SQLite connection, writer permit, or permission to render
//! assignment text inside storage.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::contract::{Message, sealed};
use crate::error::AtmError;
use crate::schema::AtmMessageId;
use crate::task_state::{
    AssignmentAttempt, TaskLifecycleState, TaskOperationId, TaskOutcome, TaskPriority, TaskRevision,
};
use crate::task_store::MessageWriteOrigin;
use crate::types::{AgentName, IsoTimestamp, MemberKey, TaskId};

/// A validated, admitted message committed by the same task transaction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PreparedMessage {
    pub message: Message,
}

/// Immutable data for a new assignment attempt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PreparedAssignment {
    pub assignee: MemberKey,
    pub assigner: MemberKey,
    pub priority: TaskPriority,
    /// The prepared admission's origin. Only a locally admitted recipient can
    /// join this one-transaction task mutation; peer delivery has no durable
    /// cross-host handoff transaction and must be rejected before persistence.
    pub delivery_origin: MessageWriteOrigin,
    pub message: PreparedMessage,
    pub template_sha: Option<crate::types::TemplateSha>,
}

/// One typed lifecycle mutation. All state changes are applied only through
/// [`AsyncTaskMutationStore`], never through the synchronous compatibility
/// reader/store.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskOperation {
    Assign(PreparedAssignment),
    Start,
    Block {
        reason: String,
    },
    Unblock {
        resolution: String,
    },
    Reassign(PreparedAssignment),
    Reopen(PreparedAssignment),
    /// Appends one successful, attempt-checked reminder audit without changing
    /// lifecycle state. Scheduler-owned metadata never mutates this row.
    RecordReminder {
        attempt: AssignmentAttempt,
        at: IsoTimestamp,
    },
    /// Appends one successful lead-notification audit for the current
    /// assignment attempt. The scheduler invokes this only after the lead
    /// mail write succeeds at a ten-reminder escalation boundary.
    RecordLeadNotified {
        attempt: AssignmentAttempt,
        at: IsoTimestamp,
        lead: AgentName,
        message_id: crate::schema::AtmMessageId,
    },
    Close {
        outcome: TaskOutcome,
        handoff: PreparedMessage,
    },
    LegacyCloseSucceeded {
        completion_notice: PreparedMessage,
    },
    Supersede {
        handoff: PreparedMessage,
        successor_task_id: TaskId,
        successor: Box<PreparedAssignment>,
    },
}

/// Idempotent, compare-and-swap mutation request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskMutationRequest {
    pub operation_id: TaskOperationId,
    pub actor: MemberKey,
    pub task_id: TaskId,
    pub expected_revision: Option<TaskRevision>,
    pub operation: TaskOperation,
}

/// Bounded current projection returned for a committed or replayed mutation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskMutationOutcome {
    pub task_id: TaskId,
    pub state: TaskLifecycleState,
    pub revision: TaskRevision,
    /// Assignment or terminal-handoff message committed with this operation.
    /// The field is optional for state-only mutations and defaults while old
    /// persisted operation results remain readable during the interface bump.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<AtmMessageId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub successor_task_id: Option<TaskId>,
    /// The immutable current projection snapshot committed by the operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_assignee: Option<AgentName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_attempt: Option<crate::task_state::AssignmentAttempt>,
    pub replayed: bool,
}

/// A scheduler-owned, compare-and-swap reminder audit. This request cannot
/// represent a lifecycle transition or a message write.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskReminderAuditRequest {
    pub operation_id: TaskOperationId,
    pub actor: MemberKey,
    pub task_id: TaskId,
    pub expected_revision: TaskRevision,
    pub attempt: AssignmentAttempt,
    pub at: IsoTimestamp,
}

/// A scheduler-owned, compare-and-swap lead-notification audit. This request
/// cannot represent a lifecycle transition or a message write.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskLeadNotificationAuditRequest {
    pub operation_id: TaskOperationId,
    pub actor: MemberKey,
    pub task_id: TaskId,
    pub expected_revision: TaskRevision,
    pub attempt: AssignmentAttempt,
    pub at: IsoTimestamp,
    pub lead: AgentName,
    pub message_id: AtmMessageId,
}

/// A caller-owned deadline for admission to the ordered task-mutation writer.
///
/// The deadline is transport metadata, not part of the idempotent mutation
/// request: retry identity must never vary with elapsed time.
#[derive(Debug, Clone, Copy)]
pub struct TaskMutationDeadline(Instant);

impl TaskMutationDeadline {
    pub fn after(remaining: Duration) -> Result<Self, AtmError> {
        if remaining.is_zero() {
            return Err(AtmError::validation(
                "task mutation deadline must be non-zero",
            ));
        }
        Ok(Self(Instant::now() + remaining))
    }

    #[must_use]
    pub fn is_expired(self) -> bool {
        Instant::now() >= self.0
    }

    #[must_use]
    pub fn already_expired() -> Self {
        Self(Instant::now())
    }
}

/// Tokio-safe, sealed mutation boundary implemented by the storage adapter.
#[async_trait::async_trait]
pub trait AsyncTaskMutationStore: sealed::Sealed + Send + Sync {
    async fn apply(&self, request: TaskMutationRequest) -> Result<TaskMutationOutcome, AtmError>;

    /// Applies a task mutation only while its caller's writer-admission budget
    /// remains. Implementations must reject an expired queued operation before
    /// it executes on the shared SQLite writer lane.
    async fn apply_before(
        &self,
        request: TaskMutationRequest,
        deadline: TaskMutationDeadline,
    ) -> Result<TaskMutationOutcome, AtmError>;
}

/// Tokio-safe capability for scheduler audit writes only. The attention
/// scheduler receives this instead of [`AsyncTaskMutationStore`] so its type
/// cannot express a task lifecycle transition.
#[async_trait::async_trait]
pub trait AsyncTaskSchedulerAuditStore: sealed::Sealed + Send + Sync {
    async fn record_reminder(
        &self,
        request: TaskReminderAuditRequest,
    ) -> Result<TaskMutationOutcome, AtmError>;

    async fn record_lead_notification(
        &self,
        request: TaskLeadNotificationAuditRequest,
    ) -> Result<TaskMutationOutcome, AtmError>;
}
