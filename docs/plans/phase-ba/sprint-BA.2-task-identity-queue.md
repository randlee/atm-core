# BA.2 — Task identity, queue position, typed outcome, mutation boundary, migration

| Field | Value |
| --- | --- |
| Design | [`nudge-task-design.md`](./nudge-task-design.md) §3.1, §3.2, §4, §4.1, §4.3 (commit `9b5c7d876`) |
| Outcomes | B3, B5, B6, B7 |
| Recommended | arch-ctm / deep-reasoning — schema rebuild with live data and a transactional queue renumber |
| Depends on | `must_follow` BA.1 (branch ancestry — same files) |
| Worktree | `feature/ba2-task-identity-queue` off `feature/ba1-ack-task-separation` |
| Governed interfaces | SQLite **MAJOR** (plan §4 R0); `AsyncTaskLedgerReader` +1 method |
| Blocked until | R0 recorded (plan §4) |

## Scope

One row per `(team, task_id)`. At most one `active` task per agent, enforced
by SQLite. Queue order `(position, assigned_at, task_id)`. Typed close
outcome. Every task mutation flows through one `TaskOp` carried on the
write request and applied in the message writer transaction. One-way
migration of live databases. No CLI, no runtime behaviour (BA.3/BA.4).

## Phase AZ code used

| AZ artifact (`origin/integrate/phase-az`) | how |
| --- | --- |
| `crates/atm-storage-rusqlite/src/schema_version.rs:284-286` `CREATE UNIQUE INDEX one_active_task_per_agent … WHERE state = 'active'` | **copied verbatim** (column `assignee`, not `current_assignee`) — DDL below |
| `schema_version.rs:288-321` winner ranking (`ROW_NUMBER() OVER (PARTITION BY team, task_id ORDER BY precedence active>assigned>complete, updated_at DESC, assignee ASC)`) and `:339-380` deterministic active-conflict demotion + audit event | **copied and adapted** to the in-place rebuild — migration SQL below; the audit-row text is kept |
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
    Reassigned,
}

impl TaskCloseOutcome {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Refused => "refused",
            Self::Cancelled => "cancelled",
            Self::Reassigned => "reassigned",
        }
    }
}

/// Open state and a terminal outcome are unrepresentable together.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "state", content = "outcome")]
pub enum TaskState {
    Assigned,
    Active,
    Complete(TaskCloseOutcome),
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
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
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
) -> Result<Transition, TaskRejected> {
    use TaskEvent as E;
    use TaskState as S;
    match (state, event) {
        (None, E::Assigned) => Ok(Transition(S::Assigned)),
        (None, E::Started | E::Completed(_)) => Err(TaskRejected::new(format!("no open task {task_id} for {actor}"))),
        (Some(S::Assigned), E::Assigned) => Ok(Transition(S::Assigned)),   // idempotent resend
        (Some(S::Active), E::Assigned) => Ok(Transition(S::Active)),       // idempotent resend
        (Some(S::Assigned | S::Active), E::Started) => Ok(Transition(S::Active)), // idempotent when Active
        (Some(S::Assigned | S::Active), E::Completed(outcome)) => Ok(Transition(S::Complete(outcome))),
        (Some(S::Complete(_)), E::Assigned) => Err(TaskRejected::new(format!("task {task_id} already complete; use a new id"))),
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
    AlreadyComplete,
    NotAuthorized,
    ActiveElsewhere, // one-active index hit on Started
    UnknownTarget,   // Move Before(id) names no open task of the same member
}
```

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    /// Immutable after insert; `move` never touches it.
    pub assigned_at: IsoTimestamp,
    pub updated_at: IsoTimestamp,
    pub last_reminded_at: Option<IsoTimestamp>,
    pub reminder_count: u32,
    pub lead_notified_count: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskEventKind {
    Assigned,
    Acked,      // history only; never written after BA.1
    Started,
    Completed,
    Rejected,
    Reminded,
    LeadNotified,
    Moved,
    Migrated,   // written only by the BA.2 migration
}

pub struct TaskEventRow {
    pub team: TeamName,
    pub task_id: TaskId,
    pub assignee: AgentName,        // column kept; no longer part of the key
    pub seq: u64,
    pub at: IsoTimestamp,
    pub event: TaskEventKind,
    pub from_state: Option<TaskState>,
    pub to_state: Option<TaskState>, // carries the outcome for Completed
    pub actor: TaskActor,
    pub message_id: Option<AtmMessageId>,
    pub outcome: Option<ReminderOutcome>,
    pub marker: Option<TaskEventMarker>,
    pub detail: Option<String>,
}
```

```rust
// crates/atm-storage/src/task_op.rs  (new file, re-exported from lib.rs and atm_core::boundary)

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "op")]
pub enum TaskOp {
    /// Daemon-only (plan §4 R1). Resets reminder counters.
    Start,
    Close { outcome: TaskCloseOutcome, #[serde(default, skip_serializing_if = "Option::is_none")] reason: Option<String> },
    Move { target: MoveTarget },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "to")]
pub enum MoveTarget {
    /// Position 1, or position 2 when the member has an active task.
    Head,
    End,
    Before { task_id: TaskId },
}
```

`WriteRequest` (`crates/atm-core/src/send/mod.rs:127`) and the persisted
envelope (`crates/atm-storage/src/schema/inbox_message.rs`, field
`task_complete`):

```rust
    pub task_id: Option<TaskId>,
    /// Mutation applied to `task_id` in the same writer transaction as this
    /// message. `None` with `task_id` set means assign.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_op: Option<TaskOp>,
    // `task_complete: Option<TaskId>` is removed from both structs. Old
    // persisted envelopes carry the key; serde ignores it on decode.
```

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
    close_outcome TEXT NULL CHECK(close_outcome IN ('completed', 'refused', 'cancelled', 'reassigned')),
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
    close_outcome TEXT NULL CHECK(close_outcome IN ('completed', 'refused', 'cancelled', 'reassigned')),
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
`debug_assert!` after every renumber, by the tests, and reported by
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
4. Winner selection — AZ `schema_version.rs:288-321`, adapted:

```sql
WITH ranked AS (
    SELECT *, ROW_NUMBER() OVER (
               PARTITION BY team, task_id
               ORDER BY CASE state WHEN 'active' THEN 0 WHEN 'assigned' THEN 1 ELSE 2 END,
                        updated_at DESC, assignee ASC) AS winner
      FROM tasks_legacy)
INSERT INTO tasks(team, task_id, assignee, assigner, state, close_outcome, position,
                  assignment_message_id, description, assigned_at, updated_at,
                  last_reminded_at, reminder_count, lead_notified_count)
SELECT team, task_id, assignee, assigner, state,
       CASE state WHEN 'complete' THEN 'completed' END,
       NULL,   -- positions assigned in step 7
       assignment_message_id, description,
       (SELECT MIN(o.assigned_at) FROM tasks_legacy o WHERE o.team = ranked.team AND o.task_id = ranked.task_id),
       updated_at, last_reminded_at, reminder_count, lead_notified_count
  FROM ranked WHERE winner = 1;
```

5. Events: copy `task_events_legacy` renumbering `seq` per `(team, task_id)`
   by `ORDER BY at ASC, assignee ASC, seq ASC`; set `close_outcome =
   'completed'` where `event = 'completed'`. Then one `migrated` event per
   **non-winning** legacy row (AZ text: `'migrated source row:
   assignee=<a>, state=<s>, assigned_at=<t>'`, actor `atm-daemon`). No
   information leaves the database (design §3.2).
6. Active-conflict demotion — AZ `:339-380`, adapted: for each `(team,
   assignee)` with more than one `active` row, the winner is the lowest
   `(assigned_at, task_id)`; the others become `assigned` and receive one
   `migrated` event with detail `'demoted because another active task for
   this member wins by original assignment time and task id'`.
7. Positions: per `(team, assignee)`, open rows ordered `active` first then
   `(assigned_at, task_id)` receive `position = 1..=n`.
8. `DROP TABLE tasks_legacy; DROP TABLE task_events_legacy;` create the
   three indexes; commit. Any error → rollback; the legacy tables are
   untouched and the daemon fails to start with the error and the backup
   path in the message.

## Writer — `crates/atm-storage-rusqlite/src/writer/task_ops.rs`

`apply_task_message(record, connection, cache, target)` dispatches on the
envelope:

| `task_id` | `task_op` | applies |
| --- | --- | --- |
| `None` | any | nothing (a `task_op` without `task_id` is rejected at the CLI and at `WriteRequest` validation) |
| `Some` | `None` | `apply_task_assignment` — insert at `position = n+1`, or idempotent resend (`marker = resend`) |
| `Some` | `Some(Start)` | `apply_task_start` |
| `Some` | `Some(Close{..})` | `apply_task_close` |
| `Some` | `Some(Move{..})` | `apply_task_move` (also reachable without a message: BA.4 `WriteOp::TaskMove`) |

Signatures:

```rust
pub(super) fn apply_task_message(record: &Message, connection: &Connection, cache: &mut WriterStatementCache, target: &SharedDbTarget) -> Result<(), AtmError>;
fn apply_task_assignment(record: &Message, task_id: &TaskId, connection: &Connection, target: &SharedDbTarget) -> Result<(), AtmError>;
fn apply_task_start(record: &Message, task_id: &TaskId, connection: &Connection, target: &SharedDbTarget) -> Result<(), AtmError>;
fn apply_task_close(record: &Message, task_id: &TaskId, outcome: TaskCloseOutcome, reason: Option<&str>, connection: &Connection, cache: &mut WriterStatementCache, target: &SharedDbTarget) -> Result<(), AtmError>;
pub(super) fn apply_task_move(team: &TeamName, task_id: &TaskId, actor: &AgentName, target_pos: &MoveTarget, at: IsoTimestamp, connection: &Connection, target: &SharedDbTarget) -> Result<QueuePosition, AtmError>;
/// Renumbers one member's open queue to 1..=n in `order`; one UPDATE per row inside the caller's transaction.
fn renumber_queue(team: &TeamName, assignee: &AgentName, order: &[TaskId], connection: &Connection, target: &SharedDbTarget) -> Result<(), AtmError>;
```

Rules (D6): row lookup is by `(team, task_id)` only — the sender-first /
recipient fallback in `apply_task_completion` (`:305-320`) is deleted.
Authority: `Start` requires `actor == Daemon`; `Close` requires assignee,
assigner or the team's unique lead; `Move` requires assigner or unique lead
(AZ `require_unique_lead` copied; `TaskLeadMissing` /
`TaskLeadAmbiguous` error codes already exist on develop from #1378 — reuse,
do not add). `Start` sets `reminder_count = 0, lead_notified_count = 0,
last_reminded_at = NULL` and moves the row to position 1 (renumber). `Close`
sets `close_outcome`, `position = NULL`, renumbers the remainder, and keeps
`acknowledge_completed_assignment`. `Move` renumbers only; `assigned_at`
is never in any `UPDATE … SET` list in this file (grep gate).

Consecutive refusals (design §4.2): `apply_task_close` with
`outcome = Refused` computes, in the same transaction, the assignee's
trailing run of `refused` closes (`SELECT close_outcome FROM tasks WHERE team
= ?1 AND assignee = ?2 AND state = 'complete' ORDER BY updated_at DESC,
task_id DESC` read until the first non-`refused`) and returns it:

```rust
pub struct TaskCloseApplied { pub outcome: TaskCloseOutcome, pub consecutive_refusals: u32 }
// carried on WriteOpResult / SendOutcome as `task_close: Option<TaskCloseApplied>`
pub const TASK_CONSECUTIVE_REFUSAL_THRESHOLD: u32 = 3; // atm-storage/src/task_store.rs, next to TASK_STALLED_REMINDER_THRESHOLD (plan §4 R5)
```

The writer only counts; BA.4 sends the escalation. A close with any other
outcome returns `consecutive_refusals = 0`.
`one_active_task_per_agent` violation on `Start` maps to
`TaskRejectionKind::ActiveElsewhere`, not to a generic SQLite error.

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
`[contracts].notes` "state change" → `only TaskOp arms in writer/task_ops.rs
change tasks.state; events Assigned/Started/Completed`; "replay" → `per (team,
task_id)`; `[ownership].io_forbidden` += `"task_body_dereference"`;
`[enforcement].review_gates` += `"no_task_state_write_outside_task_op"`,
`"assigned_at_immutable"`. `async-task-ledger-reader*.toml`: add
`open_tasks_for_team` to the read surface. No new manifest.

## Paths to delete

- `apply_task_completion` (`task_ops.rs:295-371`) — replaced by `apply_task_close`
- `load_open_task_rows` if still present after BA.1
- `task_complete` fields: `WriteRequest`, `inbox_message.rs` envelope,
  `send/outcome.rs`; `--task-complete` **parsing stays** (BA.4 re-wires it)
  — until BA.4, `atm send --task-complete` maps to `TaskOp::Close{Completed}`
  in `crates/atm/src/commands/send.rs` so the branch never loses the close path
- `select_task_row(…, assignee)` in `task_sql.rs` → keyed by `(team, task_id)`

## Tests

Pure — `task_state.rs`:

- `transition_table_is_exhaustive_over_three_events` — all
  `(Option<TaskState> incl. every Complete(outcome), TaskEvent incl. every
  Completed(outcome))` pairs → the table above; 4 outcomes × … enumerated,
  no wildcard in the test.
- `started_on_active_is_idempotent`, `assigned_on_complete_says_use_new_id`,
  `completed_on_complete_is_already_complete_kind`.
- `task_state_serde_roundtrip_carries_outcome` —
  `{"state":"complete","outcome":"refused"}` ↔ `TaskState::Complete(Refused)`;
  `{"state":"assigned"}` has no `outcome` key.
- `queue_position_rejects_zero`, `queue_position_head_is_one`.

Writer — `crates/atm-storage-rusqlite/tests/task_identity.rs` (new):

- `assign_second_row_same_task_id_different_assignee_is_rejected` — the PK;
  rejection kind `AlreadyComplete` when complete, otherwise idempotent
  resend only when assignee matches, else `Rejected` with detail naming the
  existing assignee. Corner: **same** task id resent to a different member.
- `start_when_another_task_active_is_rejected_active_elsewhere` — the index;
  row stays `assigned`; one `rejected` event.
- `start_moves_task_to_position_one_and_resets_counters` — A has T1(pos1,
  reminder_count 7), T2(pos2); start T2 → T2 pos1 active, T1 pos2,
  counters 0/0/NULL on T2, T1's counters untouched.
- `close_renumbers_remaining_queue_contiguously` — T1..T4; close T2 →
  positions 1,2,3 for T1,T3,T4; T2 `position IS NULL`, `close_outcome` set.
- `close_each_outcome_persists_column_and_event` — 4 cases.
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
- `assigned_at_absent_from_every_update_statement` — greps the source file
  for `UPDATE tasks` statements and asserts none sets `assigned_at`.
- `close_by_third_party_is_not_authorized`, `close_by_unique_lead_succeeds`,
  `close_by_lead_when_two_leads_is_lead_ambiguous`, `move_by_assignee_is_not_authorized`.
- `start_by_member_actor_is_not_authorized`.
- `refused_close_counts_trailing_refusals_only` — closes: refused, refused,
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
- `duplicate_group_active_beats_assigned_beats_complete` — mirror pattern
  (assignee row `active`, assigner mirror row `assigned`) → one `active`
  row, assignee = winner's; one `migrated` event naming the loser.
- `duplicate_group_min_assigned_at_is_kept`.
- `two_active_tasks_same_member_demotes_later_one` — winner by
  `(assigned_at, task_id)`; loser `assigned`, `migrated` event with the AZ
  detail text; positions 1 (active) and 2.
- `legacy_events_renumbered_by_time_then_assignee_then_seq` and
  `legacy_completed_events_get_close_outcome_completed`.
- `historic_acked_events_survive_migration`.
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

## Acceptance criteria

1. Types, DDL and signatures in this document match the source byte-for-byte
   where quoted as code (QA diffs them).
2. Every test above exists by name and passes.
3. `sqlite3 <fixture> "SELECT sql FROM sqlite_master WHERE name IN ('tasks','task_events','one_active_task_per_agent','tasks_position_per_member')"` matches the DDL section.
4. `grep -n "assigned_at" crates/atm-storage-rusqlite/src/writer/task_ops.rs` shows it only in `INSERT` and `SELECT`/`ORDER BY` contexts.
5. `schema-reviewer` sign-off recorded on the PR with R0's approval comment linked.
6. `just lint-boundaries` passes with the manifest edits.

## Required validation

`just lint`, `just test`, `just lint-boundaries`, RULE-003; migration tests run
under both `--features bundled` and the system SQLite (window functions
require SQLite ≥ 3.25 — assert `sqlite_version()` in the test and fail loudly).

## Out of scope

CLI verbs and aliases (BA.4); runtime disposition (BA.3); `move` without a
message (`WriteOp::TaskMove`, BA.4 — this sprint exposes `apply_task_move`
as `pub(super)` for it).
