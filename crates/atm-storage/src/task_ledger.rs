//! Storage-owned task-ledger reader contract.

use crate::{
    AgentName, LogicalTaskRow, ReadDeadline, ReadLaneError, TaskAssignmentAttempt, TaskEventRow,
    TaskId, TaskLedgerScope, TaskLifecycleEventRow, TaskRow,
};

/// Tokio-safe read-only task ledger; implementations use a bounded storage-owned reader lane.
#[async_trait::async_trait]
pub trait AsyncTaskLedgerReader: crate::contract::sealed::Sealed + Send + Sync {
    async fn list_tasks(
        &self,
        team: crate::TeamName,
        member: Option<AgentName>,
        deadline: ReadDeadline,
    ) -> Result<Vec<TaskRow>, ReadLaneError>;

    async fn list_task_events(
        &self,
        team: crate::TeamName,
        task_id: TaskId,
        member: Option<AgentName>,
        deadline: ReadDeadline,
    ) -> Result<Vec<TaskEventRow>, ReadLaneError>;

    /// Returns logical tasks in storage-owned binding task-list order.
    async fn list_logical_tasks(
        &self,
        _team: crate::TeamName,
        _member: Option<AgentName>,
        _scope: TaskLedgerScope,
        _limit: Option<usize>,
        _deadline: ReadDeadline,
    ) -> Result<Vec<LogicalTaskRow>, ReadLaneError> {
        Err(logical_task_projections_unavailable())
    }

    /// Returns the active task or first assigned task, never blocked or closed work.
    async fn top_runnable_task(
        &self,
        _team: crate::TeamName,
        _member: AgentName,
        _deadline: ReadDeadline,
    ) -> Result<Option<LogicalTaskRow>, ReadLaneError> {
        Err(logical_task_projections_unavailable())
    }

    async fn load_logical_task(
        &self,
        _team: crate::TeamName,
        _task_id: TaskId,
        _deadline: ReadDeadline,
    ) -> Result<Option<LogicalTaskRow>, ReadLaneError> {
        Err(logical_task_projections_unavailable())
    }

    async fn list_task_assignment_attempts(
        &self,
        _team: crate::TeamName,
        _task_id: TaskId,
        _deadline: ReadDeadline,
    ) -> Result<Vec<TaskAssignmentAttempt>, ReadLaneError> {
        Err(logical_task_projections_unavailable())
    }

    async fn list_task_lifecycle_events(
        &self,
        _team: crate::TeamName,
        _task_id: TaskId,
        _limit: Option<usize>,
        _deadline: ReadDeadline,
    ) -> Result<Vec<TaskLifecycleEventRow>, ReadLaneError> {
        Err(logical_task_projections_unavailable())
    }
}

fn logical_task_projections_unavailable() -> ReadLaneError {
    ReadLaneError::Unavailable {
        message: "logical task projections are unavailable in this storage adapter".to_owned(),
    }
}
