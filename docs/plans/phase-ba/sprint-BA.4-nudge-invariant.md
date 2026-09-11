# BA.4 — Continuous evaluation of the nudge invariant

| Field | Value |
| --- | --- |
| Wave | 1 |
| Branch | `feature/ba4-nudge-invariant` |
| Base | `integrate/phase-ba` (independent PR, not stacked) |
| Dependency | `parallel_safe` with BA.1 and BA.7; **`must_follow` BA.2** — both touch `storage_and_nudge_router.rs` (PLAN-SCOPE-002). Merge-forward trigger: BA.2 development pushed. |
| recommended_agent | arch-ctm |
| recommended_model | deep-reasoning |

## Goal

Evaluate the invariant continuously, cheaply, over the exact agent state, and
never steer an agent whose state changed before the effect boundary.

**Scope note.** This sprint owns *evaluation and dispatch admission*. Escalation
terminality, the refusal streak, episode identity, and lifecycle notification
delivery moved to **BA.8**, which depends on BA.3's identity and typed outcome.
The split is deliberate: everything here runs against develop's schema
unchanged, so it can land in wave 1.

## The invariant

> Up until the point a task is complete the agent MUST keep working on it.

Evaluated continuously, not on a false→true edge. Five arms:

| agent state | with an incomplete task | with no task |
| --- | --- | --- |
| idle | nudge, rate-limited | nothing |
| active | never divert | nothing |
| blocked | escalate immediately, zero nudges | escalate |
| dead / offline | escalate immediately, zero nudges | escalate |

Re-nudging a still-violating, unchanged pair is **intended**, not a defect.

## Deliverables

> Numbering note (PLAN-SCOPE-M1): D4 and D6 are absent by design — they moved
> to BA.8 when this sprint was split. The remaining numbers are kept stable so
> solar's findings and the phase plan keep pointing at the same deliverables.

### D1. Bounded evaluation — SOLAR-BA-001, blocking

The current shape is an N+1 sequential hot path and must not be reused as-is.
On `origin/develop`: the pump polls every 5s
(`herdr_queue_wake.rs:37-45`); every Idle **or Blocked** member becomes a task
candidate (`herdr_queue_wake.rs:505-512`); `read_due_task` performs one async
`list_tasks(team, member)` per candidate before checking the 60s due timestamp
(`herdr_queue_wake_reminders.rs:82-111, 252-267`); and the candidate loop
awaits those reads **serially** (`herdr_queue_wake_reminders.rs:56-69`).

At the live roster — 51 members across 17 teams — an all-idle deployment
issues up to 612 task reads/minute to deliver at most one reminder per member
per minute, and worst-case tick latency is the sum of per-read deadlines
(5s each), not one bounded tick.

Required:

- one bounded task snapshot **per team per tick**, or an equivalently batched
  existing read — not one read per member
- join it to the exact roster snapshot **in memory**
- state the cost in the sprint's own docs as `O(members + open_tasks + teams)`
- the rate-limit gate is evaluated from that same snapshot

**Bounding the call count is not bounding the work (PLAN-CRIT-008).** One
`list_tasks` per team caps *calls*; the existing reader returns
`Vec<TaskRow>` and the SQL has no state predicate and no `LIMIT`, so with BA.3
retaining rows forever the per-team snapshot grows without bound and the
claimed `O(open_tasks)` is false. The snapshot query must carry **both** a
`state <> 'complete'` predicate and a `LIMIT`, pushed into SQL, and the
acceptance test must count **rows returned**, not calls issued.

**A `LIMIT` alone trades an unbounded read for incomplete coverage
(R2-CRIT-009).** With a bare `LIMIT` and no ordering discipline, tasks past
the first page are never evaluated — on every tick, forever — and AC1 as
originally written would have proved only that the result was small. The
projection must be bounded **and** complete:

- the snapshot selects **at most one due open task per member**, not the
  first N rows of the team's tasks: `state <> 'complete'`, partitioned by
  `current_assignee`, taking the row at the lowest `position` (BA.3 D3 makes
  that the active-or-next task, which is the only one the invariant can act
  on anyway)
- the row cap is therefore `LIMIT <members in team>`, a bound derived from
  the roster rather than an arbitrary constant
- a member whose one due task is not actionable this tick does not displace
  any other member: coverage per tick is every member, not every task

Ordering is by `(current_assignee, position)` so the projection is
deterministic and keyset-resumable if a later phase needs paging. No cursor
state is persisted in this phase because none is needed — one row per member
fits every roster this product has.

This requires **no new trait, table, or state machine**, but it **does**
require a new bounded read on the existing reader; BA.3 ships it (see BA.8
D3, which needs the same projection shape).

### D2. Exact agent state, not the picker projection

Consume `RuntimeMemberState` directly. Do **not** consume
`PickerMemberStatus`: it collapses
`Offline | Unknown | IdentityConflict | Blocked` to `Dead`
(`picker_projection.rs:76`), destroying the distinction that decides
remediation — blocked means a human can clear it in place, offline means the
process is gone.

Today the reminder path consults neither: `atm-http-runtime` contains zero
references to `picker_projection` or `PickerMemberStatus`, so a frozen agent
is nudged every 60s while doctor already reports it blocked.

### D3. Blocked and dead escalate immediately with zero nudges

A nudge cannot clear a permission prompt or revive an exited process. Both
states escalate on first observation; neither waits for the stalled-reminder
threshold.

**SOLAR-BA-005 (IMPORTANT), verified: most of this already exists.** Do not
dispatch it as new work and do not write a second path.

- `herdr_queue_wake.rs:505-512` already admits `Idle | Blocked` and sets
  `TaskCandidate.blocked`
- `herdr_queue_wake_reminders.rs:123-132` already records
  `ReminderOutcome::Blocked` and returns without emitting
- `herdr_queue_wake_reminders.rs:72-79` already calls `escalate_blocked`

This deliverable is therefore: **verify and test** the blocked arm against the
invariant, and **add the dead/offline arm**, which has no equivalent today.
`RuntimeMemberState::Offline` is not admitted as a task candidate at all, so an
agent whose process exited is simply never evaluated — silence, not escalation.
That is the actual gap.

### D3a. The full six-variant disposition matrix

**SOLAR-BA-010 (IMPORTANT).** D2 mandates exact `RuntimeMemberState`, which has
six variants; the invariant names four arms. `Unknown` and `IdentityConflict`
have no disposition. Collapsing them into Offline repeats the prohibited picker
collapse; calling them Idle mis-nudges an unresolved identity; calling them
Active hides stalled work. Ship an explicit matrix:

| `RuntimeMemberState` | disposition |
| --- | --- |
| `Idle` | nudge if an incomplete task exists, rate-limited |
| `Active` | never divert |
| `Blocked` | escalate once, zero nudges |
| `Offline` | escalate once, zero nudges; body distinguishes it from Blocked |
| `Unknown` | no nudge. No escalation while the member has been observed within the staleness window; **escalate once as Offline** when it has not (rule below) |
| `IdentityConflict` | no nudge, ever. **Escalate once immediately**, as an operator-facing condition distinct from Blocked and Offline — the roster cannot say who this is, so nudging it could steer the wrong agent |

**Decided here, not by the implementer (R2-CRIT-010).** An earlier revision
left both rows as `?` while AC5 claimed to assert "exact" dispositions, so two
materially different products could both pass.

**The staleness rule, exactly.** `Unknown` means "no runtime observation has
been accepted for this member". It becomes Offline for disposition purposes
when the member's last accepted observation is older than
**three consecutive evaluation passes** of the invariant — the same pass
cadence the sprint already runs on, so no timer, no new constant beyond the
multiplier, and no wall-clock assumption. A member never observed at all since
daemon start is `Unknown` and is **not** escalated until three passes have
elapsed since daemon start, so a restart does not escalate the whole roster.

Each of the six gets a test, and the Unknown row gets three: inside the
window, crossing it, and the daemon-restart case.

### D5. No diversion

An assignment landing on an agent that holds an Active task persists as
`assigned` and materialises **no** nudge. The `one_active_task_per_agent`
index arriving in BA.3 prevents two Active rows; it does **not** prevent an
Assigned task's assignment message being injected mid-flight. That suppression
is this sprint's responsibility and must be tested independently.

### D5a. Dispatch-time fences at the effect boundary — two of them

**SOLAR-BA-017 (BLOCKING).** Withdrawing revision revalidation removed edge
detection, correctly — but it also removed the only thing keeping a stale
candidate from steering an agent that has since gone Active. Today the poll
accepts an Idle observation and builds `TaskCandidate`
(`herdr_queue_wake.rs:444-518`), then does queued-message work and one or more
**async** task reads before emitting. During that gap the HTTP heartbeat route
can flip the same canonical roster member to Active
(`storage_and_nudge_router.rs:644-692`). The stale candidate then interrupts
live work — problem (b), caused by the fix for problem (a). D1's serial N+1
path makes the window large; fixing D1 shrinks it but does not close it.

This is **not** edge detection and must not be argued away as such: the
invariant is evaluated continuously, and its inputs must still hold **at the
effect boundary**, not merely when the candidate was built.

**PLAN-CRIT-010: a CAS followed by an `await` is not sufficient, and an
earlier revision of this plan asserted that it was.** The existing roster lock
guards snapshot mutation and is released before the awaited emitter call, so a
check-then-await reopens exactly the window it was meant to close. Requiring
"emission count is zero" while forbidding reservations left no implementable
design.

**DECIDED: option (b), and the invariant is restated to match
(R2-CRIT-011).** Leaving this to the sprint was the defect, not the fix: the
two options are incompatible guarantees, and one of them — (a) — is a
synchronous roster lock held across an awaited network emit on a Tokio task,
which is not something this codebase should grow.

The admission seam is the **last read of member state before the emitter is
invoked**, inside the existing roster guard, which is released before the
await. The guarantee is therefore:

> **No nudge is *admitted* for a member once that member is Active.** At most
> **one** already-admitted emit may still be in flight when the member
> transitions to Active, and it may land.

That is a real weakening of "an Active agent is never diverted" and it is
written into phase acceptance, BA.4's AC, and BA.0's ADR-062 amendment in
those words — not claimed away. The bound is one, not "some": the guard
admits one emit per member at a time, so a second cannot be admitted while
the first is in flight.

The alternative, for the record: closing the window completely needs an async
fence (an admission token released by the emitter's completion), which is new
concurrency state this phase's budget does not authorize. If Rand wants the
absolute guarantee, that fence must be budgeted and this ruling revisited.

No target generation, no eligibility edge, no scheduler state machine.

Test the exact race: poll observes Idle, pause **before admission**, a
heartbeat POST commits Active, resume — **admission count must be zero**.
Then the weakened case, asserted explicitly rather than left implicit: poll
observes Idle, the emit is **admitted**, the heartbeat commits Active while
the emit is in flight — at most **one** nudge lands, and a second cannot be
admitted while the first is outstanding. Also assert Idle remains eligible,
and that a Blocked or Offline transition before admission suppresses the
dispatch.

## Affected paths

- `crates/atm-http-runtime/src/herdr_queue_wake.rs`
- `crates/atm-http-runtime/src/storage_and_nudge_router.rs` — **shared with
  BA.2**. This sprint owns only the roster-mutation side of the D5a fence
  (around `:644-692`); BA.2 owns the nudge-title hunks it cherry-picks. Merge
  BA.2 forward before editing this file.
- `crates/atm-http-runtime/src/herdr_queue_wake_reminders.rs` — candidate
  collection and the blocked/offline arms only

## Paths that must not change

- `crates/atm-http-runtime/src/herdr_queue_wake_escalation.rs`,
  `herdr_escalation.rs` — BA.8
- `crates/atm-storage*` — BA.1 in wave 1, BA.3 from wave 2 (PLAN-SCOPE-M4)
- `crates/atm/src/commands/*` — BA.5
- `crates/atm-core/src/picker_projection.rs` — must stay unreferenced from
  this crate, not modified

## Acceptance criteria

1. **Scale**: a fixture with 17 teams and 51 members, all idle with open
   tasks, issues at most one task read per team per tick **and** returns at
   most one row per member. Both asserted by counting; a fixture with 10,000
   historical complete tasks must not increase rows returned
   (PLAN-CRIT-008). Wall-clock timing is diagnostic only.
1a. **Coverage** (R2-CRIT-009): a fixture with **more open tasks than the row
   cap** — 51 members holding 20 open tasks each — evaluates **every** member
   on **every** tick. Asserted by observing one nudge per eligible member
   within a single tick, not eventually. A bare-`LIMIT` implementation must
   fail this.
2. **Slow-read isolation**: one team whose task read is artificially slow does
   not serialise the global pass. This is the acceptance test SOLAR-BA-001
   requires and it must fail against `origin/develop`.
3. A blocked member receives **zero** nudges and produces **at most one
   escalation call per tick**. How many calls an episode produces over time is
   BA.8's terminality deliverable and is explicitly **not** asserted here
   (PLAN-CRIT-011) — a single-tick test must not be read as proving
   terminality.
4. An offline/dead member receives zero nudges and produces an escalation
   call distinguishable in the body from the blocked case. Must fail against
   `origin/develop`, where Offline is never admitted as a candidate.
   Terminality is BA.8's.
5. All six `RuntimeMemberState` variants have a tested disposition, including
   `Unknown` and `IdentityConflict`, and none of them routes through
   `PickerMemberStatus`. Gate: `git grep -n 'picker_projection\|PickerMemberStatus'
   -- crates/atm-http-runtime` returns nothing.
6. Assigning to an agent with an Active task writes an `assigned` row and
   emits no nudge.
7. **Roster dispatch race**: an agent observed Idle that commits Active via
   the heartbeat route before the **admission seam** receives zero nudges.
   If the sprint chose D5a option (b), the criterion instead reads: at most
   one in-flight nudge may land after the seam, and the test asserts that
   bound explicitly. Either way it must fail against `origin/develop`.
8. Idle remains eligible across the same paused window, and a Blocked or
   Offline transition in that window suppresses the dispatch.

## Required validation

- `just test`, `just lint`
- the scale and slow-read tests in criteria 1-2
- criteria 3, 5 and 7 each fail on `origin/develop`

## Non-closure

- Escalation terminality, the refusal streak, episode identity, and deferred
  lifecycle notification delivery are **BA.8**. This sprint may call the
  existing escalation path but must not change its termination behaviour.
- The task-side effect fence (close/reassign racing the emit) is BA.8 D2,
  because it serializes on the logical `(team, task_id)` identity that BA.3
  creates.
- No schema change. No new trait, table, or state machine.
