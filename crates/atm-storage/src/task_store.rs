//! Storage-neutral task ledger capability.

use std::collections::HashMap;
use std::sync::Mutex;

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

impl ReminderOutcome {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Emitted => "emitted",
            Self::Unrenderable => "unrenderable",
            Self::Blocked => "blocked",
        }
    }
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

/// Minimal in-memory implementation for composition and contract tests.
#[derive(Debug, Default)]
pub struct DummyTaskStore {
    rows: Mutex<HashMap<(MemberKey, TaskId), TaskRow>>,
    fail_reminders: bool,
}

impl DummyTaskStore {
    #[must_use]
    pub fn with_rows(rows: Vec<TaskRow>, fail_reminders: bool) -> Self {
        let rows = rows
            .into_iter()
            .map(|row| {
                (
                    (
                        MemberKey::new(row.team.clone(), row.assignee.clone()),
                        row.task_id.clone(),
                    ),
                    row,
                )
            })
            .collect();
        Self {
            rows: Mutex::new(rows),
            fail_reminders,
        }
    }

    pub fn row(&self, member: &MemberKey, task_id: &TaskId) -> TaskRow {
        self.rows
            .lock()
            .expect("dummy task rows lock")
            .get(&(member.clone(), task_id.clone()))
            .expect("dummy task row")
            .clone()
    }
}

impl sealed::Sealed for DummyTaskStore {}

impl TaskStore for DummyTaskStore {
    fn load_task(&self, member: &MemberKey, task_id: &TaskId) -> Result<Option<TaskRow>, AtmError> {
        Ok(self
            .rows
            .lock()
            .expect("dummy task rows lock")
            .get(&(member.clone(), task_id.clone()))
            .cloned())
    }

    fn open_tasks(&self, member: &MemberKey) -> Result<Vec<TaskRow>, AtmError> {
        Ok(self
            .rows
            .lock()
            .expect("dummy task rows lock")
            .iter()
            .filter(|((row_member, _), _)| row_member == member)
            .map(|(_, row)| row.clone())
            .collect())
    }

    fn list_tasks(
        &self,
        team: &TeamName,
        member: Option<&AgentName>,
    ) -> Result<Vec<TaskRow>, AtmError> {
        Ok(self
            .rows
            .lock()
            .expect("dummy task rows lock")
            .values()
            .filter(|row| &row.team == team && member.is_none_or(|agent| &row.assignee == agent))
            .cloned()
            .collect())
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
        member: &MemberKey,
        task_id: &TaskId,
        at: IsoTimestamp,
        _outcome: ReminderOutcome,
    ) -> Result<TaskRow, AtmError> {
        if self.fail_reminders {
            return Err(AtmError::new(
                crate::AtmErrorCode::InternalError,
                "injected reminder bookkeeping failure",
            ));
        }
        let mut rows = self.rows.lock().expect("dummy task rows lock");
        let row = rows
            .get_mut(&(member.clone(), task_id.clone()))
            .ok_or_else(|| {
                AtmError::new(crate::AtmErrorCode::InternalError, "dummy task row missing")
            })?;
        row.last_reminded_at = Some(at);
        row.reminder_count = row.reminder_count.saturating_add(1);
        Ok(row.clone())
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
