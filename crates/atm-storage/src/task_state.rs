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

impl TaskState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Assigned => "assigned",
            Self::Active => "active",
            Self::Complete => "complete",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskEvent {
    Assigned,
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
pub struct Transition(pub TaskState);

/// Applies the row-local task transition table without touching storage.
pub fn transition(
    state: Option<TaskState>,
    event: TaskEvent,
    task_id: &TaskId,
    actor: &AgentName,
) -> Result<Transition, TaskRejected> {
    match (state, event) {
        (None, TaskEvent::Assigned) => Ok(Transition(TaskState::Assigned)),
        (None, TaskEvent::Completed) => Err(TaskRejected::new(format!(
            "no open task {task_id} for {actor}"
        ))),
        (Some(TaskState::Assigned), TaskEvent::Assigned) => Ok(Transition(TaskState::Assigned)),
        (Some(TaskState::Assigned), TaskEvent::Completed) => Ok(Transition(TaskState::Complete)),
        (Some(TaskState::Active), TaskEvent::Assigned) => Ok(Transition(TaskState::Active)),
        (Some(TaskState::Active), TaskEvent::Completed) => Ok(Transition(TaskState::Complete)),
        (Some(TaskState::Complete), TaskEvent::Assigned | TaskEvent::Completed) => Err(
            TaskRejected::new(format!("task {task_id} is already complete")),
        ),
    }
}

/// Checks the cross-row task admission guards without touching storage.
pub fn admit(
    row: Option<&TaskRow>,
    event: TaskEvent,
    task_id: &TaskId,
    actor: &AgentName,
) -> Result<(), TaskRejected> {
    let Some(row) = row else {
        return if event == TaskEvent::Completed {
            Err(TaskRejected::new(format!(
                "no open task {task_id} for {actor}"
            )))
        } else {
            Ok(())
        };
    };

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

impl TaskEventKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Assigned => "assigned",
            Self::Acked => "acked",
            Self::Completed => "completed",
            Self::Rejected => "rejected",
            Self::Reminded => "reminded",
            Self::LeadNotified => "lead_notified",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskEventMarker {
    Resend,
    AssignmentMissing,
}

impl TaskEventMarker {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Resend => "resend",
            Self::AssignmentMissing => "assignment_missing",
        }
    }
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
    use super::{TaskEvent, TaskEventKind, TaskEventMarker, TaskRow, TaskState, admit, transition};
    use crate::schema::AtmMessageId;
    use crate::task_store::ReminderOutcome;
    use crate::types::{AgentName, IsoTimestamp, TaskId, TeamName};

    fn context() -> (TaskId, AgentName) {
        (
            "AX.3".parse::<TaskId>().expect("task"),
            "assignee".parse::<AgentName>().expect("assignee"),
        )
    }

    #[test]
    fn task_ledger_names_match_serde_snake_case_output() {
        for (value, expected) in [
            (TaskState::Assigned, "assigned"),
            (TaskState::Active, "active"),
            (TaskState::Complete, "complete"),
        ] {
            assert_eq!(
                serde_json::to_string(&value).unwrap(),
                format!("\"{expected}\"")
            );
            assert_eq!(value.as_str(), expected);
        }
        for (value, expected) in [
            (TaskEventKind::Assigned, "assigned"),
            (TaskEventKind::Acked, "acked"),
            (TaskEventKind::Completed, "completed"),
            (TaskEventKind::Rejected, "rejected"),
            (TaskEventKind::Reminded, "reminded"),
            (TaskEventKind::LeadNotified, "lead_notified"),
        ] {
            assert_eq!(
                serde_json::to_string(&value).unwrap(),
                format!("\"{expected}\"")
            );
            assert_eq!(value.as_str(), expected);
        }
        for (value, expected) in [
            (TaskEventMarker::Resend, "resend"),
            (TaskEventMarker::AssignmentMissing, "assignment_missing"),
        ] {
            assert_eq!(
                serde_json::to_string(&value).unwrap(),
                format!("\"{expected}\"")
            );
            assert_eq!(value.as_str(), expected);
        }
        for (value, expected) in [
            (ReminderOutcome::Emitted, "emitted"),
            (ReminderOutcome::Unrenderable, "unrenderable"),
            (ReminderOutcome::Blocked, "blocked"),
        ] {
            assert_eq!(
                serde_json::to_string(&value).unwrap(),
                format!("\"{expected}\"")
            );
            assert_eq!(value.as_str(), expected);
        }
    }

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
    fn transition_table_is_exhaustive_over_two_events() {
        let (task_id, actor) = context();
        let cases = [
            (None, TaskEvent::Assigned, Some(TaskState::Assigned)),
            (None, TaskEvent::Completed, None),
            (
                Some(TaskState::Assigned),
                TaskEvent::Assigned,
                Some(TaskState::Assigned),
            ),
            (
                Some(TaskState::Assigned),
                TaskEvent::Completed,
                Some(TaskState::Complete),
            ),
            (
                Some(TaskState::Active),
                TaskEvent::Assigned,
                Some(TaskState::Active),
            ),
            (
                Some(TaskState::Active),
                TaskEvent::Completed,
                Some(TaskState::Complete),
            ),
            (Some(TaskState::Complete), TaskEvent::Assigned, None),
            (Some(TaskState::Complete), TaskEvent::Completed, None),
        ];

        for (state, event, expected) in cases {
            assert_eq!(
                transition(state, event, &task_id, &actor)
                    .ok()
                    .map(|value| value.0),
                expected
            );
        }
    }

    #[test]
    fn admit_has_no_cross_row_input() {
        let assignee: AgentName = "assignee".parse().expect("assignee");
        let task_id: TaskId = "AX.3".parse().expect("task");
        let _: fn(
            Option<&TaskRow>,
            TaskEvent,
            &TaskId,
            &AgentName,
        ) -> Result<(), super::TaskRejected> = admit;
        admit(None, TaskEvent::Assigned, &task_id, &assignee).expect("assignment is admitted");
    }

    #[test]
    fn admit_rejects_completion_by_third_party() {
        let task_id: TaskId = "AX.3".parse().expect("task");
        let row = row("AX.3", TaskState::Assigned);
        let intruder: AgentName = "intruder".parse().expect("intruder");
        let rejected = admit(Some(&row), TaskEvent::Completed, &task_id, &intruder)
            .expect_err("third-party completion");
        assert!(rejected.detail.contains("not assigned to or by intruder"));
    }

    #[test]
    fn admit_accepts_completion_by_assigner_and_by_assignee() {
        let task_id: TaskId = "AX.3".parse().expect("task");
        let row = row("AX.3", TaskState::Assigned);
        let assigner: AgentName = "assigner".parse().expect("assigner");
        let assignee: AgentName = "assignee".parse().expect("assignee");
        admit(Some(&row), TaskEvent::Completed, &task_id, &assigner).expect("assigner completion");
        admit(Some(&row), TaskEvent::Completed, &task_id, &assignee).expect("assignee completion");
    }
}
