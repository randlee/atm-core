---
title: Phase BA Task Identity Storage Change
---

# ADR-064 — Phase BA Task Identity Storage Change

| Field | Value |
| --- | --- |
| ID | ADR-064 |
| Status | **Proposed — pending Rand's decision on D4** |
| Scope | The SQLite `tasks` and `task_events` schema under phase BA |
| Relates to | ADR-018, ADR-054, ADR-061, ADR-062, ADR-063, `docs/plans/phase-ba/*` |

## Why this ADR is in the plan PR and not in a sprint

ADR-061 D3 requires a major change's approval to be *"recorded … **before plan
approval**, cited by message id, issue comment, or ADR"*. An ADR written during
the phase cannot satisfy that, so this document sits in the plan PR where it
can be reviewed before anything is built. The plan's earlier revision deferred
it to sprint BA.0; that was still too late.

ADR-062's and ADR-054's amendments remain BA.0 deliverables. They must precede
the *implementation* that changes them, which wave 0 satisfies; only this
document is bound by "before plan approval".

## Context

Phase BA's defect is that one logical task can occupy several rows. The live
`atm-dev` ledger holds 109 task rows for 95 distinct task ids — 14 ids
duplicated. Two distinct causes:

- the **mirror**: an assignee's completion report opened a phantom row on the
  assigner. Already fixed by PR #1381 — `apply_task_completion`
  (`writer/task_ops.rs:295-338`) now resolves the existing row and only
  UPDATEs. The historical rows remain.
- **genuinely independent lifecycles** under one id (`FIX-1325-…` has two
  full `assigned → active → complete` histories).

The shape phase BA wants is one row per `(team, task_id)`, with the assignee as
an attribute, a queue `position`, and a typed close outcome. On
`origin/develop` (`task_store.rs:15-58`) the tables are:

```sql
tasks       PRIMARY KEY (team, task_id, assignee)
task_events PRIMARY KEY (team, task_id, assignee, seq)
```

## Decision

### D1. Classification

Narrowing those primary keys, adding `current_assignee` / `position` /
`close_outcome`, and adding a `one_active_task_per_agent` unique index is an
**ADR-061 MAJOR** change. ADR-061 D2 names it twice: *"removing or renaming a
field … changing a type, constraint, status, or meaning"* and *"tightening
validation an older peer would fail"*. A previous binary that inserts a second
row for the same task id — which is exactly what it does on every assignment
to a second assignee — would violate the narrowed key.

### D2. The additive alternative, as ADR-061's design rule requires

ADR-061 D3 ends: *"before proposing a major change, the author must show why
the capability cannot be expressed additively."* The plan asserted it could
not. **That assertion was wrong, and this is the material finding of this
ADR.**

An additive shape exists:

| | additive shape | major shape |
| --- | --- | --- |
| `tasks` PK | **unchanged** `(team, task_id, assignee)` | narrowed to `(team, task_id)` |
| `task_events` PK | **unchanged** | narrowed to `(team, task_id, seq)` |
| `current_assignee`, `position`, `close_outcome` | added, nullable or defaulted | added, `NOT NULL` |
| one row per task id | enforced in the writer, the only writer that creates task rows — the same layer where PR #1381 already fixed the mirror | enforced by the primary key |
| one active task per agent | enforced in the writer transaction, plus a `doctor` check | enforced by a unique index |
| queue contiguity | transaction invariant + `doctor` check — **already the case in the major shape**, because SQLite cannot express contiguity either | same |
| previous binary | opens, reads, and writes the rows it understands. It does not maintain the new columns; the new writer backfills them on next touch | cannot open |
| ADR-061 class | **MINOR** — columns with defaults and new indexes (D2's first bullet) | MAJOR |
| coexistence window | satisfied by construction | requires a bridge, or an exception to D3 |
| migration | reconciles historical duplicates; never required for correctness of new writes | mandatory, one-way, irreversible |

**What the additive shape costs, stated plainly:**

1. **Uniqueness is not database-enforced.** Phase BA's acceptance criterion
   "one task id is one row, enforced by the database" becomes "enforced by the
   writer, verified by a doctor check and a test". Note that contiguity and
   active-at-position-1 were *already* in that category — SQLite cannot
   express them — so this moves one more invariant across a line the plan had
   already crossed.
2. **An old binary can still create a duplicate** while any old binary is
   running. The new binary's doctor reports it; it does not prevent it. This
   is precisely the coexistence cost ADR-061 D3 chooses to pay.
3. **Historical duplicates are reconciled, not eliminated by construction.**
   The BA.10 preflight and repair artifact are still needed, but they stop
   being a gate on the schema change and become ordinary data hygiene.

**What it buys:** no ADR-061 D3 exception, no bridge, no retained old-binary
fixture, no removal ADR, no `STORAGE_SCHEMA_VERSION` prerequisite release, and
no irreversible step in the phase at all.

### D3. `STORAGE_SCHEMA_VERSION` does not exist

ADR-061 D1 states *"`STORAGE_SCHEMA_VERSION` does not exist yet and needs its
own planned sprint"*, and `git grep STORAGE_SCHEMA_VERSION origin/develop`
returns nothing. The plan's promised version gate — *the old binary refuses to
open the migrated database* — is therefore unimplementable against the actual
previous binary: a binary that predates the constant cannot inspect a marker
introduced after it shipped.

Any major-shape option must either ship a **prerequisite release** that
introduces and enforces the constant *before* the migration, or withdraw the
refuses-to-open claim. The additive shape needs neither.

### D4. The decision — **Rand's**

| | shape | what it requires from Rand |
| --- | --- | --- |
| **i** | **Additive, MINOR** (D2) | nothing beyond accepting this ADR. Cost: uniqueness enforced in the writer, not the schema |
| **ii** | MAJOR with the ADR-061 D3 coexistence window | approval of the major classification. Cost: this is phase AZ's shape — the bidirectional bridge, the retained 1.5.14 fixture, the 1.7.0 removal ADR |
| **iii** | MAJOR, one-way, no bridge | a **recorded exception to ADR-061 D3**, or an amendment to ADR-061, plus a prerequisite release shipping `STORAGE_SCHEMA_VERSION`. Cost: no supported downgrade; rollback is restore-from-backup |

**Recommendation: (i).** Phase BA exists because phase AZ was retired for
adding structure, and (ii) re-adds exactly that structure. (iii) buys a clean
schema by making the upgrade irreversible and requires suspending a governing
ADR Rand accepted six days ago. (i) delivers every user-visible outcome the
phase promises — one row per task, one active task per agent, an ordered
queue, a typed outcome — and pays for it with writer-level rather than
schema-level enforcement, at the same layer where the mirror defect was
already fixed.

If (i) is chosen, this ADR's status becomes `Accepted`, its classification
becomes MINOR, BA.3 and BA.10 are rescoped before they open, and BA.10 stops
being an irreversible step.

## Consequences

- Under (i): no phase-BA change is irreversible, no host must upgrade in
  lockstep, and the phase carries no ADR-061 major approval at all.
- Under (i): two invariants — task-id uniqueness and one-active-per-agent —
  are writer-enforced and doctor-verified rather than schema-enforced. Any
  future phase that wants them in the schema inherits this same decision.
- Under (ii) or (iii): BA.3 and BA.10 are one deployable unit and the phase
  acquires a hard human gate before either opens.
- Under (iii) only: the product loses a supported downgrade path, and a
  prerequisite release must ship first.

## Rejected alternatives

1. **Assert MAJOR without attempting the additive shape.** Rejected — this is
   what the plan did, and ADR-061 D3's design rule exists to prevent it.
2. **Narrow the key and accept lockstep upgrade.** Rejected as written: ADR-061
   D3 states *"No change may require every host to upgrade together."* It is
   available only as option (iii), with an explicit exception.
3. **Two authorities (v1 beside v2).** Rejected — this is ADR-063's shape and
   the reason phase AZ was retired.

## Required evidence

- Under (i): a test in which a previous-release binary opens the upgraded
  database, performs its supported assign/acknowledge/complete writes, and the
  new binary then reads the result; plus writer-level uniqueness and
  one-active tests, plus the doctor check reporting a duplicate an old binary
  created.
- Under any option: `schema-reviewer` sign-off on the classification recorded
  in this ADR before BA.3 opens.
- The BA.10 preflight classifier and repair artifact, whose necessity is
  unchanged by the choice.
