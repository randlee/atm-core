# BA.1 — Ack/task separation

| Field | Value |
| --- | --- |
| Design | [`nudge-task-design.md`](./nudge-task-design.md) §5.1, §5.2 (commit `9b5c7d876`) |
| Outcomes | B9 |
| Recommended | Cipher-311d / fast — bounded deletion with an exact AZ precedent |
| Depends on | none (stack bottom) |
| Worktree | `feature/ba1-ack-task-separation` off `integrate/phase-ba` |
| Governed interfaces | none |

## Scope

Acknowledging a message never reads or writes `tasks` / `task_events`, and
task state never gates an ack. After this sprint nothing on the branch moves
`assigned` → `active`; BA.2 adds `Started`. That interim gap is deliberate and
lives only on `integrate/phase-ba`.

## Phase AZ code used

| AZ artifact | how |
| --- | --- |
| `c99664acc` `fix(az3): keep task acknowledgements mail-only` (`ops.rs` −1, `task_legacy_ops.rs` −41) | **apply by hand** — AZ had renamed `task_ops.rs` → `task_legacy_ops.rs`, so the cherry-pick does not apply; the deletion is identical in content (D1, D2 below) |

## Deliverables

- [ ] D1 — delete `apply_task_acknowledgement`
  (`crates/atm-storage-rusqlite/src/writer/task_ops.rs:418-457`) and its
  call in `execute_acknowledgement` (`writer/ops.rs:516`); drop the import.
- [ ] D2 — delete the ack-refusal branch of `admit()`
  (`crates/atm-storage/src/task_state.rs:139-148`, "task … is active;
  complete it first") and the `Acked` arms of `transition()` (`:88-117`).
- [ ] D3 — remove `TaskEvent::Acked` (`task_state.rs:36-41`). `TaskEventKind::Acked`
  **stays**: rows with `event = 'acked'` exist in live databases and must
  decode. Resulting types, exactly:

```rust
// crates/atm-storage/src/task_state.rs
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskEvent {
    Assigned,
    Completed,
}

// Shape after D4 in this same sprint: `Transition` is the tuple struct
// `pub struct Transition(pub TaskState)`; `Transition::To` no longer exists (QA2-002).
pub fn transition(
    state: Option<TaskState>,
    event: TaskEvent,
    task_id: &TaskId,
    actor: &AgentName,
) -> Result<Transition, TaskRejected> {
    match (state, event) {
        (None, TaskEvent::Assigned) => Ok(Transition(TaskState::Assigned)),
        (None, TaskEvent::Completed) => Err(TaskRejected::new(format!("no open task {task_id} for {actor}"))),
        (Some(TaskState::Assigned), TaskEvent::Assigned) => Ok(Transition(TaskState::Assigned)),
        (Some(TaskState::Active), TaskEvent::Assigned) => Ok(Transition(TaskState::Active)),
        (Some(TaskState::Assigned | TaskState::Active), TaskEvent::Completed) => Ok(Transition(TaskState::Complete)),
        (Some(TaskState::Complete), TaskEvent::Assigned) => Err(TaskRejected::new(format!("task {task_id} already complete; use a new id"))),
        (Some(TaskState::Complete), TaskEvent::Completed) => Err(TaskRejected::new(format!("task {task_id} already complete"))),
    }
}

pub fn admit(
    row: Option<&TaskRow>,
    event: TaskEvent,
    task_id: &TaskId,
    actor: &AgentName,
) -> Result<(), TaskRejected> {
    match (row, event) {
        (None, TaskEvent::Completed) => Err(TaskRejected::new(format!("no open task {task_id} for {actor}"))),
        (None, TaskEvent::Assigned) => Ok(()),
        (Some(row), TaskEvent::Completed) if actor != &row.assignee && actor != &row.assigner => Err(
            TaskRejected::new(format!("task {} is not assigned to or by {actor}", row.task_id)),
        ),
        (Some(_), _) => Ok(()),
    }
}
```

  `admit` loses its `open: &[TaskRow]` parameter; `transition_for`
  (`task_ops.rs:52`) and `load_open_task_rows` (`:42`) lose their only
  callers for that purpose — delete `load_open_task_rows` if nothing else
  uses it after D1/D2.
- [ ] D4 — `Transition::NoOp` was produced only by `(None, Acked)`. Remove the
  variant; `Transition` becomes `pub struct Transition(pub TaskState)` **or**
  keep the enum with one variant — choose the struct; update the two
  `let Transition::To(next_state) = next else { … }` sites in `task_ops.rs`
  (`:331`, `:433`) to plain bindings.
- [ ] D5 — boundary manifests (ruling: phase plan §10):
  `boundaries/atm-storage/task-store.toml` and
  `boundaries/atm-storage-rusqlite/task-store-sqlite.toml` `[contracts].notes`:
  "applies Assigned/Acked/Completed" → "applies Assigned/Completed";
  "provenance: … and to the acknowledgement op" → delete the clause;
  `[enforcement].review_gates` += `"no_ack_task_coupling"`.

## Unchanged, deliberately

`acknowledge_completed_assignment` (`task_ops.rs:371-418`) stays: a
**close** marking its own assignment message acknowledged is task → mail
hygiene inside the close transaction (AX.3 C7), not ack → task coupling.
The `--task-complete` CLI flag and `apply_task_message` are BA.4's.

## Paths to delete

- `apply_task_acknowledgement` fn, `task_ops.rs`
- `load_open_task_rows` fn, `task_ops.rs` (if D3 leaves it unused)
- `TaskEvent::Acked`, `Transition::NoOp`, `task_state.rs`
- tests asserting ack → active: `task_state.rs` tests naming `acked`,
  `writer` tests naming `acknowledgement_activates` (grep `Acked` /
  `acked` under `crates/atm-storage*/src` and `crates/atm-storage-rusqlite/tests`)

## Tests (names are the acceptance evidence)

Unit — `crates/atm-storage/src/task_state.rs`:

- `transition_table_is_exhaustive_over_two_events` — every
  `(Option<TaskState>, TaskEvent)` pair produces exactly the table in D3
  (parametrised over all 8 pairs; no `_` arm in the test).
- `admit_has_no_cross_row_input` — signature has no slice parameter
  (compile-time; the test exists to pin the shape for BA.2).
- `admit_rejects_completion_by_third_party` and
  `admit_accepts_completion_by_assigner_and_by_assignee` (two actors each).

Writer — `crates/atm-storage-rusqlite/tests/task_ledger_writer.rs` (or the
existing file that houses ack tests):

- `ack_of_assignment_message_leaves_task_row_and_events_untouched` —
  assign task T to A; A acks the assignment; assert `tasks.state = 'assigned'`,
  `task_events` count unchanged, `updated_at` unchanged.
- `ack_of_assignment_when_another_task_is_active_succeeds` — A has task U
  active; assign T; A acks T's assignment → ack **succeeds** (the mail is
  acknowledged), T stays `assigned`. Corner case: the removed refusal.
- `ack_with_no_task_row_succeeds` — message with `task_id` whose row was
  never created (peer-origin write) acks cleanly.
- `historic_acked_event_rows_still_decode` — insert a raw `task_events` row
  with `event='acked'`; `list_task_events` returns
  `TaskEventKind::Acked`.

## Acceptance criteria

1. `grep -rn "TaskEvent::Acked\|apply_task_acknowledgement" crates/` returns
   nothing outside `TaskEventKind`.
2. Every test above passes; no test in the workspace asserts that an ack
   changes `tasks.state`.
3. `just lint-boundaries` passes with the D5 manifest edits.

## Required validation

`just lint`, `just test`, `just lint-boundaries`; RULE-003 via
`.just/check_line_counts.py`.

## Out of scope

`Started`, `--task-complete` requires `--task-id`, one-active enforcement
(BA.2/BA.4).
