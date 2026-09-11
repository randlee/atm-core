# BA.1 — Ack/task separation

| Field | Value |
| --- | --- |
| Wave | 1 |
| Branch | `feature/ba1-ack-task-separation` |
| Base | `integrate/phase-ba` |
| Stack | bottom layer of the BA.1 → BA.3 → BA.5 → BA.6 stack |
| Dependency | `parallel_safe` with BA.2, BA.4, BA.7 |
| recommended_agent | Cipher-311d |
| recommended_model | fast |

## Goal

Make `ack` message hygiene only. A message acknowledgement must never mutate
task state, and task state must never be able to refuse a message
acknowledgement.

Rand: *"ack has nothing to do with task."* … *"they may have conflated logic."*

This sprint is the bottom of the stack because it is the sole owner of ack
semantics in `atm-storage/src/task_state.rs`, which BA.3 then edits for
identity.

## The defect

Two directions, both live on `origin/develop`:

1. `crates/atm-storage-rusqlite/src/writer/ops.rs:516` calls
   `apply_task_acknowledgement(...)` on the message-ack write path.
   `writer/task_ops.rs:418-445` then reads `source.envelope.task_id`, emits
   `TaskEvent::Acked`, drives Assigned → Active, and runs
   `UPDATE tasks SET state='active'`. A message-lifecycle operation mutates
   task lifecycle.

2. `atm-storage/src/task_state.rs:139-148` — `admit()` lets an *unrelated*
   Active task reject a message ack:

   ```rust
   if event == TaskEvent::Acked
       && row.state == TaskState::Assigned
       && let Some(other) = open.iter()
            .find(|c| c.state == TaskState::Active && c.task_id != row.task_id)
   {
       return Err(TaskRejected::new(
           format!("task {} is active; complete it first", other.task_id)));
   }
   ```

## Deliverables

1. Cherry-pick `c99664acc` (`fix(az3): keep task acknowledgements mail-only`)
   from `origin/integrate/phase-az`. Scope is 2 files, +1 / −43:
   `writer/ops.rs` and `writer/task_legacy_ops.rs`. Zero added lines reference
   `_v2`, `tasks_v`, `TaskLifecycle`, `AsyncTaskMutation`, `TaskOperationId`,
   `assignment_attempt`, or `attention`. If it does not apply cleanly, port it
   by hand to the same effect; do not import neighbouring AZ commits to make
   it apply.
2. Delete `admit()`'s ack-refusal branch (`task_state.rs:139-148`) and the
   `(None, TaskEvent::Acked) => Ok(Transition::NoOp)` arm's dependence on task
   context. `TaskEvent::Acked` remains in the enum; only the message-ack
   coupling goes.
3. **The start receipt does NOT ship here.** An earlier draft assigned it to
   this sprint. **SOLAR-BA-004 (BLOCKING)** showed that is not deliverable:
   once ack-driven activation is deleted, `TaskEvent::Acked` was the *only*
   `Assigned → Active` input on develop
   (`task_state.rs:97-109`, applied by `writer/task_ops.rs:418-440`), so after
   this sprint nothing moves a task to Active and there is no event on which
   to hang a receipt. The receipt therefore requires the explicit start
   operation specified in **BA.5 D6**, and moves there with it.

   What this sprint MUST do is leave the hole visible rather than papered
   over: after BA.1, an assigned task stays `assigned` while it is worked.
   That is an accepted, temporary regression for one wave, called out here so
   QA does not file it twice and so nobody "fixes" it by reinstating ack
   coupling.

## Affected paths

- `crates/atm-storage-rusqlite/src/writer/ops.rs`
- `crates/atm-storage-rusqlite/src/writer/task_legacy_ops.rs`
- `crates/atm-storage-rusqlite/src/writer/task_ops.rs`
- `crates/atm-storage/src/task_state.rs`

## Paths that must not change

- `crates/atm-http-runtime/src/herdr_*` — owned by BA.4 in the same wave
- `crates/atm-core/src/boundary/mod.rs`, `nudge_dispatch.rs`,
  `send/nudge_template.rs` — owned by BA.2 in the same wave
- anything under `docs/` — owned by BA.7 in the same wave

## Acceptance criteria

1. Acknowledging a message never writes to `tasks` or `task_events`. Proven by
   a test that acks a message carrying a `task_id` and asserts both tables are
   byte-identical before and after.
2. An agent holding an Active task can acknowledge a message belonging to a
   different task. This is the regression test for the `admit()` refusal and
   must fail against `origin/develop`.
3. No `Assigned → Active` transition is reachable from a message
   acknowledgement. Gate: the transition table has no arm producing `Active`
   from `Acked`. The remaining absence of any start path is BA.5's to close
   and is **not** a finding against this sprint.
4. No production caller of `apply_task_acknowledgement` remains. Gate:
   `git grep -n apply_task_acknowledgement -- 'crates/*/src'` returns nothing.

## Required validation

- `just test` green
- `just lint` green, including `.just/check_line_counts.py`
- the two regression tests above fail on `origin/develop` and pass here

## Non-closure

- The start receipt and the explicit start operation are BA.5 D6. This sprint
  must not invent an implicit activation (roster Active, nudge handoff, or
  first outbound message) to fill the gap — SOLAR-BA-004 rules all three out:
  roster activity may be unrelated to the queue and cannot identify *which*
  assigned task began, and delivery is not work start.

- One-active-per-agent enforcement is **not** in this sprint. Deleting the
  `admit()` refusal removes the wrong enforcement of that rule; BA.3 adds the
  right one as a database constraint. Between BA.1 merging and BA.3 merging,
  one agent can hold more than one Active task. This is accepted: the
  pre-existing enforcement was already unsound, since it fired only on the
  `Acked` event and its failure mode was to reject unrelated mail.
