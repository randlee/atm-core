---
phase: AX
sprint: AX.5
title: Task reminder cycle in the Herdr pump
branch: feature/ax5-task-reminder-cycle
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/ax5-task-reminder-cycle
integration_branch: integrate/phase-ax
status: draft
recommended_agent: arch-ctm
recommended_model: deep-reasoning
execution_track: C
parallel_with: []
dependency_relations:
  - prerequisite: AX.2
    dependent: AX.5
    relation: must_follow
    rationale: the task step emits through HerdrNudgeTarget.rendered_nudge and prompt(text) from AX.2; starts only after tracks A and B are both merged to integrate/phase-ax.
  - prerequisite: AX.4
    dependent: AX.5
    relation: must_follow
    rationale: reads the tasks table and TaskStore (AX.3) through LocalServiceRuntime::task_store() and drives its scenarios with the AX.4 flags.
  - prerequisite: AX.5
    dependent: AX.6
    relation: must_follow
    rationale: AX.6 triggers the lead notification from the reminder counter this sprint maintains inside the same pump function.
---

# AX.5 — Task reminder cycle in the Herdr pump

Add the asynchronous half of task tracking for Herdr-backed members: an
idle assignee with an open task is reminded of it on a fixed cadence.
The pump reads task state and writes only reminder bookkeeping; it never
changes task state. Lead notification and doctor are AX.6.

## Pump rule

Today `HerdrQueueWakePump::tick_once`
(`crates/atm-http-runtime/src/herdr_queue_wake.rs`, cadence
`HERDR_POLL_INTERVAL_MS = 5_000`) takes `now = IsoTimestamp::now()`
(line 103), resolves the pending store (line 106) and the roster (line
114) from `service_runtime`, seeds candidates from
`list_pending_members`, lists Herdr sessions, `collect_idle_members`
keeps members that are both pending and idle, and `drain_eligible`
claims and prompts each of them while `stats.prompted <
HERDR_MAX_PROMPTS_PER_TICK` (line 256, incremented at line 371). This
sprint widens the idle set and adds a task step before the existing
drain:

```
now       := (self.clock)()                                                # injected, default IsoTimestamp::now
tasks     := service_runtime.task_store()?                                # resolved per tick like pending/roster
idle      := every roster member of a Herdr team whose Herdr session is Idle|Done   (pending or not)
reminded  := ∅
for member in idle, in roster order:
    open := tasks.open_tasks(member)                            # non-Complete, oldest first
    target := the Active row in open, else the first Assigned row, else none
    if target is none: continue
    if target.last_reminded_at is Some and now − it < TASK_REMINDER_INTERVAL_MS: continue
    if last_attempt[member] is Some and now − it < TASK_REMINDER_INTERVAL_MS: continue   # guards a store that failed to record
    if stats.prompted == HERDR_MAX_PROMPTS_PER_TICK: break
    last_attempt[member] := now
    dispatch := build_task_reminder_dispatch(service_runtime, member, target)          # C1
    match dispatch:
        Ok(None)          => continue                                    # assignee no longer Herdr-backed; nothing recorded
        Err(_)            => tasks.record_reminder(member, target.task_id, now, Unrenderable);
                             stats.task_reminders_unrenderable += 1
        Ok(Some(dispatch)) =>
            match emit(dispatch):
                Ok(_)                 => tasks.record_reminder(member, target.task_id, now, Emitted)
                                         stats.prompted += 1; stats.task_reminders += 1; reminded += member
                Err(HerdrUnavailable) => stats.breaker_open += 1         # no record; next tick retries
                Err(other)            => log; no record; next tick retries
eligible := (pending ∩ idle) \ reminded                                   # existing drain, unchanged
drain_eligible(eligible)                                                  # still bounded by stats.prompted
```

Properties that must hold and are each tested:

1. **One reminder surfaces the next task.** The reminder for the first
   `Assigned` row is the same reminder that nags about an `Active` one.
   The agent's `atm ack` moves `Assigned → Active`; the reminder does not.
2. **Cadence.** First reminder on the first idle tick after assignment;
   then at most one per `TASK_REMINDER_INTERVAL_MS` (60 s) per member
   while the member stays idle with an open task. Both the stored
   `last_reminded_at` and the in-memory last-attempt guard enforce it,
   so a store write failure cannot cause a burst.
3. **Active wins.** A member with an `Active` task is never reminded of a
   different `Assigned` task.
4. **Emit failure records nothing.** Breaker open or a Herdr error writes
   no `Reminded` row and does not touch the pending marker; the next
   tick retries. `Unrenderable` is recorded so a broken override does
   not spin the log.
5. **One prompt per member per tick, one budget.** A member reminded
   this tick is removed from the queued-mail drain; its pending claim is
   left for the next tick. Reminders and queued-mail prompts share
   `stats.prompted`, so a tick never emits more than
   `HERDR_MAX_PROMPTS_PER_TICK` Herdr prompts in total.
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
  `tick_once` builds the idle set from the roster
  (`shared_roster_store_arc()`, `DurableRosterStore`) and Herdr status
  independently of the pending list; `HerdrCandidate` keeps its
  `pending` flag; new `remind_open_tasks` runs before `drain_eligible`
  and returns the reminded set; `HerdrQueueWakeStats` gains
  `task_reminders: usize` and `task_reminders_unrenderable: usize`; the
  `herdr_queue_poll_tick` log record (line 148) gains the same two
  fields. Code contract C2.
- [ ] D2 — clock seam (code contract C2): `HerdrQueueWakePump` gains a
  `clock: Arc<dyn Fn() -> IsoTimestamp + Send + Sync>` field, defaulting
  to `IsoTimestamp::now` in `new`, settable with `with_clock` (test
  only in practice; production never calls it). Every `now` in
  `tick_once` and the task step reads the clock. The bootstrap call site
  `crates/atm-daemon-bootstrap/src/replacement_handler.rs` line 244 is
  unchanged: `new`'s four parameters stay as they are.
- [ ] D3 — task reminder dispatch, code contract C1, in
  `crates/atm-core/src/nudge_dispatch.rs` beside
  `rebuild_received_hook_dispatch`: resolves the recipient snapshot with
  `DeliveryPolicyCoordinator::new().resolve_recipient_snapshot(runtime,
  team, agent)` exactly as that function does, builds a
  `PostSendHookEvent` from the task row (`sender = assigner`,
  `message_id = assignment_message_id`, `description = row.description`,
  `task_id`, `requires_ack = true`, `is_ack = false`, `sender_team =
  team`, `sender_chat_id = None`, `sender_host` from the assignment
  message when it still exists else `None`), and renders through
  `build_built_in_dispatch` with `NudgeMode::Deferred` (Task body). The
  reminder does not depend on the mail row, so a task whose assignment
  message was already acknowledged or purged still renders.
- [ ] D4 — Herdr task mail carries no pending marker
  (`crates/atm-core/src/write/pipeline.rs` `mark_pending_if_deferred`,
  around line 112): skipped when the request has `task_id` and the
  recipient's `DeliveryRecipientSnapshot::local_herdr_post_send` is set
  (`crates/atm-core/src/delivery_policy.rs` line 64). AX.1 already made
  every task send `Deferred`, so no steer suppression is needed.
  `SendOutcome` (`crates/atm-core/src/send/outcome.rs`) gains
  `task_tracked: bool` (serialised only when true, code contract C3).
  tmux recipients keep the marker; graft recipients are unaffected (their
  queue-kind wire handoff is not marker-driven). This is the recorded
  ADR-054 exception (D5).
- [ ] D5 — docs: ADR-061 gains a "Reminder cycle" section (rule,
  properties, 60 s, 5 s tick, shared prompt budget, Herdr only); dated
  ADR-054 amendment records that task-tagged mail to a Herdr recipient
  carries no pending-nudge marker because the `tasks` row is its
  delivery record; `docs/requirements.md` §15.4 gains the reminder rule
  and the marker exception; `docs/user-documents/tasks.md` (AX.4)
  describes when the `task` body is re-sent.
- [ ] D6 — tests listed under Required validation.

### Paths to delete

None.

## Code contracts

### C1 — reminder dispatch

```rust
// crates/atm-core/src/nudge_dispatch.rs
/// Ok(None): the assignee is no longer a Herdr-backed roster member (or has
/// no session); nothing to emit. Err: the Task template failed to render.
pub fn build_task_reminder_dispatch(
    runtime: &LocalServiceRuntime,
    member: &MemberKey,
    row: &TaskRow,
) -> Result<Option<BuiltInPostSendDispatch>, AtmError>;
```

### C2 — pump surface

```rust
// crates/atm-http-runtime/src/herdr_queue_wake.rs
pub const TASK_REMINDER_INTERVAL_MS: u64 = 60_000;

impl HerdrQueueWakePump {
    // unchanged signature; clock defaults to IsoTimestamp::now
    pub fn new(
        service_runtime: LocalServiceRuntime,
        selector: Arc<dyn MessageReceivedHookSelector>,
        runtime_health: RuntimeHealth,
        herdr_process: Arc<dyn HerdrProcessAdapter>,
    ) -> Self;
    /// Test seam for the cadence properties.
    pub fn with_clock(self, clock: Arc<dyn Fn() -> IsoTimestamp + Send + Sync>) -> Self;
}
// new private fields: clock, last_attempt: Arc<Mutex<HashMap<MemberKey, IsoTimestamp>>>
// TaskStore is resolved per tick: self.service_runtime.task_store()?
```

### C3 — send outcome field

```rust
// crates/atm-core/src/send/outcome.rs (existing struct, new field)
pub struct SendOutcome {
    /* existing fields */
    /// True when the recipient is Herdr-backed and the message carries a
    /// task_id: no pending marker; the daemon reminder step is the only
    /// nudge source.
    #[serde(default, skip_serializing_if = "is_false")]
    pub task_tracked: bool,
}
```

### Unchanged surfaces

`TaskStore` trait and `task_state` transition table (AX.3);
`claim_next_pending`; `HERDR_POLL_INTERVAL_MS`; `HERDR_MAX_PROMPTS_PER_TICK`
(`ac02`); the breaker and release-streak logic; ADR-058 argv shape;
`rebuild_received_hook_dispatch`; queued non-task mail behaviour;
`HerdrQueueWakePump::new`'s parameters; the doctor `herdr_queue_pump`
report (reminder counts are observable through the `herdr_queue_poll_tick`
log record and `atm list --task-events`, not doctor).

## Acceptance criteria

1. Two tasks to one idle Herdr member: one reminder for the oldest on
   the first tick, none for the second; with the clock advanced 65 s a
   second reminder for the same task; after `atm ack` the reminders
   continue for it; after `--task-complete` the next tick reminds the
   second task.
2. A member with an open task and queued non-task mail receives exactly
   one Herdr prompt on the tick (the reminder); the queued mail is
   prompted on the following tick. Seventeen idle members with open
   tasks and queued mail produce exactly sixteen prompts on one tick.
3. `atm send --task-id` and `atm queue --task-id` to a Herdr member set
   no pending marker and report `"task_tracked": true`; to a tmux member
   they set the marker and omit the field.
4. With the breaker open, no `Reminded` row is written; when it closes
   the reminder is emitted on the next tick. An assignee moved off the
   Herdr backend gets no reminder and no `Reminded` row.
5. `herdr_queue_poll_tick` carries `task_reminders`; `just validate`
   green; `python scripts/check-nudge-taxonomy.py` unchanged allowlist;
   ADR-061 and ADR-054 amendments and requirements §15.4 merged.

## Required validation

- `crates/atm-http-runtime/src/herdr_queue_wake.rs` tests
  (`ax5_01_*` … naming, `FakeHerdrProcessAdapter`, `with_clock` fake
  clock, in-memory recording `TaskStore` double installed through
  `LocalServiceRuntime::with_task_store`): one test per property 1–6;
  AC 1, 2, 4 scenarios; the existing `ac01`–`ac12` suite unchanged and
  green.
- `crates/atm-core/src/nudge_dispatch.rs` tests: reminder dispatch
  renders the Task body from a row whose assignment message is absent
  from the mailbox; override failure returns `Err`; non-Herdr assignee
  returns `Ok(None)`.
- `crates/atm-core/tests/task_state.rs` (from AX.3) extended with AC 3
  for a Herdr and a tmux recipient on both `atm send` and `atm queue`.
- Real-startup integration test in `crates/atm-daemon-bootstrap`: the
  pump started by daemon bootstrap resolves a `TaskStore` and emits a
  task reminder against a fixture Herdr adapter, with negative proof (an
  assignee with no open task gets no prompt).
- `just validate`; quality-mgr Final Quality Report on the PR; `arch-qa`
  on the ADR-054 amendment.

## Out of scope

Lead notification and doctor (AX.6); task reminders for tmux and graft
members; configurable interval; publishing reminder counters to doctor;
task priority; reassignment; expiry.
