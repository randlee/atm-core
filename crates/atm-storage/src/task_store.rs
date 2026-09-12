//! Storage-neutral task ledger capability.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::contract::{AsyncTaskLedgerReader, ReadDeadline, ReadLaneError, sealed};
use crate::error::AtmError;
use crate::schema::AtmMessageId;
use crate::task_state::{QueuePosition, TaskEventRow, TaskRow};
use crate::types::{AgentName, IsoTimestamp, MemberKey, TaskId, TeamName};
use crate::{AgentAddress, MoveTarget};

/// Selects the daemon-wide or team-specific escalation recipient list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EscalationScope {
    Daemon,
    Team(TeamName),
}

/// Reminder count at which an open task is considered stalled and escalated.
pub const TASK_STALLED_REMINDER_THRESHOLD: u32 = 10;

/// Minimum spacing between task reminders for one assignee.
pub const TASK_REMINDER_INTERVAL_MS: i64 = 60_000;

/// Computes the next due time for a deferred queue or task reminder.
#[must_use]
pub fn next_reminder_due(now: IsoTimestamp) -> IsoTimestamp {
    IsoTimestamp::from_datetime(
        now.into_inner() + chrono::Duration::milliseconds(TASK_REMINDER_INTERVAL_MS),
    )
}

/// Consecutive refused closes that hold task prompting and escalate the run.
///
/// See Phase BA plan §2 R5.
pub const TASK_CONSECUTIVE_REFUSAL_THRESHOLD: u32 = 3;

/// Maximum recipients retained for one daemon or team escalation scope.
pub const MAX_ESCALATION_RECIPIENTS: usize = 8;

impl EscalationScope {
    #[must_use]
    pub fn key(&self) -> String {
        match self {
            Self::Daemon => "daemon".to_owned(),
            Self::Team(team) => format!("team:{team}"),
        }
    }
}

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
    fn load_task(&self, team: &TeamName, task_id: &TaskId) -> Result<Option<TaskRow>, AtmError>;
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
    fn move_task(
        &self,
        _team: &TeamName,
        _task_id: &TaskId,
        _actor: &AgentName,
        _target: &MoveTarget,
        _at: IsoTimestamp,
    ) -> Result<(AgentName, QueuePosition, QueuePosition), AtmError> {
        Err(AtmError::daemon_unavailable(
            "task store does not implement ordered task movement",
        ))
    }
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

    fn list_escalation_recipients(
        &self,
        scope: &EscalationScope,
    ) -> Result<Vec<AgentAddress>, AtmError>;

    fn add_escalation_recipient(
        &self,
        scope: &EscalationScope,
        address: &AgentAddress,
        at: IsoTimestamp,
    ) -> Result<bool, AtmError>;

    fn remove_escalation_recipient(
        &self,
        scope: &EscalationScope,
        address: &AgentAddress,
    ) -> Result<bool, AtmError>;

    fn effective_escalation_recipients(
        &self,
        team: &TeamName,
    ) -> Result<Vec<AgentAddress>, AtmError> {
        let team_recipients =
            self.list_escalation_recipients(&EscalationScope::Team(team.clone()))?;
        if team_recipients.is_empty() {
            self.list_escalation_recipients(&EscalationScope::Daemon)
        } else {
            Ok(team_recipients)
        }
    }
}

/// Minimal in-memory implementation for composition and contract tests.
#[derive(Debug, Default)]
pub struct DummyTaskStore {
    rows: Mutex<HashMap<(TeamName, TaskId), TaskRow>>,
    escalation_recipients: Mutex<HashMap<String, Vec<AgentAddress>>>,
    fail_reminders: bool,
}

impl DummyTaskStore {
    #[must_use]
    pub fn with_rows(rows: Vec<TaskRow>, fail_reminders: bool) -> Self {
        let rows = rows
            .into_iter()
            .map(|row| ((row.team.clone(), row.task_id.clone()), row))
            .collect();
        Self {
            rows: Mutex::new(rows),
            escalation_recipients: Mutex::new(HashMap::new()),
            fail_reminders,
        }
    }

    pub fn row(&self, member: &MemberKey, task_id: &TaskId) -> TaskRow {
        self.rows
            .lock()
            .expect("dummy task rows lock")
            .get(&(member.team().clone(), task_id.clone()))
            .expect("dummy task row")
            .clone()
    }
}

impl sealed::Sealed for DummyTaskStore {}

#[async_trait::async_trait]
impl AsyncTaskLedgerReader for DummyTaskStore {
    async fn open_tasks_for_team(
        &self,
        team: TeamName,
        _deadline: ReadDeadline,
    ) -> Result<Vec<TaskRow>, ReadLaneError> {
        let mut rows = self
            .rows
            .lock()
            .map_err(|_| ReadLaneError::Unavailable {
                message: "dummy task rows lock poisoned".to_owned(),
            })?
            .values()
            .filter(|row| row.team == team && row.state.is_open())
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by_key(|row| {
            (
                row.assignee.clone(),
                row.position,
                row.assigned_at,
                row.task_id.clone(),
            )
        });
        Ok(rows)
    }

    async fn refusal_run(
        &self,
        _team: TeamName,
        _assignee: AgentName,
        _deadline: ReadDeadline,
    ) -> Result<crate::RefusalRun, ReadLaneError> {
        Ok(crate::RefusalRun {
            count: 0,
            started_at: None,
        })
    }

    async fn list_tasks(
        &self,
        team: TeamName,
        member: Option<AgentName>,
        _deadline: ReadDeadline,
    ) -> Result<Vec<TaskRow>, ReadLaneError> {
        Ok(
            TaskStore::list_tasks(self, &team, member.as_ref()).map_err(|error| {
                ReadLaneError::Unavailable {
                    message: error.to_string(),
                }
            })?,
        )
    }

    async fn list_task_events(
        &self,
        team: TeamName,
        task_id: TaskId,
        member: Option<AgentName>,
        _deadline: ReadDeadline,
    ) -> Result<Vec<TaskEventRow>, ReadLaneError> {
        Ok(
            TaskStore::list_task_events(self, &team, &task_id, member.as_ref()).map_err(
                |error| ReadLaneError::Unavailable {
                    message: error.to_string(),
                },
            )?,
        )
    }
}

impl TaskStore for DummyTaskStore {
    fn load_task(&self, team: &TeamName, task_id: &TaskId) -> Result<Option<TaskRow>, AtmError> {
        Ok(self
            .rows
            .lock()
            .expect("dummy task rows lock")
            .get(&(team.clone(), task_id.clone()))
            .cloned())
    }

    fn open_tasks(&self, member: &MemberKey) -> Result<Vec<TaskRow>, AtmError> {
        Ok(self
            .rows
            .lock()
            .expect("dummy task rows lock")
            .iter()
            .filter(|((team, _), row)| team == member.team() && &row.assignee == member.agent())
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
            .get_mut(&(member.team().clone(), task_id.clone()))
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

    fn list_escalation_recipients(
        &self,
        scope: &EscalationScope,
    ) -> Result<Vec<AgentAddress>, AtmError> {
        Ok(self
            .escalation_recipients
            .lock()
            .expect("dummy escalation recipients lock")
            .get(&scope.key())
            .cloned()
            .unwrap_or_default())
    }

    fn add_escalation_recipient(
        &self,
        scope: &EscalationScope,
        address: &AgentAddress,
        _at: IsoTimestamp,
    ) -> Result<bool, AtmError> {
        let mut recipients = self
            .escalation_recipients
            .lock()
            .expect("dummy escalation recipients lock");
        let list = recipients.entry(scope.key()).or_default();
        if list.iter().any(|existing| existing == address) {
            return Ok(false);
        }
        if list.len() >= MAX_ESCALATION_RECIPIENTS {
            return Err(AtmError::validation(format!(
                "escalation recipient scope already has the maximum of {MAX_ESCALATION_RECIPIENTS} recipients"
            )));
        }
        list.push(address.clone());
        Ok(true)
    }

    fn remove_escalation_recipient(
        &self,
        scope: &EscalationScope,
        address: &AgentAddress,
    ) -> Result<bool, AtmError> {
        let mut recipients = self
            .escalation_recipients
            .lock()
            .expect("dummy escalation recipients lock");
        let Some(list) = recipients.get_mut(&scope.key()) else {
            return Ok(false);
        };
        let before = list.len();
        list.retain(|existing| existing != address);
        Ok(list.len() != before)
    }
}
