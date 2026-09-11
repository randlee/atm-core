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
                               INFORMATIONAL, never a rollback
```

If stage 2 commits and stage 3 transiently fails, return a stable recoverable
error and make the retry close **without duplicating the report** — the
existing origin message id and idempotent message insertion carry this.

RBP-001: unknown id, already closed, unauthorized actor, and post-delivery
close failure are four **distinct, stable** error/disposition contracts, not
one generic failure.

Races to test: close vs close, close vs reassign, and a crash after the report
commits but before the close.

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

Delivery mode is **deferred**, per BA.4 D6b (SOLAR-BA-016): a completion report
must never steer an Active assigner. Assignments are already forced deferred
when a `task_id` is present (`send/mod.rs:339-347`); the same rule extends to
the start receipt, the refusal, and the reassignment notice.

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

### D6. The explicit start operation — requires a ruling before this sprint opens

**SOLAR-BA-004 (BLOCKING), verified.** The closed set above deliberately omits
`start`, and BA.1 deletes ack-driven activation. On develop the *only*
`Assigned → Active` input is `TaskEvent::Acked` (`task_state.rs:97-109`,
applied by `writer/task_ops.rs:418-440`). Remove it and nothing activates a
task: rows sit at `assigned` while being worked, the
`one_active_task_per_agent` index never arbitrates (it is
`WHERE state = 'active'`, so inserting an `assigned` row cannot trip it), and
Rand's required start receipt has no event to ride on.

None of the implicit substitutes work, and the sprint must not adopt one:

- **roster `Active`** — activity may be unrelated to the queue, and with
  several assigned rows it cannot say *which* task began
- **successful nudge handoff** — delivery is not work start
- **first outbound message** — same problem, plus it fires for unrelated mail

The fix is an explicit lifecycle operation that atomically selects the task,
moves `Assigned → Active` under the unique index, appends the event, and
creates the assigner receipt. It adds no state, table, or state machine — but
it **is** a sixth verb or an equivalent explicit API call, which crosses
Rand's closed-set ruling.

> **RULING REQUIRED FROM RAND BEFORE THIS SPRINT OPENS.** Three shapes:
>
> a. `atm task start <task-id>` — a sixth verb. Most explicit; smallest
>    surprise; contradicts the five-verb ruling in letter only.
> b. An explicit harness/agent API call with no CLI verb — keeps the CLI at
>    five, but hides a lifecycle operation from the operator and from
>    `atm task` oversight.
> c. `atm task assign` self-targeted as an implicit start — overloads assign
>    with two meanings; rejected on RBP grounds unless Rand prefers it.
>
> **Recommendation: (a).** The closed set was closed to delete *redundant*
> verbs; start stopped being redundant the moment ack left the task domain.

Acceptance once ruled: concurrency (two starts racing), duplicate-start
idempotency, receipt-write failure and retry, and the crash boundary either
side of the receipt.

### D7. Reassignment — requires a ruling before this sprint opens

**SOLAR-BA-009 (BLOCKING).** Reassignment is specified as close-and-create,
task ids are never deleted or reused, and `(team, task_id)` becomes unique.
Those three cannot all hold:

- reuse the same id → it collides with the retained `complete` row, and
  `(Some(Complete), Assigned) => Err("already complete; use a new id")`
  rejects it
- mint a new id → the `reassigned` outcome has no structured successor link,
  and the external body's task-id relation changes, so oversight cannot follow
  the reassignment

This sits directly on the escape path Rand named: *"team-lead can assign to
another agent if he KNOWS."* It must not be an implementation guess.

> **RULING REQUIRED FROM RAND BEFORE THIS SPRINT OPENS.** Two shapes:
>
> a. **Same-id reassignment** — an atomic close/reopen that updates
>    `current_assignee`, resets queue position and reminder counters, and
>    retains the full event history under one id.
> b. **Old-id close + new-id assign**, with a mandatory successor task id
>    recorded in the close event.
>
> **Recommendation: (a).** It matches "one logical task, one id", keeps one
> event history, and needs no successor link — whereas (b) reintroduces
> exactly the supersession linkage this phase deletes. Rand's *"if that means
> close one and create a new one, that is acceptable"* was permission, not a
> requirement, and it predates the single-row identity ruling.

Acceptance once ruled: legality from both `assigned` and `active`, concurrent
reassignment, the receipt to the new assignee, queue position of the moved
task, and reminder-counter reset.

## Affected paths

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

1. `atm task` exposes exactly five subcommands. Gate: the CLI surface baseline
   contains no sixth, and none of the absent verbs above.
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
8. A task is observably `active` only after the explicit start operation
   (D6); two concurrent starts leave exactly one Active row and one receipt.
9. The start receipt reaches the assigner's mailbox, asserted on the mailbox,
   not on a log line.
10. Reassignment behaves per the D7 ruling, and the full event history remains
    reachable from `atm task events` afterwards.
11. A team member who is neither assignee nor assigner cannot close, move, or
    start another member's task; each rejection is a distinct stable error
    raised before any write. Must fail against a naive `(team, task_id)`
    lookup.
12. `atm task list` and `atm task events` are bounded in SQL: a fixture with a
    large event history reads a bounded number of rows, and paging through it
    under concurrent appends neither skips nor duplicates an entry.
13. Closing an unknown id persists **no** message; closing an already-complete
    id persists the message and reports informationally; a close that fails
    after delivery is retryable without duplicating the report.

## Required validation

- `just test`, `just lint`
- regenerate `cli_surface_baseline.json` and `openapi_surface_baseline.json`
  through the normal path
- criterion 2 fails on `origin/develop`

## Ruling gate

D6 and D7 each require an explicit ruling from Rand. This sprint does **not**
open until both are recorded in the phase plan. A dev agent must not pick a
shape from the options above on its own.

## Non-closure

- Ephemeral queued messages are not scheduled here; BA.6.
- No task body is read, parsed, or resolved. ATM resolves nothing but a task
  id; the body lives in markdown, a bead, or a ttl file.
