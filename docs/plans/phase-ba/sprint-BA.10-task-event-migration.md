# BA.10 — Task and event migration

| Field | Value |
| --- | --- |
| Wave | 3 |
| Branch | `feature/ba10-task-event-migration` |
| Base | `integrate/phase-ba` (independent PR, not stacked) |
| Dependency | `must_follow` BA.3 — operates on the schema BA.3 establishes. PR-completion trigger. |
| recommended_agent | arch-ctm |
| recommended_model | deep-reasoning |
| Governed interface | yes — ADR-061 minor with migration |

## Goal

Move the existing data onto BA.3's schema without losing, closing, or
inventing a task.

Split out of BA.3 (PLAN-SCOPE-005). BA.3 changes the shape; this sprint moves
the data. They have different failure modes: BA.3's is a wrong schema, this
sprint's is destroyed production history, and the second one is not recoverable
by a follow-up commit.

## The data, as it actually is

Measured on the live atm-dev ledger: **109 task rows, 95 distinct task ids, 14
duplicated ids.** The duplicate groups are not one pattern:

- `FIX-PRERELEASE-R3-…` — the mirror pattern: one real history plus a twin
  opened by a completion report.
- `FIX-1325-…` — **independent** `assigned → active → complete` histories for
  two different agents under one id. Two real tasks that share a name.

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


## Affected paths

- `crates/atm-storage-rusqlite/src/schema_version.rs`
- `crates/atm-storage-rusqlite/src/task_store.rs` — migration path only
- `crates/atm-storage-rusqlite/src/writer/task_ops.rs` — event sequence
  allocation
- migration fixtures under `crates/atm-storage-rusqlite/tests/`

## Paths that must not change

- the schema definitions BA.3 established — this sprint migrates onto them,
  it does not redesign them
- `crates/atm-http-runtime/*` — BA.4 / BA.8
- `crates/atm/src/commands/*` — BA.5

## Acceptance criteria

1. The migration **aborts before mutating** on a fixture containing a
   multi-assignee duplicate group: exits non-zero, leaves the schema version
   unchanged, and prints every conflicting row with a recovery instruction.
2. The migration succeeds on a fixture of byte-identical duplicates and on the
   mirror pattern; row and event counts are asserted before and after.
3. An agent holding two Active rows pre-migration is deterministically
   demoted, the demotion is recorded, and no live work is closed.
4. `task_events` has one total order per `(team, task_id)` after migrating an
   interleaved two-assignee legacy stream, and a post-migration append
   continues that order without collision.
5. Assignee→assigner completion resolves the sole existing row under the new
   key (the SOLAR-BA-019 regression test, re-proved).
6. A fresh database and a migrated database have byte-equivalent schema.
7. A failed migration rolls back completely and leaves the database openable
   by the pre-migration binary.
8. Legacy rows are positioned by the existing `(assigned_at, task_id)` FIFO,
   so observable queue order is unchanged by the migration alone.

## Required validation

- `just test`, `just lint`
- every fixture in criteria 1-8
- **the deployment step**: the 14 live duplicate groups classified and
  explicitly resolved, recorded as sprint evidence. This is an operator
  action with a written record, not something the code decides.
- `schema-reviewer` sign-off alongside BA.3's

## Non-closure

- No schema redesign. If the migration reveals that BA.3's shape is wrong,
  stop and report; do not fix it here.
- No task GC, prune, or retention policy. BA.3 D7's no-deletion constraint
  holds and BA.5 depends on it.
