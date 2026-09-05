//! Storage-neutral task ledger capability.

use serde::{Deserialize, Serialize};

use crate::contract::sealed;
use crate::error::AtmError;
use crate::schema::AtmMessageId;
use crate::task_state::{TaskEventRow, TaskRow};
use crate::types::{AgentName, IsoTimestamp, MemberKey, TaskId, TeamName};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReminderOutcome {
    Emitted,
    Unrenderable,
    Blocked,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum MessageWriteOrigin {
    #[default]
    Local,
    Peer,
}

/// Read and audit capability for the task ledger. Message-state transitions
/// are intentionally applied only in a backend writer transaction.
pub trait TaskStore: sealed::Sealed + Send + Sync {
    fn load_task(&self, member: &MemberKey, task_id: &TaskId) -> Result<Option<TaskRow>, AtmError>;
    fn open_tasks(&self, member: &MemberKey) -> Result<Vec<TaskRow>, AtmError>;
    fn list_tasks(
        &self,
        team: &TeamName,
        member: Option<&AgentName>,
    ) -> Result<Vec<TaskRow>, AtmError>;
    fn list_task_events(
        &self,
        team: &TeamName,
        task_id: &TaskId,
        assignee: Option<&AgentName>,
    ) -> Result<Vec<TaskEventRow>, AtmError>;
    fn record_reminder(
        &self,
        member: &MemberKey,
        task_id: &TaskId,
        at: IsoTimestamp,
        outcome: ReminderOutcome,
    ) -> Result<TaskRow, AtmError>;
    fn record_lead_notified(
        &self,
        member: &MemberKey,
        task_id: &TaskId,
        at: IsoTimestamp,
        lead: &AgentName,
        message_id: &AtmMessageId,
    ) -> Result<(), AtmError>;
}

/// Minimal no-op implementation for composition and contract tests.
#[derive(Debug, Default)]
pub struct DummyTaskStore;

impl sealed::Sealed for DummyTaskStore {}

impl TaskStore for DummyTaskStore {
    fn load_task(
        &self,
        _member: &MemberKey,
        _task_id: &TaskId,
    ) -> Result<Option<TaskRow>, AtmError> {
        Ok(None)
    }

    fn open_tasks(&self, _member: &MemberKey) -> Result<Vec<TaskRow>, AtmError> {
        Ok(Vec::new())
    }

    fn list_tasks(
        &self,
        _team: &TeamName,
        _member: Option<&AgentName>,
    ) -> Result<Vec<TaskRow>, AtmError> {
        Ok(Vec::new())
    }

    fn list_task_events(
        &self,
        _team: &TeamName,
        _task_id: &TaskId,
        _assignee: Option<&AgentName>,
    ) -> Result<Vec<TaskEventRow>, AtmError> {
        Ok(Vec::new())
    }

    fn record_reminder(
        &self,
        _member: &MemberKey,
        _task_id: &TaskId,
        _at: IsoTimestamp,
        _outcome: ReminderOutcome,
    ) -> Result<TaskRow, AtmError> {
        Err(AtmError::validation(
            "task store test double has no task row",
        ))
    }

    fn record_lead_notified(
        &self,
        _member: &MemberKey,
        _task_id: &TaskId,
        _at: IsoTimestamp,
        _lead: &AgentName,
        _message_id: &AtmMessageId,
    ) -> Result<(), AtmError> {
        Ok(())
    }
}
