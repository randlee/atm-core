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

/// Pure lifecycle input used by every storage adapter before it changes a
/// durable task row.  Message preparation is intentionally absent: it is an
/// admission concern owned by the mutation contract, not state-machine data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskLifecycleAction {
    Assign,
    Start,
    Block,
    Unblock,
    Reassign,
    Reopen,
    Close(TaskOutcome),
    LegacyCloseSucceeded,
    Supersede { successor_task_id: TaskId },
}

/// Result of applying [`TaskLifecycleAction`] to a current logical state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskLifecycleTransition {
    To(TaskLifecycleState),
}

/// The canonical v2 legal-transition table.  In particular, ordinary close
/// does not let an unstarted assignment claim success: the retained
/// `LegacyCloseSucceeded` adapter is the sole compatibility route for that
/// historical behavior.
pub fn lifecycle_transition(
    current: Option<&TaskLifecycleState>,
    action: &TaskLifecycleAction,
) -> Result<TaskLifecycleTransition, TaskRejected> {
    use TaskLifecycleAction as Action;
    use TaskLifecycleState as State;

    match (current, action) {
        (None, Action::Assign) => Ok(TaskLifecycleTransition::To(State::Assigned)),
        (Some(State::Assigned), Action::Start) => Ok(TaskLifecycleTransition::To(State::Active)),
        (Some(State::Assigned), Action::Block) | (Some(State::Active), Action::Block) => {
            Ok(TaskLifecycleTransition::To(State::Blocked))
        }
        (Some(State::Blocked), Action::Unblock) => Ok(TaskLifecycleTransition::To(State::Assigned)),
        (Some(State::Assigned | State::Blocked), Action::Reassign) => {
            Ok(TaskLifecycleTransition::To(State::Assigned))
        }
        (Some(State::Closed(_)), Action::Reassign | Action::Reopen) => {
            Ok(TaskLifecycleTransition::To(State::Assigned))
        }
        (Some(State::Assigned), Action::LegacyCloseSucceeded) => Ok(TaskLifecycleTransition::To(
            State::Closed(TaskOutcome::Succeeded),
        )),
        (Some(State::Assigned), Action::Close(TaskOutcome::Failed))
        | (Some(State::Active), Action::Close(TaskOutcome::Succeeded | TaskOutcome::Failed)) => {
            let Action::Close(outcome) = action else {
                unreachable!("matched a close action")
            };
            Ok(TaskLifecycleTransition::To(State::Closed(outcome.clone())))
        }
        (
            Some(State::Assigned | State::Active | State::Blocked),
            Action::Close(TaskOutcome::Aborted(TaskAbortReason::Cancelled)),
        ) => Ok(TaskLifecycleTransition::To(State::Closed(
            TaskOutcome::Aborted(TaskAbortReason::Cancelled),
        ))),
        (
            Some(State::Assigned | State::Active | State::Blocked),
            Action::Supersede { successor_task_id },
        ) => Ok(TaskLifecycleTransition::To(State::Closed(
            TaskOutcome::Aborted(TaskAbortReason::Superseded {
                successor_task_id: successor_task_id.clone(),
            }),
        ))),
        _ => Err(TaskRejected::new("illegal task lifecycle transition")),
    }
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

/// Storage-owned selection for logical task projections.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskLedgerScope {
    Open,
    Closed,
    All,
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

/// Monotonic compare-and-swap revision for one logical task.
#[derive(
    Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(transparent)]
pub struct TaskRevision(u64);

impl TaskRevision {
    #[must_use]
    pub const fn from_raw(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// One-based count of successful reminders for a task assignment.
#[derive(
    Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(transparent)]
pub struct ReminderOrdinal(u64);

impl ReminderOrdinal {
    #[must_use]
    pub const fn from_raw(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    #[must_use]
    pub const fn increment(self) -> Self {
        Self(self.0.saturating_add(1))
    }

    #[must_use]
    pub const fn is_multiple_of(self, value: u64) -> bool {
        self.0.is_multiple_of(value)
    }
}

impl std::fmt::Display for ReminderOrdinal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
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

    pub fn new(value: u32) -> Result<Self, TaskRejected> {
        if value == 0 {
            return Err(TaskRejected::new(
                "task assignment attempt must be one-based",
            ));
        }
        Ok(Self(value))
    }

    pub fn next(self) -> Result<Self, TaskRejected> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or_else(|| TaskRejected::new("task assignment attempt overflow"))
    }
}

/// Canonical v2 current projection.  The older [`TaskRow`] below remains the
/// deliberately narrow v1 compatibility shape for the approved coexistence
/// window; new callers must use this type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogicalTaskRow {
    pub team: TeamName,
    pub task_id: TaskId,
    pub current_assignee: AgentName,
    pub state: TaskLifecycleState,
    pub priority: TaskPriority,
    pub original_assigned_at: IsoTimestamp,
    pub current_attempt: AssignmentAttempt,
    /// Identifier of the canonical assignment message for the current attempt.
    /// This is metadata only; consumers reload the bounded title projection
    /// from the message store and never receive assignment text here.
    pub assignment_message_id: AtmMessageId,
    /// Most recent successful reminder for the current assignment attempt.
    /// Derived from the append-only v2 audit rather than duplicated mutable
    /// scheduler state, so a reassignment cannot inherit its predecessor's
    /// cadence.
    pub last_reminded_at: Option<IsoTimestamp>,
    pub reminder_ordinal: ReminderOrdinal,
    pub revision: TaskRevision,
    pub updated_at: IsoTimestamp,
}

/// Immutable assignment history for one logical task.  It deliberately
/// references the canonical message/template records rather than carrying
/// rendered text, template bytes, variables, or a copied description.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskAssignmentAttempt {
    pub team: TeamName,
    pub task_id: TaskId,
    pub attempt: AssignmentAttempt,
    pub assignee: AgentName,
    pub assigner: AgentName,
    pub assignment_message_id: AtmMessageId,
    pub template_sha: Option<crate::types::TemplateSha>,
    pub assigned_at: IsoTimestamp,
}

/// Append-only v2 task-lifecycle event vocabulary.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskLifecycleEventKind {
    Assigned,
    Started,
    Blocked,
    Unblocked,
    Reassigned,
    Reopened,
    Closed,
    Superseded,
    Rejected,
    Reminded,
    LeadNotified,
    Migrated,
    MigratedActiveConflictDemotion,
    LegacyCloseSucceeded,
    LegacyV1Updated,
}

impl TaskLifecycleEventKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Assigned => "assigned",
            Self::Started => "started",
            Self::Blocked => "blocked",
            Self::Unblocked => "unblocked",
            Self::Reassigned => "reassigned",
            Self::Reopened => "reopened",
            Self::Closed => "closed",
            Self::Superseded => "superseded",
            Self::Rejected => "rejected",
            Self::Reminded => "reminded",
            Self::LeadNotified => "lead_notified",
            Self::Migrated => "migrated",
            Self::MigratedActiveConflictDemotion => "migrated_active_conflict_demotion",
            Self::LegacyCloseSucceeded => "legacy_close_succeeded",
            Self::LegacyV1Updated => "legacy_v1_updated",
        }
    }
}

/// Immutable v2 audit row.  As with [`TaskAssignmentAttempt`], no rendered
/// task body is persisted in this projection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskLifecycleEventRow {
    pub team: TeamName,
    pub task_id: TaskId,
    pub seq: u64,
    pub operation_id: Option<TaskOperationId>,
    pub attempt: Option<AssignmentAttempt>,
    pub at: IsoTimestamp,
    pub actor: AgentName,
    pub event: TaskLifecycleEventKind,
    pub outcome: Option<TaskOutcome>,
    pub related_task_id: Option<TaskId>,
    pub detail: Option<String>,
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

    pub fn parse(value: &str) -> Result<Self, TaskRejected> {
        Ulid::from_string(value)
            .map(Self)
            .map_err(|_| TaskRejected::new("task operation id is invalid"))
    }
}

impl Default for TaskOperationId {
    fn default() -> Self {
        Self::new()
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
        AssignmentAttempt, TaskAbortReason, TaskEvent, TaskEventKind, TaskEventMarker,
        TaskLifecycleAction, TaskLifecycleState, TaskLifecycleTransition, TaskOutcome,
        TaskPriority, TaskRow, TaskState, Transition, admit, lifecycle_transition, transition,
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

    #[test]
    fn v2_lifecycle_keeps_terminal_metadata_inside_closed() {
        let successor = "AZ.3".parse().expect("task id");
        let state = TaskLifecycleState::Closed(TaskOutcome::Aborted(TaskAbortReason::Superseded {
            successor_task_id: successor,
        }));
        assert_eq!(
            serde_json::to_string(&state).expect("serialize lifecycle state"),
            r#"{"closed":{"aborted":{"superseded":{"successor_task_id":"AZ.3"}}}}"#
        );
        assert_eq!(TaskPriority::High.rank(), 0);
        assert_eq!(TaskPriority::Normal.rank(), 1);
        assert_eq!(TaskPriority::Low.rank(), 2);
    }

    #[test]
    fn v2_lifecycle_transition_table_preserves_the_legacy_success_boundary() {
        let assigned = TaskLifecycleState::Assigned;
        let active = TaskLifecycleState::Active;
        let blocked = TaskLifecycleState::Blocked;
        let closed = TaskLifecycleState::Closed(TaskOutcome::Failed);

        assert_eq!(
            lifecycle_transition(None, &TaskLifecycleAction::Assign),
            Ok(TaskLifecycleTransition::To(TaskLifecycleState::Assigned))
        );
        assert_eq!(
            lifecycle_transition(Some(&assigned), &TaskLifecycleAction::Start),
            Ok(TaskLifecycleTransition::To(TaskLifecycleState::Active))
        );
        assert_eq!(
            lifecycle_transition(Some(&blocked), &TaskLifecycleAction::Unblock),
            Ok(TaskLifecycleTransition::To(TaskLifecycleState::Assigned))
        );
        assert_eq!(
            lifecycle_transition(Some(&closed), &TaskLifecycleAction::Reopen),
            Ok(TaskLifecycleTransition::To(TaskLifecycleState::Assigned))
        );
        assert!(
            lifecycle_transition(
                Some(&assigned),
                &TaskLifecycleAction::Close(TaskOutcome::Succeeded)
            )
            .is_err()
        );
        assert_eq!(
            lifecycle_transition(Some(&assigned), &TaskLifecycleAction::LegacyCloseSucceeded),
            Ok(TaskLifecycleTransition::To(TaskLifecycleState::Closed(
                TaskOutcome::Succeeded
            )))
        );
        assert!(
            lifecycle_transition(
                Some(&blocked),
                &TaskLifecycleAction::Close(TaskOutcome::Failed)
            )
            .is_err()
        );
        assert_eq!(
            lifecycle_transition(
                Some(&active),
                &TaskLifecycleAction::Close(TaskOutcome::Succeeded)
            ),
            Ok(TaskLifecycleTransition::To(TaskLifecycleState::Closed(
                TaskOutcome::Succeeded
            )))
        );
    }

    #[test]
    fn assignment_attempt_is_one_based_and_checked() {
        assert_eq!(AssignmentAttempt::FIRST.get(), 1);
        assert_eq!(AssignmentAttempt::FIRST.next().expect("second").get(), 2);
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
