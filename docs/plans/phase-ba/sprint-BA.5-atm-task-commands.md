# BA.5 — The `atm task` closed command set

| Field | Value |
| --- | --- |
| Wave | 3 |
| Branch | `feature/ba5-atm-task-commands` |
| Base | `feature/ba3-task-identity-queue` (stack layer 3) |
| Dependency | `must_follow` BA.3 — needs the `outcome` and `position` columns |
| recommended_agent | arch-ctm |
| recommended_model | deep-reasoning |

## Goal

Task lifecycle is managed by a closed set of `atm task` subcommands with a few
aliases that serve as implied subcommands. A clap subcommand enum is closed by
construction, so this lints under ADR-001/RBP-003 with no extra machinery.

No `atm task` namespace exists on `origin/develop` today.

## The closed set

```
atm task assign <agent> --template <j2> --vars <json> [--task-id <id>]
atm task close  <task-id> <outcome> [reason]
atm task move   <task-id> --before <other-task-id> | --head | --end
atm task list   [--all]
atm task events <task-id>
```

Aliases, as implied subcommands:

```
atm send <agent>    --task-id <id> --template ... --vars ...
    → atm task assign
atm send <assigner> --task-complete --task-id <id> --template ... --vars ...
    → atm task close, outcome=completed, carrying a mandatory report
```

**Deliberately absent**: `start`, `ack`, `block`, `unblock`, `reassign`,
`reopen`, `supersede`, `fail`, `abort`. Start is implicit in beginning work
and produces the receipt (BA.1/BA.4). Reassignment is close-and-create.
Ack left the task domain in BA.1. `fail` and `abort` are outcomes of `close`,
not verbs.

> Reading reference only: AZ.3 built this namespace with **eleven** verbs in
> `crates/atm/src/commands/task.rs` and
> `crates/atm-core/src/task_command/service.rs` on
> `origin/integrate/phase-az`. Read it for structure. **Do not merge it**, do
> not import its verb set, and do not carry across `PriorityArg` — queue
> position replaces priority.

## Deliverables

### D0. The protocol path — without it this sprint ships a facade

**PLAN-CRIT-002 (BLOCKING), verified.** Every CLI operation crosses
`RequestEnvelope` / `DaemonApiClient`. On `origin/develop`, `RequestEnvelope`
has fifteen variants — `Write`, `CompatibilityPreflight`, `Heartbeat`,
`QueueGetNext`, four graft variants, `List`, `Peek`, `Receive`, `Clear`,
`Doctor`, `Search`, `ReloadRuntimeView` — and **not one task mutation**.
`TaskStore` exposes reads plus reminder/lead audit. An earlier revision of
this sprint listed only the CLI and a new `atm-core` module, which would let a
developer satisfy most of the listed surface with a command that parses,
validates, prints, and never mutates anything.

Required, and it must land before any verb is wired:

1. task mutation request and response variants on `RequestEnvelope` /
   `ResponseEnvelope` — **additive, ADR-061 MINOR** (`:59`, "a new optional
   field, argument, request variant"), with the matching
   `HTTP_API_VERSION` / `CLI_SCHEMA_VERSION` bump in the same change set
2. the router and runtime handler that dispatch them
3. the mutation entry point on the storage side, reached through the existing
   backend-neutral boundary — **not** a SQLite handle crossing the boundary
   and **not** the CLI writing storage directly

Gate: an integration test drives each verb through a real daemon round trip.
A unit test against an in-process service does not discharge this deliverable.

### D1. One id flag

`--task-id` names the task; verb flags say what to do with it. This inverts
today's shape, where two flags each carry an id and are declared mutually
exclusive (`commands/send.rs:133-138`, plus the test
`cli_rejects_task_complete_with_task_id`):

```rust
#[arg(long = "task-id")]                                   task_id: Option<TaskId>,
#[arg(long = "task-complete", conflicts_with = "task_id")] task_complete: Option<TaskId>,
```

The `conflicts_with` becomes a `requires`.

### D2. Close is three stages, in this order

Both terminal cases are already distinguished in the transition table
(`task_state.rs:96-117`); only the disposition changes.

| case | arm today | required disposition |
|---|---|---|
| already complete | `(Some(Complete), Completed) => Err("already complete")` | **deliver** the report, inform the caller |
| never existed | `(None, Completed) => Err("no open task {id} for {actor}")` | **block**, error to the caller |

**SOLAR-BA-012 (BLOCKING): "deliver first, then close" and "unknown id blocks"
contradict each other unless the order is stated as three stages.** Delivering
unconditionally means a fabricated id is delivered despite the block; applying
the transition first inside one transaction means the already-complete error
rolls the report back — which is the bug being fixed.

```
stage 1  read-only preflight   existence + authorization
                               unknown id or unauthorized actor -> BLOCK here,
                               before the message is persisted
stage 2  persist and deliver   the mandatory report commits
stage 3  idempotent close      already-complete / concurrent-close is
                               INFORMATIONAL, never a rollback.
                               REVALIDATES the assignee and assignment epoch
                               captured at stage 1 (PLAN-CRIT-023)
```

If stage 2 commits and stage 3 transiently fails, return a stable recoverable
error and make the retry close **without duplicating the report** — the
existing origin message id and idempotent message insertion carry this.

RBP-001: unknown id, already closed, unauthorized actor, and post-delivery
close failure are four **distinct, stable** error/disposition contracts, not
one generic failure.

**Stage 3 must revalidate, not assume (PLAN-CRIT-023).** Stage 1 authorizes
against the assignee it read; a same-id reassignment (R2) can land between
stages, and without revalidation the *old* assignee closes a task that now
belongs to someone else. Capture the assignment epoch at stage 1 and require
it to match at stage 3.

Races to test, each with a named winning outcome — not merely "test the race":

| race | required outcome |
| --- | --- |
| close vs close | first commits; second is informational, no duplicate report |
| close vs reassign | **reassign wins**; the stale close fails at stage 3 revalidation with a distinct error, and its already-delivered report remains delivered |
| crash after report commit, before close | retry closes without duplicating the report |

The unknown-id hard error is sound only while task rows are never deleted
(BA.3 D7).

### D2a. The authority matrix

**SOLAR-BA-021 (BLOCKING).** Today the completion lookup is accidentally scoped
to the actor: it resolves the row by message sender, then by addressed
recipient (`writer/task_ops.rs:305-320`). Narrowing the key to
`(team, task_id)` **removes that scoping**. Without an explicit replacement, a
guessed or merely observed task id is enough for any team member to close
someone else's task — permanently silencing its nudges, which is problem (a)
handed to an attacker or to a careless paste — or to reorder another agent's
queue.

Ship an explicit matrix, enforced in command admission, in the read-only
preflight of D2 stage 1, **before** any mutation:

| operation | authorized actor |
| --- | --- |
| `assign` | the existing authorized sender path, unchanged |
| `start` | the current assignee only |
| `close` with `completed` / `refused` | the current assignee |
| `close` with `cancelled` / `reassigned` | the assigner |
| `move` | the assigner |

Whether the team lead is additionally authorized for `cancel` / `reassign` /
`move` on any member's task is **Rand's call** — it is the natural shape given
"team-lead can assign to another agent if he KNOWS", and the plan assumes yes
unless Rand says otherwise. Record the answer before the sprint opens.

An actor/outcome mismatch is a stable, distinct error (RBP-001) raised before
mutation and before message persistence. This is authorization on existing
operations: no new trait, table, or state machine.

### D3. Mandatory report on assignee close

The assignee closes as part of a message to the assigner. The body is the
payload, not an accessory.

Delivery goes through the existing message API. **This sprint does not own the
deferred/no-steer guarantee** (PLAN-SCOPE-003): `crates/atm-core/src/send/mod.rs`
belongs to BA.8, and BA.8 D6 is the sole deliverable extending the deferred rule
to the start receipt, the refusal, and the reassignment notice. BA.8 AC9 is its
test. Do not implement it here and do not write a second path.

Assignments are already forced deferred when a `task_id` is present
(`send/mod.rs:339-347`), so the completion report inherits correct behaviour on
day one; the start receipt and reassignment notice do not, until BA.8 lands.

### D4. `atm task list` is a queue view

- no argument: the caller's own assigned tasks, in queue order
- `--all`: team-lead's view of all assigned tasks for all members

Agent state (blocked, dead/offline) is shown as **informational** context read
live from the roster at display time. It is not persisted, not a task row, and
not a separate escalation set.

### D5. Migrate the existing query surface

`atm list --tasks` and `atm list --task-events <id>` move into
`atm task list` and `atm task events`. Oversight lives in one namespace.
Preserve the `REMINDERS` column: it is the oversight metric for problem (a).

### D5a. The read projection must be bounded

**SOLAR-BA-020 (IMPORTANT).** BA.3 D7 makes the ledger append-only forever, and
this sprint moves the existing unbounded views onto `atm task`. Together that is
an unbounded query over a monotonically growing table — a direct threat to the
oversight and queryability this phase promises, and to daemon availability.

On `origin/develop`: `--limit` explicitly **conflicts with** both task views
(`commands/list.rs:49-62`), the sync and async readers return whole `Vec`s
(`atm-core/src/list.rs:326-365`), and the SQL has no `LIMIT`
(`task_sql.rs:12-21`). The async lane's deadline turns unbounded growth into an
eventual timeout; it does not make the query bounded.

Required: `atm task list` and `atm task events` accept `--limit` and carry a
stable pagination cursor — or ship a deliberately bounded default with an
explicit continuation mechanism. **Push the bound into the storage SQL**, not
into a truncation after the read. Test ordering and continuation under
concurrent appends, which is the case that makes an offset-based cursor wrong.

Adds no capability, table, or state machine; it makes a required read
projection operationally bounded.

### D6/D7. The lifecycle operations are **BA.9**

The explicit start operation (SOLAR-BA-004, ruling R1) and reassignment
(SOLAR-BA-009, ruling R2) moved to **BA.9** (PLAN-SCOPE-004). They were
gating this entire sprint on two unrelated human decisions, while the other
seven deliverables — the closed command set, the three-stage close, the
authority matrix, the queue view, the surface migration and bounded reads —
depend on neither.

**This sprint has no ruling gate and opens immediately.**

Consequence to state plainly: until BA.9 lands, nothing moves a task from
`assigned` to `active`, so the one-active index does not arbitrate and there
is no start receipt. That is an accepted, temporary regression carried from
BA.1, and it is BA.9's to close. Do not paper over it with an implicit
activation.

## Affected paths

- `crates/atm-core/src/protocol.rs` — new request/response variants (D0)
- `crates/atm-http-runtime/src/storage_and_nudge_router.rs` — routing for the
  new variants **only**; coordinate with BA.4/BA.2, which own other hunks
- the runtime handler wiring those variants to the storage boundary
- `crates/atm/src/commands/task.rs` — new
- `crates/atm/src/commands/mod.rs`
- `crates/atm/src/commands/send.rs` — alias flags
- `crates/atm/src/commands/list.rs` — surface migration
- `crates/atm-core/src/task_command*` — new
- `crates/atm/tests/cli_surface_baseline.json` — regenerated, never hand-edited

## Paths that must not change

- `crates/atm-http-runtime/src/herdr_*` — BA.4
- `crates/atm-storage*` schema — BA.3

## Acceptance criteria

1. `atm task` exposes exactly five subcommands **as of this sprint**. Gate:
   the CLI surface baseline contains none of the verbs listed absent above.
   BA.9 adds `start` under ruling R1(a) and **owns the amendment of this
   criterion and of the baseline** — the two sprints must not both claim the
   verb count (PLAN-CRIT-014).
1a. Every verb executes through a real daemon round trip in an integration
   test (D0). A verb that parses and prints without mutating fails.
2. `--task-complete` against an already-complete task **delivers the message**
   and returns an informational result. This must fail on `origin/develop`,
   where the body is discarded.
3. `--task-complete` against an unknown id returns an error and sends nothing.
4. `atm task close` requires a typed outcome; free text alone is rejected.
5. `atm task move --head` places the task immediately after the active task —
   position #2 when one is active, position #1 when none is.
6. `atm task list` shows the caller's queue in order; `--all` shows the team;
   blocked agents are annotated.
7. Both aliases produce byte-identical storage effects to their `atm task`
   equivalents.
8. A team member who is neither assignee nor assigner cannot close, move, or
    start another member's task; each rejection is a distinct stable error
    raised before any write. Must fail against a naive `(team, task_id)`
    lookup.
9. `atm task list` and `atm task events` are bounded in SQL: a fixture with a
    large event history reads a bounded number of rows, and paging through it
    under concurrent appends neither skips nor duplicates an entry.
10. Closing an unknown id persists **no** message; closing an already-complete
    id persists the message and reports informationally; a close that fails
    after delivery is retryable without duplicating the report.

## Required validation

- `just test`, `just lint`
- regenerate `cli_surface_baseline.json` and `openapi_surface_baseline.json`
  through the normal path
- criterion 2 fails on `origin/develop`

## Non-closure

- The start operation and reassignment are BA.9. This sprint must not invent
  an implicit activation to fill the gap.
- Ephemeral queued messages are not scheduled here; BA.6.
- No task body is read, parsed, or resolved. ATM resolves nothing but a task
  id; the body lives in markdown, a bead, or a ttl file.
