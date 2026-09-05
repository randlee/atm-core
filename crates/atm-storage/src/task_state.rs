//! Backend-neutral task ledger types and the pure task state machine.

use serde::{Deserialize, Serialize};

use crate::error::AtmError;
use crate::schema::AtmMessageId;
use crate::types::{AgentName, IsoTimestamp, TaskId, TeamName};

use crate::task_store::ReminderOutcome;

/// The reserved sender identity used by daemon-originated task events.
pub const DAEMON_ACTOR_NAME: &str = "atm-daemon";

const RECOVERY: &str = "Run: atm list --task-events <task_id> --member <assignee>";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Assigned,
    Active,
    Complete,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskEvent {
    Assigned,
    Acked,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskActor {
    Member(AgentName),
    Daemon,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskRow {
    pub team: TeamName,
    pub task_id: TaskId,
    pub assignee: AgentName,
    pub assigner: AgentName,
    pub state: TaskState,
    pub assignment_message_id: AtmMessageId,
    pub description: String,
    pub assigned_at: IsoTimestamp,
    pub updated_at: IsoTimestamp,
    pub last_reminded_at: Option<IsoTimestamp>,
    pub reminder_count: u32,
    pub lead_notified_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskRejected {
    pub detail: String,
}

impl TaskRejected {
    fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: format!("{}; {RECOVERY}", detail.into()),
        }
    }

    pub fn into_atm_error(self) -> AtmError {
        AtmError::validation(self.detail)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    To(TaskState),
    NoOp,
}

/// Applies the row-local task transition table without touching storage.
pub fn transition(state: Option<TaskState>, event: TaskEvent) -> Result<Transition, TaskRejected> {
    match (state, event) {
        (None, TaskEvent::Assigned) => Ok(Transition::To(TaskState::Assigned)),
        (None, TaskEvent::Acked) => Ok(Transition::NoOp),
        (None, TaskEvent::Completed) => Err(TaskRejected::new("no open task for actor")),
        (Some(TaskState::Assigned), TaskEvent::Assigned) => Ok(Transition::To(TaskState::Assigned)),
        (Some(TaskState::Assigned), TaskEvent::Acked) => Ok(Transition::To(TaskState::Active)),
        (Some(TaskState::Assigned), TaskEvent::Completed) => {
            Ok(Transition::To(TaskState::Complete))
        }
        (Some(TaskState::Active), TaskEvent::Assigned) => Ok(Transition::To(TaskState::Active)),
        (Some(TaskState::Active), TaskEvent::Acked) => Ok(Transition::To(TaskState::Active)),
        (Some(TaskState::Active), TaskEvent::Completed) => Ok(Transition::To(TaskState::Complete)),
        (Some(TaskState::Complete), TaskEvent::Assigned) => {
            Err(TaskRejected::new("task already complete; use a new id"))
        }
        (Some(TaskState::Complete), TaskEvent::Acked)
        | (Some(TaskState::Complete), TaskEvent::Completed) => {
            Err(TaskRejected::new("task already complete"))
        }
    }
}

/// Checks the cross-row task admission guards without touching storage.
pub fn admit(
    row: Option<&TaskRow>,
    open: &[TaskRow],
    event: TaskEvent,
    actor: &AgentName,
) -> Result<(), TaskRejected> {
    let Some(row) = row else {
        return if event == TaskEvent::Completed {
            Err(TaskRejected::new(format!("no open task for {actor}")))
        } else {
            Ok(())
        };
    };

    if event == TaskEvent::Acked && row.state == TaskState::Assigned {
        if let Some(other) = open.iter().find(|candidate| {
            candidate.state == TaskState::Active && candidate.task_id != row.task_id
        }) {
            return Err(TaskRejected::new(format!(
                "task {} is active; complete it first",
                other.task_id
            )));
        }
    }

    if event == TaskEvent::Completed && actor != &row.assignee && actor != &row.assigner {
        return Err(TaskRejected::new(format!(
            "task {} is not assigned to or by {actor}",
            row.task_id
        )));
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskEventKind {
    Assigned,
    Acked,
    Completed,
    Rejected,
    Reminded,
    LeadNotified,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskEventMarker {
    Resend,
    AssignmentMissing,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskEventRow {
    pub team: TeamName,
    pub task_id: TaskId,
    pub assignee: AgentName,
    pub seq: u64,
    pub at: IsoTimestamp,
    pub event: TaskEventKind,
    pub from_state: Option<TaskState>,
    pub to_state: Option<TaskState>,
    pub actor: TaskActor,
    pub message_id: Option<AtmMessageId>,
    pub outcome: Option<ReminderOutcome>,
    pub marker: Option<TaskEventMarker>,
    pub detail: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{TaskEvent, TaskRow, TaskState, Transition, admit, transition};
    use crate::schema::AtmMessageId;
    use crate::types::{AgentName, IsoTimestamp, TaskId, TeamName};

    fn row(task_id: &str, state: TaskState) -> TaskRow {
        TaskRow {
            team: "team".parse::<TeamName>().expect("team"),
            task_id: task_id.parse::<TaskId>().expect("task"),
            assignee: "assignee".parse::<AgentName>().expect("assignee"),
            assigner: "assigner".parse::<AgentName>().expect("assigner"),
            state,
            assignment_message_id: AtmMessageId::new(),
            description: "task".to_owned(),
            assigned_at: "2026-09-04T00:00:00Z"
                .parse::<IsoTimestamp>()
                .expect("time"),
            updated_at: "2026-09-04T00:00:00Z"
                .parse::<IsoTimestamp>()
                .expect("time"),
            last_reminded_at: None,
            reminder_count: 0,
            lead_notified_count: 0,
        }
    }

    #[test]
    fn transition_table_is_complete() {
        let cases = [
            (
                None,
                TaskEvent::Assigned,
                Some(Transition::To(TaskState::Assigned)),
            ),
            (None, TaskEvent::Acked, Some(Transition::NoOp)),
            (None, TaskEvent::Completed, None),
            (
                Some(TaskState::Assigned),
                TaskEvent::Assigned,
                Some(Transition::To(TaskState::Assigned)),
            ),
            (
                Some(TaskState::Assigned),
                TaskEvent::Acked,
                Some(Transition::To(TaskState::Active)),
            ),
            (
                Some(TaskState::Assigned),
                TaskEvent::Completed,
                Some(Transition::To(TaskState::Complete)),
            ),
            (
                Some(TaskState::Active),
                TaskEvent::Assigned,
                Some(Transition::To(TaskState::Active)),
            ),
            (
                Some(TaskState::Active),
                TaskEvent::Acked,
                Some(Transition::To(TaskState::Active)),
            ),
            (
                Some(TaskState::Active),
                TaskEvent::Completed,
                Some(Transition::To(TaskState::Complete)),
            ),
            (Some(TaskState::Complete), TaskEvent::Assigned, None),
            (Some(TaskState::Complete), TaskEvent::Acked, None),
            (Some(TaskState::Complete), TaskEvent::Completed, None),
        ];

        for (state, event, expected) in cases {
            assert_eq!(transition(state, event).ok(), expected);
        }
    }

    #[test]
    fn admission_guards_reject_missing_completion_and_second_active_task() {
        let assignee: AgentName = "assignee".parse().expect("assignee");
        let missing = admit(None, &[], TaskEvent::Completed, &assignee).expect_err("missing task");
        assert!(missing.detail.contains("no open task for assignee"));

        let assigned = row("AX.3", TaskState::Assigned);
        let active = row("AX.2", TaskState::Active);
        let rejected = admit(
            Some(&assigned),
            &[assigned.clone(), active],
            TaskEvent::Acked,
            &assignee,
        )
        .expect_err("one active task guard");
        assert!(
            rejected
                .detail
                .contains("task AX.2 is active; complete it first")
        );
    }
}
