# BA.2 — Task identity, queue position, typed outcome, mutation boundary, migration

| Field | Value |
| --- | --- |
| Design | [`nudge-task-design.md`](./nudge-task-design.md) §3.1, §3.2, §4, §4.1, §4.3 (commit `18db5acc3`) |
| Outcomes | B3, B5, B6, B7 |
| Recommended | arch-ctm / deep-reasoning — schema rebuild with live data and a transactional queue renumber |
| Depends on | `must_follow` BA.1 (branch ancestry — same files) |
| Worktree | `feature/ba2-task-identity-queue` off `feature/ba1-ack-task-separation` |
| Governed interfaces | SQLite **MAJOR** (plan §4 R0); HTTP **MINOR** `1.4.0 → 1.5.0` (additive `task_op` on `WriteRequest`; additive `close_outcome`/`position` keys on `TaskRow` JSON — plan §8); `AsyncTaskLedgerReader` +1 method |
| Blocked until | R0 recorded as a comment on PR #1398 **and** ADR-061 D6 carries the Phase BA approval entry with the D3 exception (landed in this docs PR before BA.2 opens — FNX-BA-CRIT-003) |

## Scope

One row per `(team, task_id)`. At most one `active` task per agent, enforced
by SQLite. Queue order `(position, assigned_at, task_id)`. Typed close
outcome. Every task mutation flows through one `TaskOp` carried on the
write request and applied in the message writer transaction. One-way
migration of live databases. No CLI verb and no runtime disposition change
(BA.3/BA.4); the wire gains `task_op` additively and keeps decoding the
legacy `task_complete` key (see "Wire" below).

## Deliverables

| id | deliverable | where |
| --- | --- | --- |
| D1 | `QueuePosition`, `TaskCloseOutcome` (completed, refused, cancelled); `TaskState::Complete(outcome)`, `TaskStateTag`, `TaskEvent::{Assigned,Started,Completed(outcome)}`, `transition()`, `TaskRejected`/`TaskRejectionKind` | `crates/atm-storage/src/task_state.rs` |
| D2 | `TaskRow` + `TaskRowWire`, `TaskEventRow` + `TaskEventRowWire`, `TaskEventKind::{Assigned,Started,Reassigned,Reopened,Completed,Refused,Cancelled,Moved,Migrated}` | `crates/atm-storage/src/task_state.rs` |
| D3 | `TaskOp`, `MoveTarget`, `RefusalRun`, `TASK_CONSECUTIVE_REFUSAL_THRESHOLD` | `crates/atm-storage/src/task_op.rs` (new), `task_store.rs` |
| D4 | `WriteRequest.task_op` + `task_op_normalized()`, envelope `task_op`, ``, `HTTP_API_VERSION = "1.5.0"` | `crates/atm-core/src/send/mod.rs`, `crates/atm-storage/src/schema/inbox_message.rs`, `crates/atm-core/src/protocol.rs:99` |
| D5 | `TASK_SCHEMA_DDL` rebuilt (two tables, three indexes) | `crates/atm-storage-rusqlite/src/task_store.rs:15-58` |
| D6 | `migrate_task_identity` + `TaskMigrationReport` | `crates/atm-storage-rusqlite/src/task_migration.rs` (new) |
| D7 | writer: `apply_task_message` dispatch, `apply_task_start`, `apply_task_close`, `apply_task_move`, `renumber_queue`, authority rules | `crates/atm-storage-rusqlite/src/writer/task_ops.rs` |
| D8 | `AsyncTaskLedgerReader::open_tasks_for_team`, `TaskStore::load_task(team, task_id)`, `DoctorFinding::TaskQueueGap` | `crates/atm-storage/src/contract.rs:916`, `task_store.rs:66`, `crates/atm-core/src/doctor/` |
| D9 | boundary manifest edits per plan §10 | `boundaries/atm-storage/task-store.toml`, `…-rusqlite/task-store-sqlite.toml`, `async-task-ledger-reader*.toml` |
| D10 | tests named below; ADR-061 D6 approval entry cited on the PR | `tests/task_identity.rs`, `tests/task_migration.rs` |

**Why one sprint (plan-scope PLAN-SCOPE-006):** D1–D2 change the type of
`tasks.state`, `TaskRow` and `TaskEventRow`; every existing reader and
writer of those types (`task_ops.rs`, `task_sql.rs`, `task_store.rs`,
`ledger reader`, `doctor`) stops compiling until D5–D8 land. A schema-only
sprint would therefore have to carry a temporary second `TaskRow` or a
compile-only stub writer — code the phase would then delete (no unused
code). The one seam that *can* stand alone is `apply_task_move` without a
message, and it is already in BA.4. D1→D10 is the build order; QA may check
them in that order.

## Phase AZ code used

| AZ artifact (`origin/integrate/phase-az`) | how |
| --- | --- |
| `crates/atm-storage-rusqlite/src/schema_version.rs:284-286` `CREATE UNIQUE INDEX one_active_task_per_agent … WHERE state = 'active'` | **copied verbatim** (column `assignee`, not `current_assignee`) — DDL below |
| `schema_version.rs:288-321` winner ranking (`ROW_NUMBER() OVER (PARTITION BY team, task_id ORDER BY precedence …, updated_at DESC, assignee ASC)`) and `:339-380` deterministic active-conflict demotion + audit event | **copied and adapted** to the in-place rebuild — migration SQL below; the audit-row text is kept. **Precedence inverted:** AZ ranked `active > assigned > complete`; BA ranks `complete > active > assigned` because design §3.1's live incident is a *completed* row beside a 587-reminder open twin — the completed twin is the terminal truth and the open twin is the phantom (FNX-BA-CRIT-001) |
| `schema_version.rs:383-414` `ensure_task_v2_schema`: IMMEDIATE transaction → DDL → migrate → post-DDL → commit | **shape reused** as `migrate_task_identity` |
| `crates/atm-core/src/task_command/service.rs:698-750` `require_assignee_assigner_or_lead`, `require_assigner_or_lead`, `require_unique_lead` | **copied** into the writer-side authority check (D6), minus `TaskMutationCommand`/attempt parameters |

Not used: `tasks_v2`, `task_assignment_attempts`, `task_operations`,
`storage_schema_versions`, triggers, `TaskOperationId`, `TaskPriority`,
`AssignmentAttempt`, supersession — design §8.

## Types — exactly as they land

```rust
// crates/atm-storage/src/task_state.rs

/// 1-based place in one member's open-task queue. Position 1 is the task
/// the member works next (or is working); the active task, when one exists,
/// always holds position 1.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct QueuePosition(NonZeroU32);

impl QueuePosition {
    pub const HEAD: Self = Self(NonZeroU32::MIN);
    #[must_use] pub const fn get(self) -> u32 { self.0.get() }
    pub fn new(value: u32) -> Option<Self> { NonZeroU32::new(value).map(Self) }
    #[must_use] pub fn next(self) -> Self { Self(self.0.saturating_add(1)) }
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

/// Open state and a terminal outcome are unrepresentable together.
/// Not `Serialize`/`Deserialize` on its own: the JSON shape of a task is
/// `TaskRowWire` / `TaskEventRowWire` (below), which keep the pre-BA scalar
/// `state` string and add `close_outcome` as a sibling key (ADR-061 D2:
/// additive, older readers ignore it — FNX-BA-CRIT-005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Assigned,
    Active,
    Complete(TaskCloseOutcome),
}

/// The scalar `state` value as it has always appeared in JSON and in the
/// `tasks.state` / `task_events.*_state` columns.
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
    pub const fn is_open(self) -> bool { !matches!(self, Self::Complete(_)) }
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
        match self { Self::Complete(outcome) => Some(outcome), _ => None }
    }
    /// Column/wire → typestate. `complete` requires an outcome and open
    /// states forbid one; the DDL `CHECK` enforces the same on disk.
    pub fn from_parts(tag: TaskStateTag, close_outcome: Option<TaskCloseOutcome>) -> Result<Self, AtmError> {
        match (tag, close_outcome) {
            (TaskStateTag::Assigned, None) => Ok(Self::Assigned),
            (TaskStateTag::Active, None) => Ok(Self::Active),
            (TaskStateTag::Complete, Some(outcome)) => Ok(Self::Complete(outcome)),
            (TaskStateTag::Complete, None) => Err(AtmError::validation("complete task without close_outcome")),
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

pub struct Transition(pub TaskState);

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
        (None, E::Started | E::Completed(_)) => Err(TaskRejected::new(format!("no open task {task_id} for {actor}"))),
        (Some(S::Assigned), E::Assigned) if current_assignee == Some(requested_assignee) => Ok(Transition(S::Assigned)),
        (Some(S::Active), E::Assigned) if current_assignee == Some(requested_assignee) => Ok(Transition(S::Active)),
        (Some(S::Assigned | S::Active), E::Assigned) => Ok(Transition(S::Assigned)),
        (Some(S::Complete(_)), E::Assigned) => Ok(Transition(S::Assigned)),
        (Some(S::Assigned | S::Active), E::Started) => Ok(Transition(S::Active)), // idempotent when Active
        (Some(S::Assigned | S::Active), E::Completed(outcome)) => Ok(Transition(S::Complete(outcome))),
        (Some(S::Complete(_)), E::Started | E::Completed(_)) => Err(TaskRejected::already_complete(task_id)),
    }
}
```

`TaskRejected::already_complete` is a distinct constructor so BA.4 can tell
"already complete" (inform) from every other rejection (block) without
string matching:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskRejected {
    pub kind: TaskRejectionKind,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskRejectionKind {
    NoOpenTask,
    NotAuthorized,   // wrong actor, or "lead" authority claimed on a team with 0 or 2+ leads (detail names the count)
    ActiveElsewhere, // one-active index hit on Started
    UnknownTarget,   // Move/assign placement target invalid
    StaleCounterparty, // close recipient is not the current counterparty
    AlreadyComplete, // close repeated after completion; informational rejection
}

`TaskRejectionKind::AlreadyComplete` is also exposed by the
`TaskRejected::already_complete` constructor for repeated close attempts;
assign-on-complete uses the separate reopen branch.
```

Authority failures are rejections, not error codes: a caller who is neither
assignee nor assigner is `NotAuthorized` with detail
`"<actor> is neither assignee, assigner nor the unique lead of <team>"`; a
lead on a team with `n != 1` leads is `NotAuthorized` with detail
`"team <team> has <n> leads; lead authority requires exactly one"`. No new
`AtmErrorCode` (FNX-BA-CRIT-006 / PLAN-SCOPE-005; the doctor codes
`RosterNoLead` / `RosterMultipleLeads` at `atm-error/src/error_codes.rs:141-142`
stay doctor-only).

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "TaskRowWire", into = "TaskRowWire")]
pub struct TaskRow {
    pub team: TeamName,
    pub task_id: TaskId,
    pub assignee: AgentName,
    pub assigner: AgentName,
    pub state: TaskState,
    /// `Some` iff `state.is_open()`.
    pub position: Option<QueuePosition>,
    pub assignment_message_id: AtmMessageId,
    pub description: String,
    /// reset by reassign/reopen; never by move; `move` never touches it.
    pub assigned_at: IsoTimestamp,
    pub updated_at: IsoTimestamp,
    pub last_reminded_at: Option<IsoTimestamp>,
    pub reminder_count: u32,
    pub lead_notified_count: u32,
}

/// JSON shape of `TaskRow` (`ListOutcome`, `atm task list --json`). `state`
/// keeps its 1.4.0 values; `close_outcome` and `position` are new optional
/// keys. A `complete` row without `close_outcome` (a 1.4.0 producer) decodes
/// as `Completed`, the only outcome 1.4.0 could record (ADR-061 D2:
/// receivers default omitted fields).
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
    last_reminded_at: Option<IsoTimestamp>,
    reminder_count: u32,
    lead_notified_count: u32,
}

impl TryFrom<TaskRowWire> for TaskRow {
    type Error = AtmError;
    fn try_from(wire: TaskRowWire) -> Result<Self, AtmError> {
        let close_outcome = match (wire.state, wire.close_outcome) {
            (TaskStateTag::Complete, None) => Some(TaskCloseOutcome::Completed), // 1.4.0 producer
            (_, other) => other,
        };
        let state = TaskState::from_parts(wire.state, close_outcome)?;
        // A complete row never carries a position. An open row from a 1.5.0
        // producer carries one; an open row from a 1.4.0 producer has none and
        // decodes with `position = None` (FNX-BA-CRIT-019: verbatim 1.4.0
        // payloads decode). The wire is a projection; the DDL is the invariant.
        if !state.is_open() && wire.position.is_some() {
            return Err(AtmError::validation("a complete task has no queue position"));
        }
        Ok(Self { team: wire.team, task_id: wire.task_id, assignee: wire.assignee, assigner: wire.assigner,
            state, position: wire.position, assignment_message_id: wire.assignment_message_id,
            description: wire.description, assigned_at: wire.assigned_at, updated_at: wire.updated_at,
            last_reminded_at: wire.last_reminded_at, reminder_count: wire.reminder_count,
            lead_notified_count: wire.lead_notified_count })
    }
}

impl From<TaskRow> for TaskRowWire {
    fn from(row: TaskRow) -> Self {
        Self { team: row.team, task_id: row.task_id, assignee: row.assignee, assigner: row.assigner,
            state: row.state.tag(), close_outcome: row.state.close_outcome(), position: row.position,
            assignment_message_id: row.assignment_message_id, description: row.description,
            assigned_at: row.assigned_at, updated_at: row.updated_at, last_reminded_at: row.last_reminded_at,
            reminder_count: row.reminder_count, lead_notified_count: row.lead_notified_count }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskEventKind {
    Assigned,
    Acked,      // history only; never written after BA.1
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
    Migrated,   // written only by the BA.2 migration
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "TaskEventRowWire", into = "TaskEventRowWire")]
pub struct TaskEventRow {
    pub team: TeamName,
    pub task_id: TaskId,
    pub assignee: AgentName,        // column kept; no longer part of the key
    pub seq: u64,
    pub at: IsoTimestamp,
    pub event: TaskEventKind,
    pub from_state: Option<TaskState>,
    /// Carries the outcome when `Some(Complete(_))`. The `task_events.close_outcome`
    /// column and the wire key of the same name are this value's projection
    /// (`to_state.and_then(TaskState::close_outcome)`); there is no second
    /// Rust field (PLAN-SCOPE-004).
    pub to_state: Option<TaskState>,
    pub actor: TaskActor,
    pub message_id: Option<AtmMessageId>,
    pub outcome: Option<ReminderOutcome>,
    pub marker: Option<TaskEventMarker>,
    pub detail: Option<String>,
}

/// JSON shape of `TaskEventRow` (`atm task events --json`); same rule as
/// `TaskRowWire`: 1.4.0 keys unchanged, `close_outcome` added.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TaskEventRowWire {
    team: TeamName,
    task_id: TaskId,
    assignee: AgentName,
    seq: u64,
    at: IsoTimestamp,
    event: TaskEventKind,
    from_state: Option<TaskStateTag>,
    to_state: Option<TaskStateTag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    close_outcome: Option<TaskCloseOutcome>,
    actor: TaskActor,
    message_id: Option<AtmMessageId>,
    outcome: Option<ReminderOutcome>,
    marker: Option<TaskEventMarker>,
    detail: Option<String>,
}
// TryFrom/From: `close_outcome` is the row's one outcome key. `to_state =
// Complete` pairs with it (None from a 1.4.0 producer → `Completed`), exactly
// as `TaskRowWire`. `from_state = Complete` occurs only on state-neutral events
// written against a completed task (`rejected`, `reminded`, a `migrated`
// source row), where `from_state == to_state`; both decode from the same
// `close_outcome` (FNX-BA-CRIT-022; `append_rejected_task_event`, `task_ops.rs:112-129`,
// already writes both states equal to the current row). `from_state = Complete`
// with `to_state ≠ Complete` is a validation error — terminal-to-assigned is valid only for reopened.
// Row decode from SQLite uses the same helpers with the `close_outcome` column.
```

```rust
// crates/atm-storage/src/task_op.rs  (new file, re-exported from lib.rs and atm_core::boundary)

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "op")]
pub enum TaskOp {
    /// Daemon-only (plan §4 R1). Resets reminder counters.
    Start,
    Close { outcome: TaskCloseOutcome, #[serde(default, skip_serializing_if = "Option::is_none")] reason: Option<String> },
    // No `Move`: design §5 makes move message-less; the only move boundary is
    // `WriteOp::TaskMove` / `RequestEnvelope::TaskMove` (BA.4) — FNX-BA-CRIT-023.
}

/// Target of a message-less move (`TaskMoveRequest`, BA.4). Lives here so the
/// writer (`apply_task_move`) and the envelope share one type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "to")]
pub enum MoveTarget {
    /// Position 1, or position 2 when the member has an active task.
    Head,
    End,
    Before { task_id: TaskId },
}
```

### Wire — `WriteRequest` (`crates/atm-core/src/send/mod.rs:127`) and the persisted envelope (`crates/atm-storage/src/schema/inbox_message.rs:187-193`)

The CLI talks to the daemon over local HTTP with this struct, so it is an
ADR-061 governed surface. `task_op` is **added**; `task_complete` is **kept
as a decode-only legacy key** (removing it would be MAJOR under D2 — FNX-BA-CRIT-004).
`HTTP_API_VERSION` moves `1.4.0 → 1.5.0` in this sprint (MINOR, additive),
recorded in `docs/http-api.md` and ADR-061's version table; BA.4 later moves
it to `1.6.0` for `TaskMove`.

```rust
    pub task_id: Option<TaskId>,
    /// Assignment placement; absent in 1.4.0 payloads and therefore None,
    /// which means END. BA.2 carries the existing MoveTarget type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<MoveTarget>,
    /// Mutation applied to `task_id` in the same writer transaction as this
    /// message. `None` with `task_id` set means assign.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_op: Option<TaskOp>,
    /// 1.4.0 senders' close. Decode-only: a 1.5.0 sender never sets it, the
    /// writer never reads it — `task_op_normalized()` is the only consumer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_complete: Option<TaskId>,
```

```rust
impl WriteRequest {
    /// The single place the legacy key is interpreted. Precedence: an
    /// explicit `task_op` wins; otherwise `task_complete = Some(id)` means
    /// `task_id = id, task_op = Close { Completed, reason: None }`; a
    /// request carrying both keys with different ids is invalid.
    pub fn task_op_normalized(&self) -> Result<(Option<TaskId>, Option<TaskOp>), AtmError> {
        match (&self.task_id, &self.task_op, &self.task_complete) {
            (_, Some(op), _) => Ok((self.task_id.clone(), Some(op.clone()))),
            (Some(id), None, Some(legacy)) if id != legacy =>
                Err(AtmError::validation_with_recovery("task_id and task_complete name different tasks", "pass one task: `--task-id <id> --task-complete`, or the legacy `--task-complete <id>` alone")), // error.rs:377-382 (RBP-F001)
            (_, None, Some(legacy)) => Ok((Some(legacy.clone()),
                Some(TaskOp::Close { outcome: TaskCloseOutcome::Completed, reason: None }))),
            (id, None, None) => Ok((id.clone(), None)),
        }
    }
}
```

`prepare_write` calls `task_op_normalized()` once and stores the result on
the envelope as `task_id` / `task_op`; the persisted envelope therefore
never carries `task_complete` after this sprint (old rows still do, and the
same helper decodes them). Direction matrix, both tested in this sprint:

| sender | daemon | `--task-complete X` becomes |
| --- | --- | --- |
| 1.4.0 CLI (`task_complete: X`) | 1.5.0 | `task_id = X, task_op = Close{Completed}` — a close, never an assignment |
| 1.5.0 CLI (`task_id: X, task_op: Close`) | 1.5.0 | a close |
| 1.5.0 CLI | 1.4.0 (not yet restarted) | **refused by the CLI** before the write: the CLI already runs `CompatibilityPreflight` (`protocol.rs:190`) and receives `daemon_http_api_version`; any command that sets `task_op` requires daemon `>= 1.5.0` and otherwise fails with `AtmErrorCode::DaemonIncompatible`-class error naming both versions. Without this guard the old daemon would ignore the unknown `task_op` key and read `task_id` as an assignment — BA.4 wires the guard for its verbs; this sprint wires it for the `--task-complete` alias path it re-maps (`crates/atm/src/commands/send.rs`). |

`AsyncTaskLedgerReader` (`crates/atm-storage/src/contract.rs:916`) gains the
one bounded read BA.3 needs — a team-wide open-task list in queue order:

```rust
    /// Every open task on the team, ordered `(assignee, position, assigned_at, task_id)`.
    async fn open_tasks_for_team(
        &self,
        team: TeamName,
        deadline: ReadDeadline,
    ) -> Result<Vec<TaskRow>, ReadLaneError>;
```

## Schema — exactly as it lands (`crates/atm-storage-rusqlite/src/task_store.rs::TASK_SCHEMA_DDL`)

```sql
CREATE TABLE IF NOT EXISTS tasks (
    team TEXT NOT NULL,
    task_id TEXT NOT NULL,
    assignee TEXT NOT NULL,
    assigner TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('assigned', 'active', 'complete')),
    close_outcome TEXT NULL CHECK(close_outcome IN ('completed', 'refused', 'cancelled')),
    position INTEGER NULL CHECK(position IS NULL OR position >= 1),
    assignment_message_id TEXT NOT NULL,
    description TEXT NOT NULL,
    assigned_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_reminded_at TEXT NULL,
    reminder_count INTEGER NOT NULL DEFAULT 0,
    lead_notified_count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (team, task_id),
    CHECK ((state = 'complete') = (close_outcome IS NOT NULL)),
    CHECK ((state = 'complete') = (position IS NULL))
);

CREATE UNIQUE INDEX IF NOT EXISTS one_active_task_per_agent
    ON tasks(team, assignee) WHERE state = 'active';

CREATE UNIQUE INDEX IF NOT EXISTS tasks_position_per_member
    ON tasks(team, assignee, position) WHERE state <> 'complete';

CREATE INDEX IF NOT EXISTS tasks_open_by_member
    ON tasks(team, assignee, assigned_at) WHERE state <> 'complete';

-- escalation_recipients unchanged

CREATE TABLE IF NOT EXISTS task_events (
    team TEXT NOT NULL,
    task_id TEXT NOT NULL,
    assignee TEXT NOT NULL,
    seq INTEGER NOT NULL,
    at TEXT NOT NULL,
    event TEXT NOT NULL,
    from_state TEXT NULL,
    to_state TEXT NULL,
    close_outcome TEXT NULL CHECK(close_outcome IN ('completed', 'refused', 'cancelled')),
    actor TEXT NOT NULL,
    message_id TEXT NULL,
    outcome TEXT NULL CHECK(outcome IN ('emitted', 'unrenderable', 'blocked')),
    marker TEXT NULL CHECK(marker IN ('resend', 'assignment_missing')),
    detail TEXT NULL,
    PRIMARY KEY (team, task_id, seq)
);
```

Queue contiguity (positions of a member's open tasks are exactly `1..=n`) is
not expressible in SQLite; it is a transaction invariant, asserted by
checked error after every renumber, by the tests, and reported by
`atm doctor` as `TaskQueueGap` (new `DoctorFinding` variant — the only doctor
addition).

## Migration — `crates/atm-storage-rusqlite/src/task_migration.rs` (new, ≤ 300 lines)

Trigger: `SELECT pk FROM pragma_table_info('tasks') WHERE name = 'assignee'`
returns `3` (legacy key) → migrate; `0` → already migrated; table absent →
fresh DDL only. Runs inside `shared_db::ensure_schema` before
`TASK_SCHEMA_DDL`.

```rust
pub(crate) fn migrate_task_identity(
    connection: &mut SqliteConnection,
    target: &SharedDbTarget,
) -> Result<TaskMigrationReport, AtmError>;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct TaskMigrationReport {
    pub rows_before: u64,
    pub tasks_after: u64,
    pub merged_duplicate_rows: u64,   // non-winning rows folded into task_events
    pub demoted_active_conflicts: u64,
    pub backup_path: Option<PathBuf>, // None only when the legacy table was empty
}
```

Steps, one `TransactionBehavior::Immediate` transaction (AZ
`ensure_task_v2_schema` shape):

1. `VACUUM INTO '<db>.pre-ba2.<utc>.sqlite'` **before** the transaction
   (VACUUM cannot run inside one). This is the R0 rollback artifact; its
   path is logged at `info` and returned in the report.
2. `ALTER TABLE tasks RENAME TO tasks_legacy; ALTER TABLE task_events RENAME TO task_events_legacy;`
3. Create the new `tasks` / `task_events` per the DDL above (without `IF NOT EXISTS`).
4. Winner selection — AZ `schema_version.rs:288-321`, adapted with the
   precedence **`complete > active > assigned`**: if any legacy row for
   `(team, task_id)` is complete, the task is complete — the design §3.1
   incident (one completed row, one open row with 587 reminders under one
   id) must migrate to a *closed* task, or the persistent-nudge defect
   survives the migration that claims to end it (FNX-BA-CRIT-001). Among
   open rows, `active` beats `assigned` (AZ rule):

```sql
WITH ranked AS (
    SELECT *, ROW_NUMBER() OVER (
               PARTITION BY team, task_id
               ORDER BY CASE state WHEN 'complete' THEN 0 WHEN 'active' THEN 1 ELSE 2 END,
                        updated_at DESC, assignee ASC) AS winner
      FROM tasks_legacy)
INSERT INTO tasks(team, task_id, assignee, assigner, state, close_outcome, position,
                  assignment_message_id, description, assigned_at, updated_at,
                  last_reminded_at, reminder_count, lead_notified_count)
SELECT team, task_id, assignee, assigner, state,
       CASE state WHEN 'complete' THEN 'completed' END,
       NULL,   -- positions assigned in step 7
       assignment_message_id, description,
       assigned_at,
       updated_at, last_reminded_at, reminder_count, lead_notified_count
  FROM ranked WHERE winner = 1;
```

5. Events: copy `task_events_legacy` renumbering `seq` per `(team, task_id)`
   by `ORDER BY at ASC, assignee ASC, seq ASC`; set `close_outcome =
   'completed'` wherever `to_state = 'complete'` (the only outcome legacy
   could record). Then one `migrated` event per **non-winning** legacy row
   (AZ text: `'migrated source row: assignee=<a>, state=<s>,
   assigned_at=<t>'`, actor `atm-daemon`, `from_state = to_state = <winner
   state>` — state-neutral). No information leaves the database (design §3.2).
6. Canonical history (FNX-BA-CRIT-002): for every winner whose legacy
   history event is absent that produced its state, append it, actor
   `atm-daemon`, `at = winner.updated_at`, `detail = 'synthesized by BA.2
   migration'`: a winner in `active` with no `started` event gets one
   (`from_state = assigned, to_state = active`); a winner in `complete`
   with no `completed` event gets one (`to_state = complete, close_outcome
   = completed`). Legacy `acked` events are kept and are state-neutral.
7. Active-conflict demotion — AZ `:339-380`, adapted: for each `(team,
   assignee)` with more than one `active` row, the winner is the lowest
   `(assigned_at, task_id)`; the others become `assigned` and receive one
   `migrated` event with `from_state = active, to_state = assigned` and
   detail `'demoted because another active task for this member wins by
   original assignment time and task id'`.
8. Positions: per `(team, assignee)`, open rows ordered `active` first then
   `(assigned_at, task_id)` receive `position = 1..=n`.
9. `DROP TABLE tasks_legacy; DROP TABLE task_events_legacy;` create the
   three indexes; commit. Any error → rollback; the legacy tables are
   untouched and the daemon fails to start with the error and the backup
   path in the message.

**Rollback and the pre-BA binary (SCHEMA-SQLITE-ROLLBACK).** There is no
`STORAGE_SCHEMA_VERSION` on `develop` (ADR-061 D1), so a pre-BA binary
cannot refuse the migrated database; its exact behaviour is specified so a
rollback is diagnosable rather than surprising:

- open and every read succeed — `TASK_SCHEMA_DDL` is `CREATE TABLE IF NOT
  EXISTS`, and every task query names its columns (`task_ops.rs`,
  `task_store.rs`), so the new columns are ignored;
- plain sends and reads are unaffected; a task-bearing ack still executes the
  legacy `assigned → active` transition and may succeed or be refused;
- assignment inserts fail inside the writer transaction with the new
  `CHECK constraint failed: tasks` (no `position`), and a second-assignee
  row fails with `UNIQUE constraint failed: tasks.team, tasks.task_id`;
  these are surfaced as the existing storage write error; nothing is
  half-written;
- the supported rollback is: stop the daemon, restore
  `<db>.pre-ba2.<utc>.sqlite` (step 1), start the pre-BA binary. Both task
  and mail writes made after the point-in-time snapshot are lost
  with the restore; the operator is told this in the startup log line that
  names the backup.

This is the one-way, no-bridge default of plan §4 R0; the R0 approval and
ADR-061 D3 exception record it. Test:
`pre_ba_task_insert_against_migrated_schema_fails_with_check_constraint`
(execute the exact `INSERT INTO tasks (...)` statement text from develop's
`apply_task_assignment` against a migrated fixture; assert the constraint
error; assert `mail_messages` untouched).

**Replay rule** (ADR-062 "Replay" row, made exact; FNX-BA-CRIT-020): the
one total order is `seq` under the new `PRIMARY KEY (team, task_id, seq)` —
`at` is never used for ordering and ties in `at` are irrelevant. The state of
`(team, task_id)` is the `to_state` of its highest-`seq` event whose
`to_state IS NOT NULL`. Migration steps 5, 6 and 7 append in that order, each
new event taking `max(seq) + 1` for its task, so a demoted row folds to
`assigned` and a synthesized `completed` folds to `complete`; `rejected`, `reminded`, `lead_notified`, `acked` and `moved`
events carry `to_state = from_state` (or NULL for pre-BA rows) and never
change it; `started`, `completed`, `refused`, `cancelled`, `reassigned`, and
`reopened` change state. The first `assigned` event establishes initial state;
`moved` is state-neutral.
The migration asserts, before commit, that this fold equals `tasks.state`
/ `close_outcome` for every row (checked error + the
`replay_of_migrated_history_reproduces_row_state` test).

## Writer — `crates/atm-storage-rusqlite/src/writer/task_ops.rs`

`apply_task_message(record, connection, cache, target)` dispatches on the
envelope:

| `task_id` | `task_op` | applies |
| --- | --- | --- |
| `None` | any | nothing (a `task_op` without `task_id` is rejected at the CLI and at `WriteRequest` validation) |
| `Some` | `None` | `apply_task_assignment`: (a) no row inserts `assigned`; (b) same agent is a no-op; (c) other agent renumbers both queues, updates assignee/state/assigned_at/position per `placement`, and emits `reassigned`; (d) closed row resets outcome/counters, assigns position per `placement`, and emits `reopened`. `placement: Option<MoveTarget>` is consumed only here; None means END. |
| (legacy `task_complete = Some`) | — | never reaches the writer: `WriteRequest::task_op_normalized()` has already turned it into `task_id = Some, task_op = Some(Close{Completed})` |
| `Some` | `Some(Start)` | `apply_task_start` |
| `Some` | `Some(Close{..})` | `apply_task_close` |
| — | — | `apply_task_move` is **not** message-carried: only `WriteOp::TaskMove` (BA.4, message-less `RequestEnvelope::TaskMove`) reaches it (design §5; FNX-BA-CRIT-023) |

Signatures:

```rust
pub(super) fn apply_task_message(record: &Message, connection: &Connection, cache: &mut WriterStatementCache, target: &SharedDbTarget) -> Result<(), AtmError>;
fn apply_task_assignment(record: &Message, task_id: &TaskId, connection: &Connection, target: &SharedDbTarget) -> Result<(), AtmError>;
fn apply_task_start(record: &Message, task_id: &TaskId, connection: &Connection, target: &SharedDbTarget) -> Result<(), AtmError>;
fn apply_task_close(record: &Message, task_id: &TaskId, outcome: TaskCloseOutcome, reason: Option<&str>, connection: &Connection, cache: &mut WriterStatementCache, target: &SharedDbTarget) -> Result<(), AtmError>;
pub(super) fn apply_task_move(team: &TeamName, task_id: &TaskId, actor: &AgentName, target_pos: &MoveTarget, at: IsoTimestamp, connection: &Connection, target: &SharedDbTarget) -> Result<QueuePosition, AtmError>;
/// Renumbers one member's open queue to 1..=n in `order`, inside the caller's transaction, in two
/// phases so `tasks_position_per_member` (immediate; SQLite has no deferrable UNIQUE) never sees a
/// transient duplicate (FNX-BA-CRIT-021):
///   1. `UPDATE tasks SET position = position + ?3 WHERE team = ?1 AND assignee = ?2 AND state <> 'complete'`
///      with ?3 = order.len() — parks every open row above the old range (positions stay >= 1);
///   2. one `UPDATE tasks SET position = ?3 WHERE team = ?1 AND task_id = ?2` per entry of `order`, ?3 = 1..=n.
/// A swap of positions 1 and 2 therefore goes 1,2 → 3,4 → 2,1 with no collision.
fn renumber_queue(team: &TeamName, assignee: &AgentName, order: &[TaskId], connection: &Connection, target: &SharedDbTarget) -> Result<(), AtmError>;
```

Rules (D6): row lookup is by `(team, task_id)` only — the sender-first /
recipient fallback in `apply_task_completion` (`:305-320`) is deleted.
Authority: `Start` requires `actor == Daemon`; `Close` requires assignee,
assigner or the team's unique lead; `Move` requires assigner or unique lead
(AZ `require_unique_lead` copied; a team with `n != 1` leads rejects with
`NotAuthorized` and the count in `detail` — no error code exists or is added,
see the `TaskRejectionKind` note above). `Start` sets `reminder_count = 0, lead_notified_count = 0` and moves the row
to position 1 (renumber); it leaves `last_reminded_at` exactly as the handoff
audit wrote it — a `NULL` here would make the task due again on the next
tick (FNX-BA-CRIT-029), and `state = assigned AND position = 1 AND
last_reminded_at IS NOT NULL` is the "start owed" predicate BA.3 retries on. `Close`
sets `close_outcome`, `position = NULL`, renumbers the remainder, and keeps
`acknowledge_completed_assignment`. `Move` renumbers only; `assigned_at`
appears in an `UPDATE … SET` list only in the reassign and reopen branches of `apply_task_assignment` (never Start, Close, Move, or renumber).

`apply_task_move` — the active task holds position 1 by invariant and is
never repositioned or preempted (design §4.3). Exact arm, before any
renumber:

```rust
    let row = load_task_row(team, task_id, connection, target)?
        .ok_or_else(|| task_rejected(TaskRejectionKind::NoOpenTask, format!("no task {task_id} on {team}")))?;
    let Some(current) = row.position else {
        return Err(task_rejected(TaskRejectionKind::NotAuthorized, format!("task {task_id} requires explicit assign to reopen")));
    };
    if row.state == TaskState::Active {
        append_task_event(team, task_id, &row.assignee, TaskEventKind::Moved, Some(row.state), Some(row.state),
            actor, None, None, None, Some(format!("{current}→{current}")), at, connection, target)?;
        return Ok(current); // accepted, nothing renumbered
    }
    let order = resolve_move_order(&open_rows, task_id, target_pos)?; // Head → after the active task if any
    renumber_queue(team, &row.assignee, &order, connection, target)?;
```

`detail` for every other move is `"<from>→<to>"` with the 1-based positions
(e.g. `"3→1"`).

Consecutive refusals (design §4.2): `atm-storage-rusqlite/src/task_sql.rs`
owns `trailing_refusal_run(conn, team, assignee) -> RefusalRun`:
`SELECT event, at FROM task_events WHERE team = ?1 AND assignee = ?2 AND
event IN ('assigned','reassigned','reopened','completed','refused','cancelled') ORDER BY
rowid DESC`. Rust counts leading refused rows; `started_at` is the
oldest refused timestamp. Started/assigned/moved/migrated do not reset it;
completed and cancelled end it; assigned, reassigned, and reopened also end it; started, moved, and migrated neither count nor reset. `apply_task_close` with
`outcome = Refused` computes, in the same transaction, the assignee's
trailing run of `refused` closes (`SELECT close_outcome FROM tasks WHERE team
= ?1 AND assignee = ?2 AND state = 'complete' ORDER BY updated_at DESC,
task_id DESC` read until the first non-`refused`) and returns it:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefusalRun { pub count: u32, pub started_at: Option<IsoTimestamp> }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
`one_active_task_per_agent` violation on `Start` maps to
`TaskRejectionKind::ActiveElsewhere`, not to a generic SQLite error.

The writer's Start gate uses the latest reminder audit:
`SELECT outcome FROM task_events WHERE team = ?1 AND task_id = ?2 AND event = 'reminded' AND rowid > (SELECT COALESCE(MAX(rowid),0) FROM task_events WHERE team = ?1 AND task_id = ?2 AND event IN ('assigned','reassigned','reopened')) ORDER BY rowid DESC LIMIT 1`.
It starts only for `outcome = 'emitted'`; otherwise Start is a silent no-op.

Every accepted op appends one `task_events` row (`started` / `completed` +
`close_outcome` / `moved` with `detail = "<from>→<to>"`); every rejection
appends one `rejected` row with the `TaskRejectionKind` in `detail`
(`append_rejected_task_event` at `:68`, unchanged).

`TaskStore` (`crates/atm-storage/src/task_store.rs:66`) is unchanged except
`load_task(&self, team: &TeamName, task_id: &TaskId)` — the `MemberKey`
parameter goes away with the key. `open_tasks(&MemberKey)` keeps its shape
and now returns rows ordered by `position`.

## Boundary manifests (ruling: phase plan §10)

`boundaries/atm-storage/task-store.toml`, `…-rusqlite/task-store-sqlite.toml`:
`[contracts].notes` "state change" → `tasks.state is written by writer/task_ops.rs (apply_task_assignment, apply_task_start, apply_task_close) and, at schema-ensure time only, by task_migration.rs::migrate_task_identity; no other module; state-changing audit kinds are assigned, reassigned, reopened, started, completed, refused, cancelled, migrated; moved is state-neutral`; "replay" → `per (team,
task_id)`; `[ownership].io_forbidden` += `"task_body_dereference"`;
`[enforcement].review_gates` += `"no_task_state_write_outside_task_ops"`,
`"assigned_at_updated_only_by_reassign_and_reopen"`. `async-task-ledger-reader*.toml`: add
`open_tasks_for_team` to the read surface. No new manifest.

## Paths to delete

- `apply_task_completion` (`task_ops.rs:295-364`) — replaced by `apply_task_close`
- `load_open_task_rows` if still present after BA.1
- every *read* of `task_complete` except `task_op_normalized()` (the field
  itself stays, decode-only — see "Wire"); the `task_complete` field on
  `--task-complete` **parsing stays** (BA.4 re-wires it to a bool) — from
  this sprint `atm send --task-complete X` builds `task_id = X, task_op =
  Close{Completed}` in `crates/atm/src/commands/send.rs` and runs the
  `>= 1.5.0` daemon guard, so the close path never regresses
- `select_task_row(…, assignee)` in `task_sql.rs` → keyed by `(team, task_id)`

## Tests

Pure — `task_state.rs`:

- `transition_table_is_exhaustive_over_three_events` — all
  `(Option<TaskState> incl. every Complete(outcome), TaskEvent incl. every
  Completed(outcome))` pairs → the table above; 3 outcomes × … enumerated,
  no wildcard in the test.
- `started_on_active_is_idempotent`, `assigned_on_complete_reopens_same_id`,
  `completed_on_complete_is_already_complete_kind`.
- `task_row_json_keeps_scalar_state_and_adds_close_outcome` — a
  `Complete(Refused)` row serialises to `"state":"complete","close_outcome":"refused"`
  and no `position` key; an `Assigned` row to `"state":"assigned","position":2`
  and no `close_outcome` key; both round-trip.
- `task_row_json_from_1_4_0_producer_decodes` — the exact `TaskRow` JSON a
  1.4.0 daemon emits (fixture string, `state:"complete"`, no new keys) →
  `Complete(Completed)`; the exact `state:"active"` and `state:"assigned"`
  rows a 1.4.0 daemon emits (no `position` key) → `Active` / `Assigned` with
  `position = None` (FNX-BA-CRIT-019).
- `task_row_json_rejects_position_on_complete_row` — `state:"complete"` with
  `position: 1` → validation error.
- `task_event_row_json_reopened_round_trips_and_other_kinds_reject_complete_to_assigned` — a `reopened` row `from_state: "complete", close_outcome: "<o>", to_state: "assigned"` round-trips for each of the three prior outcomes; the same `from_state`/`to_state` pair with any other `event` is rejected.
- `task_state_from_parts_rejects_mismatched_outcome` — 5 arms of
  `from_parts`.
- `task_event_row_json_projects_close_outcome_from_to_state`.
- `queue_position_rejects_zero`, `queue_position_head_is_one`.

Writer — `crates/atm-storage-rusqlite/tests/task_identity.rs` (new):

- `assign_existing_open_id_to_other_agent_reassigns_in_place_and_renumbers_both_queues` — one id, both queues renumbered.
- `assign_closed_id_reopens_in_place_clearing_outcome_and_counters`.
- `reassign_releases_old_active_slot_in_same_transaction`.
- `assign_same_agent_open_id_emits_no_event`.
- `start_when_another_task_active_is_rejected_active_elsewhere` — the index;
  row stays `assigned`; one `rejected` event.
- `start_moves_task_to_position_one_and_resets_counters` — A has T1(pos1,
  reminder_count 7), T2(pos2); start T2 → T2 pos1 active, T1 pos2,
  counters 0/0/NULL on T2, T1's counters untouched.
- `close_renumbers_remaining_queue_contiguously` — T1..T4; close T2 →
  positions 1,2,3 for T1,T3,T4; T2 `position IS NULL`, `close_outcome` set.
- `close_each_outcome_persists_column_and_event` — 3 cases.
- `move_head_with_active_task_lands_at_position_two`; `move_head_without_active_lands_at_one`;
  `move_end`; `move_before_target_of_other_member_is_unknown_target`;
  `move_before_self_is_noop_with_moved_event`; `move_never_changes_assigned_at`
  (assert every row's `assigned_at` byte-equal before/after).
- `move_of_active_task_is_a_noop_with_moved_event` — the active task holds
  position 1 by invariant; `Move` on it is accepted, changes nothing, and
  appends one `moved` event with `detail = "1→1"`.
- `renumber_is_atomic_under_failure` — inject a failing statement mid-renumber
  (test hook on `renumber_queue`); assert the queue is unchanged and
  contiguous.
- `renumber_swap_of_positions_one_and_two_never_violates_unique_index` —
  with `tasks_position_per_member` in place, move T2 `--head` over T1 (a pure
  swap) and move T4 `--head` over T1..T3; no `SQLITE_CONSTRAINT`, final
  positions contiguous (FNX-BA-CRIT-021).
- `assigned_at_updated_only_by_reassign_and_reopen` — Start, Close, Move and renumber leave `assigned_at` unchanged; reassign and reopen set it to `now`.
- `close_by_third_party_is_not_authorized`, `close_by_unique_lead_succeeds`,
  `close_by_lead_when_two_leads_is_not_authorized_with_count_in_detail`,
  `close_by_lead_when_no_lead_is_not_authorized`, `move_by_assignee_is_not_authorized`.
- `move_of_active_task_returns_current_position_and_renumbers_nothing` —
  assert every other row's `position` and `updated_at` byte-equal.
- `legacy_task_complete_request_closes_the_task` — a `WriteRequest` decoded
  from the exact 1.4.0 JSON (`"task_complete":"T1"`, no `task_op`) closes T1
  `completed`; a request with `task_id = "T1"` and `task_complete = "T2"`
  fails validation; `task_op` present + `task_complete` present → `task_op`
  wins.
- `refusal_run_is_tick_driven` — `` is
  `Some(RefusalRun{Refused, 1})` after one refused close.
- `start_by_member_actor_is_not_authorized`.
- `trailing_refusal_run_counts_raw_stored_event_values` — closes: refused, refused,
  completed, refused, refused, refused → the last returns 3; a following
  `completed` returns 0; a following `refused` returns 1 (run restarted).

Migration — `crates/atm-storage-rusqlite/tests/task_migration.rs` (new),
each fixture built from the **legacy** DDL (copied into the test as a
string, never from production code):

- `fresh_database_gets_new_ddl_without_migration` — report all zero, no backup.
- `already_migrated_database_is_a_noop` — second `ensure_schema` returns
  `rows_before == tasks_after`, no backup file.
- `single_row_tasks_migrate_with_positions_by_assigned_at` — 3 assigned
  rows for one member → positions 1..3 in `assigned_at` order; ties broken
  by `task_id`.
- `duplicate_group_complete_twin_closes_the_task` — the design §3.1 shape:
  row A `complete`, row B `active` with `reminder_count = 587` and
  `last_reminded_at` recent, same `(team, task_id)` → one row, `state =
  complete`, `close_outcome = completed`, `position IS NULL`, assignee = A's;
  one `migrated` event naming B; `open_tasks(B's member)` is empty and
  `open_tasks_for_team` does not contain the id. Corner: B's `updated_at`
  is *newer* than A's (it was being reminded) and still loses.
- `duplicate_group_active_beats_assigned` — mirror pattern (assignee row
  `active`, assigner mirror row `assigned`) → one `active` row, assignee =
  winner's; one `migrated` event naming the loser.
- `duplicate_group_winner_keeps_its_own_assigned_at_and_losers_are_in_events`.
- `winner_query_executes_against_duplicate_group_fixture` — runs this exact
  statement on a fixture with two legacy rows for one id and asserts one
  canonical row carrying the winner's own `assigned_at`.
- `two_active_tasks_same_member_demotes_later_one` — winner by
  `(assigned_at, task_id)`; loser `assigned`, `migrated` event with the AZ
  detail text; positions 1 (active) and 2.
- `legacy_events_renumbered_by_time_then_assignee_then_seq` and
  `legacy_complete_to_state_gets_close_outcome_completed`.
- `historic_acked_events_survive_migration` — and are state-neutral in replay.
- `migrated_active_row_without_started_event_gets_one` and
  `migrated_complete_row_without_completed_event_gets_one` — synthesized
  `started` / `completed`, actor `atm-daemon`, `detail` as specified; a row
  that already has the event gets nothing.
- `replay_of_migrated_history_reproduces_row_state`
- `replay_uses_seq_not_at_when_timestamps_tie` — two events with identical
  `at` and opposite `to_state`; the fold follows `seq` (FNX-BA-CRIT-020).
- `replay_after_active_conflict_demotion_yields_assigned` — a demoted row's
  synthesized `started` (step 6) then `migrated active→assigned` (step 7)
  fold to `assigned`. — for each of the five
  fixtures (migrated `assigned`, migrated `active`, migrated `complete`,
  duplicate loser folded, active-conflict demotion) fold the `task_events`
  rows by the replay rule and assert equality with `tasks.state` /
  `close_outcome`; the demotion fixture's fold ends on the `migrated`
  event's `to_state = assigned`.
- `migration_replay_mismatch_rolls_back_whole_transaction` — a fixture whose
  re-fold cannot equal the row (corrupted event) → migrate returns AtmError,
  no table changed, schema version unchanged.
- `migration_failure_leaves_legacy_tables_and_backup` — inject failure after
  step 4; assert `tasks_legacy` absent (renamed back by rollback → original
  `tasks` present with original PK), backup file exists.
- `backup_is_written_before_transaction` — backup contents equal the
  pre-migration database (row counts + PK shape).
- `live_fixture_snapshot_migrates` — a committed **anonymised** copy of the
  14-duplicate-group shape from the dev host (team/agent names replaced,
  descriptions emptied; no message bodies) migrates with
  `merged_duplicate_rows == 14`. If the anonymised fixture cannot be
  produced without message content, this test is replaced by a synthetic
  14-group fixture and the sprint says so.

Reader:

- `open_tasks_for_team_orders_by_assignee_position` and excludes complete rows.
- `doctor_reports_task_queue_gap` — hand-write positions 1,3 → finding.

Required tests: `assign_existing_open_id_to_other_agent_reassigns_in_place_and_renumbers_both_queues`, `assign_closed_id_reopens_in_place_clearing_outcome_and_counters`, `reassign_releases_old_active_slot_in_same_transaction`, `assign_same_agent_open_id_emits_no_event`, `trailing_refusal_run_counts_raw_stored_event_values`, `replay_active_to_assigned_via_reassigned`, `replay_complete_to_assigned_via_reopened`, `task_event_row_json_reopened_round_trips_and_other_kinds_reject_complete_to_assigned`, `write_request_placement_round_trips_through_envelope`, `assign_with_before_survives_to_transaction`, `duplicate_group_winner_keeps_its_own_assigned_at_and_losers_are_in_events`, `placement_rejected_on_start_and_close`, `assign_before_active_target_is_unknown_target`, and `assign_before_foreign_member_target_is_unknown_target`.

## Acceptance criteria

1. Types, DDL and signatures in this document match the source byte-for-byte
   where quoted as code (QA diffs them).
2. Every test above exists by name and passes.
3. `sqlite3 <fixture> "SELECT sql FROM sqlite_master WHERE name IN ('tasks','task_events','one_active_task_per_agent','tasks_position_per_member')"` matches the DDL section.
4. `grep -n "assigned_at" crates/atm-storage-rusqlite/src/writer/task_ops.rs` shows it in `UPDATE … SET` only inside the reassign and reopen branches of `apply_task_assignment`.
5. `schema-reviewer` sign-off recorded on the PR with R0's approval comment
   linked **and** ADR-061 D6 showing the Phase BA approval + D3 exception
   entry (a PR comment alone does not satisfy this — FNX-BA-CRIT-003).
6. `just lint-boundaries` passes with the manifest edits.
7. `HTTP_API_VERSION == "1.5.0"`; `docs/http-api.md` and the ADR-061 version
   table record the `task_op` / `TaskRow` additions; the
   `legacy_task_complete_request_closes_the_task` and
   `task_row_json_from_1_4_0_producer_decodes` fixtures use verbatim 1.4.0
   JSON captured from `origin/develop` (committed as test strings).
8. The `>= 1.5.0` daemon guard exists and is exercised by
   `send_task_complete_refuses_daemon_below_1_5_0` (CLI test with a stub
   preflight verdict).

## Required validation

`just lint`, `just test`, `just lint-boundaries`, RULE-003; migration tests run
under both `--features bundled` and the system SQLite (window functions
require SQLite ≥ 3.25 — assert `sqlite_version()` in the test and fail loudly).

## Out of scope

CLI verbs and aliases (BA.4); runtime disposition (BA.3); `move` without a
message (`WriteOp::TaskMove`, BA.4 — this sprint exposes `apply_task_move`
as `pub(super)` for it).


Tests include `state_write_from_other_module_fails_boundary_gate`.

Tests: `reassign_from_stalled_row_starts_fresh_episode`; `prior_assignment_reminder_never_makes_new_assignment_start_owed`; `close_after_reassignment_between_preflight_and_write_is_rejected_then_recomposed`.

Canonicalization: after merge, fold history and append exactly one `canonicalized by BA.2 migration` event when the winner state differs; enforce replay mismatch as a rollback error, never debug-only.

Branch c resets `reminder_count = 0, lead_notified_count = 0, last_reminded_at = NULL`; branch d uses the identical clause.

Branch d resets `reminder_count = 0, lead_notified_count = 0, last_reminded_at = NULL`.
