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
| Sprints | 12 |
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
| BA.0 | Normative ADR decisions, before any implementation | 0 | Cipher-311d / fast |
| BA.1 | Ack/task separation (salvage) | 1 | Cipher-311d / fast |
| BA.2 | Nudge title metadata (salvage evaluation) | 1 | Cipher-311d / fast |
| BA.3 | Task identity, one-active, queue position | 2 | arch-ctm / deep-reasoning |
| BA.4 | Continuous evaluation of the nudge invariant | 1 | arch-ctm / deep-reasoning |
| BA.5 | The `atm task` closed command set | 3 | arch-ctm / deep-reasoning |
| BA.6 | Ephemeral queued messages | 4 | arch-ctm / deep-reasoning |
| BA.7 | Documentation and CLAUDE.md correction | 1 | Cipher-311d / fast |
| BA.8 | Escalation terminality and lifecycle delivery | 5 | arch-ctm / deep-reasoning |
| BA.9 | Lifecycle operations: start and reassign | 4 | arch-ctm / deep-reasoning |
| BA.10 | Task and event migration | 3 | arch-ctm / deep-reasoning |
| BA.11 | Final code-verified documentation and consistency sweep | 6 | Cipher-311d / fast |

`recommended_agent` / `recommended_model` are advice, not assignment.

### Wave plan

```
wave 0   BA.0                               (ADR decisions; blocks every code sprint)
wave 1   BA.2 -> BA.4     BA.1     BA.7   (BA.1/BA.7 parallel; BA.4 follows BA.2)
wave 2   BA.3                              (must_follow BA.1 and BA.4)
wave 3   BA.5   BA.10                      (both must_follow BA.3)
wave 4   BA.6   BA.9                       (BA.6: BA.5+BA.4; BA.9: BA.5+BA.3)
wave 5   BA.8                               (must_follow BA.3, BA.4, BA.5, BA.6)
wave 6   BA.11                              (must_follow everything)
```

**BA.0 exists because BA.11 was the wrong place for normative decisions
(R2-CRIT-004).** ADR-054 says later sprints *implement* its contract and never
redefine it; ADR-061 D3 requires MAJOR approval **before plan approval**. An
amendment written after the code lets the implementer define the contract, and
an ADR-064 marked `Accepted` after the migration records approval for
something irreversible that already happened. So the phase splits in two:
**BA.0 decides**, every code sprint conforms, **BA.11 verifies** what shipped
against the decision and writes the user-facing documentation. BA.11 keeps no
normative content.

The diagram is a summary; the dependency table below is authoritative.

**BA.8 moved from wave 3 to wave 5 (PLAN-CRIT-012, PLAN-CRIT-015).** The
earlier plan had BA.6 follow BA.8 *and* BA.8 depend on behaviour only BA.6
creates — a cycle. It is broken in the only direction that works: BA.6's
message-row state and remindability are the substrate, BA.8's escalation
delivery is the consumer. BA.8 also stopped being `parallel_safe` with BA.5:
it must serialize with BA.5's mutations and test BA.5's receipts.

## Dependency relations

| sprint | relation | rationale |
|---|---|---|
| BA.1 | `parallel_safe` with BA.2, BA.4, BA.7 | owns `atm-storage-rusqlite/src/writer/*` and `atm-storage/src/task_state.rs`; no other wave-1 sprint touches either |
| BA.2 | `parallel_safe` with BA.1, BA.7 | owns the `atm-core` nudge-title surface and the title boundary manifests; disjoint from storage |
| BA.4 | `parallel_safe` with BA.1, BA.7; **`must_follow` BA.2** | both edit `crates/atm-http-runtime/src/storage_and_nudge_router.rs` — BA.2's cherry-picks touch it and BA.4 D5a adds the fence at `:644-692`. PLAN-SCOPE-002; they are not parallel-safe. BA.2 first: its hunks are mechanical, BA.4's fence should sit on top. Merge-forward trigger: BA.2 development pushed. |
| BA.7 | `parallel_safe` with all of wave 1 | documentation only; no code, no tests, no boundary manifests |
| BA.3 | `must_follow` BA.1 | BA.1 is the sole owner of `task_state.rs` ack semantics; BA.3 then changes identity in the same file. Merge-forward trigger: BA.1 development pushed, not QA. |
| BA.3 | **`must_follow` BA.4** | both edit the selection code in `crates/atm-http-runtime/src/herdr_queue_wake.rs`: BA.4 replaces edge detection with continuous evaluation there, BA.3 changes what selection reads (`position`, logical identity). BA.4 first — BA.3 should land on the new evaluation shape, not be rewritten by it. PLAN-CRIT-020. This row was missing while BA.3's header asserted the dependency (PLAN-SCOPE-010); the table is authoritative, so the table is where it has to be. *(Round 2 re-cited this as PLAN-CRIT-002 — wrong; 002 is BA.5's mutation boundary, which was separately unaddressed. R2-CRIT-019.)* |
| BA.5 | `must_follow` BA.3 | the command set writes the `outcome` and `position` columns that BA.3 creates |
| BA.8 | `must_follow` BA.3, BA.4, **BA.5**, **BA.6** | BA.3 gives the logical `(team, task_id)` identity and the typed outcome the refusal streak reads; BA.4 owns the reminder path it extends; **BA.5** because BA.8 serializes with BA.5's mutations and its AC tests BA.5's receipts (PLAN-CRIT-015); **BA.6** because AC11's read-without-ack remindability is BA.6's deliverable (PLAN-CRIT-012) |
| BA.5 / BA.8 | ~~`parallel_safe`~~ **withdrawn** | file ownership is still disjoint (BA.5 `crates/atm/src/commands/*`, BA.8 `crates/atm-http-runtime/src/herdr_*` and `atm-core/src/send/mod.rs`), but disjoint files are not sufficient when one sprint's acceptance criteria assert the other's behaviour. PLAN-CRIT-015 |
| BA.10 | `must_follow` BA.3 | migrates the data onto the schema BA.3 establishes. Split from BA.3 (PLAN-SCOPE-005): a wrong schema is fixable by a follow-up commit, destroyed production history is not. PR-completion trigger. |
| BA.10 | ~~`parallel_safe` with BA.5, BA.8~~ **withdrawn** | BA.3+BA.10 are one deployable unit (R2-CRIT-002); nothing may consume the new columns until BA.10 has merged, so BA.5 and BA.8 sit above BA.10, not beside it |
| BA.9 | `must_follow` BA.5, `must_follow` BA.3 | adds the start and reassign operations to the command surface BA.5 creates. Split from BA.5 (PLAN-SCOPE-004) so seven unrelated deliverables stop waiting on two human rulings. **Gated per half**: the start half on R1, the reassign half on R2. |
| BA.6 | `must_follow` BA.5, `must_follow` BA.4 (PR-completion) | needs BA.5's close semantics and BA.4's invariant evaluation. **The former dependency on BA.8 is reversed** (PLAN-CRIT-012): the non-starvation rule is now normative in BA.6 D4 and needs nothing from BA.8, while BA.8 does need BA.6 |
| BA.6 / BA.9 | `parallel_safe` with each other | both in wave 4, both downstream of BA.5, and disjoint: BA.9 owns the `atm task` clap enum and `crates/atm/src/commands/task*`, BA.6 owns `mail_message_states` and the scheduling pass in `crates/atm-http-runtime/`. Neither's acceptance criteria assert the other's behaviour — the test PLAN-CRIT-015 established. BA.9 is a stack layer, BA.6 is an independent branch (PLAN-SCOPE-011) |
| BA.8 | **`must_follow` BA.9** | AC9 exercises the **start** receipt, which BA.9 introduces; BA.5 D6/D7 moved `start` out of BA.5. Wave order already sequences them, but an undocumented dependency is what produced the BA.6/BA.8 cycle once already. PLAN-SCOPE-016 |
| BA.0 | blocks **every** code sprint | it carries the ADR-054 and ADR-062 amendments. No sprint may implement a contract its ADR has not yet decided (ADR-054's "later sprints implement, never define"). The **migration decision is not here** — ADR-061 D3's "before plan approval" puts ADR-064 in this PR. Doc-only, so it is one short wave, but it is a hard gate, not a formality |
| BA.11 | `must_follow` every other sprint (PR-completion) | it records what shipped. An amendment written ahead of the code is a prediction; phase AZ produced several that then disagreed with the merge. Doc-only, so it costs one short wave |

`parallel_safe` claims above are made on non-intersecting crates, files,
public contracts, and boundary manifests. `plan-scope-reviewer` verifies them.

## Rulings

Three questions the design did not answer, plus one that turned out not to be
mine at all — the task identity storage change, now `ADR-064` in this PR. Rand's standing instruction is that
he is not in iteration loops — *make the call, record it, report the outcome* —
and his 2026-09-11 direction was to fix all reviewer findings and iterate to
clean. All three are therefore **decided here by fenix and reversible by Rand**,
not held open.

### R0 — the task identity storage change — **DECIDED BY ADR-064, IN THIS PR**

> **R0 is no longer a ruling in this plan. It is `docs/adr/ADR-064-phase-ba-task-identity-storage-change.md`, shipped in this PR as `Proposed`, awaiting Rand's decision on its D4.**
>
> Three things moved it there:
>
> 1. **ADR-061 D3 requires the approval "before plan approval."** An ADR
>    written during the phase cannot satisfy that, so deferring it to BA.0 was
>    still too late. Rand's observation — *"I looked at pr, there are no adr in
>    the pr"* — is the defect: a plan that changes a governed interface and
>    carries no ADR has nothing for `schema-reviewer` to approve.
> 2. **ADR-061 D3 forbids the shape I ruled.** *"No change may require every
>    host to upgrade together."* One-way, no bridge, old-binary-refuses is
>    exactly that.
> 3. **The version gate I promised cannot exist.** ADR-061 D1:
>    *"`STORAGE_SCHEMA_VERSION` does not exist yet"*, confirmed by
>    `git grep` on `origin/develop`. A binary predating the constant cannot
>    check it.
>
> **And the premise underneath all of it was wrong.** ADR-061 D3 obliges the
> author to show why a capability cannot be expressed additively. I asserted
> it could not and never did the work. ADR-064 D2 does it, and an additive
> shape exists: keep both primary keys, add `current_assignee` / `position` /
> `close_outcome` as defaulted columns, and enforce task-id uniqueness and
> one-active-per-agent **in the writer** — the same layer where PR #1381
> already fixed the mirror — with a `doctor` check and tests. That is ADR-061
> MINOR, needs no bridge, no exception, no prerequisite release, and leaves
> nothing in the phase irreversible.
>
> Its cost is real and is stated in ADR-064: uniqueness stops being
> schema-enforced. Contiguity and active-at-position-1 were already in that
> category because SQLite cannot express them, so this moves one more
> invariant across a line the plan had already crossed.
>
> **ADR-064 recommends the additive shape (option i).** Until Rand decides
> D4, BA.3 and BA.10 do not open and the sections below are provisional.



**Decision: accept MAJOR. One-way migration with a version gate. No bridge.**

The earlier claim of "one MINOR with migration" was wrong and solar was right
to block it. ADR-061 classifies as major "removing or **renaming** a field,
variant, route, **table**" and any change the previous binary cannot operate
against. BA.3 renames `assignee` to `current_assignee`, narrows two primary
keys, and makes a previously legal multi-row write illegal. That is major under
the ADR's own words, and no amount of framing changes it.

What that does **not** mean is that BA re-acquires AZ's cost. AZ's expense was
never the major label — it was the *coexistence strategy*: canonical v2 tables
beside retained v1 tables, a bidirectional dual-write bridge retained through
every 1.6.x release, compatibility triggers, a retained 1.5.14 binary fixture,
and a 1.7.0 removal ADR. That is two authorities for one fact, which is the
defect this phase exists to delete.

BA takes the other shape:

  - one-way migration, no v1 objects retained, no bridge, no triggers
  - the schema version gates: a binary older than the migration **refuses to
    open** the database with a clear message, rather than silently operating
    on a shape it does not understand
  - rollback is restore-from-backup, not a supported dual-write window

Consequences, stated rather than hidden: this is a **breaking upgrade**. Every
host upgrades its daemon and CLI together, and a downgrade needs a database
restore. That is acceptable for a tool whose entire deployment is Rand's own
hosts; it would not be for a shipped product with external operators. **If Rand
wants a supported downgrade path, this decision is wrong and must be revisited
before BA.3 opens** — it is the one decision here with a cost he may weigh
differently than I do.

Requires Rand's explicit recorded approval and sign-off per ADR-061, plus
`schema-reviewer` sign-off. Recorded as an ADR in BA.11.

### R1 — the explicit start operation (SOLAR-BA-004)

**Decision: option (a), `atm task start <task-id>`.** A sixth verb.

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

**Decision: option (a), same-id reassignment**, with the event semantics fixed
below (PLAN-CRIT-025). *(Round 2 briefly deleted this citation as a
mis-reference. That was wrong — PLAN-CRIT-025 is exactly this finding. The
citation is restored; see R2-CRIT-019.)*

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

**The event semantics, fixed (PLAN-CRIT-025).** "Atomic close/reopen" was not
a specification and two implementations could both claim to satisfy it with
different audit truth. Reassignment is **one** transition, not a close
followed by an assign:

  - a single `Reassigned` event carrying `actor`, `from_assignee`,
    `to_assignee`, and a reason
  - the row's `state` never passes through `complete`, so no terminal
    suppression fires and no completion is recorded for work that did not
    complete
  - `current_assignee` is updated, `position` is recomputed in the new
    assignee's queue, and reminder counters reset in the same transaction
  - `close_outcome` stays `NULL` — the task is still open. `Reassigned` as a
    *close outcome* applies only when a task is closed in favour of a
    different task id, which R2(a) makes unnecessary — so **`Reassigned` is
    not a `TaskCloseOutcome` variant at all**. BA.3 D4 originally shipped one;
    under this ruling no code path could ever write it (PLAN-SCOPE-014), and
    the complexity budget forbids shipping an unreachable value. It is
    removed from the enum and from the SQL `CHECK`.

Silently changing `current_assignee` without the event is explicitly
forbidden: it loses the actor and the handover history.

### R3 — team-lead authority over another member's task (PLAN-SCOPE-015)

BA.5 D2a shipped an authority matrix and then left one row open in prose:
whether the team lead is *additionally* authorized for `cancel` / `reassign` /
`move` on a task assigned to someone else. That is a gating question of the
same kind as R0–R2, and leaving it as an assumption buried in a sprint
deliverable meant a reader of this section would believe nothing was
outstanding.

**Decision: yes.** The team lead is authorized for `cancel`, `reassign` and
`move` on any task within their team, in addition to the assigner. Rand named
this shape directly — *"team-lead can assign to another agent if he KNOWS"* —
and the escape path is worthless if the lead cannot take a task away from an
agent that has stalled. The assignee's authority is unchanged: `start`,
`close completed`, `close refused`.

The narrow cost, stated so Rand can reverse it: a lead can cancel work they
did not assign, and the ledger records the lead as actor. That is auditable,
not silent. If Rand wants lead authority limited to `reassign` only — taking
work away but never destroying it — this ruling is wrong and BA.5 D2a's
matrix changes before the sprint opens.

## Complexity budget — the hard ceiling

**The exhaustive additions list lives in BA.0 D4.** The budget below counts
traits, capabilities, tables, state machines and id types — all zero. Several
deliverables add *types and fields*, which are none of those and were
accumulating unrecorded: `TaskCloseOutcome`, `TaskOp`, `WriteRequest.task_op`,
`AsyncTaskLedgerReader::recent_closed_tasks`, the runtime `escalation_epoch`
field, and `mail_message_states.delivery_mode` / `handed_off_at`. Anything not
on BA.0's list at phase end is a breach, and a sprint that needs a new entry
amends that list before it builds.


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
| `STORAGE_SCHEMA_VERSION` | 2.0.0 MAJOR **+ dual-write bridge through all 1.6.x + retained 1.5.14 fixture + 1.7.0 removal ADR** | one MAJOR, **one-way, no bridge, no dual authority** |
| `atm task` verbs | 11 | **5**, or 6 if Rand rules R1(a) |
| sprints | 4 merged of an open-ended set | 10 |
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

Sprint count rose 7 → 10 under review (BA.4 split to BA.8; BA.3 split to
BA.10; BA.5 split to BA.9). That is sprint **granularity**, not product
complexity: no row of the table above moved, and each split separates two
different failure modes rather than adding work.

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

## Plan review — solar, 2026-09-11 (second pass)

Solar then reviewed **this plan** as `critical-plan-reviewer` and returned
twenty-five findings. As with the design pass, every citation was re-verified
before it was folded in; none were rebutted. The dispositions:

| id | disposition |
| --- | --- |
| PLAN-CRIT-001 | BA.3 classified MINOR though it renames `assignee` and narrows two primary keys → **R0**, now reopened and escalated |
| PLAN-CRIT-002 | BA.5's command surface cannot reach backend-neutral task mutation → **still open until this round**; see BA.5 D0a. My round-1 catalogue mis-mapped this id and the finding went unaddressed (R2-CRIT-019) |
| PLAN-CRIT-003 | phase BA changes nearly every normative ADR-062 decision → **BA.0 D1** (was BA.11 D1; moved earlier by R2-CRIT-004) |
| PLAN-CRIT-004 | BA.6 redefines ADR-054's durable queue contract → **BA.0 D2** + BA.6 D1's named columns |
| PLAN-CRIT-005 | BA.10 D5 classified the mirror pattern mergeable then commanded abort on any multi-assignee group → three-class preflight table |
| PLAN-CRIT-006 | contiguity claimed as database-enforced but not expressible → BA.3 D3 enforced / not-enforced table |
| PLAN-CRIT-007 | close outcome needs a column **and** an event projection → BA.3 D4 |
| PLAN-CRIT-008 | one `list_tasks` per team bounds calls, not rows → BA.4 D1 (and R2-CRIT-009: bounding without coverage was also wrong) |
| PLAN-CRIT-009 | BA.4 D2's "nudges a blocked agent every 60s" premise was false → corrected at source |
| PLAN-CRIT-010 | CAS-then-await is not a fence → BA.4 D5a, **decided this round** (R2-CRIT-011) |
| PLAN-CRIT-011 | BA.4 AC required an escalation call BA.4 excludes → per-tick wording made exact |
| PLAN-CRIT-012 | BA.6/BA.8 dependency cycle → BA.8 moved to wave 5 behind BA.6 |
| PLAN-CRIT-013 | absolute message priority vs no-starvation → BA.6 D4's four normative rules |
| PLAN-CRIT-014 | BA.5 AC1 still asserted five verbs while AC8-9 required a sixth → verb-count amendment, now **seven** (R2-CRIT-016) |
| PLAN-CRIT-015 | BA.8 declared parallel-safe with BA.5 → withdrawn, `must_follow` |
| PLAN-CRIT-016 | refusal streak needs a cross-task tail `AsyncTaskLedgerReader` cannot produce → BA.8 D3, **still not closed until this round** (R2-CRIT-012) |
| PLAN-CRIT-017 | mail-then-audit is not exactly-once across a crash → deterministic escalation id, **still underspecified until this round** (R2-CRIT-013) |
| PLAN-CRIT-018 | audit records only a resolved lead → recipient writes, **single-recipient only until this round** (R2-CRIT-014) |
| PLAN-CRIT-019 | phase AC promised one escalation while BA.8 let the sprint pick a weaker guarantee → decided: at-least-once per daemon epoch, both documents aligned |
| PLAN-CRIT-020 | BA.3 and BA.4 both edit `herdr_queue_wake.rs` but were not ordered → BA.3 `must_follow` BA.4. **This is the correct id for that finding**; round 1 cited it as the protocol-path finding and round 2's "correction" made it worse (R2-CRIT-019) |
| PLAN-CRIT-021 | BA.7 documents the surface before the rulings; nothing documents the shipped one → BA.11 D4 |
| PLAN-CRIT-022 | "Already built" still said no lead means nobody → corrected |
| PLAN-CRIT-023 | three-stage close did not revalidate at stage 3 → BA.5 D2 epoch revalidation |
| PLAN-CRIT-024 | BA.8 had a branch and dependencies but no worktree row and no independent-PR row → both added |
| PLAN-CRIT-025 | "atomic close/reopen" does not define the state-machine event → R2's fixed event semantics. **This is the correct id**; round 2 mistakenly deleted the citation as a mis-reference (R2-CRIT-019) |

Three of these (003, 004, 021) had the same shape as PLAN-SCOPE-001: a real
deliverable with no owning sprint. That is what BA.11 exists to close.

## Plan-scope review, round 2 — 2026-09-11

`plan-scope-reviewer` re-ran against `d9a8c1f42` and returned eight findings,
six blocking. Every one was verified against the documents and against
`origin/develop` before it was applied; none were rebutted. Five of the eight
are **defects the round-1 fixes introduced**, which is the value of running
the pass twice.

| id | disposition |
| --- | --- |
| PLAN-SCOPE-009 | the "ADR-061 governed interfaces" section still said **minor**, the one place R0 did not reach. Rewritten; R0 is now the single classification statement |
| PLAN-SCOPE-010 | BA.3's `must_follow` BA.4 existed only in its own header and the non-authoritative wave diagram, and cited the wrong finding id. Row added to the authoritative table; citation corrected to PLAN-CRIT-002 |
| PLAN-SCOPE-011 | three sources described three different BA.6/BA.9 stack topologies, and a linear stack cannot hold two siblings. **BA.6 leaves the stack**; it is `integrate/phase-ba`-based with a PR-completion trigger. BA.6/BA.9 `parallel_safe` row added |
| PLAN-SCOPE-012 | BA.6 AC6a demanded column names the sprint never supplied. `delivery_mode` and `handed_off_at` are now specified with type, default, write site and migration backfill, and the selection predicate stops reading `nudge_pending_at` |
| PLAN-SCOPE-013 | three docs described three mutually exclusive verb surfaces. Resolved: **six verbs** (`assign`, `start`, `close`, `move`, `list`, `events`); reassignment is `assign` onto an existing open id and adds no verb; BA.11's invented `reassign` and `show` are deleted |
| PLAN-SCOPE-014 | `TaskCloseOutcome::Reassigned` and its `CHECK` member became unreachable the moment R2(a) was adopted. Both removed — shipping an unwritable value fails the phase's own no-unused-code rule |
| PLAN-SCOPE-015 | **R3** added: the team lead is authorized for `cancel`, `reassign` and `move` on any task in their team. It was a real gating question hidden in a BA.5 deliverable while the Rulings section claimed all questions were decided |
| PLAN-SCOPE-016 | BA.8's AC9 tests BA.9's start receipt; the dependency was undocumented. Added |

Two further inconsistencies were found while verifying these and fixed in the
same pass: R2 mis-cited PLAN-CRIT-025 (the `schema-reviewer` precondition
finding), and BA.9 still carried "RULING REQUIRED BEFORE THIS HALF OPENS"
blocks for R1 and R2 while its own header said both were decided.

The reviewer also recorded that it was invoked without the Step-1
`plan-hardening-guidelines` handoff its contract expects. That is true and
deliberate — there is no hardening pass to hand it and I will not fabricate
one — so its `ready_for_next_step: false` reflects the contract, not the
findings. The findings themselves are independent of that gap and all eight
are applied.

## Critical plan review, round 2 — solar, 2026-09-11

Solar re-ran `critical-plan-reviewer` and returned twenty findings, thirteen
blocking, plus one wording item. It verified ten of the round-1 findings
closed and fifteen **not** closed — several because my round-1 disposition
catalogue mis-mapped their ids, so a finding could be marked applied while
the actual defect stood. Every finding below was verified against
`origin/develop` before it was applied; none were rebutted.

| id | disposition |
| --- | --- |
| R2-CRIT-001 | **R0 reopened and escalated to Rand.** ADR-061 D3 forbids a major change that requires every host to upgrade together. The governed-interface inventory was also wrong: BA.5, BA.6 and BA.9 each carry a MINOR change that had no owner |
| R2-CRIT-002 | `TASK_SCHEMA_DDL` is `CREATE TABLE IF NOT EXISTS`, so BA.3 alone changes nothing on an existing database. **BA.10 is now a stack layer directly above BA.3** and BA.5/BA.9 stack above BA.10 |
| R2-CRIT-003 | `STORAGE_SCHEMA_VERSION` does not exist on `origin/develop`, so "the old binary refuses to open the migrated database" is unimplementable against the actual previous binary. Folded into the R0 escalation |
| R2-CRIT-004 | **BA.0 created.** Normative ADR decisions move to wave 0, before any implementation; BA.11 keeps only verification and code-verified documentation |
| R2-CRIT-005 | PLAN-CRIT-002 was never addressed — the storage mutation boundary. **BA.5 D0a** names it: extend `WriteRequest` with `task_op`, no new trait, applied in the existing writer transaction |
| R2-CRIT-006 | BA.6's columns are now named in BA.6 D1 and decided in BA.0 D2, with the MINOR classification and version obligation |
| R2-CRIT-007 | BA.10's `identical` classifier is now field-by-field over every durable column **and** the full ordered event history; any inequality aborts |
| R2-CRIT-008 | BA.10 ships a bounded pre-migration repair artifact: `atm doctor task-duplicates`, a reviewed disposition file, dry-run, restore point, audit record, post-repair preflight |
| R2-CRIT-009 | a bare `LIMIT` traded an unbounded read for incomplete coverage. BA.4 D1 now selects **one due task per member**, cap derived from the roster, with a coverage AC that a `LIMIT`-only implementation fails |
| R2-CRIT-010 | BA.4's two `?` dispositions and the staleness rule are **decided**: `Unknown` escalates as Offline after three passes, `IdentityConflict` escalates immediately and is never nudged |
| R2-CRIT-011 | BA.4 D5a **decided: option (b)**, and the invariant is restated everywhere — *no nudge is admitted once a member is Active; at most one already-admitted emit may land.* The absolute guarantee needs an async fence this budget does not authorize |
| R2-CRIT-012 | BA.8 D3 said rows and then events. **Rows.** The bounded projection does not exist, so BA.3 ships `recent_closed_tasks` as an additive reader method and BA.8 follows BA.3 |
| R2-CRIT-013 | the daemon epoch is now fully defined — source, lifetime, scope, deterministic id derivation, collision test, recipient visibility — and budgeted |
| R2-CRIT-014 | escalation audit is **per target**: one row per `(team, task_id, epoch, target)`, retries only unsucceeded targets, suppression needs all of them. Two-recipient partial-fan-out AC added |
| R2-CRIT-015 | a second cycle: BA.9's AC4 and AC12 asserted BA.8 behaviour. Moved to BA.8 AC6a; BA.9 owns persistence and events only |
| R2-CRIT-016 | the verb surface is **seven**: `assign`, `start`, `close`, `reassign`, `move`, `list`, `events`. My six-verb answer overloaded `assign` by row existence — the same defect this sprint rejected for `start` |
| R2-CRIT-017 | `TaskCloseOutcome::Reassigned` removed (already applied from PLAN-SCOPE-014) |
| R2-CRIT-018 | dependency metadata reconciled across the table, headers, affected paths and the stack order; BA.2's header no longer claims `parallel_safe` with BA.4 |
| R2-CRIT-019 | **my round-1 catalogue was wrong in four places** and masked an open blocking finding. Rebuilt from the original review JSON; the two "corrections" round 2 made on the strength of the bad catalogue are reverted |
| R2-CRIT-020 | stale-instruction sweep: BA.1 pointed start at BA.5, BA.10 claimed completion still creates mirrors, BA.9 still said "gated" |
| R2-CRIT-M1 | sprint counts corrected to twelve |

`granularity_assessment` was that eleven documents were not the problem but
that four of them hid structural failures. Three of those four are fixed here
(the BA.3/BA.10 intermediate, the BA.8/BA.9 cycle, BA.6's delegated schema);
the fourth — BA.11 deferring normative decisions — is fixed by BA.0.

## Worktrees and the gh stack

One integration branch, created first, off `develop`:

```bash
/sc-git-worktree --create integrate/phase-ba develop
```

Every sprint worktree is created from `integrate/phase-ba`, never from `main`
and never from `develop`:

```bash
/sc-git-worktree --create docs/ba0-normative-adr-decisions      integrate/phase-ba
/sc-git-worktree --create feature/ba1-ack-task-separation      integrate/phase-ba
/sc-git-worktree --create feature/ba2-nudge-title-salvage      integrate/phase-ba
/sc-git-worktree --create feature/ba4-nudge-invariant          integrate/phase-ba
/sc-git-worktree --create docs/ba7-task-nudge-documentation    integrate/phase-ba
/sc-git-worktree --create feature/ba3-task-identity-queue      integrate/phase-ba
/sc-git-worktree --create feature/ba5-atm-task-commands        integrate/phase-ba
/sc-git-worktree --create feature/ba6-ephemeral-queued-messages integrate/phase-ba
/sc-git-worktree --create feature/ba8-escalation-terminality    integrate/phase-ba
/sc-git-worktree --create feature/ba9-lifecycle-operations      integrate/phase-ba
/sc-git-worktree --create feature/ba10-task-event-migration     integrate/phase-ba
/sc-git-worktree --create docs/ba11-final-verification          integrate/phase-ba
```

That is twelve worktrees for twelve sprints. An earlier revision listed seven
and omitted BA.8 entirely, which would have silently dropped a sprint carrying
four blocking dispositions (PLAN-SCOPE-001); a later one omitted BA.11, which
would have left three accepted ADRs describing behaviour this phase deletes
(PLAN-CRIT-003, PLAN-CRIT-004, PLAN-CRIT-021).

### The `must_follow` chain is one gh stack

BA.1 → BA.3 → BA.10 → BA.5 → BA.9 form a **branch-ancestry** chain, so they are linked
as a single stack rooted on the integration branch:

```bash
gh stack init --base integrate/phase-ba \
  feature/ba1-ack-task-separation \
  feature/ba3-task-identity-queue \
  feature/ba10-task-event-migration \
  feature/ba5-atm-task-commands \
  feature/ba9-lifecycle-operations
gh stack submit --auto
```

**BA.10 sits immediately above BA.3, and BA.3 may not merge to
`integrate/phase-ba` without it (R2-CRIT-002).** `TASK_SCHEMA_DDL` is
`CREATE TABLE IF NOT EXISTS tasks`
(`task_store.rs:15`), so changing the declaration does **nothing** to an
existing database. BA.3 alone therefore produces a head where new decoders
read `current_assignee` / `position` / `close_outcome` from a table that still
has the old columns — a non-operational intermediate that BA.5 was previously
allowed to build on in parallel. The split from PLAN-SCOPE-005 survives (a
wrong schema is fixable by a follow-up commit, destroyed history is not), but
it is now a **stack layer, not an independent branch**: two PRs, one merge
event, and BA.5/BA.9 stack above BA.10 rather than above BA.3.

BA.9 is in the stack because it edits **BA.5's own files** — it adds `start`
to the same clap enum and the same command module. That is ancestry, not
sequencing.

**BA.6 is not in the stack (PLAN-SCOPE-011).** An earlier revision listed it
as a layer *and* described BA.9 as sitting above BA.5, while both sprint
headers claimed the same base — three sources describing three topologies,
and a linear stack cannot hold two siblings anyway. BA.6's files are disjoint
from BA.5's command surface; what it needs is BA.5's *merged close semantics*,
which a PR-completion trigger delivers. Stacking it would force a rebase of
BA.6 every time BA.9 moved, for ancestry it does not require — the same
argument that keeps BA.8 out.

If R1 and R2 have both been answered by the time BA.5 opens, consider merging
BA.9 back into BA.5 rather than carrying a fourth layer — the split exists to
stop the rulings blocking BA.5, not to create work.

Layers are added to the stack **when their PR opens**, not held for CI.

### The parallel sprints are not stacked

BA.0, BA.2, BA.4, BA.6, BA.7, BA.8 and BA.11 are independent branches with
ordinary PRs targeting `integrate/phase-ba`. **BA.10 is not** — see below. Stacking them would impose an order the
dependency analysis says does not exist, and would force a rebase of unrelated
work every time a lower layer moves.

Several dependencies here are by **PR completion**, not branch ancestry,
because the predecessor merges to `integrate/phase-ba` well before the
dependent opens: BA.8 on BA.4, BA.5 and BA.6; BA.6 on BA.4; and BA.11 on
everything. BA.4 on BA.2 is different — same wave, same file — so BA.4 merges
BA.2 forward rather than waiting for its PR.

BA.8's dependency on BA.5 and BA.6 does **not** put it in the stack. Its files
are disjoint from both; what it needs is their *merged behaviour*, which a
PR-completion trigger delivers. Stacking it would force a rebase of BA.8 every
time BA.5 or BA.6 moves, for no ancestry it actually requires.

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
  recipients: Vec<String> }` (`herdr_escalation.rs:204-207`). Lead and
  recipients resolve **independently** (`:209-243`) and `write_target_mail`
  (`:289-327`) writes every configured recipient whether or not a lead was
  resolved; `lead` is `(leads.len() == 1).then(...)`, so zero leads and two
  leads both yield `None` without suppressing recipients. The empty case Rand
  accepted is no lead AND no recipients AND a failed Herdr notify.
  (PLAN-CRIT-022: an earlier revision restated the corrected-away claim here.)
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

**BA.3 is not the only governed-interface change (R2-CRIT-001).** The
inventory, all three interfaces:

| sprint | interface | change | ADR-061 class |
| --- | --- | --- | --- |
| BA.5 D0 | HTTP/peer API | new `RequestEnvelope` / `ResponseEnvelope` variants for task mutation; `CLI_SCHEMA_VERSION` and `HTTP_API_VERSION` bumps | **MINOR** — additive variants, older consumers ignore them (ADR-061:59). Needs the bump in the same change set and an older-consumer test |
| BA.6 D1 | SQLite storage | `delivery_mode`, `handed_off_at` added to `mail_message_states` with defaults | **MINOR** — columns with defaults (ADR-061:59). Needs a version bump and an older-consumer test, and must be named in the pre-implementation ADR |
| BA.3 / BA.10 | SQLite storage | primary-key narrowing, constraint and meaning changes | **MAJOR** — and blocked on R0 above |
| BA.9 | HTTP/peer API + CLI | `start` and `reassign` variants and verbs | **MINOR** — additive |

An earlier revision said "BA.3 is the only governed-interface change", which
was false, and it is the reason two MINOR bumps had no owner. Each MINOR above
carries ADR-061 D3's full minor obligation: the bump, the documentation, and
a test proving the older consumer still works.

BA.3's classification is **ADR-064's to state, not this plan's** — and ADR-064
D2 finds an additive shape the plan had wrongly declared impossible. Under
ADR-061:64 — the primary-key narrowing renames/removes a field and the
previous binary cannot operate against the migrated database. See **R0**,
which is the single authoritative classification statement; this section
carries only what R0 does not.

*This section previously said "minor with a migration" and was left stale when
R0 was decided (PLAN-SCOPE-009). It is corrected, not reinterpreted: MINOR was
wrong, and one document must not state a classification two ways.*

What retiring AZ withdraws — and what BA does **not** inherit from it: the
full-`1.6.x` bidirectional bridge obligation, the compatibility triggers, the
retained 1.5.14 rollback fixture, and the 1.7.0 bridge-removal ADR. None of
those shipped, and BA's one-way shape needs none of them. The MAJOR label is
inherited; AZ's coexistence *strategy* is not.

`schema-reviewer` sign-off and Rand's recorded approval are **preconditions
for opening BA.3 and BA.10**, not plan-review checkboxes.

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
   diverted; a blocked or dead agent is escalated and never nudged.
   **Terminality is at-least-once per daemon epoch** (BA.8 D5 option a,
   decided — PLAN-CRIT-019): a daemon restart may re-report a still-blocked
   agent, and the notice carries the epoch so a recipient can tell a restart
   duplicate from a new episode. A duplicate after a restart is cheap;
   permanently suppressing a real re-block is the failure this phase exists
   to remove.
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
10. Every sprint in the sequence table has a worktree-creation command and a
    dependency row. Gate: the counts match.
11. **The complexity budget above is met exactly.** Gate at phase end: zero
    new tables, zero new sealed traits, zero new semantic capabilities, zero
    new state machines, and **one schema change** to the task tables, whose
    ADR-061 classification and shape ADR-064 D4 decides. A sprint that needs
    more stops and asks. **Whether the schema change is MAJOR or MINOR is
    ADR-064 D4's to answer**; this criterion asserts the count, not the
    classification.
12. **Every accepted ADR that phase BA contradicts was amended before the
    code that contradicts it, and the shipped code matches.** Gate: BA.0's
    amendments merged before wave 1, ADR-064 is no longer `Proposed`, BA.11's
    verification criteria pass, and no agent-facing document describes the
    task surface in the future tense (PLAN-CRIT-003, PLAN-CRIT-004,
    PLAN-CRIT-021, R2-CRIT-004).
