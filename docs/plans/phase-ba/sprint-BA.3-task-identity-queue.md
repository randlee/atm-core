# BA.3 — Task identity, one-active, and queue position

| Field | Value |
| --- | --- |
| Wave | 2 |
| Branch | `feature/ba3-task-identity-queue` |
| Base | `feature/ba1-ack-task-separation` (stack layer 2) |
| Dependency | `must_follow` BA.1 — merge-forward trigger: BA.1 development pushed, not QA |
| recommended_agent | arch-ctm |
| recommended_model | deep-reasoning |
| Governed interface | yes — ADR-061 minor with migration |

## Goal

One task id is one row. One agent holds at most one active task, enforced by
the database. Team-lead can place a task anywhere in an agent's queue without
rewriting assignment timestamps.

## The defect

`origin/develop`, `task_store.rs:16-30`:

```sql
CREATE TABLE IF NOT EXISTS tasks (
    team TEXT NOT NULL,
    task_id TEXT NOT NULL,
    assignee TEXT NOT NULL,
    ...
    PRIMARY KEY (team, task_id, assignee)
);
CREATE INDEX IF NOT EXISTS tasks_open_by_member
    ON tasks(team, assignee, assigned_at) WHERE state <> 'complete';
```

The assignee is **part of identity**, so one task id can have one row per
participant. Live evidence: `FIX-PRERELEASE-R3-20260909T045045Z` carries a
completed row and a 587-reminder row under one id; the nudged twin was already
done on its sibling and was structurally uncloseable.

**SOLAR-BA-019 (IMPORTANT): the writer that created those mirrors is already
fixed.** `origin/develop` at 143355254 (PR #1381, issue #1378) changed
`apply_task_completion` (`writer/task_ops.rs:295-338`) to resolve the existing
row — sender first, recipient as fallback — error when none exists, and only
UPDATE. Completion reports no longer insert a second row. Do not write this
sprint as if they do.

What remains, and what this sprint is for: the 14 existing duplicate groups are
migration debt #1381 does not clean up, and the key still *permits* ambiguous
identity. #1381 closed the one writer that exercised the shape; BA.3 removes
the shape.

There is no uniqueness on open tasks per agent: `tasks_open_by_member` is a
plain index. The only one-active guard was `admit()`, which BA.1 deletes.

Ordering is strict FIFO with no way to promote, in two places that agree:

- `task_sql.rs:45` — `ORDER BY assigned_at ASC, task_id ASC`
- `herdr_queue_wake.rs:857-864` — sort by `assigned_at`, then `task_id`;
  Active first, else oldest Assigned

Consequently close-and-create can only **demote**: a recreated task is the
newest and lands at the back.

## Deliverables

### D1. Single-row identity

```sql
PRIMARY KEY (team, task_id)
```

`current_assignee` becomes an attribute, not identity. Copied from
`schema_version.rs` on `origin/integrate/phase-az`.

### D2. One active task per agent

```sql
CREATE UNIQUE INDEX IF NOT EXISTS one_active_task_per_agent
    ON tasks(team, current_assignee) WHERE state = 'active';
```

Copied from `schema_version.rs:284-285` on `origin/integrate/phase-az`. The
partial predicate is `state = 'active'` only: an agent may hold many
`assigned` rows. That **is** the queue.

### D3. Queue position

A `position` column, **separate from `assigned_at`**. Reordering must never
rewrite the assignment timestamp: timestamps are what make incident
reconstruction possible, and the 587-reminder analysis depended on them.

- sort key becomes `(position, assigned_at, task_id)` in both sites above
- default position is **end** — FIFO remains the behaviour when nobody
  intervenes
- the queue is one agent's open tasks, a handful of rows, so renumber the
  whole queue in one transaction. No sparse gaps, no LexoRank, no fractional
  keys.

**SOLAR-BA-014 (IMPORTANT): the invariants below are part of the deliverable,
not implementation detail.** Without a database-level invariant two writers can
expose duplicate or non-positive positions and the selector's order becomes
ambiguous — which is exactly the "constraint and queue are the same structure"
claim failing.

1. **Domain.** Positions are one-based and contiguous across **all open rows**
   for one `(team, current_assignee)`. The active row, if any, is fixed at
   position 1; assigned rows are contiguous from 2. A closed row has no
   position.
2. **Uniqueness.** Enforced in the database, not in application code —
   a unique index over `(team, current_assignee, position)` restricted to open
   rows, plus a `CHECK(position >= 1)`. Verified by a concurrent-writer test.
3. **Migration initialization.** Legacy rows are numbered by the existing
   `(assigned_at, task_id)` FIFO, which preserves today's observable order.
4. **Renumbering events.** Specify and test what happens on close, start,
   reassign, and assign — every path that adds or removes an open row must
   leave the queue contiguous in the same transaction.
5. **`--before` validation.** Naming another assignee's task, a completed
   task, or the task itself is a typed, stable error (RBP-001), never a
   silent no-op and never a partial renumber.
6. **Concurrency.** Renumbering runs in a SQLite immediate single-writer
   transaction. Concurrent move/assign/close must not interleave into a
   duplicate or gapped state. No new state machine, no advisory lock.

### D4. Typed close outcome — a distinct type, not the reminder outcome

`completed | refused | cancelled | reassigned`. Required because reassignment
is close-and-create, so the outcome is the only thing in the ledger
distinguishing a finished task from an abandoned one. Free text cannot be
counted by oversight.

`refused` is the **task** outcome. `blocked` is an **agent** state and must
not appear in this enum.

**SOLAR-BA-013 (IMPORTANT): do not reuse `TaskEventRow.outcome`.** That field
is `Option<ReminderOutcome>` and its SQLite `CHECK` admits only
`emitted | unrenderable | blocked` (`task_state.rs:203-216`,
`task_store.rs:42-56`). It describes *reminder delivery*. Overloading one
primitive column with two unrelated domains violates RBP-004 and makes
`blocked` ambiguous across the exact two meanings this phase exists to
separate.

The type is fixed here, not left to sprint execution (PLAN-SCOPE-006) — this
deliverable exists to close an ambiguity, so leaving its shape open reopens it:

```rust
/// Why a task left the open set. Distinct from `ReminderOutcome`, which
/// describes whether a *reminder* was delivered. `blocked` is an agent
/// state and deliberately has no variant here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskCloseOutcome {
    Completed,
    Refused,
    Cancelled,
    Reassigned,
}
```

```sql
close_outcome TEXT NULL CHECK(close_outcome IN
    ('completed', 'refused', 'cancelled', 'reassigned'))
```

`NULL` while the task is open. The sprint still owns the event projection, the
migration default for historical `complete` rows, and the JSON compatibility
story for `atm task events` consumers. This is a column and type change; it is
not a new table and not a new state machine.

### D5. Migration and event renumbering are **BA.10**

The duplicate-group migration (SOLAR-BA-002) and the task_events global
renumbering (SOLAR-BA-003) moved to **BA.10** (PLAN-SCOPE-005). Both are
live-data migrations against the real atm-dev ledger with their own
abort/rollback/proof obligations and a manual deployment step; neither shares
a failure mode with the schema, index and type work above.

This sprint delivers the **shape**. BA.10 delivers the **data move**, and
`must_follow` this sprint.

### D7. Retention constraint

Nothing currently deletes task rows: no `DELETE FROM tasks`, no
`DELETE FROM task_events`, no retention or prune path. BA.5's rule that an
unknown task id is a hard error is **only sound while this holds**. Record it
as a constraint in the sprint doc and in the code comment on the migration.

## Affected paths

- `crates/atm-storage-rusqlite/src/task_store.rs`
- `crates/atm-storage-rusqlite/src/task_sql.rs`
- `crates/atm-storage-rusqlite/src/writer/task_ops.rs` — event sequence allocation
- `crates/atm-storage-rusqlite/src/schema_version.rs`
- `crates/atm-storage/src/task_state.rs`
- `crates/atm-http-runtime/src/herdr_queue_wake.rs` — sort key only

## Paths that must not change

- `crates/atm-http-runtime/src/herdr_queue_wake_reminders.rs`,
  `herdr_queue_wake_escalation.rs`, `herdr_escalation.rs` — BA.4
- `crates/atm/src/commands/*` — BA.5

## Acceptance criteria

1. Two rows for one task id are unrepresentable after migration.
2. A second Active task for one agent is rejected by the database, not by
   application code.
3. An agent may hold many `assigned` rows concurrently.
4. `atm task move --before` reorders without any change to `assigned_at`.
   Asserted by comparing timestamps before and after.
4a. Positions are contiguous and unique per open assignee after every
   assign / move / start / close / reassign path, enforced by the database and
   proven under concurrent writers.
4b. An invalid `--before` target (other assignee, completed task, self)
   returns a typed error and leaves positions unchanged.
4c. A close outcome is stored in its own typed column; `ReminderOutcome`'s
   `CHECK` constraint is unchanged and still admits only its three delivery
   values.
5. The no-deletion retention constraint (D7) is stated in both this sprint doc
   and a code comment on the schema module, naming BA.5's unknown-id hard
   error as the dependent (PLAN-SCOPE-008).
6. A fresh database has the full target schema, including both indexes, the
   `position` column and the `close_outcome` column with its CHECK.
10. The sort key is `(position, assigned_at, task_id)` in both `task_sql.rs`
    and `herdr_queue_wake.rs`, with no third ordering site.

## Required validation

- `just test`, `just lint`
- a fresh-database schema assertion; migration fixtures are BA.10's
- `schema-reviewer` sign-off on the ADR-061 minor classification **before**
  this sprint opens

## Non-closure

- No migration ships here; BA.10 owns it. A developer must not "just fix" the
  14 live duplicate groups while in the schema files.
- No `atm task move` command ships here; D3 delivers the column and the sort.
  The command is BA.5. Criterion 4 is tested through the storage API.
- No v2 table, no assignment-attempt table, no operations table, no
  supersession. This sprint adds columns and one index to the existing
  `tasks` table.
