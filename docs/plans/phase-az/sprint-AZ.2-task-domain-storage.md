---
phase: AZ
sprint: AZ.2
title: Durable task domain and storage migration
branch: feature/az2-task-domain-storage
integration_branch: feature/az1-task-nudge-contract
final_integration_branch: develop
status: planned
recommended_agent: arch-ctm
recommended_model: deep-reasoning
execution_track: stacked
dependency_relations:
  - prerequisite: AZ.1
    dependent: AZ.2
    relation: must_follow
    rationale: AZ.2 replaces task records consumed by the AZ.1 reminder projection; AZ.1 development must be pushed first, merged forward before every AZ.2 round, and its PR must merge before AZ.2 completes.
---

# AZ.2 — Durable task domain and storage migration

## Goal

Replace the message-derived `Assigned/Active/Complete` row with one stable
logical task, immutable assignment attempts, append-only lifecycle events, and
transactional mutation primitives. Migrate existing SQLite ledgers without
losing task identity or audit history, enforce one current assignee and at most
one active task per agent in the database, and make stale task nudge cleanup
part of the same transaction as every transition that makes an assignment
attempt ineligible.

This sprint is a production-ready storage/domain foundation. It intentionally
does not expose the new public CLI until AZ.3, and it does not change the idle
scheduler until AZ.4.

## Binding state and identity contract

### Types

```rust
pub enum TaskState {
    Assigned,
    Active,
    Blocked,
    Closed(TaskOutcome),
}

pub enum TaskOutcome {
    Succeeded,
    Failed,
    Aborted(TaskAbortReason),
}

pub enum TaskAbortReason {
    Cancelled,
    Superseded { successor_task_id: TaskId },
}

pub enum TaskPriority {
    High,
    Normal,
    Low,
}

pub struct AssignmentAttempt(u32);
pub struct TaskOperationId(AtmMessageId);

pub struct TaskRow {
    pub team: TeamName,
    pub task_id: TaskId,
    pub current_assignee: AgentName,
    pub state: TaskState,
    pub priority: TaskPriority,
    pub original_assigned_at: IsoTimestamp,
    pub current_attempt: AssignmentAttempt,
    pub revision: u64,
    pub updated_at: IsoTimestamp,
}

pub struct TaskAssignmentAttempt {
    pub team: TeamName,
    pub task_id: TaskId,
    pub attempt: AssignmentAttempt,
    pub assignee: AgentName,
    pub assigner: AgentName,
    pub assignment_message_id: AtmMessageId,
    pub template_sha: Option<TemplateSha>,
    pub assigned_at: IsoTimestamp,
}

pub enum TaskEventKind {
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
}
```

`TaskAssignmentAttempt` is insert-only. It stores the durable message and
optional template identity, not a rendered body, template bytes, variables,
summary, or duplicate task description. The immutable message/template stores
remain authoritative for those records. `TaskEventRow` adds
`operation_id`, `attempt`, actor, transition, outcome/reason, and optional
related/successor task id; it remains append-only and contains no rendered
message body.

`TaskId` stays an opaque validated identifier. A Beads id is valid input, but
the storage layer does not import Beads types, query Beads, or copy Beads task
details.

### Legal transitions

| Current | Operation | Next | Required durable effect |
| --- | --- | --- | --- |
| none | assign | Assigned | create logical row and attempt 1 |
| Assigned | start | Active | append started event |
| Assigned | block | Blocked | append reason; invalidate current attempt nudge |
| Assigned | reassign | Assigned | append a new attempt; retain original ordering time |
| Assigned | abort | Closed(Aborted) | persist handoff; invalidate all attempt nudges |
| Active | block | Blocked | append reason; invalidate current attempt nudge |
| Active | succeed/fail/abort | Closed(outcome) | persist handoff; invalidate all attempt nudges |
| Blocked | unblock | Assigned | append resolution; preserve priority/original time; never activate |
| Blocked | reassign | Assigned | append a new attempt; preserve original time |
| Closed | reopen/reassign | Assigned | clear terminal projection, append a new attempt, preserve history/priority/original time |
| any open | supersede | old Closed(Aborted(Superseded)); new Assigned | link a distinct successor `TaskId` and persist its first attempt atomically |

Every other transition rejects without partial state, event, message, or queue
mutation. Ordinary acknowledgement changes mail acknowledgement state only; it
does not produce `Assigned -> Active`.

### Storage-neutral mutation boundary

```rust
pub struct TaskMutationRequest {
    pub operation_id: TaskOperationId,
    pub actor: MemberKey,
    pub task_id: TaskId,
    pub expected_revision: Option<u64>,
    pub operation: TaskOperation,
}

pub enum TaskOperation {
    Assign(PreparedAssignment),
    Start,
    Block { reason: String },
    Unblock { resolution: String },
    Reassign(PreparedAssignment),
    Reopen(PreparedAssignment),
    Close {
        outcome: TaskOutcome,
        handoff: PreparedMessage,
    },
    Supersede {
        handoff: PreparedMessage,
        successor_task_id: TaskId,
        successor: PreparedAssignment,
    },
}

#[async_trait::async_trait]
pub trait AsyncTaskMutationStore: sealed::Sealed + Send + Sync {
    async fn apply(
        &self,
        request: TaskMutationRequest,
    ) -> Result<TaskMutationOutcome, TaskMutationError>;
}
```

`PreparedAssignment` contains the assignee, assigner, priority, durable
assignment message, and optional template SHA. `PreparedMessage` is the
already validated canonical message admission record; storage never renders a
template. The SQLite implementation uses the existing bounded writer lane and
one transaction. No caller receives a connection or writer permit.

## SQLite v2 schema and migration

The migration rebuilds the task ledger transactionally:

1. Create `tasks_v2` keyed by `(team, task_id)`,
   `task_assignment_attempts` keyed by `(team, task_id, attempt)`, and
   `task_events_v2` keyed by `(team, task_id, seq)`.
2. Group legacy rows and events by `(team, task_id)`. Deterministically merge
   the old per-assignee streams by event time, assignee, and old sequence.
   Every accepted legacy `Assigned` event becomes an immutable attempt; a
   legacy row with no surviving assignment event receives one explicit
   `Migrated` attempt using its assignment message id. The attempt associated
   with the row having greatest `updated_at`, then greatest deterministic
   attempt number is current.
3. Set `original_assigned_at` to the minimum legacy `assigned_at`, priority to
   `Normal`, and revision to the number of accepted migrated transitions.
   Map `assigned -> Assigned`, `active -> Active`, and
   `complete -> Closed(Succeeded)`.
4. Re-key the merged legacy events into one deterministic task sequence. Map
   `Acked` to `Started` and
   `Completed` to `Closed(Succeeded)`; retain reminder, rejection, and lead
   audit facts with their mapped attempt. Append one explicit migration event
   when multiple legacy rows collapse into one logical task.
5. Never copy legacy `description`. Before swapping tables, verify every
   retained assignment message id still identifies the canonical message when
   present; a missing historical message remains a typed audit condition, not
   reconstructed body text.
6. Validate foreign keys/check constraints, row/attempt/event counts, terminal
   outcome consistency, and active uniqueness; rename tables only after every
   check succeeds. Any failure rolls back the entire migration and leaves the
   legacy schema readable.

Required database invariants include:

```sql
-- tasks
PRIMARY KEY (team, task_id)
FOREIGN KEY (team, task_id, current_attempt)
  REFERENCES task_assignment_attempts(team, task_id, attempt)
-- task_assignment_attempts
PRIMARY KEY (team, task_id, attempt)
-- task_events
PRIMARY KEY (team, task_id, seq)
CREATE UNIQUE INDEX one_result_per_operation
  ON task_events(team, operation_id) WHERE operation_id IS NOT NULL;
CREATE UNIQUE INDEX one_active_task_per_agent
  ON tasks(team, current_assignee) WHERE state = 'active';
```

The Rust sum type makes an open state with a terminal outcome
unrepresentable. SQLite uses separate `state`, `outcome`, `abort_reason`,
and `superseded_by` columns for indexed queries; check constraints require an
outcome only for `closed`, an abort reason only for `aborted`, and
`superseded_by` only for `closed/aborted/superseded`. A trigger or
transaction validation forbids self-supersession and a missing successor row.

## Transaction and concurrency rules

- A client-generated `TaskOperationId` is mandatory. Replaying the same id and
  byte-equivalent request returns the original outcome without a second event,
  attempt, handoff, or message. Reusing it with different input is a typed
  conflict.
- Mutations use optimistic `expected_revision` plus the SQLite writer
  transaction. Stale revisions and races return typed conflicts; callers may
  reload and retry with a new operation id.
- The partial unique active index is the final authority for concurrent starts.
  Exactly one competing start commits; the loser observes the active-task
  conflict and no partial event.
- Assign/reassign/reopen commits the new assignment message, immutable attempt,
  current-task projection, and event together.
- Close commits its handoff message, terminal projection, audit event, assignment
  message acknowledgement/supersession normalization, and clearing of every
  `mail_message_states.nudge_pending_at` tied to any attempt together.
- Block and reassign clear the superseded current attempt's pending nudge in the
  transition transaction. Unblock never recreates a pending marker; AZ.4 derives
  future eligibility from task state.
- Supersede commits old closure, linkage, old-attempt cleanup, successor row,
  successor assignment message/attempt, and both audit histories together.

## Deliverables

This is the sole authoritative deliverables list for AZ.2. Every item must land
at a production-ready level; type-only, schema-only, or test-only completion is
insufficient.

- [ ] D1 — Amend `docs/requirements.md`, `docs/architecture.md`,
  create the canonical `docs/task-lifecycle-schema.md`, amend
  `docs/atm-storage/boundaries.md`, and amend ADR-062 plus
  `docs/adr/INDEX.md` for the corrected lifecycle, stable identity, immutable
  attempts/events, transition table, priority, idempotency, migration, and
  transaction ownership.
- [ ] D2 — Replace the pure task model in
  `crates/atm-storage/src/task_state.rs` with the types and legal transitions
  above. Add typed rejections for illegal transition, stale revision,
  operation-id conflict, active-task conflict, and invalid terminal metadata.
- [ ] D3 — Extend storage-neutral contracts in
  `crates/atm-storage/src/task_store.rs`, `contract.rs`, and `factory.rs`
  with logical-task/attempt/event reads and the bounded
  `AsyncTaskMutationStore`. Update the synchronous compatibility store only as
  a delegating test/legacy bridge; it must not become a second mutation policy.
- [ ] D4 — Implement the transactional v2 migration and indexes in
  `atm-storage-rusqlite`. Remove the v1 `description` column after verified
  swap, preserve deterministic history, and make fresh and upgraded databases
  converge to byte-equivalent schema.
- [ ] D5 — Implement all mutation transactions in the existing SQLite writer
  lane, including idempotent results, compare-and-swap revision, active
  uniqueness, prepared message persistence, assignment acknowledgement
  normalization, supersession linkage, and task-aware pending-nudge cleanup.
- [ ] D6 — Update machine-readable task/read/mutation boundary records and add
  pure-state, migration, replay, malformed-legacy, retry, concurrent-start,
  concurrent-reassign, atomic-rollback, no-body, and queue-cleanup tests using
  temporary SQLite databases and real writer/read adapters.

## Affected paths

```text
crates/atm-storage/src/task_state.rs
crates/atm-storage/src/task_store.rs
crates/atm-storage/src/contract.rs
crates/atm-storage/src/factory.rs
crates/atm-storage/src/lib.rs
crates/atm-storage/src/testing.rs
crates/atm-storage-rusqlite/src/task_store.rs
crates/atm-storage-rusqlite/src/task_sql.rs
crates/atm-storage-rusqlite/src/task_ledger_reader.rs
crates/atm-storage-rusqlite/src/writer/mod.rs
crates/atm-storage-rusqlite/src/writer/ops.rs
crates/atm-storage-rusqlite/src/writer/task_ops.rs
crates/atm-storage-rusqlite/src/pending_nudge_store.rs
crates/atm-storage-rusqlite/src/lib.rs
boundaries/atm-storage/task-store.toml
boundaries/atm-storage/async-task-ledger-reader.toml
boundaries/atm-storage/async-task-mutation-store.toml
boundaries/atm-storage-rusqlite/task-store-sqlite.toml
boundaries/atm-storage-rusqlite/async-task-ledger-reader-sqlite.toml
boundaries/atm-storage-rusqlite/async-task-mutation-store-sqlite.toml
docs/requirements.md
docs/architecture.md
docs/task-lifecycle-schema.md
docs/atm-storage/boundaries.md
docs/adr/ADR-062-task-state-machine.md
docs/adr/INDEX.md
docs/plans/phase-az/phase-az-plan.md
docs/plans/phase-az/issues.md
docs/project-plan.md
```

### Paths to delete

- The v1 `tasks` and `task_events` table definitions after successful
  transactional replacement; no source file is deleted.

### Paths that must not change

- `crates/atm-daemon/**` and every legacy synchronous daemon runtime/dispatch
  path.
- AZ.1 event/title/template semantics and its focused regression contract.
- clap commands, user-facing task help, HTTP routes, and idle scheduler behavior;
  AZ.3 and AZ.4 own those changes.
- Beads storage, CLI, and schemas.

## Acceptance criteria

This is the sole authoritative acceptance list for AZ.2.

1. The pure state table accepts every listed transition and rejects every
   unlisted transition; unblock is exactly `Blocked -> Assigned` and retains
   priority/original assignment time without activating.
2. One `TaskId` has one current row and immutable numbered attempts; reassign
   and reopen retain identity/history, while supersede atomically links a
   distinct successor id.
3. Existing rows migrate to `Normal`; complete rows become
   `Closed(Succeeded)`; deterministic replay reproduces current projection,
   attempts, counters, and terminal linkage without retaining `description`.
4. Database constraints and race tests prove one current assignee per task and
   at most one active task per agent. Losing concurrent operations leave no
   partial messages, attempts, or events.
5. Same-operation retries are no-ops returning the original outcome; conflicting
   operation-id reuse and stale revisions fail closed.
6. Close/supersede/handoff, assignment changes, acknowledgement normalization,
   and task-related pending-nudge invalidation have atomic rollback tests.
   Closed, blocked, or superseded attempts leave no claimable queue marker.
7. Source and migrated schema tests prove no task or attempt row contains a
   rendered body, duplicated description, template bytes, or Beads details.
8. Every changed storage contract has matching Rust docs, boundary TOML, crate
   boundary prose, and concrete-adapter tests.

## Required validation

This is the sole authoritative validation list for AZ.2. Run from its worktree
with temporary stores only.

1. `cargo test -p atm-storage`
2. `cargo test -p atm-storage-rusqlite task`
3. `cargo test -p atm-storage-rusqlite migration`
4. `cargo test -p atm-storage-rusqlite pending_nudge`
5. `cargo test -p atm-storage-rusqlite concurrent`
6. `cargo test -p atm-storage-rusqlite`
7. `cargo fmt --check`
8. `cargo clippy --workspace --all-targets -- -D warnings`
9. `python3 .just/run_lint.py boundaries`
10. `python3 .just/run_lint.py nudge-taxonomy`
11. `git diff --check`

## Non-closure

- AZ.2 does not expose `atm task`, alter CLI help, or retire legacy flags.
- AZ.2 does not change task-reminder selection, idle cadence, fairness, or Herdr
  prompt emission.
- AZ.2 does not run a live/test daemon or perform any tag, release, package
  publish, or installation operation.
