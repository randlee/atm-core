---
phase: AX
sprint: AX.4b
title: Task nudge cycle, lead notification, and doctor
branch: feature/ax4b-task-nudge-cycle
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/ax4b-task-nudge-cycle
integration_branch: integrate/phase-ax
status: draft
recommended_agent: arch-ctm
recommended_model: deep-reasoning
dependency_relations:
  - related: AX.2
    relation: must_follow
    rationale: extends the pump tick in herdr_queue_wake.rs that AX.2 changes to pass rendered text.
  - related: AX.4a
    relation: must_follow
    rationale: reads the tasks table and TaskStore delivered by AX.4a; the steer suppression added here depends on the task rows existing.
  - related: AX.3
    relation: must_follow
    rationale: AX.3 proves this sprint's behaviour live on the integrate head.
---

# AX.4b — Task nudge cycle, lead notification, and doctor

Add the asynchronous half of task tracking: an idle Herdr assignee with
an open task is reminded of it on a fixed cadence, the lead is told when
a task stalls, and doctor reports a team without a lead. The pump reads
task state and writes only nudge bookkeeping; it never changes task
state.

## Pump rule — assignee nudge cycle

Evaluated inside the existing `HerdrQueueWakePump::tick_once` (10 s
cadence) for every member observed `idle`, **before** the existing
non-task `claim_next_pending` step. Inputs: Herdr status and
`TaskStore::open_tasks`. Writes: `record_nudge`, `record_lead_notified`,
one queued message on every tenth nudge.

```
target := the member's Active row, else its oldest Assigned row, else none
if target and (target.last_task_nudge_at is None
               or now − target.last_task_nudge_at ≥ 60 s):
    text := rendered dispatch for target.assignment_message_id with NudgeKind::Queue   (Task template)
    emit Herdr prompt(text)                         (AX.2 emitter)
    row := record_nudge(target, now, assignment_message_id)
    if row.task_nudge_count % 10 == 0: notify lead (D3); record_lead_notified
```

Properties that must hold and are each tested:

- The nudge that surfaces the oldest `Assigned` task is the same nudge
  that reminds about an `Active` one. The agent's `atm ack` moves
  `Assigned → Active`; the nudge does not.
- First nudge is immediate on the first idle tick after assignment; then
  at most one per 60 s per member while the member stays idle with an
  open task.
- A member with an `Active` task is never nudged for a different
  `Assigned` task.
- Emit failure (breaker open, Herdr error) writes no `Nudged` row, so the
  next tick retries; nothing else is recorded.
- Non-task queued mail is still claimed and nudged on the same tick after
  the task step; the two are independent.
- The pump never calls a state-changing path.

## Deliverables

This is the authoritative deliverable checklist. Every listed deliverable
lands production-ready for the scope this sprint claims; partial or
shape-only completion fails the sprint.

- [ ] D1 — pump task step in
  `crates/atm-http-runtime/src/herdr_queue_wake.rs` per the rule above;
  `HerdrQueueWakeStats` gains `task_nudges: usize` and
  `lead_notifications: usize`; the pump is constructed with an
  `Arc<dyn TaskStore>` in `crates/atm-daemon-bootstrap` where the pump
  is started today.
- [ ] D2 — Herdr task sends bypass the steer and the pending-nudge
  marker: in `crates/atm-core/src/delivery_policy.rs` (or the
  `build_send_delivery_plan` site in `crates/atm-core/src/send/mod.rs`)
  a message with `task_id` set whose recipient roster entry has
  `local_backend == Herdr` gets `NudgeMode::Deferred` and **no**
  `mark_pending` call; the pump's task step is its only nudge source.
  tmux and graft recipients keep today's behaviour (steer on `atm send`,
  marker on `atm queue`). Code contract C1.
- [ ] D3 — lead notification: on every tenth nudge for one task, the
  pump writes one queued message to the team member whose `agent_type ==
  AgentType::Lead` using sender name `atm-daemon` (reserved; rejected as
  a roster member name by `atm teams add-member`), body per code contract
  C2, no ack required. When no lead exists or more than one, no message
  is written and a `LeadNotified` row is **not** appended; the condition
  is visible through doctor (D4). The write must succeed with a
  non-roster sender: if the post-send path today requires a sender roster
  row (hook-config lookup by sender `home_dir`), D3 includes the minimal
  fallback for the reserved sender (built-in path, no external hook
  lookup) and a test proving the lead's nudge still renders.
- [ ] D4 — doctor (`crates/atm-core/src/doctor/mod.rs`, codes in
  `crates/atm-error/src/error_codes.rs`): `ATM_ROSTER_NO_LEAD` warning
  per team with no `Lead` member; `ATM_ROSTER_MULTIPLE_LEADS` warning per
  team with more than one; `ATM_TASK_STALLED` warning per open task with
  `task_nudge_count >= 10`; info line with `assigned`/`active` counts per
  member. The roster checks run on every doctor pass.
- [ ] D5 — `docs/requirements.md` §11.3 lists the three doctor codes and
  §15.4 gains the nudge-cycle rule (60 s, tenth-nudge lead message,
  pump never changes state); ADR-061 gains a "Nudge cycle" section;
  `docs/user-documents/nudge-templates.md` describes when the `task`
  body is re-sent.
- [ ] D6 — tests listed under Required validation.

### Paths to delete

None.

## Code contracts

### C1 — delivery policy for task sends

```rust
// crates/atm-core/src/delivery_policy.rs (or send/mod.rs plan builder)
/// Task-tagged mail to a Herdr-backed recipient is always deferred and is
/// never marked pending: the daemon task step is its only nudge source.
pub(crate) fn nudge_mode_for_task(recipient: &RosterEntry, task_id: Option<&TaskId>, requested: NudgeMode) -> (NudgeMode, MarkPending) {
    match (task_id, recipient.local_backend) {
        (Some(_), LocalBackend::Herdr) => (NudgeMode::Deferred, MarkPending::No),
        _ => (requested, MarkPending::from(requested)),
    }
}
```

The exact enum names follow whatever the plan builder already uses; the
contract is the two-row table, not the identifiers.

### C2 — lead notification body

Sent with `atm-daemon` as `from`, `NudgeMode::Deferred`, `requires_ack ==
false`, no `task_id`:

```
task <task_id> assigned to <assignee> has been nudged <count> times while idle
(first nudge <first_nudge_at>, last <last_nudge_at>). Run: atm list --task-events <task_id>
```

### C3 — doctor codes

```rust
// crates/atm-error/src/error_codes.rs (additions, warning severity)
RosterNoLead,        // "ATM_ROSTER_NO_LEAD"
RosterMultipleLeads, // "ATM_ROSTER_MULTIPLE_LEADS"
TaskStalled,         // "ATM_TASK_STALLED"
```

Remediation text for `ATM_ROSTER_NO_LEAD`:
`atm teams update-member <team> <member> --agent-type lead`.

### Unchanged surfaces

`TaskStore` trait and `task_state` transition table (AX.4a);
`claim_next_pending`; the 10 s tick and 16-prompt burst cap
(`ac02`); ADR-058 argv shape.

## Acceptance criteria

1. Two tasks to one idle Herdr member: one nudge for the oldest on the
   first tick, none for the second; after 65 s a second nudge for the
   same task; after `atm ack` the nudges continue for it; after
   `--task-complete` the next tick nudges the second task.
2. Ten nudges on one task produce exactly one message to the lead; the
   twentieth produces the second; a team without a lead produces none and
   doctor warns `ATM_ROSTER_NO_LEAD`.
3. `atm send --task-id` to a Herdr member emits no steer and sets no
   pending-nudge marker; to a tmux member it steers as before.
4. Doctor on a team with two `lead` members warns
   `ATM_ROSTER_MULTIPLE_LEADS`; doctor with an open task at ten nudges
   warns `ATM_TASK_STALLED`.
5. `just validate` green; ADR-061 and requirements updates merged.

## Required validation

- `crates/atm-http-runtime/src/herdr_queue_wake.rs` tests
  (`ac07_task_*` naming, mocked Herdr status, injected clock): the six
  properties under Pump rule, each as one test; AC 1 and AC 2 scenarios.
- `crates/atm-core/tests/task_state.rs` (from AX.4a) extended with AC 3
  for a Herdr and a tmux recipient.
- `crates/atm-core/src/doctor/mod.rs` tests for the three codes and the
  info counts.
- Real-startup integration test in `crates/atm-daemon-bootstrap`: the
  pump started by daemon bootstrap has a `TaskStore` and emits a task
  nudge against a fixture Herdr adapter (negative proof: with the task
  step disabled the nudge does not occur).
- `just validate`; quality-mgr Final Quality Report on the PR.

## Out of scope

Task re-nudge for tmux and graft members; configurable interval or
threshold; task priority; reassignment; expiry.
