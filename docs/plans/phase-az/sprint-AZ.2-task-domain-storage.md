---
phase: AZ
sprint: AZ.2
title: Durable task domain and storage migration
branch: feature/az2-task-domain-storage
integration_branch: feature/az1-task-nudge-contract
final_integration_branch: develop
status: complete
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
one active task per `(team, agent)` in the database, and make stale task nudge
cleanup part of the same transaction as every transition that makes an
assignment attempt ineligible.

This sprint is a production-ready storage/domain foundation. It intentionally
does not expose the new public CLI until AZ.3, and it does not change the idle
scheduler until AZ.4. It also preserves the current acknowledgement-to-active
adapter until AZ.3 can land `atm task start` and remove that implicit activation
in the same change set; develop therefore has no activation dead window.

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
pub struct TaskOperationId(Ulid);

pub struct TaskRow {
    pub team: TeamName,
    pub task_id: TaskId,
    pub current_assignee: AgentName,
    pub state: TaskState,
    pub priority: TaskPriority,
    pub original_assigned_at: IsoTimestamp,
    pub current_attempt: AssignmentAttempt,
    /// Successful reminder ordinal across every assignment attempt.
    pub reminder_ordinal: u64,
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
    MigratedActiveConflictDemotion,
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
details. `TaskOperationId` is its own opaque UUID/ULID-domain newtype and
idempotency-key namespace. It is never an alias for, constructed from, or
interpreted as an `AtmMessageId`; a mutation may create message ids, but its
operation identity remains independent of every resulting message.

### Legal transitions

| Current | Operation | Next | Required durable effect |
| --- | --- | --- | --- |
| none | assign | Assigned | create logical row and attempt 1 |
| Assigned | start | Active | append started event |
| Assigned | legacy complete | Closed(Succeeded) | AZ.3 compatibility provenance only; persist completion notice |
| Assigned | fail | Closed(Failed) | persist terminal handoff; invalidate all attempt nudges |
| Assigned | block | Blocked | append reason; invalidate current attempt nudge |
| Assigned | reassign | Assigned | append a new attempt; retain original ordering time |
| Assigned | abort | Closed(Aborted) | persist handoff; invalidate all attempt nudges |
| Active | block | Blocked | append reason; invalidate current attempt nudge |
| Active | succeed/fail/abort | Closed(outcome) | persist handoff; invalidate all attempt nudges |
| Blocked | unblock | Assigned | append resolution; preserve priority/original time; never activate |
| Blocked | reassign | Assigned | append a new attempt; preserve original time |
| Blocked | abort | Closed(Aborted) | persist handoff; invalidate all attempt nudges |
| Closed | reopen/reassign | Assigned | clear terminal projection, append a new attempt, preserve history/priority/original time |
| any open | supersede | old Closed(Aborted(Superseded)); new Assigned | link a distinct successor `TaskId` and persist its first attempt atomically |

Every other transition rejects without partial state, event, message, or queue
mutation. AZ.2's new mutation boundary names explicit `start`, but the existing
task-linked acknowledgement adapter continues to invoke that transition until
AZ.3 ships the public start command and changes acknowledgement to mail-only.
No intermediate develop commit removes the only activation path.

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
    LegacyCloseSucceeded {
        completion_notice: PreparedMessage,
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

## SQLite v2 schema, compatibility window, and migration

`STORAGE_SCHEMA_VERSION` is introduced as `2.0.0` and persisted in the
database by ATM `1.6.0`. This is an approved ADR-061 **major** change, recorded
in ADR-061 D6 and ADR-063 D6. Every `1.6.x` release retains the v1/v2 bridge.
ATM `1.7.0` is the planned removal target and earliest permitted removal
release, under a separate ADR-061 major approval. During `1.6.x`, the retained
v1 projection and bridge continue to admit the supported
assign/acknowledge/complete writes and reconcile them to authoritative v2
semantics. The crate-level proof is a direct-v1-projection fixture; a real
retained-1.5.14 executable against a migrated ledger is integration evidence
owned by `AZ-TASK-CROSS-BINARY-COMPAT` in the Colima testbed.

The migration builds the canonical v2 task ledger transactionally while
retaining the v1 `tasks`/`task_events` compatibility projection and its
`description` column during that window:

1. Create `tasks_v2` keyed by `(team, task_id)`,
   `task_assignment_attempts` keyed by `(team, task_id, attempt)`, and
   `task_events_v2` keyed by `(team, task_id, seq)`.
2. Group legacy rows and events by `(team, task_id)`. Deterministically merge
   the old per-assignee streams by event time, assignee, and old sequence.
   Every accepted legacy `Assigned` event becomes an immutable attempt; a
   legacy row with no surviving assignment event receives one explicit
   `Migrated` attempt using its assignment message id. The attempt associated
   with the winning state below and greatest `updated_at`, then greatest
   deterministic attempt number is current.
3. Collapse conflicting legacy states by the total precedence
   `Active > Assigned > Complete`: any open row keeps the logical task open,
   and an active row wins over an assigned row. Every non-winning source row,
   including its assignee, state, timestamp, and deterministic source key, is
   recorded in the append-only `Migrated` event; no live row silently becomes
   closed and no fully completed logical task reopens.
4. Remediate pre-existing `(team, assignee)` active conflicts deterministically.
   The active task with earliest `original_assigned_at`, then lexical `TaskId`,
   remains `Active`; every other conflicting active task becomes `Assigned`
   without changing assignee, priority, or original time, and receives a
   `MigratedActiveConflictDemotion` audit event. This preserves all work and
   makes the uniqueness index installable instead of trapping the host on v1.
5. Set `original_assigned_at` to the minimum legacy `assigned_at`, priority to
   `Normal`, `reminder_ordinal` to the total accepted legacy reminder events
   across all source rows, and revision to the number of accepted migrated
   transitions.
   Map `assigned -> Assigned`, `active -> Active`, and
   `complete -> Closed(Succeeded)`.
6. Re-key the merged legacy events into one deterministic task sequence. Map
   `Acked` to `Started` and
   `Completed` to `Closed(Succeeded)`; retain reminder, rejection, and lead
   audit facts with their mapped attempt. Append one explicit migration event
   when multiple legacy rows collapse into one logical task.
7. Never copy legacy `description` into canonical v2 rows. Before enabling v2,
   verify every
   retained assignment message id still identifies the canonical message when
   present; a missing historical message remains a typed audit condition, not
   reconstructed body text.
8. Install a compatibility bridge: new writers update v2 and the retained v1
   projection in one transaction; triggers on supported v1 writes translate
   legacy assign/ack/complete mutations into v2 attempts/events. Those triggers
   reuse steps 2–4's deterministic reconciliation rather than rejecting a
   write the legacy binary can make: multiple v1 assignee rows become immutable
   attempts, current state/assignee use the same `Active > Assigned > Complete`
   precedence and deterministic winning-attempt rule, and any resulting
   one-active-per-agent collision keeps the earliest `original_assigned_at`,
   then lexical `TaskId`, active. Every surplus active task is demoted to
   `Assigned` with `MigratedActiveConflictDemotion`, exactly as in the
   one-time migration. The bridge must not introduce last-write-wins or a
   second rejection policy. The crate-level v1-projection compatibility fixture
   must create
   multiple assignee rows and an active collision, then assign, acknowledge,
   and complete against the migrated database; the new binary reopens it and
   observes the same reconciled state and audit events. No v1 table/column is
   dropped in Phase AZ; retirement requires a separate ADR-061 major approval
   no earlier than ATM `1.7.0`, after the `1.6.x` co-existence window.
9. Validate foreign keys/check constraints, row/attempt/event counts, terminal
   outcome consistency, active uniqueness, and v1/v2 projection agreement
   before committing. Any failure rolls back the entire migration and leaves
   the legacy schema readable.

Required database invariants include:

```sql
-- tasks_v2
PRIMARY KEY (team, task_id)
FOREIGN KEY (team, task_id, current_attempt)
  REFERENCES task_assignment_attempts(team, task_id, attempt)
-- task_assignment_attempts
PRIMARY KEY (team, task_id, attempt)
-- task_events
PRIMARY KEY (team, task_id, seq)
-- task_operations owns idempotency; multiple task events may reference one op
PRIMARY KEY (team, operation_id)
CREATE UNIQUE INDEX one_active_task_per_agent
  ON tasks_v2(team, current_assignee) WHERE state = 'active';
CREATE INDEX task_list_order
  ON tasks_v2(team, current_assignee, state, priority,
              original_assigned_at, task_id);
CREATE INDEX IF NOT EXISTS idx_mail_messages_task_id
  ON mail_messages(team, json_extract(envelope_json, '$.taskId'));
```

`idx_mail_messages_task_id` is owned by
`crates/atm-storage-rusqlite/src/mail_messages_schema.rs` and must be added to
`mail_messages_index_ddl!()`, the single canonical index batch shared by
fresh schema creation and legacy table rebuild. Its leading columns exactly
match the invalidation predicate `(team, taskId)`; inserting `agent` between
them would prevent SQLite from seeking the task id. The existing
migrated-versus-fresh schema-object test must prove the new index is
byte-identical on both paths.

The active-task uniqueness key is exactly `(team, current_assignee)`: the task
ledger permits at most one `Active` task for that key.

The Rust sum type makes an open state with a terminal outcome
unrepresentable. SQLite uses separate `state`, `outcome`, `abort_reason`,
and `superseded_by` columns for indexed queries; check constraints require an
outcome only for `closed`, an abort reason only for `aborted`, and
`superseded_by` only for `closed/aborted/superseded`. A trigger or
transaction validation forbids self-supersession and a missing successor row.
`task_operations` stores the request fingerprint and serialized typed result
once per `(team, TaskOperationId)`; event rows reference it without a uniqueness
constraint. A supersede operation may therefore append the old closure and new
assignment histories under one idempotency key.

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
- Assign/reassign/reopen validates that the prepared assignment recipient is a
  same-host roster member before mutation, then commits the new assignment
  message, immutable attempt, current-task projection, and event together. A
  cross-host recipient rejects with
  `ATM_TASK_HANDOFF_CROSS_HOST_UNSUPPORTED` before an operation row, message,
  task, attempt, or event is written.
- Close commits its handoff message, terminal projection, audit event,
  assignment-message acknowledgement/supersession normalization, and clearing
  of every pending marker whose canonical message has the same `(team,
  task_id)`. Without adding a compatibility-breaking mailbox column, the query
  explicitly joins
  `json_extract(mail_messages.envelope_json, '$.taskId') -> tasks_v2.task_id`;
  it is not limited to assignment-message ids recorded on attempts. Progress
  mail, requeued duplicates, legacy task-linked rows, and every historical
  assignment message are therefore covered. Attempt ids remain an additional
  integrity check, not the invalidation key.
- Block and reassign use the same task-id join to clear pending markers in the
  transition transaction. Unblock never recreates a pending marker; AZ.4
  derives future eligibility from task state.
- Supersede applies the same same-host validation to the successor assignee,
  then commits old closure, linkage, old-attempt cleanup, successor row,
  successor assignment message/attempt, and both audit histories together.

## Read ordering contract

Storage owns bounded ordering so AZ.3 and AZ.4 cannot independently recreate
it. Open task lists sort `Active` first, then `Assigned` by `High`, `Normal`,
`Low`, `original_assigned_at`, and `TaskId`, then `Blocked` by original time and
id. Closed history sorts by terminal event time descending and `TaskId`.
`top_runnable_task(team, assignee)` uses the same covering index, returns the
single `Active` task when present, otherwise the first `Assigned` task, and
never returns `Blocked` or `Closed`.

## Deliverables

This is the sole authoritative deliverables list for AZ.2. Every item must land
at a production-ready level; type-only, schema-only, or test-only completion is
insufficient.

- [ ] D1 — Amend `docs/requirements.md`, `docs/architecture.md`,
  create the canonical `docs/task-lifecycle-schema.md`, amend
  `docs/atm-storage/boundaries.md`, amend ADR-061/ADR-062, maintain the accepted
  `ADR-063-phase-az-task-and-attention-capabilities.md`, and index it in
  `docs/adr/INDEX.md`. Record the corrected lifecycle, stable identity,
  capability-trait recount, schema-v2 major classification, coexistence,
  immutable attempts/events, ordering, priority, idempotency, migration, and
  transaction ownership. Remove the Phase-AM-deleted `OutboundMessageQuery`
  from ADR-036's capability inventory and update its matching boundary TOMLs.
  Preserve Rand's explicit ADR-061 major-change approval and version-bounded
  coexistence record in ADR-061 D6, ADR-063 D6, and the phase plan.
- [ ] D2 — Replace the pure task model in
  `crates/atm-storage/src/task_state.rs` with the types and legal transitions
  above. Add typed rejections for illegal transition, stale revision,
  operation-id conflict, active-task conflict, and invalid terminal metadata.
  The retained v1 bridge deterministically reconciles every supported legacy
  write; it has no separate compatibility-rejection path in this sprint.
  Reuse
  `ATM_TASK_HANDOFF_CROSS_HOST_UNSUPPORTED` for every assignment-bearing
  mutation whose recipient is not same-host. Register public codes and recovery
  text in the unified ADR-032 error catalog and machine-readable boundary.
- [ ] D3 — Extend storage-neutral contracts in
  `crates/atm-storage/src/task_store.rs`, `contract.rs`, and `factory.rs`
  with logical-task/attempt/event reads, the binding list/top-runnable ordering,
  and the bounded `AsyncTaskMutationStore`. Update the synchronous compatibility store only as
  a delegating test/legacy bridge; it must not become a second mutation policy.
- [ ] D4 — Implement the transactional v2 migration and indexes in
  `atm-storage-rusqlite`, introduce/persist `STORAGE_SCHEMA_VERSION = 2.0.0`,
  and retain the v1 tables/description compatibility projection for the
  approved coexistence window. Implement deterministic state precedence and
  active-conflict demotion once and reuse them for both initial migration and
  every supported v1 bridge write; add v1-projection multi-assignee and
  active-conflict rollback tests. Fresh and upgraded databases must converge to byte-equivalent v2 plus
  compatibility schema. Add `idx_mail_messages_task_id` through
  `mail_messages_index_ddl!()` and extend its migrated-versus-fresh
  index-identity test; Phase AZ deletes no v1 table or column.
- [ ] D5 — Implement all mutation transactions in the existing SQLite writer
  lane, including idempotent results, compare-and-swap revision, active
  uniqueness, same-host prepared-assignment validation, message persistence,
  assignment acknowledgement normalization, supersession linkage, a dedicated
  idempotent operations table, and task-id-joined pending-nudge cleanup across
  all task-linked messages.
- [ ] D6 — Update machine-readable task/read/mutation boundary records and add
  pure-state, migration, replay, malformed-legacy, retry, concurrent-start,
  concurrent-reassign, bridge-time multi-assignee/active-conflict reconciliation,
  atomic-rollback, no-body, and queue-cleanup tests using temporary SQLite
  databases and real writer/read adapters. Extend the frozen
  nudge-identifier inventory only for identifiers actually introduced by this
  sprint, beside ADR-063; never bulk-regenerate it.

## Affected paths

```text
crates/atm-storage/src/task_state.rs
crates/atm-storage/src/task_store.rs
crates/atm-storage/src/task_mutation.rs
crates/atm-storage/src/contract.rs
crates/atm-storage/src/factory.rs
crates/atm-storage/src/lib.rs
crates/atm-storage/src/testing.rs
crates/atm-storage-rusqlite/src/task_store.rs
crates/atm-storage-rusqlite/src/task_sql.rs
crates/atm-storage-rusqlite/src/task_operations.rs
crates/atm-storage-rusqlite/src/task_ledger_reader.rs
crates/atm-storage-rusqlite/src/schema_version.rs
crates/atm-storage-rusqlite/src/shared_db.rs
crates/atm-storage-rusqlite/src/mail_messages_schema.rs
crates/atm-storage-rusqlite/src/writer/mod.rs
crates/atm-storage-rusqlite/src/writer/ops.rs
crates/atm-storage-rusqlite/src/writer/task_ops.rs
crates/atm-storage-rusqlite/src/pending_nudge_store.rs
crates/atm-storage-rusqlite/src/lib.rs
crates/atm-storage-rusqlite/tests/schema_version_compat.rs
boundaries/atm-storage/task-store.toml
boundaries/atm-storage/async-task-ledger-reader.toml
boundaries/atm-storage/async-task-mutation-store.toml
boundaries/atm-storage-rusqlite/task-store-sqlite.toml
boundaries/atm-storage-rusqlite/async-task-ledger-reader-sqlite.toml
boundaries/atm-storage-rusqlite/async-task-mutation-store-sqlite.toml
boundaries/atm-error/error-codes.toml
scripts/check-nudge-taxonomy.py
crates/atm-storage/src/error_codes.rs
crates/atm-storage/src/error_catalog.rs
docs/requirements.md
docs/architecture.md
docs/task-lifecycle-schema.md
docs/atm-storage/boundaries.md
docs/atm-rusqlite/requirements.md
docs/atm-rusqlite/architecture.md
docs/atm-error-codes.md
docs/adr/ADR-036-storage-boundary-and-composition-topology.md
docs/adr/ADR-061-governed-interface-schema-versioning.md
docs/adr/ADR-062-task-state-machine.md
docs/adr/ADR-063-phase-az-task-and-attention-capabilities.md
docs/adr/INDEX.md
docs/plans/phase-az/phase-az-plan.md
docs/plans/phase-az/issues.md
docs/project-plan.md
```

### Paths to delete

None. The v1 `tasks`, `task_events`, and `description` compatibility surface
must remain for the ADR-061 coexistence window. Its eventual removal is a
separately approved major migration.

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
   priority/original assignment time without activating. The legacy completion
   provenance is the only `Assigned -> Closed(Succeeded)` compatibility route;
   explicit `fail` is the separate intentional `Assigned -> Closed(Failed)`
   terminal route.
2. One `TaskId` has one current row and immutable numbered attempts; reassign
   and reopen retain identity/history, while supersede atomically links a
   distinct successor id.
3. Existing rows migrate to `Normal`; state collapse uses
   `Active > Assigned > Complete`, records every non-winning row, and
   deterministically demotes pre-existing active conflicts to `Assigned`.
   The v1 bridge applies those same rules and
   `MigratedActiveConflictDemotion` events to post-migration multi-assignee and
   active-conflict writes. Replay reproduces current projection, attempts,
   counters, and terminal linkage without copying `description` into v2.
4. Database constraints and race tests prove one current assignee per task and
   at most one active task per `(team, agent)`. Losing concurrent operations
   leave no partial messages, attempts, or events.
   Assign/reassign/reopen/supersede reject cross-host recipients with
   `ATM_TASK_HANDOFF_CROSS_HOST_UNSUPPORTED` before any durable mutation.
5. Same-operation retries are no-ops returning the original outcome; conflicting
   operation-id reuse and stale revisions fail closed. Supersession writes both
   task histories under one operation record without violating uniqueness.
6. Close/supersede/handoff, assignment changes, acknowledgement normalization,
   and task-related pending-nudge invalidation have atomic rollback tests. The
   task-id join clears assignment, progress, requeued, and legacy task-linked
   markers; closed, blocked, or superseded tasks leave no claimable marker.
7. Canonical v2 task/attempt rows contain no rendered body, duplicate
   description, template bytes, or Beads details. The temporary v1
   compatibility projection is the sole approved legacy-description exception.
8. `STORAGE_SCHEMA_VERSION` is 2.0.0; fresh/upgraded schemas converge; and the
   crate-level v1-projection fixture proves supported assign/ack/complete
   writes reconcile into the migrated ledger and are observed after reopening.
   `AZ-TASK-CROSS-BINARY-COMPAT` separately owns retained-1.5.14 executable
   evidence in the Colima integration testbed. No v1 object is dropped.
9. Storage list/top-runnable tests prove the binding order and covering-index
   query plan without materializing unbounded task or event history.
10. Every changed storage contract has matching Rust docs, boundary TOML, crate
   boundary prose, public error code, and concrete-adapter tests. ADR-036 and
   matching boundary TOMLs no longer list the deleted `OutboundMessageQuery`.
   ADR-063 is accepted and indexed, and its D6 approval record matches ADR-061
   D6 and the phase plan.

## Required validation

This is the sole authoritative validation list for AZ.2. Run from its worktree
with temporary stores only.

1. `cargo test -p atm-storage`
2. `cargo test -p atm-storage-rusqlite task`
3. `cargo test -p atm-storage-rusqlite migration`
4. `cargo test -p atm-storage-rusqlite pending_nudge`
5. `cargo test -p atm-storage-rusqlite concurrent`
6. `cargo test -p atm-storage-rusqlite`
7. `cargo test -p atm-storage-rusqlite --test schema_version_compat`
8. `cargo fmt --check`
9. `cargo clippy --workspace --all-targets -- -D warnings`
10. `python3 .just/run_lint.py boundaries`
11. `python3 .just/run_lint.py nudge-taxonomy`
12. `python3 .just/check_line_counts.py`
13. `git diff --check`

## Non-closure

- AZ.2 does not expose `atm task`, alter CLI help, or retire legacy flags.
- AZ.2 does not change acknowledgement activation; the existing ack-to-active
  adapter remains until AZ.3 lands it atomically with `atm task start`.
- AZ.2 does not change task-reminder selection, idle cadence, fairness, or Herdr
  prompt emission.
- AZ.2 does not run a live/test daemon or perform any tag, release, package
  publish, or installation operation.
- AZ.2 does not remove the v1 compatibility schema after migration. Every ATM
  `1.6.x` release retains it. ATM `1.7.0` is the planned removal target and
  earliest permitted removal release, and removal remains a separate ADR-061
  major-change decision.
