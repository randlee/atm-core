# BA.8 — Escalation terminality and lifecycle notification delivery

| Field | Value |
| --- | --- |
| Wave | 3 |
| Branch | `feature/ba8-escalation-terminality` |
| Base | `integrate/phase-ba` (independent PR, not stacked) |
| Dependency | `must_follow` BA.4 (owns the reminder path) and BA.3 (logical identity, typed outcome) — PR-completion trigger: both merged to `integrate/phase-ba` |
| recommended_agent | arch-ctm |
| recommended_model | deep-reasoning |

## Goal

A stalled task is reported **once** and then goes quiet — but only once
somebody has durably been told. Nothing in the lifecycle steers an agent that
is working.

This sprint was split out of BA.4 after solar's critical review: five of its
six deliverables depend on BA.3's logical `(team, task_id)` identity or its
typed close outcome, and all of them live in the escalation modules rather
than the evaluation path.

## The defect

Problem (a) in one line: escalation exists, but it is multiplicative and never
terminal, so a stalled task nudges forever and the notification becomes noise.
Live evidence from the atm-dev ledger — 587 reminders on one task over roughly
ten hours, with about 58 notifications to lead, and a tail of 59 / 39 / 21 /
16 / 13 / 13 / 11 / 10 / 10 behind it.

## Deliverables

### D1. Terminal escalation threshold

Today the threshold is multiplicative and never stops the nudging
(`herdr_queue_wake_escalation.rs:37-43`):

```rust
let threshold = row.lead_notified_count.saturating_add(1)
    .saturating_mul(atm_core::boundary::TASK_STALLED_REMINDER_THRESHOLD);
if row.reminder_count < threshold { return; }
```

It fires at 10, 20, 30 … forever. One live task reached **587 reminders** over
roughly ten hours with about 58 notifications to lead.

Required: at `TASK_STALLED_REMINDER_THRESHOLD` (10), escalate **once** and
**stop nudging**.

**SOLAR-BA-008 (BLOCKING) constrains both ends of that rule.**

*Suppression must be earned, not counted.* The current order durably increments
`reminder_count` first and only then calls `maybe_escalate_task`
(`herdr_queue_wake_reminders.rs:174-203, 207-230`). A naive `count >= 10 → stop`
permanently silences a task if the daemon exits between the tenth reminder write
and the escalation write — nobody was ever told. The same happens when every
mail write fails or there is no durable target; a Herdr notify success is
transient and is **not** mailbox durability (`herdr_escalation.rs:138-151,
161-201`). Therefore: **enter terminal suppression only after a durable
successful escalation-mail write followed by the task escalation audit event.**
If either does not complete, later ticks retry the escalation and the task does
not go quiet.

*Reset must be narrow.* "Resume on a state change" is too broad and names no
fact: an `Unknown → Idle` observation or an unrelated flap would grant another
ten-reminder burst with zero task progress. Reset only on evidence of progress
or recovery — an intervening observed Active episode for that assignee and
task, a task lifecycle transition, or a reassignment. Never on a bare
`RuntimeMemberState` change.

*Both dispositions stay queryable* (RBP-001): oversight must be able to tell
"capped and told" from "capped but the notification is still retrying", and a
suppressed task must still read as stalled in `atm task list`.


### D2. The task-side effect fence

**SOLAR-BA-018 (BLOCKING).** The identical gap exists on the task side. The
reminder path reads and selects a `TaskRow`, renders, and emits
asynchronously before recording the outcome
(`herdr_queue_wake_reminders.rs:82-112, 114-203`). A concurrent close can
commit between selection and emit, producing a **post-completion nudge** — the
exact stale-nudge class this phase exists to eliminate. A concurrent
reassignment sends the *old* assignee a stale reminder for a task that is no
longer theirs.

Required: task close, reassign, and start serialize with reminder admission on
the logical `(team, task_id)` identity, with a final "still incomplete, still
this `current_assignee`" check **inside** that boundary. Again: invariant
validation at the effect boundary, not edge tracking, not a new state machine.

Test both paused-before-emit races. Close must win and reassign must win;
emission count must be zero in each. If the product instead accepts one
in-flight stale nudge, that weakening has to be written down explicitly — the
current absolute wording does not permit it.

### D3. Consecutive-refusal guard, derived from durable events

**SOLAR-BA-011 (IMPORTANT).** The design's consecutive-refusal guard is
underspecified and its cited precedent is unrelated: `release_streaks`
(`herdr_queue_wake.rs:75, 579-591`) counts queue-claim releases in RAM, keyed by
member only, and exists to bound repeated delivery attempts — not durable task
outcomes. Reusing it means a daemon restart clears the protection and the whole
queue can be dumped into lead's mailbox again.

Required: derive the streak from the tail of the existing durable task events,
not from `release_streaks`. Specify and test the threshold, the reset rule
(successful completion, start, reassignment, or empty queue), and crash
behaviour. Sequences to cover: refuse/refuse, refuse/complete/refuse, restart
mid-streak, and reassignment.

### D4. Escalation is a message, not a stream

One notification per transition. It persists in the recipient's mailbox until
read or acked; the mailbox is the durable record.

**SOLAR-BA-006 (IMPORTANT): the target policy is lead OR recipients, never
lead-gated.** An earlier draft of the design said "no lead → nobody is
notified" and called it current behaviour. That is false, and implementing it
would silently suppress configured oversight. On develop,
`herdr_escalation.rs:209-243` resolves lead and recipients independently —
`lead` is `(leads.len() == 1).then(...)`, so both zero leads and multiple leads
yield `None` — and `:289-327` writes to every configured recipient regardless of
whether a lead exists. Keep that shape. The genuinely empty case, which Rand
accepted, is *no lead AND no recipients AND a failed or unavailable Herdr
notify*; that case must be observable, not silent. Re-notification falls out of
the same invariant applied to lead — lead idle with unresolved work is nudged;
lead busy is not interrupted.

### D4a. Escalation notices are `requires_ack`, and the promise is bounded

**SOLAR-BA-022 (BLOCKING).** The design claims an ordinary mailbox message
preserves "unresolved work". It does not — message state preserves delivery and
ack hygiene, nothing more. Under BA.6's rule a non-`requires_ack` message closes
on read, so a lead can read the escalation, do nothing, and the stalled task
stays terminally suppressed **forever**, with no open scheduling item and no way
for the invariant to infer the incident is still live. That is a permanent
unnudged-task path sitting in the middle of problem (a)'s fix.

Required, and it costs no new structure:

1. Every escalation notice is sent with `requires_ack = true`, so it does not
   discharge on read.
2. State the promise precisely in the doc and the code comment: **ATM
   re-nudges until a human acknowledges custody. After that the human or
   oversight layer owns resolution, even if the source task stays
   incomplete.** ATM does not track "unresolved work" and must not claim to.

Tests: read-without-ack (still remindable), ack (stops), lead dies after
reading but before acking (redelivered), and the source task terminally
suppressed (the escalation remains the live item).

### D5. "Exactly once per episode" needs an episode identity — or a weaker claim

**SOLAR-BA-015 (BLOCKING).** The design promises exactly one blocked/dead
notification per transition and argues that mailbox durability makes the
in-RAM `blocked_since` / `last_blocked_notice` harmless. Nothing durable
identifies *which* episode a mailbox message reports:

- the canonical roster record is explicitly ephemeral
- `EscalationState` is in-RAM `HashMap`s (`herdr_escalation.rs:61-103`)
- escalation mail is an ordinary deferred, non-ack message with an arbitrary
  id and free body text (`herdr_escalation.rs:382-417`)

So after a restart with the same process still blocked, a fresh `blocked_since`
looks like a new episode. Suppressing on "any prior blocked mail for this
member" silences a genuine later unblock→reblock **forever**; ignoring history
duplicates on every restart. Read/clear weakens the mailbox as proof further.
The claim as written is not implementable.

Pick one and state it in the sprint doc:

> a. **Weaken the guarantee explicitly** to *at-least-once per daemon epoch*,
>    accepting restart duplicates, and stamp the escalation with the daemon
>    epoch so a recipient can tell a restart duplicate from a new episode.
> b. **Define a durable episode identity** carried in roster observation or
>    message metadata and queried from mailbox history. If no existing source
>    can supply one across a restart, say so plainly and justify the new
>    persisted state against the repeated-notification problem.
>
> **Recommendation: (a).** A duplicate after a daemon restart is cheap and
> self-explanatory; permanent suppression of a real re-block is exactly the
> failure this phase exists to remove. Do not spend new persisted state on
> the stronger claim unless Rand asks for it.

Either way: **stop claiming the mailbox alone answers "have I already reported
this episode."** Tests: restart while still blocked, and unblock→reblock.

### D6. Lifecycle notifications are deferred, never immediate

**SOLAR-BA-016 (IMPORTANT).** The start receipt and the mandatory
completion/reassignment reports have no delivery-mode rule. On the ordinary
`atm send` default they would steer an Active assigner or lead the instant they
are written — recreating problem (b) through the very machinery meant to fix
it. Task assignments are already forced deferred when a `task_id` is present
(`send/mod.rs:339-347`); the receipt does not exist yet and nothing binds it.

Required: every lifecycle notification — start receipt, completion report,
refusal, reassignment notice, escalation — is an ordinary **durable deferred**
mailbox message unless Rand explicitly chooses immediate for a specific one.
None of them is a task assignment, and all obey the same no-diversion rule as
everything else in this sprint.

Retire `EscalationState.blocked_cursor` (round-robin fairness across blocked
members — pointless once each episode is reported once). **Retain**
`blocked_since` and `last_blocked_notice`.

## Affected paths

- `crates/atm-http-runtime/src/herdr_queue_wake_escalation.rs`
- `crates/atm-http-runtime/src/herdr_escalation.rs`
- `crates/atm-http-runtime/src/herdr_queue_wake_reminders.rs` — outcome
  recording and the emit boundary only
- `crates/atm-core/src/send/mod.rs` — the deferred rule for lifecycle
  notifications

## Paths that must not change

- `crates/atm-http-runtime/src/herdr_queue_wake.rs` candidate collection — BA.4
- `crates/atm-storage*` schema — BA.3
- `crates/atm/src/commands/*` — BA.5

## Acceptance criteria

1. A task reaching 10 reminders produces one escalation and then **no further
   reminders**, proven by advancing the clock well past the eleventh interval
   and asserting the reminder count is unchanged.
2. **Crash before escalation durability**: a daemon killed after the tenth
   reminder write but before the escalation mail write retries the escalation
   on the next tick and does **not** enter suppression.
3. **No durable target**: with no lead, no recipients, and every mail write
   failing, the task does not enter suppression, and the condition is
   observable in `atm task list` and in logs rather than silent.
4. Escalation reaches configured recipients when the team has zero leads, and
   again when it has two leads. Must fail against any implementation that
   gates recipients on a resolved lead.
5. Reminders resume after an intervening observed Active episode, a task
   lifecycle transition, or a reassignment — and do **not** resume after a
   bare `Unknown → Idle` roster flap.
6. **Task dispatch race**: a task closed after selection and before emit
   produces zero nudges; a task reassigned in the same window sends nothing to
   the old assignee. Both must fail against `origin/develop`.
7. Consecutive refusals are bounded by a streak derived from durable task
   events, and the bound survives a daemon restart.
8. A daemon restart while a member is still blocked behaves per the D5 choice,
   and an unblock→reblock always produces a new notification.
9. Starting or closing a task while the assigner is Active persists the
   receipt with **no** steer, and it is delivered once the assigner is Idle. A
   receipt write failure does not roll back the started task and is
   idempotently retryable.
10. "Capped and told" and "capped but the notification is still retrying" are
    distinguishable in `atm task list` output (RBP-001).
11. An escalation notice read but not acked is still remindable; acking stops
    it; a lead that dies between read and ack has it redelivered. A terminally
    suppressed source task does not suppress its escalation notice.

## Required validation

- `just test`, `just lint`
- crash-boundary tests on both sides of the escalation mail write and the
  escalation audit event
- the paused-before-emit close and reassign races
- criteria 4 and 6 each fail on `origin/develop`

## Non-closure

- Candidate collection, the state matrix, and the roster-side fence are BA.4.
- No new table and no new state machine. D5 option (b) is the one place new
  persisted state may be proposed, and only with the written justification
  that deliverable requires.
