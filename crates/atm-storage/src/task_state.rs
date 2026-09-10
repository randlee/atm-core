//! Backend-neutral task ledger types and the pure task state machine.

use serde::{Deserialize, Serialize};
use ulid::Ulid;

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

/// Canonical v2 state for a logical task. `TaskState` above remains the
/// narrow v1 compatibility projection until the approved coexistence window
/// ends; new task-domain code must use this richer state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskLifecycleState {
    Assigned,
    Active,
    Blocked,
    Closed(TaskOutcome),
}

/// A durable terminal result is carried only by `Closed`, making an open
/// lifecycle state with terminal metadata unrepresentable in Rust.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskOutcome {
    Succeeded,
    Failed,
    Aborted(TaskAbortReason),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskAbortReason {
    Cancelled,
    Superseded { successor_task_id: TaskId },
}

/// Explicit durable sort key for open task lists.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum TaskPriority {
    High,
    Normal,
    Low,
}

impl TaskPriority {
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Self::High => 0,
            Self::Normal => 1,
            Self::Low => 2,
        }
    }
}

/// One-based immutable assignment-attempt ordinal.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssignmentAttempt(u32);

impl AssignmentAttempt {
    pub const FIRST: Self = Self(1);

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    pub fn next(self) -> Result<Self, TaskRejected> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or_else(|| TaskRejected::new("task assignment attempt overflow"))
    }
}

/// Client-generated idempotency identity for a task operation. It is
/// purposefully distinct from `AtmMessageId`: one operation can produce
/// messages, but it never borrows a message identity.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct TaskOperationId(Ulid);

impl TaskOperationId {
    #[must_use]
    pub fn new() -> Self {
        Self(Ulid::new())
    }

    #[must_use]
    pub const fn as_ulid(self) -> Ulid {
        self.0
    }
}

impl std::fmt::Display for TaskOperationId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
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
pub fn transition(
    state: Option<TaskState>,
    event: TaskEvent,
    task_id: &TaskId,
    actor: &AgentName,
) -> Result<Transition, TaskRejected> {
    match (state, event) {
        (None, TaskEvent::Assigned) => Ok(Transition::To(TaskState::Assigned)),
        (None, TaskEvent::Acked) => Ok(Transition::NoOp),
        (None, TaskEvent::Completed) => Err(TaskRejected::new(format!(
            "no open task {task_id} for {actor}"
        ))),
        (Some(TaskState::Assigned), TaskEvent::Assigned) => Ok(Transition::To(TaskState::Assigned)),
        (Some(TaskState::Assigned), TaskEvent::Acked) => Ok(Transition::To(TaskState::Active)),
        (Some(TaskState::Assigned), TaskEvent::Completed) => {
            Ok(Transition::To(TaskState::Complete))
        }
        (Some(TaskState::Active), TaskEvent::Assigned) => Ok(Transition::To(TaskState::Active)),
        (Some(TaskState::Active), TaskEvent::Acked) => Ok(Transition::To(TaskState::Active)),
        (Some(TaskState::Active), TaskEvent::Completed) => Ok(Transition::To(TaskState::Complete)),
        (Some(TaskState::Complete), TaskEvent::Assigned) => Err(TaskRejected::new(format!(
            "task {task_id} already complete; use a new id"
        ))),
        (Some(TaskState::Complete), TaskEvent::Acked)
        | (Some(TaskState::Complete), TaskEvent::Completed) => Err(TaskRejected::new(format!(
            "task {task_id} already complete"
        ))),
    }
}

/// Checks the cross-row task admission guards without touching storage.
pub fn admit(
    row: Option<&TaskRow>,
    open: &[TaskRow],
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

    if event == TaskEvent::Acked
        && row.state == TaskState::Assigned
        && let Some(other) = open.iter().find(|candidate| {
            candidate.state == TaskState::Active && candidate.task_id != row.task_id
        })
    {
        return Err(TaskRejected::new(format!(
            "task {} is active; complete it first",
            other.task_id
        )));
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
    use super::{
        TaskEvent, TaskEventKind, TaskEventMarker, TaskRow, TaskState, Transition, admit,
        transition,
    };
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
    fn transition_table_is_complete() {
        let (task_id, actor) = context();
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
            assert_eq!(transition(state, event, &task_id, &actor).ok(), expected);
        }
    }

    #[test]
    fn admission_guards_reject_missing_completion_and_second_active_task() {
        let assignee: AgentName = "assignee".parse().expect("assignee");
        let task_id: TaskId = "AX.3".parse().expect("task");
        let missing =
            admit(None, &[], TaskEvent::Completed, &task_id, &assignee).expect_err("missing task");
        assert!(missing.detail.contains("no open task AX.3 for assignee"));

        let assigned = row("AX.3", TaskState::Assigned);
        let active = row("AX.2", TaskState::Active);
        let rejected = admit(
            Some(&assigned),
            &[assigned.clone(), active],
            TaskEvent::Acked,
            &task_id,
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
