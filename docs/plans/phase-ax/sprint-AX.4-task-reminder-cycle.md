---
phase: AX
sprint: AX.4
title: Task reminder cycle in the Herdr pump
branch: feature/ax4-task-reminder-cycle
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/ax4-task-reminder-cycle
integration_branch: integrate/phase-ax
status: draft
recommended_agent: arch-ctm
recommended_model: deep-reasoning
dependency_relations:
  - prerequisite: AX.3
    dependent: AX.4
    relation: must_follow
    rationale: reads the tasks table and TaskStore delivered by AX.3; the steer and marker suppression added here depend on the task rows existing.
  - prerequisite: AX.4
    dependent: AX.5
    relation: must_follow
    rationale: AX.5 triggers the lead notification from the reminder counter this sprint maintains.
---

# AX.4 — Task reminder cycle in the Herdr pump

Add the asynchronous half of task tracking for Herdr-backed members: an
idle assignee with an open task is reminded of it on a fixed cadence.
The pump reads task state and writes only reminder bookkeeping; it never
changes task state. Lead notification and doctor are AX.5.

## Pump rule

Today `HerdrQueueWakePump::tick_once`
(`crates/atm-http-runtime/src/herdr_queue_wake.rs`, cadence
`HERDR_POLL_INTERVAL_MS = 5_000`) seeds candidates from
`PendingNudgeStore::list_pending_members`, lists Herdr sessions,
`collect_idle_members` keeps members that are both pending and idle, and
`drain_eligible` claims and prompts each of them up to
`HERDR_MAX_PROMPTS_PER_TICK = 16`. This sprint widens the idle set and
adds a task step before the existing drain:

```
idle      := every roster member of a Herdr team whose Herdr session is Idle|Done   (pending or not)
reminded  := ∅
for member in idle, in roster order:
    open := TaskStore::open_tasks(member)                      # non-Complete, oldest first
    target := the Active row in open, else the first Assigned row, else none
    if target is none: continue
    if target.last_reminded_at is Some and now − it < 60 s: continue
    if in-memory last_attempt[member] is Some and now − it < 60 s: continue   # guards a store that failed to record
    if prompts_this_tick == HERDR_MAX_PROMPTS_PER_TICK: break
    last_attempt[member] := now
    dispatch := build_task_reminder_dispatch(team, target)    # Task body from the task row (C1)
    match dispatch:
        None            => record_reminder(target, now, Unrenderable); stats.task_reminders_unrenderable += 1
        Some(dispatch)  =>
            match emit(dispatch):
                Ok(_)                 => record_reminder(target, now, Emitted); reminded += member; prompts_this_tick += 1
                Err(HerdrUnavailable) => stats.breaker_open += 1              # no record; next tick retries
                Err(other)            => log; no record; next tick retries
eligible := (pending ∩ idle) \ reminded                          # existing drain, one prompt per member per tick
drain_eligible(eligible)
```

Properties that must hold and are each tested:

1. **One reminder surfaces the next task.** The reminder for the first
   `Assigned` row is the same reminder that nags about an `Active` one.
   The agent's `atm ack` moves `Assigned → Active`; the reminder does not.
2. **Cadence.** First reminder on the first idle tick after assignment;
   then at most one per 60 s per member while the member stays idle with
   an open task. Both the stored `last_reminded_at` and the in-memory
   last-attempt guard enforce it, so a store write failure cannot cause
   a burst.
3. **Active wins.** A member with an `Active` task is never reminded of a
   different `Assigned` task.
4. **Emit failure records nothing.** Breaker open or a Herdr error writes
   no `Reminded` row and does not touch the pending marker; the next
   tick retries. `Unrenderable` is recorded so a broken override does
   not spin the log.
5. **One prompt per member per tick.** A member reminded this tick is
   removed from the queued-mail drain; its pending claim is left for the
   next tick. Task reminders count against
   `HERDR_MAX_PROMPTS_PER_TICK`.
6. **The pump never calls a state-changing path**; the only `TaskStore`
   methods it uses are `open_tasks` and `record_reminder`.

Daemon-down behaviour is unchanged from queued mail: no reminder until
the pump runs.

## Deliverables

This is the authoritative deliverable checklist. Every listed deliverable
lands production-ready for the scope this sprint claims; partial or
shape-only completion fails the sprint.

- [ ] D1 — pump task step in
  `crates/atm-http-runtime/src/herdr_queue_wake.rs` per the rule above:
  `tick_once` builds the idle set from the roster and Herdr status
  independently of the pending list; `HerdrCandidate` keeps its
  `pending` flag; new `remind_open_tasks` runs before `drain_eligible`
  and returns the reminded set; `HerdrQueueWakeStats` gains
  `task_reminders: usize` and `task_reminders_unrenderable: usize`. The
  pump holds `Arc<dyn TaskStore>` (imported via `atm_core::boundary`)
  and a per-member last-attempt map. Constructor call site:
  `crates/atm-daemon-bootstrap/src/replacement_handler.rs` (around line
  244) passes the rusqlite `TaskStore`.
- [ ] D2 — task reminder dispatch: new
  `build_task_reminder_dispatch(runtime, team, row: &TaskRow) ->
  Option<ReceivedHookDispatch>` in `crates/atm-core/src/nudge_dispatch.rs`
  that builds a `PostSendHookEvent` from the task row (`from =
  assigner`, `message_id = assignment_message_id`, `description =
  row.description`, `task_id`, `requires_ack = true`, `is_ack = false`)
  and renders through `build_built_in_dispatch` with `NudgeMode::Deferred`
  (Task body, AX.1 C2). The reminder does not re-read the mail row, so a
  task whose assignment message was already acked still renders.
- [ ] D3 — Herdr task mail carries no steer and no marker
  (`crates/atm-core/src/write/pipeline.rs`): `mark_pending_if_deferred`
  (around line 112) skips the marker and the steer-suppression block
  (around line 243) emits no immediate dispatch when the request has
  `task_id` and the recipient's `DeliveryRecipientSnapshot::local_herdr_post_send`
  is set (`crates/atm-core/src/delivery_policy.rs`). `SendOutcome`
  (`crates/atm-core/src/send/outcome.rs`) gains `task_tracked: bool`
  (serialised only when true, code contract C3) so `atm send --json`
  and `atm queue --json` say why no nudge fired; the retained log line
  records the same. tmux and graft recipients keep
  today's behaviour (steer on `atm send`, marker on `atm queue`). This is
  the recorded ADR-054 exception (D4).
- [ ] D4 — docs: ADR-061 gains a "Reminder cycle" section (rule,
  properties, 60 s, 5 s tick, Herdr only); dated ADR-054 amendment
  records that task-tagged mail to a Herdr recipient carries no
  pending-nudge marker because the `tasks` row is its delivery record;
  `docs/requirements.md` §15.4 gains the reminder rule and the marker
  exception; `docs/user-documents/nudge-templates.md` describes when the
  `task` body is re-sent.
- [ ] D5 — tests listed under Required validation.

### Paths to delete

None.

## Code contracts

### C1 — reminder dispatch

```rust
// crates/atm-core/src/nudge_dispatch.rs
pub fn build_task_reminder_dispatch<R>(
    runtime: &R,
    team: &TeamName,
    row: &TaskRow,
) -> Option<ReceivedHookDispatch>
where
    R: RetainedServiceRuntime + ?Sized;
```

Returns `None` only when the Task template fails to render (override
error); the caller records `ReminderOutcome::Unrenderable`.

### C2 — pump surface

```rust
// crates/atm-http-runtime/src/herdr_queue_wake.rs
impl HerdrQueueWakePump {
    pub fn new(
        pending: Arc<dyn PendingNudgeStore>,
        tasks: Arc<dyn TaskStore>,            // new
        roster: Arc<dyn RosterStore>,         // already available to bootstrap
        herdr: Arc<dyn HerdrProcessAdapter>,
        /* existing selector / breaker / clock params unchanged */
    ) -> Self;
}
pub const TASK_REMINDER_INTERVAL_MS: u64 = 60_000;
```

### C3 — send outcome field

```rust
// crates/atm-core/src/send/outcome.rs (existing struct, new field)
pub struct SendOutcome {
    /* existing fields */
    /// True when the recipient is Herdr-backed and the message carries a
    /// task_id: no steer, no pending marker; the daemon reminder step is
    /// the only nudge source.
    #[serde(default, skip_serializing_if = "is_false")]
    pub task_tracked: bool,
}
```

### Unchanged surfaces

`TaskStore` trait and `task_state` transition table (AX.3);
`claim_next_pending`; `HERDR_POLL_INTERVAL_MS`; `HERDR_MAX_PROMPTS_PER_TICK`
(`ac02`); the breaker and release-streak logic; ADR-058 argv shape;
`rebuild_received_hook_dispatch`; queued non-task mail behaviour.

## Acceptance criteria

1. Two tasks to one idle Herdr member: one reminder for the oldest on
   the first tick, none for the second; after 65 s a second reminder for
   the same task; after `atm ack` the reminders continue for it; after
   `--task-complete` the next tick reminds the second task.
2. A member with an open task and queued non-task mail receives exactly
   one Herdr prompt on the tick (the reminder); the queued mail is
   prompted on the following tick.
3. `atm send --task-id` and `atm queue --task-id` to a Herdr member emit
   no steer, set no pending marker, and report `"task_tracked": true`;
   to a tmux member they behave as before and omit the field.
4. With the breaker open, no `Reminded` row is written; when it closes
   the reminder is emitted on the next tick.
5. `just validate` green; `python scripts/check-nudge-taxonomy.py`
   unchanged allowlist; ADR-061 and ADR-054 amendments and requirements
   §15.4 merged.

## Required validation

- `crates/atm-http-runtime/src/herdr_queue_wake.rs` tests
  (`ax4_01_*` … naming, `FakeHerdrProcessAdapter`, injected clock,
  `DummyTaskStore` replaced by an in-memory recording double): one test
  per property 1–6; AC 1, 2, 4 scenarios; the existing `ac01`–`ac12`
  suite unchanged and green.
- `crates/atm-core/src/nudge_dispatch.rs` tests: reminder dispatch
  renders the Task body from a row whose assignment message is absent
  from the mailbox; override failure returns `None`.
- `crates/atm-core/tests/task_state.rs` (from AX.3) extended with AC 3
  for a Herdr and a tmux recipient on both `atm send` and `atm queue`.
- Real-startup integration test in `crates/atm-daemon-bootstrap`: the
  pump started by daemon bootstrap has a `TaskStore` and emits a task
  reminder against a fixture Herdr adapter, with negative proof (an
  assignee with no open task gets no prompt).
- `just validate`; quality-mgr Final Quality Report on the PR; `arch-qa`
  on the ADR-054 amendment.

## Out of scope

Lead notification and doctor (AX.5); task reminders for tmux and graft
members; configurable interval; task priority; reassignment; expiry.
