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

This requires **no new trait, table, or state machine**.

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
| `Unknown` | ? — no nudge, no escalation until the staleness condition below |
| `IdentityConflict` | ? — no nudge; escalate as an operator-facing condition |

The two `?` rows must be decided **in this sprint** and justified in the doc,
together with the availability/staleness condition that separates "not observed
yet" from "observed and gone". Each of the six gets a test.

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

Required: reminder and queued-message dispatch admission is serialized with
canonical roster mutation, or performs an atomic current-state check
immediately before committing the steer, so a known Active transition wins and
suppresses the dispatch. **A plain second read without serialization is still
TOCTOU and does not satisfy this.** A per-member lock or CAS at the existing
ephemeral roster boundary is sufficient — no target generation, no eligibility
edge, no scheduler state machine.

Test the exact race: poll observes Idle, pause before emit, a heartbeat POST
commits Active, resume — emission count must be zero. Also assert Idle remains
eligible, and that a Blocked or Offline transition in the same window
suppresses the dispatch.

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
   tasks, issues at most one bounded task read per team per tick. Asserted on
   a counted read, not on wall-clock timing.
2. **Slow-read isolation**: one team whose task read is artificially slow does
   not serialise the global pass. This is the acceptance test SOLAR-BA-001
   requires and it must fail against `origin/develop`.
3. A blocked member receives **zero** nudges and produces exactly one
   escalation call for the episode.
4. An offline/dead member receives zero nudges and produces exactly one
   escalation call, distinguishable in the escalation body from the blocked
   case. This must fail against `origin/develop`, where Offline is never
   admitted as a candidate.
5. All six `RuntimeMemberState` variants have a tested disposition, including
   `Unknown` and `IdentityConflict`, and none of them routes through
   `PickerMemberStatus`. Gate: `git grep -n 'picker_projection\|PickerMemberStatus'
   -- crates/atm-http-runtime` returns nothing.
6. Assigning to an agent with an Active task writes an `assigned` row and
   emits no nudge.
7. **Roster dispatch race**: an agent observed Idle that commits Active via
   the heartbeat route before the emit boundary receives **zero** nudges.
   Must fail against `origin/develop`.
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
