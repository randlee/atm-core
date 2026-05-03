use crate::store::{InsertOutcome, MessageKey, StoreBoundary, StoreError};
use crate::types::{IsoTimestamp, TaskId};

/// Canonical task status persisted by the Phase Q store boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    PendingAck,
    Acknowledged,
}

impl TaskStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PendingAck => "pending_ack",
            Self::Acknowledged => "acknowledged",
        }
    }
}

/// Durable task row keyed by validated `task_id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRecord {
    pub task_id: TaskId,
    pub message_key: MessageKey,
    pub status: TaskStatus,
    pub created_at: IsoTimestamp,
    pub acknowledged_at: Option<IsoTimestamp>,
    pub metadata_json: Option<String>,
}

/// Durable task-store boundary kept separate from `MailStore` so mail/task
/// concerns do not collapse into a single god-interface.
pub trait TaskStore: StoreBoundary {
    fn upsert_task(&self, task: &TaskRecord) -> Result<InsertOutcome<TaskRecord>, StoreError>;

    fn load_task(&self, task_id: &TaskId) -> Result<Option<TaskRecord>, StoreError>;
}
