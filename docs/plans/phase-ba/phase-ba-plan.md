<!--
Phase BA plan. Supersedes phase AZ.
Design source: Rand's rulings, 2026-09-10.
Every code claim in this document was verified against origin/develop and
origin/integrate/phase-az and is cited file:line or by SHA.
-->

# Phase BA: one invariant, one queue, one task command set

| Field | Value |
| --- | --- |
| Supersedes | Phase AZ — retired unmerged; branch retained, not deleted |
| Base | `develop` |
| Integration branch | `integrate/phase-ba` |
| Sprints | 7 |
| Widest parallel wave | 4 |

## Why this phase exists

Rand named two operational failures, verbatim:

> a) "agent stops for no acceptable reason."
>
> b) "team-lead/orchestrator interrupts current tasks in process which wreck
> context by diverting an agent to a different task/branch."

Every deliverable is judged against those two. Anything that does not serve
them is out of scope for this phase.

Phase AZ pursued the same goals by making ATM the authoritative task workflow
engine: a v2 task schema, immutable assignment attempts, an operations
idempotency namespace, supersession, and a durable attention scheduler with
lane cursors and reservations. Rand rejected that direction — *"I am really
concerned about how much this has become overcomplicated and an attempt to
make this task system the authoritative."*

The replacement is smaller than the problem looked. Of the seven state machines
that governed one nudge, two survive: roster state and task state.

A large fraction of what this phase delivers already exists in the codebase and
is either mis-wired or undocumented. See **Already built**.

## Binding outcomes

### B1. The invariant

> "Up until the point a task is complete the agent MUST keep working on it."

This is an invariant, not an edge. An idle agent holding an incomplete task is
out of compliance **whenever observed**, not only at the instant it became
true. There is no false→true predicate, no target generation, no revision
revalidation, and no edge coalescing.

| agent state | has incomplete task | action |
|---|---|---|
| idle | yes | nudge, subject to the rate limit |
| idle | no | nothing |
| active | any | **leave alone — never divert** (problem b) |
| blocked | any | escalate immediately, **zero nudges** |
| dead/offline | any | escalate immediately, **zero nudges** |

Blocked and dead are not "nudge less." A nudge cannot clear a permission prompt
or revive an exited process, so each one is noise that buries the reason.

### B2. The queue

`assigned` is queued; `active` is working; at most one active task per agent,
enforced by the database. Ordering is `(position, assigned_at, task_id)`.
Default position is end. `--head` means next up, never preemption.

### B3. Task authority

ATM resolves nothing but a task id. The task body lives in a markdown file, a
bead, or a ttl file; ATM never dereferences it. There is no provider trait, no
provider resolution, and no ambiguity taxonomy. ATM records identity,
assignment, state, position, and an append-only event log.

### B4. `ack` and task are disjoint

Ack is message hygiene and is never gated on task state. Task state and
notification go through `atm task`.

### B5. Two meanings of "blocked", never one word

| sense | belongs to | word |
|---|---|---|
| agent frozen on ask-user-question / permission-request | agent state | `blocked` |
| agent examined the task and cannot do it | task outcome | `refused` |

The codebase already owns `Blocked`/`MemberBlocked` for the first sense.
Reusing the word for the second guarantees the conflation returns.

## Why AZ is retired rather than salvaged

AZ is 158 commits, 237 files, 18,979 insertions across four merged sprints
(AZ.1 nudge contract, AZ.2 task domain storage, AZ.3 task command handoff,
AZ.4 attention scheduler).

- Of roughly 8,000 lines in the task and attention subsystems, about 5,700 are
  deleted outright under this design.
- AZ.3 built an `atm task` namespace with **eleven** verbs — List, Events,
  Assign, Start, Block, Unblock, Reassign, Reopen, Complete, Fail, Abort. The
  closed set here is **five**. Every extra verb is one Rand explicitly ruled
  out; it carries `PriorityArg`, which queue position replaces; and it lacks
  `move`, the one verb needed.
- There is no clean fork point: AZ.3's CLI is wired to
  `AsyncTaskMutationStore` from AZ.2, the schema being deleted.
- AZ persists `STORAGE_SCHEMA_VERSION` 2.0.0 then 2.1.0 as an **ADR-061 major**
  change, obligating the v1 bridge through every 1.6.x release, a retained
  1.5.14 rollback fixture, and a separate approval to remove it in 1.7.0.
  Because **zero v2 tables exist on `develop`**, retiring AZ un-commits all of
  that at no cost. Keeping any part of AZ inherits the obligation.

A survey by file area, by commit subject, and by spot-check of the four least
task-shaped files found no further unrelated work worth rescuing.

Phase AZ plan documents and `.triage/phase-az` records are history and are
retained unchanged.

## Sprint sequence

Ordered so the live defects and Rand's two problems land first, and so the
widest possible wave runs in parallel.

| sprint | title | wave | recommended |
|---|---|---|---|
| BA.1 | Ack/task separation (salvage) | 1 | Cipher-311d / fast |
| BA.2 | Nudge title metadata (salvage evaluation) | 1 | Cipher-311d / fast |
| BA.3 | Task identity, one-active, queue position | 2 | arch-ctm / deep-reasoning |
| BA.4 | Continuous evaluation of the nudge invariant | 1 | arch-ctm / deep-reasoning |
| BA.5 | The `atm task` closed command set | 3 | arch-ctm / deep-reasoning |
| BA.6 | Ephemeral queued messages | 4 | arch-ctm / deep-reasoning |
| BA.7 | Documentation and CLAUDE.md correction | 1 | Cipher-311d / fast |
| BA.8 | Escalation terminality and lifecycle delivery | 3 | arch-ctm / deep-reasoning |

`recommended_agent` / `recommended_model` are advice, not assignment.

### Wave plan

```
wave 1   BA.1   BA.2   BA.4   BA.7        (four in parallel)
wave 2   BA.3                              (must_follow BA.1)
wave 3   BA.5   BA.8                       (both must_follow BA.3)
wave 4   BA.6                              (must_follow BA.5, BA.8)
```

## Dependency relations

| sprint | relation | rationale |
|---|---|---|
| BA.1 | `parallel_safe` with BA.2, BA.4, BA.7 | owns `atm-storage-rusqlite/src/writer/*` and `atm-storage/src/task_state.rs`; no other wave-1 sprint touches either |
| BA.2 | `parallel_safe` with BA.1, BA.4, BA.7 | owns `atm-core` nudge-title surface and `boundaries/*` title manifests; disjoint from storage and from `atm-http-runtime` |
| BA.4 | `parallel_safe` with BA.1, BA.2, BA.7 | owns `atm-http-runtime/src/herdr_*`; touches no storage schema and no CLI |
| BA.7 | `parallel_safe` with all of wave 1 | documentation only; no code, no tests, no boundary manifests |
| BA.3 | `must_follow` BA.1 | BA.1 is the sole owner of `task_state.rs` ack semantics; BA.3 then changes identity in the same file. Merge-forward trigger: BA.1 development pushed, not QA. |
| BA.5 | `must_follow` BA.3 | the command set writes the `outcome` and `position` columns that BA.3 creates |
| BA.8 | `must_follow` BA.3, `must_follow` BA.4 | serializes on the logical `(team, task_id)` identity BA.3 creates and derives the refusal streak from BA.3's typed outcome; BA.4 owns the reminder path it extends |
| BA.5 / BA.8 | `parallel_safe` with each other | BA.5 owns `crates/atm/src/commands/*`, BA.8 owns `crates/atm-http-runtime/src/herdr_*`; the one shared file is `atm-core/src/send/mod.rs`, and only BA.8 edits it |
| BA.6 | `must_follow` BA.5, `must_follow` BA.8 (PR-completion) | needs BA.5's close semantics and BA.8's escalation for the non-starvation rule |

`parallel_safe` claims above are made on non-intersecting crates, files,
public contracts, and boundary manifests. `plan-scope-reviewer` verifies them.

## Rulings required before wave 3 opens

Solar's critical review surfaced two questions that the design does not
answer and that a dev agent must not answer on its own. **BA.5 does not open
until both are recorded here.**

### R1 — the explicit start operation (SOLAR-BA-004)

Once BA.1 deletes ack-driven activation, nothing on develop moves a task from
`assigned` to `active`: `TaskEvent::Acked` was the only input
(`task_state.rs:97-109`). Without an explicit start, tasks are worked while
still `assigned`, the `one_active_task_per_agent` index never arbitrates, and
the start receipt Rand asked for has no event to ride on. No implicit
substitute works — roster Active cannot say *which* task began, and nudge
delivery is not work start.

| option | effect |
| --- | --- |
| **a. `atm task start <task-id>`** | a sixth verb; most explicit; contradicts the five-verb ruling in letter only — **recommended** |
| b. explicit harness/agent API call, no CLI verb | keeps the CLI at five; hides a lifecycle operation from the operator |
| c. self-targeted `atm task assign` | overloads assign with two meanings; rejected on RBP grounds unless Rand prefers it |

The closed set was closed to remove *redundant* verbs. Start stopped being
redundant the moment ack left the task domain.

### R2 — reassignment identity (SOLAR-BA-009)

Reassignment as close-and-create, ids never reused, and unique
`(team, task_id)` cannot all hold at once. Reusing the id collides with the
retained `complete` row; minting a new one leaves the `reassigned` outcome
with no successor link, so oversight cannot follow the handover — on exactly
the escape path Rand named ("team-lead can assign to another agent if he
KNOWS").

| option | effect |
| --- | --- |
| **a. same-id reassignment** | atomic close/reopen; updates `current_assignee`, resets position and reminder counters, retains one event history — **recommended** |
| b. old-id close + new-id assign | needs a mandatory successor id in the close event, which reintroduces the supersession linkage this phase deletes |

Rand's *"if that means close one and create a new one, that is acceptable"*
was permission, not a requirement, and it predates the single-row identity
ruling.

## Complexity budget — the hard ceiling

Phase AZ was retired for adding structure, not for being wrong. This phase
inherits an explicit ceiling, and it is a **merge gate**, not an aspiration.
Rand: *"I don't want to re-add complexity."*

| structure | phase AZ | phase BA — ceiling |
| --- | --- | --- |
| new sealed capability traits | 3 | **0** |
| new semantic storage capabilities | 2 (11th, 12th) | **0** |
| new tables | v2 tasks, v2 task_events, task_operations, attention_lane_cursors, attention_opportunities | **0** |
| new id types | `TaskOperationId` | **0** |
| new state machines | attention scheduler, assignment attempts, supersession, breaker maps, two-lane fair selector | **0** |
| `STORAGE_SCHEMA_VERSION` | 2.0.0 MAJOR + bridge through all 1.6.x + retained 1.5.14 fixture + 1.7.0 removal ADR | **one MINOR with migration** |
| `atm task` verbs | 11 | **5**, or 6 if Rand rules R1(a) |
| priority / reordering model | `TaskPriority` with `rank()` | one integer `position` column |

What BA *does* add, in full — this list is exhaustive and any addition beyond
it needs a written ruling:

- `tasks`: key narrowed to `(team, task_id)`; `assignee` becomes
  `current_assignee`; two new columns (`position`, typed close `outcome`)
- `task_events`: key narrowed to `(team, task_id, seq)`; `assignee` demoted
  from identity to data
- two indexes: `one_active_task_per_agent`, and position uniqueness per open
  assignee
- columns on the existing message row so a deferred item survives handoff
  (BA.6 D1) — no new table
- `--limit` and a cursor on two read commands (BA.5 D5a)
- concurrency fences implemented as a per-member CAS and per-task
  serialization at existing boundaries — no generation counters, no
  reservations, no scheduler state

**The one open door**, and it is deliberately narrow: BA.8 D5 option (b) may
propose persisted escalation-episode identity. The recommendation is option
(a), which adds nothing. If a sprint reaches for (b), it needs Rand's sign-off
with the justification that deliverable demands.

Solar's twenty-two findings raised the *rigour* of this phase — preflights,
fences, authorization, bounded reads — without moving any row of the table
above. That was the constraint the review was run under and it held.

## Critical review — solar, 2026-09-11

Solar reviewed the design document against `origin/develop` and the live
atm-dev ledger and returned twenty-two findings. Every code citation was
independently re-verified before folding it in; all twenty-two held. Three of them
falsified claims in the design document itself, which have been corrected at
source rather than worked around.

| id | sev | disposition |
| --- | --- | --- |
| SOLAR-BA-001 | BLOCKING | BA.4 D1 — bounded per-team snapshot, scale + slow-read tests |
| SOLAR-BA-002 | BLOCKING | BA.3 D5 — no universal merge rule; preflight classifies and aborts |
| SOLAR-BA-003 | BLOCKING | BA.3 D6 — `task_events` identity and sequence become task-global |
| SOLAR-BA-004 | BLOCKING | **R1 ruling** + BA.5 D6; BA.1's start receipt withdrawn |
| SOLAR-BA-005 | IMPORTANT | design §2 corrected — blocked handling already exists; BA.4 D3 scoped to verify + add the Offline arm |
| SOLAR-BA-006 | IMPORTANT | design §6.2 corrected — lead and recipients are independent; BA.8 D4 |
| SOLAR-BA-007 | BLOCKING | BA.6 D1 / D2a — handoff durability, post-handoff cadence, non-starvation |
| SOLAR-BA-008 | BLOCKING | BA.8 D1 — suppression only after durable escalation; narrow reset |
| SOLAR-BA-009 | BLOCKING | **R2 ruling** + BA.5 D7 |
| SOLAR-BA-010 | IMPORTANT | BA.4 D3a — six-variant disposition matrix |
| SOLAR-BA-011 | IMPORTANT | BA.8 D3 — refusal streak from durable events, not `release_streaks` |
| SOLAR-BA-012 | BLOCKING | BA.5 D2 — three-stage close: preflight, deliver, idempotent close |
| SOLAR-BA-013 | IMPORTANT | BA.3 D4 — close outcome is its own type, not `ReminderOutcome` |
| SOLAR-BA-014 | IMPORTANT | BA.3 D3 — position invariants enforced in the database |
| SOLAR-BA-015 | BLOCKING | BA.8 D5 — episode identity, or an explicitly weakened guarantee |
| SOLAR-BA-016 | IMPORTANT | BA.8 D6 — lifecycle notifications are deferred, never immediate |
| SOLAR-BA-017 | BLOCKING | BA.4 D5a fence 1 — roster CAS at the effect boundary |
| SOLAR-BA-018 | BLOCKING | BA.8 D2 — task-side fence on close and reassign |
| SOLAR-BA-019 | IMPORTANT | design §3 corrected — PR #1381 already stopped mirror creation; BA.3 reframed around existing debt |
| SOLAR-BA-020 | IMPORTANT | BA.5 D5a — bounded read projection pushed into SQL |
| SOLAR-BA-021 | BLOCKING | BA.5 D2a — authority matrix replaces the scoping the key narrowing removes |
| SOLAR-BA-022 | BLOCKING | BA.8 D4a — escalation notices are `requires_ack`; the promise is bounded to custody, not resolution |

Findings 004 and 009 are **not** closed by a sprint deliverable. They require
Rand's ruling first; see the section above.

Four of the eighteen (008, 015, 017, 018) share one root cause worth naming:
the design deleted edge detection, correctly, but edge detection was also
doing the work of a concurrency fence. Continuous evaluation still requires
its inputs to hold **at the effect boundary**. That is invariant validation,
not edge tracking, and it is now explicit in BA.4 D5a and BA.8 D2.

## Worktrees and the gh stack

One integration branch, created first, off `develop`:

```bash
/sc-git-worktree --create integrate/phase-ba develop
```

Every sprint worktree is created from `integrate/phase-ba`, never from `main`
and never from `develop`:

```bash
/sc-git-worktree --create feature/ba1-ack-task-separation      integrate/phase-ba
/sc-git-worktree --create feature/ba2-nudge-title-salvage      integrate/phase-ba
/sc-git-worktree --create feature/ba4-nudge-invariant          integrate/phase-ba
/sc-git-worktree --create docs/ba7-task-nudge-documentation    integrate/phase-ba
/sc-git-worktree --create feature/ba3-task-identity-queue      integrate/phase-ba
/sc-git-worktree --create feature/ba5-atm-task-commands        integrate/phase-ba
/sc-git-worktree --create feature/ba6-ephemeral-queued-messages integrate/phase-ba
```

### The `must_follow` chain is one gh stack

BA.1 → BA.3 → BA.5 → BA.6 form a dependency chain, so they are linked as a
single stack rooted on the integration branch rather than four independent PRs:

```bash
gh stack init --base integrate/phase-ba \
  feature/ba1-ack-task-separation \
  feature/ba3-task-identity-queue \
  feature/ba5-atm-task-commands \
  feature/ba6-ephemeral-queued-messages
gh stack submit --auto
```

Layers are added to the stack **when their PR opens**, not held for CI.

### The parallel sprints are not stacked

BA.2, BA.4 and BA.7 are independent branches with ordinary PRs targeting
`integrate/phase-ba`. Stacking them would impose an order the dependency
analysis says does not exist, and would force a rebase of unrelated work every
time a lower layer moves.

BA.6 depends on BA.4 by **PR completion**, not by branch ancestry: BA.4 merges
to `integrate/phase-ba` during wave 1, long before BA.6 opens in wave 4.

### Stack discipline

- Read stack state with `/gh-stack-view`; never fan out `gh pr view` per branch.
- Only the stack owner runs `gh stack sync` / `gh stack rebase`, and only when
  `/gh-stack-view` shows `origin ok` on every layer.
- Nobody pushes to `integrate/phase-ba` while a stack sync targeting it is in
  flight.
- Merge with a merge commit; never squash.

## Salvage from phase AZ

Salvage is a sprint deliverable with named SHAs, not an informal reference.
All three commits were checked for coupling to the deleted v2 and attention
types; the count of added lines referencing `_v2`, `tasks_v`, `TaskLifecycle`,
`AsyncTaskMutation`, `TaskOperationId`, `assignment_attempt` or `attention` is
**zero** in each.

| SHA | subject | scope | sprint |
|---|---|---|---|
| `c99664acc` | `fix(az3): keep task acknowledgements mail-only` | 2 files, +1 / −43 | BA.1 |
| `fc81d97cc` | `fix(nudge): dual-write bounded title metadata` | `atm-core` boundary/graft/router, 3 boundary manifests, ADR-054 | BA.2 |
| `4682bbbc0` | `fix(nudge): project persisted titles only` | `atm-core` nudge dispatch/template/hook, graft-python | BA.2 |

Copy-by-hand, not cherry-picked:

| what | from | sprint |
|---|---|---|
| `PRIMARY KEY (team, task_id)` | `schema_version.rs` on `integrate/phase-az` | BA.3 |
| `CREATE UNIQUE INDEX one_active_task_per_agent` | `schema_version.rs:284-285` on `integrate/phase-az` | BA.3 |
| `commands/task.rs`, `task_command/service.rs` | AZ.3 | BA.5, **reading reference only — never merged** |

Cherry-pick cleanliness is a sprint task, not a plan assertion. Each salvage
deliverable has independent acceptance so one can be dropped without the
sprint claiming false success.

## Already built — do not design again

Verified present on `origin/develop`:

- **escalation recipients**: `escalation_recipients (scope_key, address,
  added_at)`; `EscalationScope { Daemon, Team(TeamName) }`;
  `MAX_ESCALATION_RECIPIENTS = 8`; `atm escalation add|remove|list`. Addresses
  are free-form text, so `omega-prime@hermes` works today and a future
  `rand@human` needs no schema change.
- **escalation targets**: `EscalationTargets { lead: Option<AgentName>,
  recipients: Vec<String> }` (`herdr_escalation.rs:204-207`). No oversight
  account configured means lead only; no lead means nobody — accepted by Rand,
  not a defect.
- **doctor warnings** `RosterNoLead` and `RosterMultipleLeads`
  (`doctor/ax6.rs:171-192`).
- **blocked-agent escalation**: `EscalationKind::BlockedEscalated`,
  `blocked_since`, `last_blocked_notice`, `BLOCKED_RENOTIFY_MS`.
- **agent blocked state** end to end: `HerdrAgentStatus`,
  `HerdrError::AgentBlocked`, `RuntimeMemberState::Blocked`,
  `AtmErrorCode::MemberBlocked`.
- **the deferred delivery path**: `atm queue` is `NudgeMode::Deferred`.
- **task query surface**: `atm list --tasks`, `atm list --task-events <id>`.
- **task rows are never deleted**: no `DELETE FROM tasks`, no
  `DELETE FROM task_events`, no retention or prune path.

Future, explicitly **not** this phase: whitelist/blacklist filtering on the
recipients table for enterprise multi-human oversight.

## What this phase deletes

Nothing in this list ships on `develop`, so none of it is a removal migration
and none of it requires an ADR-061 removal review.

- the attention scheduler entirely: `attention_opportunities`,
  `attention_lane_cursors`, reservations, lane cursors, dispositions,
  `PermanentlyFailed`, the prune path
- assignment attempts, `current_attempt`, `reassign_v2`
- `task_operations` and the `TaskOperationId` idempotency namespace
- supersession
- `breaker_escalation_gates`, `breaker_cycle_opened_at`,
  `breaker_failure_counts`, `EscalationState.blocked_cursor`
- the two-lane message/task scheduler and its fairness cursor
- `admit()`'s ack refusal
- `TaskPriority` — replaced by queue position

`EscalationState.blocked_since` and `.last_blocked_notice` are **retained**;
they implement the blocked-agent escalation this phase requires.

## ADR-061 governed interfaces

BA.3 is the only governed-interface change: additive columns plus a
primary-key narrowing on a table whose v2 form has never shipped. It is
classified **minor with a migration**, not the major classification AZ carried.

Retiring AZ withdraws the 2.0.0 major classification, the full-1.6.x bridge
obligation, the retained 1.5.14 rollback fixture, and the 1.7.0 removal ADR —
none of which shipped.

`schema-reviewer` must confirm this classification in plan review, before BA.3
opens.

## Architecture boundary

- The frozen synchronous daemon is not touched. Composition is through the
  Tokio/Axum runtime and backend-neutral traits.
- The nudge path consumes exact `RuntimeMemberState`. It must **not** consume
  `PickerMemberStatus`, which collapses
  `Offline | Unknown | IdentityConflict | Blocked` to `Dead`
  (`picker_projection.rs:76`) and destroys the distinction that decides
  remediation.
- "Blockers not satisfied" is a dependency statement and is the natural seed
  for a dependency graph inside ATM. A refusal reason is a typed outcome plus
  human-readable text. It is **never** an input the scheduler reasons over.
- No new sealed storage capability trait is authorised by this phase.

## Phase acceptance

1. An idle agent holding an incomplete task is nudged; a busy agent is never
   diverted; a blocked or dead agent is escalated once and never nudged.
2. No task can accumulate unbounded reminders. A stalled task produces one
   escalation and stops.
3. One agent holds at most one active task, enforced by the database.
4. One task id is one row; the phantom mirror is unrepresentable.
5. Team-lead can place a task anywhere in an agent's queue without rewriting
   assignment timestamps.
6. A close that fails never destroys the accompanying report.
7. Agent state and task state are both queryable.
8. `atm queue` is documented and CLAUDE.md no longer steers task assignment
   into the interrupting path.
9. No task can be closed, moved, or started by an agent with no authority
   over it.
10. **The complexity budget above is met exactly.** Gate at phase end: zero
    new tables, zero new sealed traits, zero new semantic capabilities, zero
    new state machines, and one MINOR schema bump. A sprint that needs more
    stops and asks.
