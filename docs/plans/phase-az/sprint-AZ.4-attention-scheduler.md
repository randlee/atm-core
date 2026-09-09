---
phase: AZ
sprint: AZ.4
title: Fair idle attention scheduler
branch: feature/az4-attention-scheduler
integration_branch: feature/az3-task-command-handoff
final_integration_branch: develop
status: planned
recommended_agent: arch-ctm
recommended_model: deep-reasoning
execution_track: stacked
dependency_relations:
  - prerequisite: AZ.3
    dependent: AZ.4
    relation: must_follow
    rationale: The scheduler consumes AZ.3's public lifecycle behavior and AZ.2's attempt-aware query/invalidation contracts; merge AZ.3 forward before every AZ.4 round and merge its PR first.
---

# AZ.4 — Fair idle attention scheduler

## Goal

Replace the drain-first-plus-reminder sequence with one derived idle attention
selector. Each eligible idle opportunity emits at most one item for an agent,
preserves separate message and task lifecycle storage, alternates fairly when
both lanes remain due, and repeats task reminders only while the selected
assignment attempt remains open and unblocked.

This sprint closes the runtime behavior at production quality. It does not
change the AZ.1 metadata boundary or the AZ.2/AZ.3 task transition and command
contracts.

## Derived attention model

```rust
pub enum AttentionLane {
    Ephemeral,
    PersistentTask,
}

pub enum AttentionItem {
    EphemeralMessage {
        member: MemberKey,
        message_id: AtmMessageId,
    },
    PersistentTaskReminder {
        member: MemberKey,
        task_id: TaskId,
        attempt: AssignmentAttempt,
        assignment_message_id: AtmMessageId,
    },
}

pub struct AttentionCandidates {
    pub ephemeral: Option<EphemeralMessageCandidate>,
    pub task: Option<PersistentTaskCandidate>,
}

pub fn select_attention_item(
    next_lane: AttentionLane,
    candidates: AttentionCandidates,
) -> AttentionSelection;

pub struct AttentionSelection {
    pub item: Option<AttentionItem>,
    pub next_lane: AttentionLane,
}
```

Selection is pure and contains ids/metadata only. It never accepts message body,
task description, rendered template content, SQLite handles, or an emitter.
Projection later reloads the AZ.1 persisted title by
`assignment_message_id`; the recipient retrieves the body with `atm read`.

## Eligibility, ordering, and fairness

For one idle agent:

1. The ephemeral candidate is the first eligible pending queue message in
   `message_id`/ULID FIFO order. If it became read and acknowledged before the
   claim, it is suppressed and the selector retries without emitting.
2. The persistent candidate is the one `Active` task when present; otherwise
   the first `Assigned` task by `High, Normal, Low`, then
   `original_assigned_at`, then `TaskId`. `Blocked` and `Closed` are
   ineligible.
3. If only one lane has a candidate, select it. If both do, select the lane in
   the durable per-member alternation cursor. A new cursor starts
   `Ephemeral`; after a committed selection it points to the other lane.
4. One `IdleOpportunityId` may commit zero or one selection. It never emits
   both a message and task reminder. A repeated opportunity id returns the same
   selection/final state and cannot duplicate an emission.
5. Task priority applies only within the persistent lane. It never overrides the
   cross-lane cursor.

The cursor is scheduler metadata in a dedicated
`attention_lane_cursors(team, agent, next_lane, revision)` table. It contains
no message/task lifecycle state or body. Message consumption remains owned by
`PendingNudgeStore`; task state/attempts remain owned by the task ledger. The
cursor survives restart so repeated restarts cannot indefinitely favor the
ephemeral lane.

Assignment admission never puts task mail on the ordinary message-key pending
queue and never emits an immediate post-send task nudge. This selector is the
only path that may choose a task reminder, after confirming that the assignment
attempt is the assignee's top runnable task and that the assignee is idle.

## Runtime flow

```text
idle/done heartbeat
  -> query first ephemeral candidate and top persistent candidate independently
  -> pure select_attention_item(cursor, candidates)
  -> reserve that one item under one IdleOpportunityId
  -> atomically claim only the selected lane
  -> revalidate idle state and candidate eligibility
  -> project one AZ.1 bounded nudge and emit
  -> finalize the owning lane and reservation
```

Reservation and cursor advancement commit together before lane claim. Replaying
an opportunity returns the same reserved item and can never select the other
lane. If the candidate loses a race or becomes ineligible, that opportunity
finalizes as stale without emission; a later idle opportunity starts from the
other lane. A transient emitter retry remains attached to the same reservation
and item. This is an at-most-one-*item* contract, not an impossible claim that a
process crash can make an external prompt sink exactly-once. The global prompt
budget still bounds a pump tick, but it no longer permits two different items
for one member/opportunity.

An ephemeral item is consumed after its one accepted nudge. A transient failure
before accepted emission follows the shared
`MAX_NUDGE_ATTEMPTS = 5` contract; the same counter and ceiling apply to both
lanes and no per-channel retry budget is introduced. After the fifth failed
delivery for one reservation, it becomes `PermanentlyFailed`; that terminalizes
the reservation, not the underlying message/task. A persistent task reminder
does not consume the task. After successful emission it appends an
attempt-aware eligibility audit and advances a **task-scoped** reminder ordinal.
The ordinal never resets on reassign, reopen, or unblock, so ordinal 10, 20, 30,
and so on retain ADR-062's lead-escalation boundary. The current attempt is
eligible again only after the retained 60-second cadence while it remains
`Assigned` or `Active`.

Task-lifecycle `Blocked` and `Closed` suppress normal reminders immediately.
Reassignment makes the old attempt ineligible and the new attempt eligible;
unblock returns the task to `Assigned` ordering and does not auto-start.
Runtime-member `blocked` remains a distinct process-health/escalation signal,
not a task state or normal task reminder, and must not reintroduce a Task prompt
for a lifecycle-blocked row. User-facing docs and prompt text use
"lifecycle-blocked task" and "runtime-blocked member" rather than the bare
ambiguous word where both concepts can appear.

## Storage-neutral scheduler boundaries

```rust
pub struct AttentionCursor {
    pub next_lane: AttentionLane,
    pub revision: u64,
}

pub struct AttentionReservation {
    pub opportunity_id: IdleOpportunityId,
    pub item: AttentionItem,
    pub status: AttentionReservationStatus,
}

pub enum AttentionReservationStatus {
    Reserved,
    Delivered,
    Stale,
    PermanentlyFailed,
}

pub trait AttentionScheduleStore: sealed::Sealed + Send + Sync {
    fn load_cursor(&self, member: &MemberKey) -> Result<AttentionCursor, AtmError>;
    fn reserve(
        &self,
        member: &MemberKey,
        opportunity_id: &IdleOpportunityId,
        expected_cursor_revision: u64,
        item: AttentionItem,
    ) -> Result<AttentionReservation, AtmError>;
    fn finalize(
        &self,
        member: &MemberKey,
        opportunity_id: &IdleOpportunityId,
        status: AttentionReservationStatus,
    ) -> Result<AttentionReservation, AtmError>;
}

#[async_trait::async_trait]
pub trait AsyncAttentionScheduleStore: sealed::Sealed + Send + Sync {
    async fn load_cursor(
        &self,
        member: MemberKey,
        deadline: ReadDeadline,
    ) -> Result<AttentionCursor, ReadLaneError>;

    async fn reserve(
        &self,
        request: AttentionReservationRequest,
    ) -> Result<AttentionReservation, AtmError>;

    async fn finalize(
        &self,
        request: AttentionFinalizeRequest,
    ) -> Result<AttentionReservation, AtmError>;
}
```

The concrete SQLite adapter owns `attention_lane_cursors` and
`attention_opportunities` SQL. Opportunity rows store only member,
opportunity id, selected lane/item ids, and terminal reservation status—never
message/task lifecycle state or body. The runtime composes
`AsyncTaskLedgerReader`, `PendingNudgeStore`, and
`AsyncAttentionScheduleStore`; it does not merge their tables or reopen the
database. The task reader adds a bounded `top_runnable_task(member, now)` query
that returns ids, priority, state, attempt, message id, and cadence metadata
only.

AZ.4 is an ADR-061 minor/additive SQLite change. It moves
`STORAGE_SCHEMA_VERSION` from `2.0.0` to `2.1.0` and registers an idempotent
`ensure_attention_schedule_schema` entry in `DB_MIGRATIONS`. The migration
creates `attention_lane_cursors` and `attention_opportunities` with defaults
and indexes only; the retained 2.0/1.5.14 consumer ignores them and continues
to read/write its supported surface. Fresh-2.1 and upgraded-2.0 databases must
converge to byte-equivalent schema, and the older-consumer fixture is rerun.
ADR-061's version record, the storage schema document, and migration baseline
are updated in the same change.

## Deliverables

This is the sole authoritative deliverables list for AZ.4. Every item must land
at a production-ready level; a pure selector without real pump integration, or
runtime wiring without durable fairness, is insufficient.

- [ ] D1 — Add the pure `AttentionLane`, `AttentionItem`, candidate,
  selection, and opportunity-id contracts with exhaustive eligibility/order/
  alternation tests and no body-capable fields.
- [ ] D2 — Add the storage-neutral sync/async schedule boundaries, private
  SQLite cursor/reservation tables, idempotent opportunity reservation/
  finalization, and bounded top-runnable task query. Bump
  `STORAGE_SCHEMA_VERSION` to 2.1.0 through the registered idempotent migration,
  update matching boundary/schema/ADR-061 records, prove fresh/upgraded schema
  convergence, and rerun the older-consumer compatibility fixture.
- [ ] D3 — Refactor `HerdrQueueWakePump` so each idle member/opportunity invokes
  the one selector, claims/revalidates exactly the selected lane, and emits no
  more than one prompt. Preserve global prompt budget, shutdown, breaker,
  delivery-channel, and shared `MAX_NUDGE_ATTEMPTS` retry behavior. Split the
  selector/reservation orchestration into
  `crates/atm-http-runtime/src/herdr_attention_scheduler.rs` before the existing
  pump exceeds RULE-003; no lint-cap increase is authorized.
- [ ] D4 — Make reminders attempt-aware and scheduler-derived: active before
  assigned, assigned priority/time ordering, 60-second repeat only while open,
  no blocked/closed reminder, old-attempt invalidation, and unblock without
  activation. Keep the escalation ordinal task-scoped across attempts, emit
  lead audit at every tenth successful reminder, and keep management
  escalation distinct from reminder eligibility.
- [ ] D5 — Amend product/runtime/Herdr/storage requirements, architecture,
  `docs/task-lifecycle-schema.md`, ADR-061/ADR-062/ADR-063,
  machine-readable boundaries, and operator docs for
  `AttentionItem`, separate lanes, durable fairness, one-item opportunities,
  cadence, retry terminalization, task-scoped escalation counting, queue-cleanup
  interaction, lifecycle-blocked versus runtime-blocked vocabulary, and the
  replacement of stale "after draining mail"/"Task body" requirements.
- [ ] D6 — Add real composed-runtime tests covering FIFO, persistent ordering,
  alternating dual-lane opportunities, single-lane progress, restart cursor
  persistence, concurrent opportunity idempotency, read/ack suppression,
  close/block/reassign races, transient emit failures, shutdown, breaker, and
  absence of body sentinels in emitted prompts. Extend the ADR-054 frozen
  nudge-identifier inventory only for identifiers actually introduced; never
  bulk-regenerate the allowlist.

## Affected paths

```text
crates/atm-storage/src/attention.rs
crates/atm-storage/src/task_store.rs
crates/atm-storage/src/contract.rs
crates/atm-storage/src/factory.rs
crates/atm-storage/src/lib.rs
crates/atm-storage/src/testing.rs
crates/atm-storage-rusqlite/src/attention_schedule_store.rs
crates/atm-storage-rusqlite/src/schema_version.rs
crates/atm-storage-rusqlite/src/shared_db.rs
crates/atm-storage-rusqlite/src/task_ledger_reader.rs
crates/atm-storage-rusqlite/src/pending_nudge_store.rs
crates/atm-storage-rusqlite/src/lib.rs
crates/atm-storage-rusqlite/tests/schema_version_compat.rs
crates/atm-http-runtime/src/herdr_queue_wake.rs
crates/atm-http-runtime/src/herdr_attention_scheduler.rs
crates/atm-http-runtime/src/herdr_queue_wake_reminders.rs
crates/atm-http-runtime/src/herdr_queue_wake_escalation.rs
crates/atm-http-runtime/src/storage_and_nudge_router.rs
boundaries/atm-storage/attention-schedule-store.toml
boundaries/atm-storage/async-attention-schedule-store.toml
boundaries/atm-storage/async-task-ledger-reader.toml
boundaries/atm-storage/pending-nudge-store.toml
boundaries/atm-storage-rusqlite/attention-schedule-store-sqlite.toml
boundaries/atm-storage-rusqlite/async-attention-schedule-store-sqlite.toml
boundaries/atm-storage-rusqlite/async-task-ledger-reader-sqlite.toml
boundaries/atm-storage-rusqlite/pending-nudge-store-sqlite.toml
boundaries/atm-http-runtime/http-runtime.toml
scripts/check-nudge-taxonomy.py
docs/requirements.md
docs/architecture.md
docs/task-lifecycle-schema.md
docs/atm-storage/boundaries.md
docs/atm-rusqlite/requirements.md
docs/atm-rusqlite/architecture.md
docs/atm-http-runtime/architecture.md
docs/atm-herdr/requirements.md
docs/atm-herdr/architecture.md
docs/atm-herdr/boundaries.md
docs/adr/ADR-062-task-state-machine.md
docs/adr/ADR-061-governed-interface-schema-versioning.md
docs/adr/ADR-063-phase-az-task-and-attention-capabilities.md
docs/user-documents/tasks.md
docs/plans/phase-az/phase-az-plan.md
docs/plans/phase-az/issues.md
docs/project-plan.md
```

### Paths to delete

None. The existing pump methods may be collapsed or renamed in place, but no
source-file deletion is required.

### Paths that must not change

- `crates/atm-daemon/**` and every legacy synchronous daemon runtime/dispatch
  path.
- AZ.1 title/body contract and admission-time summary generation.
- AZ.2 lifecycle transitions, persistence identities, and migration semantics.
- AZ.3 command grammar, authorization, handoff, and legacy compatibility
  adapters.
- Message/template bodies, Beads data, and task objective content.

## Acceptance criteria

This is the sole authoritative acceptance list for AZ.4.

1. The selector DTO contains only lane ids/metadata. The message queue, task
   ledger, and fairness cursor remain independent stores behind
   storage-neutral boundaries. Reservation rows contain item identities only,
   never copied message/task content.
2. For each idle opportunity, composed-runtime tests observe zero or one
   selected item, never one from each lane. Same-opportunity replay and
   concurrent attempts return the same reservation; sink retry cannot change
   its item identity.
3. With both lanes continuously due, observed sequence is ephemeral, task,
   ephemeral, task across ticks and process restart. With one lane empty, the
   other progresses without artificial delay.
4. Ephemeral selection is FIFO and is consumed after one accepted nudge; a
   message read and acknowledged before conditional claim is suppressed.
5. Persistent selection chooses active first, otherwise assigned by
   priority/original time/task id. Priority never changes cross-lane fairness.
   Blocked and closed tasks are never normal-reminder candidates.
6. Reminder audit/cadence is scoped to assignment attempt. Close, block,
   reassign, reopen, and supersede races cannot emit for an ineligible old
   attempt; unblock returns to assigned order and never starts. The escalation
   ordinal is task-scoped, survives every attempt change, and still notifies at
   reminders 10, 20, 30, and so on.
7. Transient failure, breaker-open, runtime-blocked, shutdown, and budget tests
   preserve existing structured outcomes without losing durable message/task
   eligibility or emitting a second item. Both lanes use the one
   `MAX_NUDGE_ATTEMPTS = 5`; `PermanentlyFailed` occurs on the fifth failed
   reservation delivery and never closes its message/task.
8. Emitted prompts satisfy AZ.1's bounded title contract; unique message/task
   body sentinels never appear. No legacy daemon code or direct SQLite access is
   introduced.
9. Requirements, architecture, ADR, crate docs, user docs, Rust contracts, and
   boundary TOMLs describe the same one-item, fair, persistent scheduler.
10. `STORAGE_SCHEMA_VERSION` is 2.1.0; fresh and 2.0-upgraded schemas are
    byte-equivalent, the prior consumer ignores the additive tables, and the
    ADR-061 version record agrees.

## Required validation

This is the sole authoritative validation list for AZ.4. Use fakes, in-process
runtime composition, and temporary databases only.

1. `cargo test -p atm-storage attention`
2. `cargo test -p atm-storage-rusqlite attention`
3. `cargo test -p atm-storage-rusqlite pending_nudge`
4. `cargo test -p atm-http-runtime herdr_queue_wake`
5. `cargo test -p atm-http-runtime attention`
6. `cargo test -p atm-http-runtime storage_and_nudge_router`
7. `cargo test -p atm-storage`
8. `cargo test -p atm-storage-rusqlite`
9. `cargo test -p atm-http-runtime`
10. `cargo test -p atm-storage-rusqlite --test schema_version_compat`
11. `cargo fmt --check`
12. `cargo clippy --workspace --all-targets -- -D warnings`
13. `python3 .just/run_lint.py boundaries`
14. `python3 .just/run_lint.py nudge-taxonomy`
15. `python3 .just/check_line_counts.py`
16. `git diff --check`

## Non-closure

- AZ.4 does not alter task state/authorization or add task commands.
- AZ.4 does not merge message/task lifecycle storage or copy content into
  scheduler rows.
- AZ.4 does not modify the legacy synchronous daemon, run a live/test daemon, or
  perform a tag, release, package publish, or installation.
- AZ.4 changes only the Herdr idle attention pump. The bare-CLI pull contract
  remains ADR-054's existing behavior: each pull drains all steer items and at
  most one oldest queue item; this sprint makes no universal scheduler claim.
