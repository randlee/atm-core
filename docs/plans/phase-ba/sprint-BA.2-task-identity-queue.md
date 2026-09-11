# BA.2 — Task identity, queue position, typed outcome, migration

| Field | Value |
| --- | --- |
| Design | [`nudge-task-design.md`](./nudge-task-design.md) §3.1, §3.1a, §3.2, §4, §4.1, §4.2, §4.3 (commit `18db5acc3`) |
| Recommended | arch-ctm / deep-reasoning — schema rebuild with live data and a transactional queue renumber |
| Depends on | `must_follow` BA.1 (branch ancestry — same files) |
| Worktree | `feature/ba2-task-identity-queue` off `feature/ba1-ack-task-separation` |
| Governed interfaces | SQLite **MAJOR** (plan §2 R0, approved); HTTP **MINOR** `1.4.0 → 1.5.0` (additive `task_op`, `placement` on `WriteRequest`; additive `close_outcome`/`position` on `TaskRow` JSON — plan §6); `AsyncTaskLedgerReader` +2 methods |

## Tasks

1. Land the types below — `crates/atm-storage/src/task_state.rs`, `task_op.rs` (new) (see "Types").
2. Add `task_op`, `placement`, `task_op_normalized()` to `WriteRequest` and the envelope — `crates/atm-core/src/send/mod.rs:127`, `inbox_message.rs:187-193` (see "Wire").
3. Add `SendOutcome.already_closed`; `HTTP_API_VERSION = "1.5.0"` — `crates/atm-core/src/send/outcome.rs`, `protocol.rs:99`.
4. Add `open_tasks_for_team`, `refusal_run`; rekey `load_task(team, task_id)` — `crates/atm-storage/src/contract.rs:916`, `task_store.rs:66`.
5. Rebuild `TASK_SCHEMA_DDL` — `crates/atm-storage-rusqlite/src/task_store.rs:15-58` (see "Schema").
6. Write `migrate_task_identity` — `crates/atm-storage-rusqlite/src/task_migration.rs` (new) (see "Migration").
7. Rewrite the writer ops — `crates/atm-storage-rusqlite/src/writer/task_ops.rs` (see "Writer").
8. Add `trailing_refusal_run` — `crates/atm-storage-rusqlite/src/task_sql.rs` (see "Writer").
9. Re-map `--task-complete X` to `task_id = X, task_op = Close{Completed}` behind the `>= 1.5.0` guard — `crates/atm/src/commands/send.rs`.
10. Edit the three boundary manifests — plan §8.
11. Delete the paths under "Paths to delete"; write the tests under "Tests".

## Phase AZ code used

| AZ artifact (`origin/integrate/phase-az`) | how |
| --- | --- |
| `crates/atm-storage-rusqlite/src/schema_version.rs:284-286` `one_active_task_per_agent` index | **copied verbatim** — DDL below |
| `schema_version.rs:288-321` winner ranking; `:339-380` active-conflict demotion + audit event | **copied and adapted**; precedence inverted to `complete > active > assigned` (design §3.1's incident: the completed twin is the truth) |
| `schema_version.rs:383-414` IMMEDIATE transaction → DDL → migrate → commit | **shape reused** as `migrate_task_identity` |

Not used: `tasks_v2`, `task_operations`, `storage_schema_versions`, triggers,
`TaskOperationId`, `TaskPriority`, supersession — design §8.

## Types — exactly as they land

```rust
// crates/atm-storage/src/task_state.rs

/// 1-based queue place; the active task, when one exists, holds 1.
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
pub enum TaskCloseOutcome { Completed, Refused, Cancelled }

impl TaskCloseOutcome {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self { Self::Completed => "completed", Self::Refused => "refused", Self::Cancelled => "cancelled" }
    }
}

/// Open state and a terminal outcome are unrepresentable together. JSON goes
/// through `TaskRowWire` / `TaskEventRowWire` (scalar `state` + `close_outcome`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState { Assigned, Active, Complete(TaskCloseOutcome) }

/// The scalar `state` value as it appears in JSON and in the columns.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStateTag { Assigned, Active, Complete }

impl TaskState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self { Self::Assigned => "assigned", Self::Active => "active", Self::Complete(_) => "complete" }
    }
    #[must_use]
    pub const fn is_open(self) -> bool { !matches!(self, Self::Complete(_)) }
    #[must_use]
    pub const fn tag(self) -> TaskStateTag {
        match self { Self::Assigned => TaskStateTag::Assigned, Self::Active => TaskStateTag::Active, Self::Complete(_) => TaskStateTag::Complete }
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
pub enum TaskEvent { Assigned, Started, Completed(TaskCloseOutcome) }

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
        (Some(S::Complete(_)), E::Started | E::Completed(_)) => {
            unreachable!("complete-row start/close is handled by the writer before transition()")
        }
    }
}

/// Writer control flow and the `rejected` audit `detail`. Never crosses HTTP:
/// the daemon returns `ResponseEnvelope::Error(AtmError)` with the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRejected { pub detail: String }
```

The five rejection messages, exactly:

| cause | `detail` |
| --- | --- |
| no row / row not open for Start, Close, Move | `no open task <id> for <actor>` |
| close by neither assignee nor assigner (develop rule, unchanged) | `task <id> is not assigned to or by <actor>` |
| `one_active_task_per_agent` hit on Start | `task <id>: <assignee> already has an active task` |
| `--before` target unknown, not open, active, or another member's | `task <id>: placement target <other> is not an open queued task of <assignee>` |
| close recipient is no longer the current counterparty | `task <id>: <recipient> is no longer the counterparty — re-run the command` |

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "TaskRowWire", into = "TaskRowWire")]
pub struct TaskRow {
    pub team: TeamName,
    pub task_id: TaskId,
    pub assignee: AgentName,
    pub assigner: AgentName,
    pub state: TaskState,
    pub position: Option<QueuePosition>, // Some iff open
    pub assignment_message_id: AtmMessageId,
    pub description: String,
    pub assigned_at: IsoTimestamp,       // set by assign/reassign/reopen; never by move or start
    pub updated_at: IsoTimestamp,
    pub last_reminded_at: Option<IsoTimestamp>,
    pub reminder_count: u32,
    pub lead_notified_count: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskEventKind {
    Assigned, Acked /* history only */, Started, Completed, Refused, Cancelled,
    Reassigned, Reopened, Rejected, Reminded, LeadNotified, Moved,
    Migrated /* written only by the BA.2 migration */,
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
    pub to_state: Option<TaskState>,
    pub actor: TaskActor,
    pub message_id: Option<AtmMessageId>,
    pub outcome: Option<ReminderOutcome>,
    pub marker: Option<TaskEventMarker>,
    pub detail: Option<String>,
}
```

`TaskRowWire` / `TaskEventRowWire` (private): the same fields with
`TaskStateTag` states plus optional `close_outcome` (and `position` on the row
wire), `#[serde(default, skip_serializing_if = "Option::is_none")]`. `TryFrom`
uses `from_parts`; a `complete` tag with no `close_outcome` (1.4.0 producer)
decodes as `Completed`; a complete row carrying `position` is rejected. On an
event row `close_outcome` is the outcome of whichever side is `Complete`
(`from_state` for `reopened`). SQLite decode uses the same helpers.

```rust
// crates/atm-storage/src/task_op.rs  (new; re-exported from lib.rs and atm_core::boundary)

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "op")]
pub enum TaskOp {
    /// Daemon-only (plan §2 R1). Resets reminder counters.
    Start,
    Close { outcome: TaskCloseOutcome, #[serde(default, skip_serializing_if = "Option::is_none")] reason: Option<String> },
    // No `Move`: move is message-less; its only boundary is `WriteOp::TaskMove` (BA.4).
}

/// Placement for assign (`WriteRequest.placement`) and target for move (BA.4).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "to")]
pub enum MoveTarget {
    /// Position 1, or position 2 when the member has an active task.
    Head,
    End,
    Before { task_id: TaskId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefusalRun { pub count: u32, pub started_at: Option<IsoTimestamp> }
```

### Wire — `WriteRequest` (`crates/atm-core/src/send/mod.rs:127`) and the persisted envelope (`inbox_message.rs:187-193`)

`task_op` and `placement` are added; `task_complete` stays decode-only
(removing it would be MAJOR under ADR-061 D2).

```rust
    pub task_id: Option<TaskId>,
    /// Assignment placement; absent in 1.4.0 payloads, None means END.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<MoveTarget>,
    /// Mutation applied to `task_id` in the same writer transaction as this
    /// message. `None` with `task_id` set means assign.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_op: Option<TaskOp>,
    /// 1.4.0 senders' close. Decode-only; `task_op_normalized()` is the only consumer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_complete: Option<TaskId>,

impl WriteRequest {
    /// Precedence: an explicit `task_op` wins; otherwise `task_complete = Some(id)`
    /// means `task_id = id, task_op = Close { Completed, reason: None }`; both keys
    /// with different ids is invalid.
    pub fn task_op_normalized(&self) -> Result<(Option<TaskId>, Option<TaskOp>), AtmError> {
        match (&self.task_id, &self.task_op, &self.task_complete) {
            (_, Some(op), _) => Ok((self.task_id.clone(), Some(op.clone()))),
            (Some(id), None, Some(legacy)) if id != legacy =>
                Err(AtmError::validation_with_recovery("task_id and task_complete name different tasks", "pass one task: `--task-id <id> --task-complete`, or the legacy `--task-complete <id>` alone")),
            (_, None, Some(legacy)) => Ok((Some(legacy.clone()),
                Some(TaskOp::Close { outcome: TaskCloseOutcome::Completed, reason: None }))),
            (id, None, None) => Ok((id.clone(), None)),
        }
    }
}
```

`prepare_write` calls `task_op_normalized()` once and stores `task_id` /
`task_op` / `placement` on the envelope; old envelopes still decode through the
same helper. A CLI that sets `task_op` requires daemon `>= 1.5.0`
(`CompatibilityPreflight`, `protocol.rs:190`) and otherwise refuses before the
write, naming both versions.

`AsyncTaskLedgerReader` (`crates/atm-storage/src/contract.rs:916`) gains:

```rust
    /// Every open task on the team, ordered `(assignee, position, assigned_at, task_id)`.
    async fn open_tasks_for_team(&self, team: TeamName, deadline: ReadDeadline) -> Result<Vec<TaskRow>, ReadLaneError>;
    /// Trailing consecutive-refusal run for one assignee (design §4.2).
    async fn refusal_run(&self, team: TeamName, assignee: AgentName, deadline: ReadDeadline) -> Result<RefusalRun, ReadLaneError>;
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

Queue contiguity (`1..=n` per member) is a transaction invariant asserted by
checked error after every renumber and by the tests.

## Migration — `crates/atm-storage-rusqlite/src/task_migration.rs` (new, ≤ 300 lines)

Trigger: `SELECT pk FROM pragma_table_info('tasks') WHERE name = 'assignee'`
= `3` → migrate; `0` → done; table absent → fresh DDL. Runs inside
`shared_db::ensure_schema` before `TASK_SCHEMA_DDL`.

```rust
pub(crate) fn migrate_task_identity(connection: &mut SqliteConnection, target: &SharedDbTarget) -> Result<TaskMigrationReport, AtmError>;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct TaskMigrationReport {
    pub rows_before: u64,
    pub tasks_after: u64,
    pub merged_duplicate_rows: u64,   // non-winning rows folded into task_events
    pub demoted_active_conflicts: u64,
    pub backup_path: Option<PathBuf>, // None only when the legacy table was empty
}
```

Steps, one `TransactionBehavior::Immediate` transaction:

1. `VACUUM INTO '<db>.pre-ba2.<utc>.sqlite'` **before** the transaction. This
   is the R0 rollback artifact; its path is logged at `info` and returned.
2. `ALTER TABLE tasks RENAME TO tasks_legacy; ALTER TABLE task_events RENAME TO task_events_legacy;`
3. Create the new `tasks` / `task_events` per the DDL above (without `IF NOT EXISTS`).
4. Winner selection with precedence `complete > active > assigned`; among
   open rows `active` beats `assigned`:

```sql
WITH ranked AS (
    SELECT *, ROW_NUMBER() OVER (
               PARTITION BY team, task_id
               ORDER BY CASE state WHEN 'complete' THEN 0 WHEN 'active' THEN 1 ELSE 2 END,
                        updated_at DESC, assignee ASC) AS winner
      FROM tasks_legacy),
     winners AS (SELECT * FROM ranked WHERE winner = 1)
INSERT INTO tasks(team, task_id, assignee, assigner, state, close_outcome, position,
                  assignment_message_id, description, assigned_at, updated_at,
                  last_reminded_at, reminder_count, lead_notified_count)
SELECT team, task_id, assignee, assigner, state,
       CASE state WHEN 'complete' THEN 'completed' END,
       CASE WHEN state = 'complete' THEN NULL
            ELSE ROW_NUMBER() OVER (
                   PARTITION BY team, assignee, (state = 'complete')
                   ORDER BY CASE state WHEN 'active' THEN 0 ELSE 1 END,
                            assigned_at ASC, task_id ASC) END,   -- position: active first, then FIFO
       assignment_message_id, description,
       assigned_at,
       updated_at, last_reminded_at, reminder_count, lead_notified_count
  FROM winners;
```

5. Events: copy `task_events_legacy` renumbering `seq` per `(team, task_id)`
   by `ORDER BY at ASC, assignee ASC, seq ASC`; `close_outcome = 'completed'`
   wherever `to_state = 'complete'`. One `migrated` event per non-winning row
   (`'migrated source row: assignee=<a>, state=<s>, assigned_at=<t>'`, actor
   `atm-daemon`, `from_state = to_state = <winner state>`).
6. Canonical history: a winner in `active` with no `started` event, or in
   `complete` with no `completed` event, gets one (actor `atm-daemon`, `at =
   winner.updated_at`, `detail = 'synthesized by BA.2 migration'`). Legacy
   `acked` events are kept, state-neutral.
7. Active-conflict demotion (AZ `:339-380`): per `(team, assignee)` with more
   than one `active` row, the lowest `(assigned_at, task_id)` wins; the others
   become `assigned` with a `migrated` event `active → assigned`, detail
   `'demoted because another active task for this member wins by original
   assignment time and task id'`.
8. Verify positions: `SELECT team, assignee, COUNT(*), MAX(position) FROM tasks
   WHERE state <> 'complete' GROUP BY 1,2 HAVING COUNT(*) <> MAX(position)`
   returns no rows; a hit is an `AtmError` and rolls back.
9. Verify replay: the state of `(team, task_id)` is the `to_state` (and
   `close_outcome`) of its highest-`seq` event whose `to_state IS NOT NULL`;
   it must equal `tasks.state` / `close_outcome` for every row, else
   `AtmError` and rollback. Steps 5, 6, 7 append in that order, each new event
   taking `max(seq) + 1`.
10. `DROP TABLE tasks_legacy; DROP TABLE task_events_legacy;` create the
    three indexes; commit. Any error → rollback; the legacy tables are
    untouched and the daemon fails to start with the error and the backup
    path in the message.

**Rollback.** A pre-BA binary cannot refuse the migrated database (no
`STORAGE_SCHEMA_VERSION` on `develop`): reads and plain sends work, assignment
inserts fail with `CHECK constraint failed: tasks` as the existing storage
write error. The supported rollback is: stop the daemon, restore
`<db>.pre-ba2.<utc>.sqlite`, start the pre-BA binary; writes after the
snapshot are lost, and the startup log line that names the backup says so.

## Writer — `crates/atm-storage-rusqlite/src/writer/task_ops.rs`

`apply_task_message(record, connection, cache, target)` dispatches on the
envelope:

| `task_id` | `task_op` | applies |
| --- | --- | --- |
| `None` | any | nothing (a `task_op` without `task_id` is rejected at `WriteRequest` validation) |
| `Some` | `None` | `apply_task_assignment` — (a) no row: insert `assigned` at `placement` (None = END); (b) same agent, open row: acknowledge the prior assignment message, `UPDATE tasks SET assignment_message_id, description, updated_at`, nothing else, no event; (c) other agent, open row: acknowledge the prior assignment message, renumber both queues, set `assignee`, `assigner`, `state = assigned`, `position` per `placement`, `assigned_at = now`, `reminder_count = 0`, `lead_notified_count = 0`, `last_reminded_at = NULL`, emit `reassigned`; (d) closed row: clear `close_outcome`, set `assignee`, `assigner`, `position`, `assigned_at = now`, `reminder_count = 0`, `lead_notified_count = 0`, `last_reminded_at = NULL`, emit `reopened` |
| `Some` | `Some(Start)` | `apply_task_start` |
| `Some` | `Some(Close{..})` | `apply_task_close` |
| — | — | `apply_task_move` is not message-carried: only `WriteOp::TaskMove` (BA.4) reaches it |

```rust
pub(super) fn apply_task_message(record: &Message, connection: &Connection, cache: &mut WriterStatementCache, target: &SharedDbTarget) -> Result<(), AtmError>;
fn apply_task_assignment(record: &Message, task_id: &TaskId, placement: Option<&MoveTarget>, connection: &Connection, target: &SharedDbTarget) -> Result<(), AtmError>;
fn apply_task_start(record: &Message, task_id: &TaskId, connection: &Connection, target: &SharedDbTarget) -> Result<(), AtmError>;
fn apply_task_close(record: &Message, task_id: &TaskId, outcome: TaskCloseOutcome, reason: Option<&str>, connection: &Connection, cache: &mut WriterStatementCache, target: &SharedDbTarget) -> Result<Option<TaskCloseOutcome>, AtmError>;
pub(super) fn apply_task_move(team: &TeamName, task_id: &TaskId, actor: &AgentName, target_pos: &MoveTarget, at: IsoTimestamp, connection: &Connection, target: &SharedDbTarget) -> Result<QueuePosition, AtmError>;
/// Renumbers one member's open queue to 1..=n in `order`, in two phases so the
/// immediate `tasks_position_per_member` index never sees a transient duplicate:
///   1. `UPDATE tasks SET position = position + ?3 WHERE team = ?1 AND assignee = ?2 AND state <> 'complete'` with ?3 = order.len();
///   2. one `UPDATE tasks SET position = ?3 WHERE team = ?1 AND task_id = ?2` per entry of `order`, ?3 = 1..=n.
fn renumber_queue(team: &TeamName, assignee: &AgentName, order: &[TaskId], connection: &Connection, target: &SharedDbTarget) -> Result<(), AtmError>;
```

Rules:

- Row lookup is by `(team, task_id)` only; the recipient fallback in
  `apply_task_completion` (`:305-320`) is deleted.
- `Start` requires actor `atm-daemon` and the latest `reminded` event since the
  last `assigned`/`reassigned`/`reopened` to have `outcome = 'emitted'`
  (`SELECT outcome FROM task_events WHERE team = ?1 AND task_id = ?2 AND event = 'reminded' AND rowid > (SELECT COALESCE(MAX(rowid),0) FROM task_events WHERE team = ?1 AND task_id = ?2 AND event IN ('assigned','reassigned','reopened')) ORDER BY rowid DESC LIMIT 1`);
  otherwise it is a silent no-op. On `active` it is idempotent (no event).
- `Start` sets `reminder_count = 0, lead_notified_count = 0`, moves the row to
  position 1 (renumber), and leaves `last_reminded_at` as the handoff audit
  wrote it. The `one_active_task_per_agent` violation maps to the "already
  has an active task" rejection, not a generic SQLite error.
- `Close` is accepted from the assignee or assigner only (develop rule). It
  sets `close_outcome`, `position = NULL`, renumbers the remainder, and
  acknowledges the current `assignment_message_id` for both `assigned` and
  `active` rows (BA.1).
- `Close` on a row that is already `complete`: the report is written as
  ordinary mail in the same transaction with the `task_id` link dropped, no
  event, row unchanged, and `Some(outcome)` is returned as
  `SendOutcome.already_closed`.
- `Close` whose recipient is not the row's other party (reassigned between
  preflight and write) is the stale-counterparty rejection; nothing written.
- Assign and Move check nothing about the caller. The sender of an assign
  message is written as `assigner` in branches a, c and d.
- `commit_write` with `task_op.is_some()` or `task_id.is_some()` and a `to`
  whose team differs from the caller's or whose host is set returns the plan
  §2 R8 validation error before opening the transaction.
- `assigned_at` is set by branches a, c, d (each is an assignment, design
  §3.1a) and by nothing else; `apply_task_move` and `apply_task_start` never
  touch it (design §4.3).
- BA.1's `admit()` keeps its role; its arms are rewritten to match
  `TaskEvent::Completed(_)`, and BA.1's tests keep their names.
- `apply_task_move`: no row or not open → rejection; a move of the active
  task is accepted, renumbers nothing, and appends `moved` with `detail =
  "1→1"`; otherwise resolve the order (`Head` → after the active task if any;
  `Before` must name an open queued task of the same member), then
  `renumber_queue`; `detail = "<from>→<to>"`.
- Every accepted op appends one event; every rejection appends one
  `rejected` row with the message in `detail`. `append_rejected_task_event`
  (`task_ops.rs:68-110`) is rewritten for the one-id key: after the savepoint
  rollback it loads the row by `(team, task_id)`, audits `assignee =
  row.assignee` and the row's state (the requested recipient only when no row
  exists), and also matches `WriteOp::TaskMove`.
- `trailing_refusal_run(conn, team, assignee) -> RefusalRun` in `task_sql.rs`:
  `SELECT event, at FROM task_events WHERE team = ?1 AND assignee = ?2 AND event IN ('assigned','reassigned','reopened','completed','refused','cancelled') ORDER BY rowid DESC`;
  Rust counts leading `refused` rows, `started_at` is the oldest of them; any
  other listed event ends the run.

## Boundary manifests

Exactly the edits ruled in plan §8; no new manifest.

## Paths to delete

- `apply_task_completion` (`task_ops.rs:295-364`) — replaced by `apply_task_close`
- `load_open_task_rows` if still present after BA.1
- every read of `task_complete` except `task_op_normalized()`
- `select_task_row(…, assignee)` in `task_sql.rs` → keyed by `(team, task_id)`

## Tests

Pure — `task_state.rs`:

- `transition_table_is_exhaustive_over_three_events` — every
  `(Option<TaskState>, TaskEvent)` pair including every outcome, no wildcard;
  covers idempotent Start on `active` and reopen on `complete`.
- `task_state_from_parts_rejects_mismatched_outcome` — 5 arms.
- `queue_position_bounds` — `new(0)` is `None`; `HEAD.get() == 1`.
- `task_row_json_keeps_scalar_state_and_adds_close_outcome` —
  `Complete(Refused)` → `"state":"complete","close_outcome":"refused"`, no
  `position`; `Assigned` → `"state":"assigned","position":2`, no
  `close_outcome`; both round-trip; `complete` with `position` is rejected.
- `task_row_json_from_1_4_0_producer_decodes` — verbatim 1.4.0 fixtures:
  `complete` → `Complete(Completed)`; `active`/`assigned` → `position = None`.
- `task_event_row_json_reopened_round_trips_and_other_kinds_reject_complete_to_assigned`.
- `legacy_task_complete_request_closes_the_task` — verbatim 1.4.0 JSON
  (`"task_complete":"T1"`) → `Close{Completed}` on T1; `task_id = T1` with
  `task_complete = T2` fails validation; both keys present → `task_op` wins.

Writer — `crates/atm-storage-rusqlite/tests/task_identity.rs` (new):

- `assign_existing_open_id_to_other_agent_reassigns_in_place_and_renumbers_both_queues`
  — one id; the superseded assignment message is acknowledged; `reassigned`
  event; counters reset.
- `assign_closed_id_reopens_in_place_clearing_outcome_and_counters`.
- `reassign_releases_old_active_slot_in_same_transaction`.
- `same_agent_resend_refreshes_message_link_without_event` — assign M1, resend
  M2; M1 acknowledged, row description is M2's, zero new events,
  position/assigned_at/counters byte-equal; close acknowledges M2.
- `assign_with_before_survives_to_transaction` — `placement` round-trips
  through the persisted envelope and lands the row before the target.
- `assign_before_invalid_target_is_rejected` — target active, closed, or
  another member's → rejection, one `rejected` event, no row change.
- `start_when_another_task_active_is_rejected_active_elsewhere` — row stays
  `assigned`; one `rejected` event.
- `start_moves_task_to_position_one_and_resets_counters_preserving_last_reminded_at`.
- `start_gate_rejects_member_actor_and_unrenderable_reminder` — actor not
  `atm-daemon` → rejected; latest `reminded` outcome `unrenderable` → no-op,
  zero `started` events.
- `close_renumbers_remaining_queue_contiguously` — T1..T4; close T2 →
  T1,T3,T4 at 1,2,3; T2 `position IS NULL`, `close_outcome` set.
- `close_each_outcome_persists_column_and_event` — 3 cases.
- `close_of_complete_row_delivers_mail_and_returns_already_closed` — no event,
  row unchanged, mail row has no `task_id`.
- `close_by_third_party_is_not_authorized` — rejected event audits the
  canonical holder.
- `close_by_stale_counterparty_is_rejected_atomically` — reassign between
  preflight and write → rejection, nothing written.
- `move_head_end_before_land_at_expected_positions` — head with an active
  task → 2; head without → 1; end; before.
- `move_before_target_of_other_member_is_rejected` — one rejected event,
  state unchanged.
- `move_of_active_task_is_a_noop_with_moved_event` — `detail = "1→1"`, every
  other row byte-equal.
- `renumber_swap_of_positions_one_and_two_never_violates_unique_index` —
  swap T2 over T1; move T4 `--head` over T1..T3; no `SQLITE_CONSTRAINT`.
- `assigned_at_set_only_by_assignment` — start, move, close leave
  `assigned_at` byte-equal; reassign and reopen set it to the event's `at`
  and `last_reminded_at` to `NULL`.
- `writer_rejects_task_op_on_foreign_team_or_host_recipient`.
- `trailing_refusal_run_counts_raw_stored_event_values` — refused, refused,
  completed, refused, refused, refused → 3; then `completed` → 0; then
  `refused` → 1.

Migration — `crates/atm-storage-rusqlite/tests/task_migration.rs` (new), each
fixture built from the **legacy** DDL copied into the test as a string:

- `fresh_and_already_migrated_databases_skip_migration` — report all zero, no
  backup; second `ensure_schema` is a no-op.
- `single_row_tasks_migrate_with_positions_by_assigned_at` — ties by `task_id`.
- `duplicate_group_winner_precedence` — (1) row A `complete`, row B `active`
  with `reminder_count = 587` and newer `updated_at` → one
  `complete(completed)` row, assignee A's, one `migrated` event naming B,
  `open_tasks_for_team` excludes the id; (2) `active` beside `assigned` → one
  `active` row.
- `migration_inserts_open_winners_with_contiguous_positions` — A: active=1,
  FIFO 2,3; B: 1; complete rows NULL.
- `two_active_tasks_same_member_demotes_later_one` — AZ detail text;
  positions 1 (active) and 2.
- `legacy_events_renumbered_and_acked_rows_survive` — `seq` order by time,
  assignee, seq; `acked` rows kept and state-neutral in replay.
- `migrated_rows_without_history_events_get_synthesized_ones` — `started` /
  `completed` per step 6; a row that already has the event gets nothing.
- `replay_of_migrated_history_reproduces_row_state` — five fixtures
  (migrated assigned/active/complete, duplicate loser, demotion).
- `migration_failure_rolls_back_and_leaves_backup` — replay mismatch and an
  injected failure after step 4: original `tasks` present with original PK,
  no table changed, backup exists.
- `pre_ba_task_insert_against_migrated_schema_fails_with_check_constraint` —
  develop's exact `apply_task_assignment` INSERT text → constraint error;
  `mail_messages` untouched.
- `live_fixture_snapshot_migrates` — anonymised 14-duplicate-group fixture →
  `merged_duplicate_rows == 14` (synthetic if an anonymised copy needs message
  content; the PR says so).

Reader:

- `open_tasks_for_team_orders_by_assignee_position` — excludes complete rows.
- `refusal_run_reads_the_trailing_run` — one refused close →
  `RefusalRun { count: 1, started_at: Some(<close at>) }`.

## Acceptance criteria

1. Code quoted in this document matches the source byte-for-byte (QA diffs it).
2. Every test above exists by name and passes under `just test`.
3. `sqlite3 <fixture> "SELECT sql FROM sqlite_master WHERE name IN ('tasks','task_events','one_active_task_per_agent','tasks_position_per_member')"` matches the DDL section.
4. `grep -n "assigned_at" crates/atm-storage-rusqlite/src/writer/task_ops.rs` shows it only in `apply_task_assignment` (insert, reassign, reopen); never in `apply_task_move` or `apply_task_start`.
5. `grep -rn "TaskRejectionKind\|TaskQueueGap" crates/` → nothing.
6. `schema-reviewer` sign-off recorded on the PR citing the ADR-061 D6 Phase BA entry.
7. `just lint-boundaries` passes with the manifest edits.
8. `HTTP_API_VERSION == "1.5.0"`; the ADR-061 version table records the bump.
9. `send_task_complete_refuses_daemon_below_1_5_0` (CLI test, stub verdict) passes.

## Required validation

`just lint`, `just test`, `just lint-boundaries`, RULE-003; migration tests run
under both `--features bundled` and the system SQLite (window functions
require SQLite ≥ 3.25 — assert `sqlite_version()` in the test and fail loudly).
