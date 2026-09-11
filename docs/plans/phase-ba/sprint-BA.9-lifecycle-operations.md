# BA.9 — Task lifecycle operations: start and reassign

| Field | Value |
| --- | --- |
| Wave | 4 |
| Branch | `feature/ba9-lifecycle-operations` |
| Base | `feature/ba5-atm-task-commands` (stack layer 5) |
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

### D1. The explicit start operation — **R1 decided: (a)**

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
it **is** an additional verb or an equivalent explicit API call, which
crosses Rand's closed-set ruling. R1 decided it: a verb.

> **DECIDED: (a)** — R1 in the phase plan. The three shapes considered:
>
> a. `atm task start <task-id>` — a sixth verb. Most explicit; smallest
>    surprise; contradicts the five-verb ruling in letter only.
> b. An explicit harness/agent API call with no CLI verb — keeps the CLI at
>    five, but hides a lifecycle operation from the operator and from
>    `atm task` oversight.
> c. `atm task assign` self-targeted as an implicit start — overloads assign
>    with two meanings; rejected on RBP grounds unless Rand prefers it.
>
> The closed set was closed to delete *redundant* verbs; start stopped being
> redundant the moment ack left the task domain. (b) and (c) are not built.

The syntax is exactly `atm task start <task-id>`. No flags: the actor is the
caller, and the authority rule (D2a) is "the current assignee only".

Acceptance: concurrency (two starts racing), duplicate-start
idempotency, receipt-write failure and retry, and the crash boundary either
side of the receipt.

### D2. Reassignment — **R2 decided: (a)**

**SOLAR-BA-009 (BLOCKING), as originally filed against the design.**
Reassignment *was* specified as close-and-create,
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

> **DECIDED: (a)** — R2 in the phase plan, with its event semantics fixed
> there. The two shapes considered:
>
> a. **Same-id reassignment** — an atomic close/reopen that updates
>    `current_assignee`, resets queue position and reminder counters, and
>    retains the full event history under one id.
> b. **Old-id close + new-id assign**, with a mandatory successor task id
>    recorded in the close event.
>
> (a) matches "one logical task, one id", keeps one event history, and needs
> no successor link — whereas (b) reintroduces exactly the supersession
> linkage this phase deletes. Rand's *"if that means close one and create a
> new one, that is acceptable"* was permission, not a requirement, and it
> predates the single-row identity ruling. (b) is not built.

**The command syntax (PLAN-SCOPE-013, corrected by R2-CRIT-016).**
Reassignment is its **own verb**:

```
atm task reassign <task-id> --to <new-agent> --reason <text>
```

*An earlier revision made it `assign` onto an existing task id, selecting the
meaning by whether the id already existed. That was wrong and it contradicted
this sprint's own reasoning: D1 rejected shape (c), "`atm task assign`
self-targeted as an implicit start", on RBP grounds for overloading `assign`
with two meanings. An implicit mode switch keyed on row existence is the same
defect, and worse — it is invisible at the call site and races another writer
creating the id.*

The closed set is therefore **seven**: `assign`, `start`, `close`,
`reassign`, `move`, `list`, `events`. Seven closed verbs is not a budget
breach: the budget counts traits, capabilities, tables, state machines and id
types, and a clap variant is none of those. The five-verb figure was a design
preference, and it stopped being achievable the moment ack left the task
domain (R1) and reassignment became a first-class transition (R2).

- `reassign` on an **open** task → one `Reassigned` event
  carrying actor / `from_assignee` / `to_assignee` / reason, `current_assignee`
  updated, position recomputed in the new assignee's queue, reminder counters
  reset, all in one transaction. `state` never reaches `complete` and
  `close_outcome` stays `NULL`.
- `reassign` on a **closed** task → the existing stable error. Reopening is
  not in this phase.
- `reassign` on an **unknown** id → a distinct stable error, never a silent
  assignment.
- `--reason` is **required**. A handover with no recorded reason is the audit
  gap this deliverable exists to close.

Acceptance: legality from both `assigned` and `active`, concurrent
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
   not on a log line. **Whether it steers an Active assigner is BA.8's
   assertion, not this sprint's** (R2-CRIT-015): BA.8 owns deferred lifecycle
   delivery and follows this sprint, so an AC here would test behaviour that
   does not exist at this head.
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
    `close_outcome`. **The terminal-suppression reset on reassignment is
    BA.8's assertion** (R2-CRIT-015) — BA.8 D1 owns the reset predicate and
    follows this sprint. This sprint owns the persistence and the event; BA.8
    owns what the reminder loop then does with them.
13. `atm task` exposes exactly **seven** subcommands — `assign`, `start`,
    `close`, `reassign`, `move`, `list`, `events` — and the regenerated CLI
    surface baseline says so.
14. `atm task reassign` on an unknown id is a distinct stable error and never
    creates a task; on a closed id it is the existing stable error.
15. A reassignment with no `--reason` is rejected before any write.
16. `atm task assign` has **no** implicit reassignment behaviour: assigning
    onto an existing open id is rejected, not silently reinterpreted.

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
