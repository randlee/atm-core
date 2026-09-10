//! Storage-owned mutation request contract for the v2 logical task ledger.
//!
//! Callers prepare canonical messages before crossing this boundary; they do
//! not receive a SQLite connection, writer permit, or permission to render
//! assignment text inside storage.

use serde::{Deserialize, Serialize};

use crate::contract::{Message, sealed};
use crate::error::AtmError;
use crate::task_state::{TaskLifecycleState, TaskOperationId, TaskOutcome, TaskPriority};
use crate::types::{MemberKey, TaskId};

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
        successor: PreparedAssignment,
    },
}

/// Idempotent, compare-and-swap mutation request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskMutationRequest {
    pub operation_id: TaskOperationId,
    pub actor: MemberKey,
    pub task_id: TaskId,
    pub expected_revision: Option<u64>,
    pub operation: TaskOperation,
}

/// Bounded current projection returned for a committed or replayed mutation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskMutationOutcome {
    pub task_id: TaskId,
    pub state: TaskLifecycleState,
    pub revision: u64,
    pub replayed: bool,
}

/// Tokio-safe, sealed mutation boundary implemented by the storage adapter.
#[async_trait::async_trait]
pub trait AsyncTaskMutationStore: sealed::Sealed + Send + Sync {
    async fn apply(&self, request: TaskMutationRequest) -> Result<TaskMutationOutcome, AtmError>;
}
