//! Transport-neutral task command contracts.
//!
//! This module deliberately carries composed message content rather than a
//! template path. Template source verification belongs to the canonical write
//! admission pipeline before a request reaches the durable task transaction.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::AtmError;
use crate::types::{TaskId, TeamName};
use atm_storage::MemberKey;
use atm_storage::{
    AssignmentAttempt, LogicalTaskRow, TaskLifecycleEventRow, TaskLifecycleState, TaskOperationId,
    TaskOutcome, TaskPriority, TemplateSha,
};

pub const DEFAULT_TASK_PAGE_LIMIT: usize = 200;
pub const MAX_TASK_PAGE_LIMIT: usize = 10_000;

/// One command accepted by the canonical task service.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskCommandRequest {
    List(TaskListQuery),
    Events(TaskEventQuery),
    Mutate(TaskMutationCommand),
}

/// A bounded task-list query. Closed work is never mixed into the actionable
/// default list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskListQuery {
    pub team: TeamName,
    pub assignee: Option<crate::types::AgentName>,
    pub scope: TaskListScope,
    pub page: TaskPage,
}

/// Selects either actionable work or terminal history.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskListScope {
    Open,
    Closed,
}

/// A bounded event-history query for one stable task identity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskEventQuery {
    pub team: TeamName,
    pub task_id: TaskId,
    pub assignee: Option<crate::types::AgentName>,
    pub page: TaskPage,
}

/// Explicit pagination prevents accidental unbounded task-history reads.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskPage {
    Bounded { limit: usize },
    All,
}

impl TaskPage {
    pub fn bounded(limit: usize) -> Result<Self, AtmError> {
        if !(1..=MAX_TASK_PAGE_LIMIT).contains(&limit) {
            return Err(AtmError::validation(format!(
                "task limit must be between 1 and {MAX_TASK_PAGE_LIMIT}",
            )));
        }
        Ok(Self::Bounded { limit })
    }

    #[must_use]
    pub const fn default_bounded() -> Self {
        Self::Bounded {
            limit: DEFAULT_TASK_PAGE_LIMIT,
        }
    }
}

/// An idempotent mutation request authenticated as `actor`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskMutationCommand {
    pub operation_id: TaskOperationId,
    pub actor: MemberKey,
    pub task_id: TaskId,
    pub expected_revision: Option<u64>,
    pub action: TaskAction,
}

/// Every lifecycle mutation accepted by the canonical service.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskAction {
    Assign(AssignmentInput),
    Start,
    Block {
        reason: NonEmptyText,
    },
    Unblock {
        resolution: NonEmptyText,
    },
    Reassign(AssignmentInput),
    Reopen(AssignmentInput),
    Complete(HandoffInput),
    /// Compatibility-only completion provenance for deprecated `atm send`.
    LegacyComplete(LegacyCompletionNoticeInput),
    Fail(HandoffInput),
    Abort {
        reason: AbortInput,
        handoff: HandoffInput,
    },
}

/// A new durable assignment attempt and its already-composed local mail.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssignmentInput {
    pub assignee: MemberKey,
    pub priority: TaskPriority,
    pub message: ComposedMessageInput,
}

/// A terminal, same-host handoff and its already-composed local mail.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandoffInput {
    pub recipient: MemberKey,
    pub message: ComposedMessageInput,
}

/// The narrow legacy terminal-notice exemption retained during migration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LegacyCompletionNoticeInput {
    pub recipient: MemberKey,
    pub message: ComposedMessageInput,
}

/// Terminal abort metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AbortInput {
    Cancelled,
    Superseded {
        successor_task_id: TaskId,
        successor: AssignmentInput,
    },
}

/// Template-agnostic rendered message input for a task mutation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComposedMessageInput {
    pub body: NonEmptyText,
    pub template_sha: Option<TemplateSha>,
}

/// Text admitted before a task request crosses the service boundary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct NonEmptyText(String);

impl NonEmptyText {
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(AtmError::validation("task text must not be empty"));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Service response corresponding to [`TaskCommandRequest`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskCommandResponse {
    List(TaskListResponse),
    Events(TaskEventsResponse),
    Mutation(TaskMutationResponse),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskListResponse {
    pub rows: Vec<LogicalTaskRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskEventsResponse {
    pub rows: Vec<TaskLifecycleEventRow>,
}

/// Stable outcome envelope for a committed or replayed mutation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskMutationResponse {
    pub operation_id: TaskOperationId,
    pub task_id: TaskId,
    pub prior_state: Option<TaskLifecycleState>,
    pub state: TaskLifecycleState,
    pub revision: u64,
    pub current_attempt: AssignmentAttempt,
    pub current_assignee: crate::types::AgentName,
    pub outcome: Option<TaskOutcome>,
    pub message_id: Option<crate::schema::AtmMessageId>,
    pub successor_task_id: Option<TaskId>,
    pub replayed: bool,
}

/// The sole shared policy/service boundary for task commands.
#[async_trait]
pub trait TaskCommandService: crate::boundary::sealed::Sealed + Send + Sync {
    async fn execute(&self, request: TaskCommandRequest) -> Result<TaskCommandResponse, AtmError>;
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_TASK_PAGE_LIMIT, MAX_TASK_PAGE_LIMIT, TaskPage};

    #[test]
    fn task_page_enforces_the_documented_bounds() {
        assert_eq!(
            TaskPage::default_bounded(),
            TaskPage::Bounded {
                limit: DEFAULT_TASK_PAGE_LIMIT,
            }
        );
        assert!(TaskPage::bounded(0).is_err());
        assert!(TaskPage::bounded(MAX_TASK_PAGE_LIMIT + 1).is_err());
        assert_eq!(
            TaskPage::bounded(MAX_TASK_PAGE_LIMIT).expect("upper bound"),
            TaskPage::Bounded {
                limit: MAX_TASK_PAGE_LIMIT,
            }
        );
    }
}
