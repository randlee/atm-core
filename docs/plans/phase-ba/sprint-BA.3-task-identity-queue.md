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

The sprint must name the new type, its column and event projection, the
migration default for historical `complete` rows, and the JSON compatibility
story for `atm task events` consumers. This is a column and type change; it is
not a new table and not a new state machine.

### D5. Migration — there is no universal automatic merge rule

**SOLAR-BA-002 (BLOCKING), verified against the live atm-dev ledger: 109 task
rows, 95 distinct task ids, 14 duplicated ids.** The duplicate groups are not
all phantom mirrors:

- `FIX-PRERELEASE-R3-…` is the misleading-twin pattern — one real history plus
  a mirror opened by the completion report.
- `FIX-1325-…` carries **independent** `assigned → active → complete` histories
  for two different agents under one id. These are two real tasks that happen
  to share a name.

A precedence rule — complete-wins, latest-wins, open-wins, or AZ's
`Active > Assigned > Complete` — cannot distinguish these. It will silently
conflate distinct historical tasks or pick the wrong assignee, and for a future
group with two **live** assignees it destroys real work and directly
manufactures problem (b). **Do not ship a universal merge rule, and do not
copy AZ's precedence.**

Required instead:

1. **A migration preflight that classifies every duplicate group** before any
   schema mutation. Classes: byte-identical duplicates (mergeable), mirror
   pattern (mergeable under a stated rule), and everything else.
2. **Abort before mutation** on any non-identical or multi-assignee group. Emit
   `team / task_id / assignee / state / assigned_at / updated_at` per row plus
   a concrete operator recovery instruction. The migration exits non-zero and
   the schema version does not change.
3. **Resolve the 14 existing groups explicitly as a deployment step**, recorded
   in the sprint evidence — not silently by code.
4. **Prove row and event counts, and oversight history, after migration.**

Also determine and test: an agent holding two or more Active rows before the
unique index exists (deterministic demotion to `assigned`, never silent closure
of live work); and what happens when an assignee reports completion to the
assigner under the single-row key — today that creates the mirror, now it
either updates the same row or collides. **Write the test; do not assume.**

Migration must never silently close live work, reopen a completed task, or make
an existing host unupgradable. Failure rolls back before the version changes.

### D6. Event identity must become task-global

**SOLAR-BA-003 (BLOCKING), verified.** Narrowing only the `tasks` key is half
an identity change. `task_events` is keyed per assignee and allocates its
sequence per assignee:

```sql
PRIMARY KEY (team, task_id, assignee, seq)          -- task_store.rs:42-57
SELECT COALESCE(MAX(seq), 0) + 1 FROM task_events
  WHERE team = ?1 AND task_id = ?2 AND assignee = ?3 -- task_store.rs:167-176
```

and the history read orders by `seq` alone
(`task_sql.rs:18-21`: `... WHERE team = ?1 AND task_id = ?2 AND (?3 IS NULL OR
assignee = ?3) ORDER BY seq ASC`).

Live duplicate histories already contain `seq = 1, 2, …` for **both** assignees.
The moment `tasks` is keyed `(team, task_id)`, `atm task events <id>` returns
duplicate sequence values in a non-total order, and a future reassignment
restarts numbering for the new assignee. That breaks exactly the timestamped
oversight history this phase promises.

Required: `task_events` identity and sequence allocation become task-global —
`PRIMARY KEY (team, task_id, seq)`, `MAX(seq)` scoped to `(team, task_id)` —
with `assignee` retained as event **data**. Legacy interleaved streams must be
renumbered into one total order during the migration, deterministically (order
by `at`, then existing assignee, then existing seq) with a proof test.

This is not a new table or state machine. It is the second half of the declared
identity change and cannot be deferred past it.

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
5. The migration **aborts before mutating** on a fixture containing a
   multi-assignee duplicate group, exits non-zero, leaves the schema version
   unchanged, and prints every conflicting row plus a recovery instruction.
6. The migration succeeds on a fixture of byte-identical duplicates and on the
   mirror pattern, and the resulting row and event counts are asserted.
7. An agent holding two Active rows pre-migration is deterministically demoted,
   with the demotion recorded; no live work is closed.
8. `task_events` has one total order per `(team, task_id)` after migrating an
   interleaved two-assignee legacy stream, and a post-migration append
   continues that order without collision.
9. A fresh database and a migrated database have byte-equivalent schema.
10. The sort key is `(position, assigned_at, task_id)` in both `task_sql.rs`
    and `herdr_queue_wake.rs`, with no third ordering site.

## Required validation

- `just test`, `just lint`
- migration fixtures for every D5 duplicate class, the abort path, the
  two-Active demotion, the D6 interleaved-event renumber, and the
  fresh-vs-migrated equivalence check
- the deployment step resolving the 14 live duplicate groups, recorded as
  sprint evidence
- `schema-reviewer` sign-off on the ADR-061 minor classification **before**
  this sprint opens

## Non-closure

- No `atm task move` command ships here; D3 delivers the column and the sort.
  The command is BA.5. Criterion 4 is tested through the storage API.
- No v2 table, no assignment-attempt table, no operations table, no
  supersession. This sprint adds columns and one index to the existing
  `tasks` table.
