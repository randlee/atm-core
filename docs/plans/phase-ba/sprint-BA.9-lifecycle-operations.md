# BA.9 — Task lifecycle operations: start and reassign

| Field | Value |
| --- | --- |
| Wave | 4 |
| Branch | `feature/ba9-lifecycle-operations` |
| Base | `feature/ba5-atm-task-commands` (stack layer 4) |
| Dependency | `must_follow` BA.5 (owns the command surface), `must_follow` BA.3 (identity and typed outcome) |
| recommended_agent | arch-ctm |
| recommended_model | deep-reasoning |
| Ruling gate | **Both decided**: R1(a) explicit `atm task start`, R2(a) same-id reassignment. Reversible by Rand. |

## Goal

Close the one hole the rest of the phase leaves open: a task can actually
become `active`, and it can change hands.

Split out of BA.5 (PLAN-SCOPE-004). These two deliverables were gating seven
unrelated ones on two unrelated human rulings.

## Rulings — both decided, no gate remains

R1(a) and R2(a) are recorded in the phase plan. This sprint opens on its
dependencies alone.

**This sprint owns the verb-count amendment (PLAN-CRIT-014).** BA.5 ships five
subcommands and its acceptance criterion says so; R1(a) makes the set six.
BA.9 amends BA.5's criterion and regenerates `cli_surface_baseline.json` to
six. Exactly one sprint owns that number at a time.

R2(a)'s event semantics are fixed in the phase plan and are **not** open here:
one `Reassigned` event, the state never passes through `complete`,
`close_outcome` stays `NULL`, position and reminder counters reset in the same
transaction.

## Deliverables

### D1. The explicit start operation — gated on R1

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

> **RULING REQUIRED BEFORE THIS HALF OPENS.** Three shapes:
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

### D2. Reassignment — gated on R2

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

> **RULING REQUIRED BEFORE THIS HALF OPENS.** Two shapes:
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

- `crates/atm/src/commands/task.rs`
- `crates/atm-core/src/task_command/*`
- `crates/atm-storage/src/task_state.rs` — the `Assigned → Active` arm
- `crates/atm-storage-rusqlite/src/writer/task_ops.rs`
- `crates/atm/tests/cli_surface_baseline.json` — regenerated, never hand-edited

## Paths that must not change

- `crates/atm-http-runtime/*` — BA.4 / BA.8
- the `tasks` schema — BA.3; this sprint uses the columns, it does not add any

## Acceptance criteria

Start half:

1. A task is observably `active` only after the explicit start operation.
2. Two concurrent starts leave exactly one Active row and exactly one receipt.
3. A duplicate start is idempotent, not an error and not a second receipt.
4. The start receipt reaches the assigner's mailbox, asserted on the mailbox,
   not on a log line, and does not steer an Active assigner (BA.8 D6's rule).
5. A receipt write failure does not roll back the started task and is
   retryable without duplicating the receipt.
6. Starting a second task while one is active is rejected by the
   `one_active_task_per_agent` index, not by application code.
7. Only the current assignee may start; any other actor is rejected with the
   BA.5 D2a authority error before any write.

Reassign half:

8. Reassignment is legal from both `assigned` and `active`.
9. Two concurrent reassignments leave one winner and one stable error.
10. The new assignee receives a notice; queue position and reminder counters
    are reset per the R2 shape.
11. The full event history remains reachable from `atm task events` after
    reassignment, and the `Reassigned` event records actor, from-assignee and
    to-assignee.
12. Reassignment never writes `state = 'complete'` and never sets
    `close_outcome`; a terminally suppressed task that is reassigned resumes
    reminders (the BA.8 D1 reset predicate).
13. `atm task` exposes exactly six subcommands and the regenerated CLI surface
    baseline says so.

## Required validation

- `just test`, `just lint`
- regenerate `cli_surface_baseline.json` through the normal path
- criteria 2, 3, 5 and 9 are the concurrency and crash matrix; none may be
  satisfied by a single-threaded test

## Non-closure

- No new table, state machine, or capability. If R1 resolves to option (a) it
  adds one CLI verb and nothing else; the complexity budget already accounts
  for that.
- If Rand reverses R1 or R2, the affected half stops and the plan is
  re-derived; a dev agent must not re-litigate the shape mid-sprint.
