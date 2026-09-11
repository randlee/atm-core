//! Backend-neutral task ledger types and the pure task state machine.

use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

use crate::error::AtmError;
use crate::schema::AtmMessageId;
use crate::types::{AgentName, IsoTimestamp, TaskId, TeamName};

use crate::task_store::ReminderOutcome;

/// 1-based queue place; the active task, when one exists, holds 1.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct QueuePosition(NonZeroU32);

impl QueuePosition {
    pub const HEAD: Self = Self(NonZeroU32::MIN);
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }
    pub fn new(value: u32) -> Option<Self> {
        NonZeroU32::new(value).map(Self)
    }
    #[must_use]
    pub fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskCloseOutcome {
    Completed,
    Refused,
    Cancelled,
}

impl TaskCloseOutcome {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Refused => "refused",
            Self::Cancelled => "cancelled",
        }
    }
}

/// The reserved sender identity used by daemon-originated task events.
pub const DAEMON_ACTOR_NAME: &str = "atm-daemon";

/// Open state and a terminal outcome are unrepresentable together. JSON goes
/// through `TaskRowWire` / `TaskEventRowWire` (scalar `state` + `close_outcome`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Assigned,
    Active,
    Complete(TaskCloseOutcome),
}

/// The scalar `state` value as it appears in JSON and in the columns.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStateTag {
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
            Self::Complete(_) => "complete",
        }
    }
    #[must_use]
    pub const fn is_open(self) -> bool {
        !matches!(self, Self::Complete(_))
    }
    #[must_use]
    pub const fn tag(self) -> TaskStateTag {
        match self {
            Self::Assigned => TaskStateTag::Assigned,
            Self::Active => TaskStateTag::Active,
            Self::Complete(_) => TaskStateTag::Complete,
        }
    }
    #[must_use]
    pub const fn close_outcome(self) -> Option<TaskCloseOutcome> {
        match self {
            Self::Complete(outcome) => Some(outcome),
            _ => None,
        }
    }
    /// Column/wire → typestate. `complete` requires an outcome and open
    /// states forbid one; the DDL `CHECK` enforces the same on disk.
    pub fn from_parts(
        tag: TaskStateTag,
        close_outcome: Option<TaskCloseOutcome>,
    ) -> Result<Self, AtmError> {
        match (tag, close_outcome) {
            (TaskStateTag::Assigned, None) => Ok(Self::Assigned),
            (TaskStateTag::Active, None) => Ok(Self::Active),
            (TaskStateTag::Complete, Some(outcome)) => Ok(Self::Complete(outcome)),
            (TaskStateTag::Complete, None) => {
                Err(AtmError::validation("complete task without close_outcome"))
            }
            (_, Some(_)) => Err(AtmError::validation("open task with close_outcome")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskEvent {
    Assigned,
    Started,
    Completed(TaskCloseOutcome),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskActor {
    Member(AgentName),
    Daemon,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "TaskRowWire", into = "TaskRowWire")]
pub struct TaskRow {
    pub team: TeamName,
    pub task_id: TaskId,
    pub assignee: AgentName,
    pub assigner: AgentName,
    pub state: TaskState,
    pub position: Option<QueuePosition>,
    pub assignment_message_id: AtmMessageId,
    pub description: String,
    pub assigned_at: IsoTimestamp,
    pub updated_at: IsoTimestamp,
    pub last_reminded_at: Option<IsoTimestamp>,
    pub reminder_count: u32,
    pub lead_notified_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TaskRowWire {
    team: TeamName,
    task_id: TaskId,
    assignee: AgentName,
    assigner: AgentName,
    state: TaskStateTag,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    close_outcome: Option<TaskCloseOutcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    position: Option<QueuePosition>,
    assignment_message_id: AtmMessageId,
    description: String,
    assigned_at: IsoTimestamp,
    updated_at: IsoTimestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_reminded_at: Option<IsoTimestamp>,
    reminder_count: u32,
    lead_notified_count: u32,
}

impl TryFrom<TaskRowWire> for TaskRow {
    type Error = AtmError;

    fn try_from(wire: TaskRowWire) -> Result<Self, Self::Error> {
        let close_outcome = if wire.state == TaskStateTag::Complete && wire.close_outcome.is_none()
        {
            Some(TaskCloseOutcome::Completed)
        } else {
            wire.close_outcome
        };
        let state = TaskState::from_parts(wire.state, close_outcome)?;
        if !state.is_open() && wire.position.is_some() {
            return Err(AtmError::validation("complete task with queue position"));
        }
        Ok(Self {
            team: wire.team,
            task_id: wire.task_id,
            assignee: wire.assignee,
            assigner: wire.assigner,
            state,
            position: wire.position,
            assignment_message_id: wire.assignment_message_id,
            description: wire.description,
            assigned_at: wire.assigned_at,
            updated_at: wire.updated_at,
            last_reminded_at: wire.last_reminded_at,
            reminder_count: wire.reminder_count,
            lead_notified_count: wire.lead_notified_count,
        })
    }
}

impl From<TaskRow> for TaskRowWire {
    fn from(row: TaskRow) -> Self {
        Self {
            team: row.team,
            task_id: row.task_id,
            assignee: row.assignee,
            assigner: row.assigner,
            state: row.state.tag(),
            close_outcome: row.state.close_outcome(),
            position: row.position,
            assignment_message_id: row.assignment_message_id,
            description: row.description,
            assigned_at: row.assigned_at,
            updated_at: row.updated_at,
            last_reminded_at: row.last_reminded_at,
            reminder_count: row.reminder_count,
            lead_notified_count: row.lead_notified_count,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskRejected {
    pub detail: String,
}

impl TaskRejected {
    fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
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
    current_assignee: Option<&AgentName>,
    requested_assignee: &AgentName,
) -> Result<Transition, TaskRejected> {
    use TaskEvent as E;
    use TaskState as S;
    match (state, event) {
        (None, E::Assigned) => Ok(Transition(S::Assigned)),
        (None, E::Started | E::Completed(_)) => Err(TaskRejected::new(format!(
            "no open task {task_id} for {actor}"
        ))),
        (Some(S::Assigned), E::Assigned) if current_assignee == Some(requested_assignee) => {
            Ok(Transition(S::Assigned))
        }
        (Some(S::Active), E::Assigned) if current_assignee == Some(requested_assignee) => {
            Ok(Transition(S::Active))
        }
        (Some(S::Assigned | S::Active), E::Assigned) => Ok(Transition(S::Assigned)),
        (Some(S::Complete(_)), E::Assigned) => Ok(Transition(S::Assigned)),
        (Some(S::Assigned | S::Active), E::Started) => Ok(Transition(S::Active)),
        (Some(S::Assigned | S::Active), E::Completed(outcome)) => {
            Ok(Transition(S::Complete(outcome)))
        }
        (Some(S::Complete(_)), E::Started | E::Completed(_)) => {
            unreachable!("complete-row start/close is handled by the writer before transition()")
        }
    }
}

/// Checks the cross-row task admission guards without touching storage.
pub fn admit(
    row: Option<&TaskRow>,
    event: TaskEvent,
    task_id: &TaskId,
    actor: &AgentName,
) -> Result<(), TaskRejected> {
    match (row, event) {
        (None, TaskEvent::Completed(_)) => Err(TaskRejected::new(format!(
            "no open task {task_id} for {actor}"
        ))),
        (None, TaskEvent::Assigned) => Ok(()),
        (None, TaskEvent::Started) => Err(TaskRejected::new(format!(
            "no open task {task_id} for {actor}"
        ))),
        (Some(row), TaskEvent::Completed(_))
            if actor != &row.assignee && actor != &row.assigner =>
        {
            Err(TaskRejected::new(format!(
                "task {} is not assigned to or by {actor}",
                row.task_id
            )))
        }
        (Some(_), _) => Ok(()),
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskEventKind {
    Assigned,
    Acked,
    Started,
    Completed,
    Refused,
    Cancelled,
    Reassigned,
    Reopened,
    Rejected,
    Reminded,
    LeadNotified,
    Moved,
    Migrated,
}

impl TaskEventKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Assigned => "assigned",
            Self::Acked => "acked",
            Self::Started => "started",
            Self::Completed => "completed",
            Self::Refused => "refused",
            Self::Cancelled => "cancelled",
            Self::Reassigned => "reassigned",
            Self::Reopened => "reopened",
            Self::Rejected => "rejected",
            Self::Reminded => "reminded",
            Self::LeadNotified => "lead_notified",
            Self::Moved => "moved",
            Self::Migrated => "migrated",
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "TaskEventRowWire", into = "TaskEventRowWire")]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TaskEventRowWire {
    team: TeamName,
    task_id: TaskId,
    assignee: AgentName,
    seq: u64,
    at: IsoTimestamp,
    event: TaskEventKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    from_state: Option<TaskStateTag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    to_state: Option<TaskStateTag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    close_outcome: Option<TaskCloseOutcome>,
    actor: TaskActor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    message_id: Option<AtmMessageId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    outcome: Option<ReminderOutcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    marker: Option<TaskEventMarker>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

impl TryFrom<TaskEventRowWire> for TaskEventRow {
    type Error = AtmError;

    fn try_from(wire: TaskEventRowWire) -> Result<Self, Self::Error> {
        let outcome_for = |tag: TaskStateTag| {
            if tag == TaskStateTag::Complete {
                Some(wire.close_outcome.unwrap_or(TaskCloseOutcome::Completed))
            } else {
                None
            }
        };
        if wire.close_outcome.is_some()
            && wire.from_state != Some(TaskStateTag::Complete)
            && wire.to_state != Some(TaskStateTag::Complete)
        {
            return Err(AtmError::validation(
                "task event close_outcome without complete state",
            ));
        }
        Ok(Self {
            team: wire.team,
            task_id: wire.task_id,
            assignee: wire.assignee,
            seq: wire.seq,
            at: wire.at,
            event: wire.event,
            from_state: wire
                .from_state
                .map(|tag| TaskState::from_parts(tag, outcome_for(tag)))
                .transpose()?,
            to_state: wire
                .to_state
                .map(|tag| TaskState::from_parts(tag, outcome_for(tag)))
                .transpose()?,
            actor: wire.actor,
            message_id: wire.message_id,
            outcome: wire.outcome,
            marker: wire.marker,
            detail: wire.detail,
        })
    }
}

impl From<TaskEventRow> for TaskEventRowWire {
    fn from(row: TaskEventRow) -> Self {
        let close_outcome = row
            .to_state
            .and_then(TaskState::close_outcome)
            .or_else(|| row.from_state.and_then(TaskState::close_outcome));
        Self {
            team: row.team,
            task_id: row.task_id,
            assignee: row.assignee,
            seq: row.seq,
            at: row.at,
            event: row.event,
            from_state: row.from_state.map(TaskState::tag),
            to_state: row.to_state.map(TaskState::tag),
            close_outcome,
            actor: row.actor,
            message_id: row.message_id,
            outcome: row.outcome,
            marker: row.marker,
            detail: row.detail,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RefusalRun {
    pub count: u32,
    pub started_at: Option<IsoTimestamp>,
}

#[cfg(test)]
mod tests {
    use super::{
        QueuePosition, TaskCloseOutcome, TaskEvent, TaskEventKind, TaskEventMarker, TaskRow,
        TaskState, admit, transition,
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
    fn queue_position_bounds() {
        assert!(QueuePosition::new(0).is_none());
        assert_eq!(QueuePosition::HEAD.get(), 1);
        assert_eq!(QueuePosition::HEAD.next().get(), 2);
    }

    #[test]
    fn close_outcome_names_are_stable() {
        assert_eq!(TaskCloseOutcome::Completed.as_str(), "completed");
        assert_eq!(TaskCloseOutcome::Refused.as_str(), "refused");
        assert_eq!(TaskCloseOutcome::Cancelled.as_str(), "cancelled");
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
            position: None,
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
