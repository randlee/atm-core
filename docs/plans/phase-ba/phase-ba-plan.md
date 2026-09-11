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
| Sprints | 11 |
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
| BA.8 | Escalation terminality and lifecycle delivery | 5 | arch-ctm / deep-reasoning |
| BA.9 | Lifecycle operations: start and reassign | 4 | arch-ctm / deep-reasoning |
| BA.10 | Task and event migration | 3 | arch-ctm / deep-reasoning |
| BA.11 | ADR amendments and final task-surface documentation | 6 | Cipher-311d / fast |

`recommended_agent` / `recommended_model` are advice, not assignment.

### Wave plan

```
wave 1   BA.2 -> BA.4     BA.1     BA.7   (BA.1/BA.7 parallel; BA.4 follows BA.2)
wave 2   BA.3                              (must_follow BA.1 and BA.4)
wave 3   BA.5   BA.10                      (both must_follow BA.3)
wave 4   BA.6   BA.9                       (BA.6: BA.5+BA.4; BA.9: BA.5+BA.3)
wave 5   BA.8                               (must_follow BA.3, BA.4, BA.5, BA.6)
wave 6   BA.11                              (must_follow everything)
```

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
| BA.5 | `must_follow` BA.3 | the command set writes the `outcome` and `position` columns that BA.3 creates |
| BA.8 | `must_follow` BA.3, BA.4, **BA.5**, **BA.6** | BA.3 gives the logical `(team, task_id)` identity and the typed outcome the refusal streak reads; BA.4 owns the reminder path it extends; **BA.5** because BA.8 serializes with BA.5's mutations and its AC tests BA.5's receipts (PLAN-CRIT-015); **BA.6** because AC11's read-without-ack remindability is BA.6's deliverable (PLAN-CRIT-012) |
| BA.5 / BA.8 | ~~`parallel_safe`~~ **withdrawn** | file ownership is still disjoint (BA.5 `crates/atm/src/commands/*`, BA.8 `crates/atm-http-runtime/src/herdr_*` and `atm-core/src/send/mod.rs`), but disjoint files are not sufficient when one sprint's acceptance criteria assert the other's behaviour. PLAN-CRIT-015 |
| BA.10 | `must_follow` BA.3 | migrates the data onto the schema BA.3 establishes. Split from BA.3 (PLAN-SCOPE-005): a wrong schema is fixable by a follow-up commit, destroyed production history is not. PR-completion trigger. |
| BA.10 | `parallel_safe` with BA.5, BA.8 | owns the migration path in `schema_version.rs` and the migration fixtures; BA.3 has already landed the schema definitions it migrates onto |
| BA.9 | `must_follow` BA.5, `must_follow` BA.3 | adds the start and reassign operations to the command surface BA.5 creates. Split from BA.5 (PLAN-SCOPE-004) so seven unrelated deliverables stop waiting on two human rulings. **Gated per half**: the start half on R1, the reassign half on R2. |
| BA.6 | `must_follow` BA.5, `must_follow` BA.4 (PR-completion) | needs BA.5's close semantics and BA.4's invariant evaluation. **The former dependency on BA.8 is reversed** (PLAN-CRIT-012): the non-starvation rule is now normative in BA.6 D4 and needs nothing from BA.8, while BA.8 does need BA.6 |
| BA.11 | `must_follow` every other sprint (PR-completion) | it records what shipped. An amendment written ahead of the code is a prediction; phase AZ produced several that then disagreed with the merge. Doc-only, so it costs one short wave |

`parallel_safe` claims above are made on non-intersecting crates, files,
public contracts, and boundary manifests. `plan-scope-reviewer` verifies them.

## Rulings

Three questions the design did not answer. Rand's standing instruction is that
he is not in iteration loops — *make the call, record it, report the outcome* —
and his 2026-09-11 direction was to fix all reviewer findings and iterate to
clean. All three are therefore **decided here by fenix and reversible by Rand**,
not held open.

### R0 — BA.3 is a MAJOR storage change (PLAN-CRIT-001)

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

**Decision: option (a), same-id reassignment**, with the event semantics fixed below (PLAN-CRIT-025).

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
    different task id, which R2(a) makes unnecessary.

Silently changing `current_assignee` without the event is explicitly
forbidden: it loses the actor and the handover history.

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
| PLAN-CRIT-001 | **R0 ruling** — BA.3 is ADR-061 MAJOR; budget row, BA.3/BA.10 headers and phase AC11 corrected |
| PLAN-CRIT-002 | BA.3 header — `must_follow` BA.4 added |
| PLAN-CRIT-003 | **BA.11 D1** — ADR-062 amendment; BA.7 could not own it |
| PLAN-CRIT-004 | **BA.11 D2** — ADR-054 queue-contract amendment; BA.6 must name its columns |
| PLAN-CRIT-005 | BA.10 — three-class preflight table replaces the self-contradictory abort rule |
| PLAN-CRIT-006 | BA.3 D3 — enforced / not-enforced table; SQLite cannot express contiguity |
| PLAN-CRIT-007 | BA.3 D4 — close outcome projected into an event, not only a column |
| PLAN-CRIT-008 | BA.4 D1 — bound is on rows returned, not calls issued |
| PLAN-CRIT-009 | BA.4 D3 — stale premise corrected at source |
| PLAN-CRIT-010 | BA.4 D5a — admission seam stated as (a) or (b), one must be chosen |
| PLAN-CRIT-011 | BA.4 — per-tick wording made exact |
| PLAN-CRIT-012 | **wave restructure** — BA.6/BA.8 cycle broken; BA.8 moves to wave 5 |
| PLAN-CRIT-013 | BA.6 D4/D2a — four normative scheduling rules replace "eventually" |
| PLAN-CRIT-014 | BA.9 owns the verb-count amendment and the baseline regeneration |
| PLAN-CRIT-015 | BA.5/BA.8 `parallel_safe` withdrawn; BA.8 `must_follow` BA.5 |
| PLAN-CRIT-016 | BA.8 D3 — streak from a bounded read of closed task rows, not an unreachable event tail |
| PLAN-CRIT-017 | BA.8 — deterministic escalation message id from `(team, task_id, epoch)` |
| PLAN-CRIT-018 | BA.8 — audit records recipient writes, not only a resolved lead |
| PLAN-CRIT-019 | BA.8 D5 **decided**: option (a), at-least-once per daemon epoch; phase AC1 matches |
| PLAN-CRIT-020 | BA.5 D0 — the protocol path was missing entirely; `RequestEnvelope` has no task mutation |
| PLAN-CRIT-021 | **BA.11 D4** — BA.7 documents a planned surface; BA.11 documents the shipped one |
| PLAN-CRIT-022 | "Already built" — the stale "no lead means nobody is notified" claim removed here too |
| PLAN-CRIT-023 | BA.5 D2 — stage-3 epoch revalidation with a named-outcome race table |
| PLAN-CRIT-024 | BA.2 — cherry-pick scope is 32 files, three governed manifests, one retired AZ sprint doc |
| PLAN-CRIT-025 | BA.3/BA.10 — `schema-reviewer` sign-off named as a precondition, not a validation step |

Three of these (003, 004, 021) had the same shape as PLAN-SCOPE-001: a real
deliverable with no owning sprint. That is what BA.11 exists to close.

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
/sc-git-worktree --create feature/ba8-escalation-terminality    integrate/phase-ba
/sc-git-worktree --create feature/ba9-lifecycle-operations      integrate/phase-ba
/sc-git-worktree --create feature/ba10-task-event-migration     integrate/phase-ba
/sc-git-worktree --create docs/ba11-adr-amendments              integrate/phase-ba
```

That is eleven worktrees for eleven sprints. An earlier revision listed seven
and omitted BA.8 entirely, which would have silently dropped a sprint carrying
four blocking dispositions (PLAN-SCOPE-001); a later one omitted BA.11, which
would have left three accepted ADRs describing behaviour this phase deletes
(PLAN-CRIT-003, PLAN-CRIT-004, PLAN-CRIT-021).

### The `must_follow` chain is one gh stack

BA.1 → BA.3 → BA.5 → BA.6 form a dependency chain, so they are linked as a
single stack rooted on the integration branch rather than four independent PRs:

```bash
gh stack init --base integrate/phase-ba \
  feature/ba1-ack-task-separation \
  feature/ba3-task-identity-queue \
  feature/ba5-atm-task-commands \
  feature/ba6-ephemeral-queued-messages \
  feature/ba9-lifecycle-operations
gh stack submit --auto
```

BA.9 joins the stack above BA.5 because it extends BA.5's command surface in
the same files. If R1 and R2 have both been answered by the time BA.5 opens,
consider merging BA.9 back into BA.5 rather than carrying a fifth layer —
the split exists to stop the rulings blocking BA.5, not to create work.

Layers are added to the stack **when their PR opens**, not held for CI.

### The parallel sprints are not stacked

BA.2, BA.4, BA.7, BA.8, BA.10 and BA.11 are independent branches with
ordinary PRs targeting `integrate/phase-ba`. Stacking them would impose an order the
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
    new state machines, and **one MAJOR schema change** — one-way,
    version-gated, no bridge, no dual authority (R0). A sprint that needs
    more stops and asks. The earlier "one MINOR bump" wording was wrong and
    is corrected here; PLAN-CRIT-001 falsified it.
12. **Every accepted ADR that phase BA contradicts has been amended, and no
    amendment describes anything the phase did not ship.** Gate: BA.11's
    acceptance criteria pass, ADR-064 quotes Rand's recorded approval, and no
    agent-facing document describes the task surface in the future tense
    (PLAN-CRIT-003, PLAN-CRIT-004, PLAN-CRIT-021).
