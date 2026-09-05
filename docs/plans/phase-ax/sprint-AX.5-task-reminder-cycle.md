---
phase: AX
sprint: AX.5
title: Task reminder cycle in the Herdr pump
branch: feature/ax5-task-reminder-cycle
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/ax5-task-reminder-cycle
integration_branch: integrate/phase-ax
status: complete
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
`HERDR_POLL_INTERVAL_MS = 5_000`) calls `IsoTimestamp::now()` once, as
the `last_tick_at` initialiser (line 103; the clock seam replaces that
call and supplies the task step's `now`), resolves the pending store
(line 106) and the roster (line
114) from `service_runtime`, seeds candidates from
`list_pending_members`, lists Herdr sessions, `collect_idle_members`
keeps members that are both pending and idle, and `drain_eligible`
claims and prompts each of them while `stats.prompted <
HERDR_MAX_PROMPTS_PER_TICK` (line 256, incremented at line 371). This
sprint widens the observed set and adds a task step **after** the
existing drain:

```
now       := (self.clock)()                                                # injected, default IsoTimestamp::now
idle      := every roster member of a Herdr team whose Herdr session is Idle|Done   (pending or not)
blocked   := every roster member of a Herdr team whose Herdr session is Blocked
eligible  := pending ∩ idle                                                # existing drain input, unchanged
prompted  := drain_eligible(eligible)                                      # existing drain runs FIRST; bounded by stats.prompted
tasks     := match service_runtime.task_store():
                 Ok(store) => store
                 Err(e)    => { warn on first failure only; stats.task_step_skipped = true; save_stats; return }   # drain already ran
for member in idle ∪ blocked, in roster order:
    open := tasks.open_tasks(member)                            # non-Complete, oldest first
    target := the Active row in open, else the first Assigned row, else none
    if target is none: continue
    if member in prompted: last_attempt[member] := now; continue          # the drain's nudge rendered the Task body this tick
    if target.last_reminded_at is Some and now − it < TASK_REMINDER_INTERVAL_MS: continue
    if last_attempt[member] is Some and now − it < TASK_REMINDER_INTERVAL_MS: continue   # guards a store that failed to record
    if member in blocked:
        tasks.record_reminder(member, target.task_id, now, Blocked)       # no prompt: Herdr rejects input to a blocked agent
        stats.task_reminders_blocked += 1; last_attempt[member] := now
        continue
    if stats.prompted == HERDR_MAX_PROMPTS_PER_TICK: break
    dispatch := build_task_reminder_dispatch(service_runtime, member, target)          # C1
    match dispatch:
        Ok(None)          => continue                                    # assignee no longer Herdr-backed; nothing recorded
        Err(_)            => tasks.record_reminder(member, target.task_id, now, Unrenderable);
                             stats.task_reminders_unrenderable += 1; last_attempt[member] := now
        Ok(Some(dispatch)) =>
            match self.selector.select_emitter(&dispatch):                # line 311; the sealed core boundary
                None          => log herdr_queue_poll_outcome outcome="reminder_target_not_present"; continue   # no record, no budget, no stamp
                Some(emitter) =>
                    match emitter.emit_received_message(dispatch, RequestDeadline::after(HERDR_REQUEST_DEADLINE)):   # line 350
                        Ok(_)                 => tasks.record_reminder(member, target.task_id, now, Emitted)
                                                 stats.prompted += 1; stats.task_reminders += 1; last_attempt[member] := now
                        Err(HerdrUnavailable) => stats.breaker_open += 1         # no record, no stamp; next 5 s tick retries
                        Err(other)            => log; no record, no stamp; next 5 s tick retries
```

Properties that must hold and are each tested:

1. **One reminder surfaces the next task.** The reminder for the first
   `Assigned` row is the same reminder that nags about an `Active` one.
   The agent's `atm ack` moves `Assigned → Active`; the reminder does not.
2. **Cadence.** First reminder on the first idle tick after assignment;
   then at most one per `TASK_REMINDER_INTERVAL_MS` (60 s) per member
   while the member stays idle with an open task. Both the stored
   `last_reminded_at` and the in-memory last-attempt guard enforce it,
   so a store write failure cannot cause a burst. The guard is stamped
   only when an outcome is recorded (`emitted`, `blocked`,
   `unrenderable`) or the drain prompted the member; a breaker-open,
   transient emit error, or absent emitter leaves it unset, so the next
   5 s tick retries exactly as AC 4 promises.
3. **Active wins.** A member with an `Active` task is never reminded of a
   different `Assigned` task.
4. **Emit failure records nothing.** Breaker open or a Herdr error writes
   no `Reminded` row and does not touch the pending marker; the next
   tick retries. `Unrenderable` is recorded so a broken override does
   not spin the log.
5. **Drain first, one budget.** The existing drain runs first. A member
   it prompted this tick is not reminded this tick and its last-attempt
   is stamped `now`, so a freshly delivered task gets its first reminder
   one interval after the delivery nudge, not 5 s after it. Reminders and
   queued-mail prompts share `stats.prompted`, so a tick never emits more
   than `HERDR_MAX_PROMPTS_PER_TICK` Herdr prompts in total.
6. **The pump applies no task-state transition**: it calls no path that
   writes `tasks.state` or an `Assigned` / `Acked` / `Completed` event
   row. Read and append-only audit methods are permitted (`open_tasks`,
   `record_reminder` here; `list_task_events`, `record_lead_notified`
   from AX.6). The pump never calls `HerdrProcessAdapter::prompt`
   directly; every emission goes through `self.selector`.
7. **Blocked is counted, not hidden.** A blocked assignee with an open
   task gets a `Reminded` row with outcome `blocked` on the same 60 s
   cadence and no Herdr prompt; `reminder_count` advances, so the AX.6
   lead notification and `ATM_TASK_STALLED` fire on the same schedule as
   for an idle assignee. Runtime health records the member as `Blocked`,
   never `Active`.
8. **A missing `TaskStore` disables only the task step.** When
   `service_runtime.task_store()` errs, `task_reminders` stays 0 and
   `drain_eligible` still runs; queued-mail delivery is never affected.
   The warning is logged on transition only (once when the store first
   becomes unavailable, once on recovery), not on every 5 s tick, and
   the poll-tick record carries `task_step_skipped: bool` so the
   condition stays observable while the log stays quiet.

Daemon-down behaviour is unchanged from queued mail: no reminder until
the pump runs.

## Deliverables

This is the authoritative deliverable checklist. Every listed deliverable
lands production-ready for the scope this sprint claims; partial or
shape-only completion fails the sprint.

- [x] D1 — pump task step in
  `crates/atm-http-runtime/src/herdr_queue_wake.rs` per the rule above:
  `tick_once` builds the idle set from the roster
  (`shared_roster_store_arc()`, `DurableRosterStore`) and Herdr status
  independently of the pending list; `HerdrCandidate` keeps its
  `pending` flag; `drain_eligible` returns the set of members it
  prompted; new `remind_open_tasks` runs after it and skips that set
  (property 5); a failing `task_store()` skips only `remind_open_tasks`
  (property 8). `stats.idle_members` **keeps** its current meaning
  (members both pending and idle, line 228–235) so existing dashboards
  and the AX.7 evidence read unchanged; `HerdrQueueWakeStats` gains
  `task_reminders: usize`, `task_reminders_unrenderable: usize`, and
  `task_reminders_blocked: usize`; the `herdr_queue_poll_tick` log record
  (line 148) gains the same three fields. `runtime_state` (line 530) maps
  `HerdrAgentStatus::Blocked` to the new `RuntimeMemberState::Blocked`
  (`crates/atm-core/src/protocol.rs` line 433, serialised `blocked`) so
  `atm members` shows `state=blocked age=…`; `Working` alone maps to
  `Active`. The CLI and daemon ship together, so the new wire value needs
  no compatibility shim. Code contract C2.
- [x] D2 — clock seam (code contract C2): `HerdrQueueWakePump` gains a
  `clock: Arc<dyn Fn() -> IsoTimestamp + Send + Sync>` field, defaulting
  to `IsoTimestamp::now` in `new`, settable with `with_clock` (test
  only in practice; production never calls it). Every `now` in
  `tick_once` and the task step reads the clock. The bootstrap call site
  `crates/atm-daemon-bootstrap/src/replacement_handler.rs` line 244 is
  unchanged: `new`'s four parameters stay as they are.
- [x] D3 — task reminder dispatch, code contract C1, in
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
- [x] D4 — no marker exception. Task mail to a Herdr member sets the
  pending marker like every `Deferred` send (AX.1 D9;
  `mark_pending_if_deferred`, pipeline.rs line 112, is unchanged). The
  drain's nudge for that marker already renders the Task body because
  kind selection keys on `task_id` (AX.1 C2), so the task step treats a
  drain prompt as this tick's reminder attempt (property 5). The marker
  is backend-neutral: an assignee moved to tmux before reading is nudged
  by the tmux hook path, and an assignee moved after reading is an
  ordinary non-Herdr assignee (reminders out of scope, phase plan §5).
  `SendOutcome` is unchanged.
- [x] D5 — docs: ADR-061 gains a "Reminder cycle" section (rule,
  properties, 60 s, 5 s tick, drain-first ordering, shared prompt budget,
  Herdr only, `idle_members` semantics unchanged, Blocked handling);
  `docs/requirements.md` §15.4 gains the reminder rule;
  `docs/user-documents/tasks.md` (AX.4) describes when the `task` body is
  re-sent. No ADR-054 amendment from this sprint: the marker contract is
  unchanged.
- [x] D6 — tests listed under Required validation.

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
// TaskStore is resolved per tick via self.service_runtime.task_store(); Err skips only the task step (property 8)
```

### Unchanged surfaces

`TaskStore` trait and `task_state` transition table (AX.3);
`mark_pending_if_deferred`; `SendOutcome`; ADR-054; `claim_next_pending`;
`HerdrProcessAdapter::prompt` is never called from the pump directly;
`HERDR_POLL_INTERVAL_MS`; `HERDR_MAX_PROMPTS_PER_TICK`
(`ac02`); the breaker and release-streak logic; ADR-058 argv shape;
`rebuild_received_hook_dispatch`; queued non-task mail behaviour;
`HerdrQueueWakePump::new`'s parameters; the doctor `herdr_queue_pump`
report (reminder counts are observable through the `herdr_queue_poll_tick`
log record and `atm list --task-events`, not doctor).

## Acceptance criteria

1. Two tasks to one idle Herdr member whose marker has already been
   drained: one reminder for the oldest on the first tick, none for the
   second; with the clock advanced 65 s a
   second reminder for the same task; after `atm ack` the reminders
   continue for it; after `--task-complete` the next tick reminds the
   second task.
2. A member with a reminder due and freshly queued non-task mail
   receives exactly one Herdr prompt on the tick (the drain's queue
   nudge); its reminder follows one interval later. Seventeen idle
   members with reminders due and no queued mail produce exactly sixteen
   prompts on one tick and the seventeenth is reminded on the next.
3. `atm send --task-id` and `atm queue --task-id` to a Herdr member set
   the pending marker; the drain's nudge renders the Task body; no
   reminder is emitted until 60 s after that nudge.
4. With the breaker open, no `Reminded` row is written; when it closes
   the reminder is emitted on the next tick. When `select_emitter`
   returns `None`, no row is written, no budget is consumed, and one
   `herdr_queue_poll_outcome` log names `reminder_target_not_present`.
   An assignee moved off the Herdr backend gets no reminder and no
   `Reminded` row.
5. `herdr_queue_poll_tick` carries `task_reminders`,
   `task_reminders_unrenderable`, and `task_reminders_blocked`; `just
   validate` green; `python scripts/check-nudge-taxonomy.py` unchanged
   allowlist; ADR-061 and requirements §15.4 merged.
6. An assignee whose Herdr status is `blocked` shows `state=blocked` in
   `atm members`, receives no prompt, and accrues one `Reminded` row with
   outcome `blocked` per 60 s while blocked; when it returns to idle the
   next reminder is `emitted`.
7. With no `TaskStore` installed the pump still drains queued mail and
   logs the task-store-unavailable warning on transition only.

## Required validation

- `crates/atm-http-runtime/src/herdr_queue_wake.rs` tests
  (`ax5_01_*` … naming, `FakeHerdrProcessAdapter`, `with_clock` fake
  clock, in-memory recording `TaskStore` double installed through
  `LocalServiceRuntime::with_task_store`): one test per property 1–8;
  AC 1, 2, 4, 6, 7 scenarios; the existing `ac01`–`ac12` suite unchanged and
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
  on the ADR-061 reminder-cycle section.

## Out of scope

Lead notification and doctor (AX.6); task reminders for tmux and graft
members; configurable interval; publishing reminder counters to doctor;
task priority; reassignment; expiry.
