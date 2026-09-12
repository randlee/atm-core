---
status: complete
branch: feature/bb4-task-start
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/bb4-task-start
---

# BB.4 — `atm task start`

| Field | Value |
| --- | --- |
| Design | [`design.md`](../nudge-transition-templates/design.md) §4.4 (R8, R11); plan §1 rows 4, 5, 11, 12, 13, 16–20; plan P1, P12 |
| Recommended | arch-ctm / deep-reasoning |
| Depends on | `must_follow` BB.1 (dev push) — emits `TaskTransition::Started`; stacks on `feature/bb1-transition-templates` |
| Worktree | `feature/bb4-task-start` |
| Governed interfaces | route/envelope: none — `TaskOp::Start` already exists on `WriteRequest` (`crates/atm-storage/src/task_op.rs:10-17`); this sprint changes who may send it. Error contract: new additive stable code `ATM_TASK_ALREADY_ACTIVE` (MINOR), appended to the ADR-061 D5 1.8.0 entry BB.1 opens; schema-reviewer sign-off |
| Requirements / ADRs edited | `docs/requirements.md` 2996–2997, 3010–3013; ADR-062 134–137 → Phase BB amendment |

After this sprint the only actor that applies `Started` is the assignee,
by command. The daemon never starts a task. Nothing else about the state
machine changes (plan P8).

## Tasks

1. Add the `start` subcommand (D1).
2. Writer admission for `Start`: assignee actor, any position, no reminder gate (D2).
3. Delete the daemon receipt and the prompt-time transition (D3).
4. Requirements and ADR-062 edits (D4).
5. Tests.

## Deliverables

- [ ] D1 — `crates/atm/src/commands/task.rs:42-55` `TaskSubcommand` gains
  `Start(TaskStartCommand)` between `Assign` and `Close`:

```rust
/// Start an assigned task: moves it to active and tells the assigner.
#[derive(Debug, Args)]
struct TaskStartCommand {
    task_id: TaskId,
    /// Optional note to the assigner (what you will do first). Also accepts --stdin/--file/--template.
    message: Option<String>,
    #[command(flatten)]
    report: MessageSourceArgs,
    #[arg(long)]
    json: bool,
    #[command(flatten)]
    caller: CallerArgs,
}
```

  `execute` mirrors `TaskCloseCommand::execute` (`:299-367`):
  `preflight_daemon_api(…, HttpApiVersion::parse("1.8.0")?, "task start")`;
  `task_list_request` for the row; unknown id → `task {id} does not exist on
  team {team}`; `row.assignee != caller` → `task {id} is not assigned to
  {caller}` (exit 1, nothing sent); recipient = `row.assigner`; body =
  message source, default `started {task_id}` when none; then
  `SendCommand::for_task(…).build_task_start_request(home, cwd, task_id)`
  (`crates/atm/src/commands/send.rs`, beside `build_task_close_request`
  `:394-408`: sets `task_id`, `task_op = Some(TaskOp::Start)`,
  `NudgeMode::default()`); `validate_task_request`; `composition.send`.
  Output: `started {id}` on an applied start; on a writer rejection the
  write fails and the CLI prints the daemon's error (`ATM_TASK_ALREADY_ACTIVE`
  `task {id} is already active`, `… is already complete`,
  `… already has an active task`, `… is not assigned to …`) and fails exactly
  as a failed `atm send` does: the same typed error code and the exit status
  `main.rs` already assigns to that code (lead ruling, fenix,
  BB4-TASK-START). Nothing is delivered on rejection.
  The preflight row is used only for the fast-fail above; it never decides
  idempotency (plan P12). JSON: the `SendResult`.

- [ ] D2 — `crates/atm-storage-rusqlite/src/writer/task_ops.rs`:
  - `load_startable_task` (`:454-478`): replace the `DAEMON_ACTOR_NAME`
    check (`:472-476`) with
    `if record.envelope.from != row.assignee { return Err(task_not_counterparty(format!("task {task_id} is not assigned to {}", record.envelope.from))); }`.
  - `crates/atm-error/src/error_codes.rs:76-80`: add
    `AtmErrorCode::TaskAlreadyActive` with wire name
    `ATM_TASK_ALREADY_ACTIVE` (`:446-450` parse arm), beside
    `TaskAlreadyClosed`; update the error-code surface fixtures and
    `openapi.yaml` the way the last added code did (additive, MINOR).
  - `writer/task_rejection.rs`: add
    `pub(super) fn task_already_active(detail) -> AtmError` beside
    `task_already_closed` and add `TaskAlreadyActive` to the
    `is_task_rejection` match, so `append_rejected_task_event`
    (`task_ops.rs:42-112`) audits it like the other rejections.
  - `apply_task_start` (`:396-452`): delete the
    `start_reminder_was_emitted` call (`:412`); the silent
    `row.state == TaskState::Active` early return becomes
    `return Err(task_already_active(format!("task {task_id} is already active")));`.
    The error propagates exactly as the existing start rejections do:
    `apply_task_message` (`:115-145`) returns it, `writer/batch.rs:432-438`
    drops the savepoint (the report row is rolled back, nothing is
    delivered) and appends one `rejected` row `active → active` with
    `actor = assignee`, `message_id = record.envelope.message_id`,
    `detail = error.message()`. No retained-report path is added for starts
    (plan P12). `reject_concurrent_active_task` (`:500-527`,
    `ATM_TASK_MOVE_INVALID`) and the move-to-head (`:419-421`) stay. The
    `started` event row keeps `actor = TaskActor::Member(assignee)` and
    `message_id = record.envelope.message_id`. Because admission is
    serialized in the writer transaction, two concurrent starts of one task
    yield exactly one `started` row and one rejection; a start that races a
    reassign to another agent fails the assignee check (row 13); a start
    that races a close fails on `complete` (row 11).
  - `transition()` in `crates/atm-storage/src/task_state.rs` is unchanged:
    `(assigned, Started) → active`, `(active, Started) → active` (the
    rejection above fires before `transition` is consulted),
    `(complete, Started) → reject "task {id} is already complete"`.

- [ ] D3 — delete `start_assigned_task` and the call in
  `complete_task_handoff` (`crates/atm-http-runtime/src/herdr_task_start.rs:19-71`);
  `complete_task_handoff` now only records the reminder (BB.5 deletes it);
  delete `render_task_started_template` (`crates/atm-core/src/send/nudge_template.rs:50-58`)
  and `crates/atm-core/templates/task_started.xml`; delete the
  `task_started:` summary literal and its tests. `DAEMON_ACTOR_NAME` stays
  (escalation mail).

- [ ] D4 — `docs/requirements.md:2996-2997` item 4 → "Starting a task MUST be
  `atm task start <id>` by the assignee; it MUST move the task from
  `assigned` to `active`, MUST move it to the head of the queue, and MUST
  send the assigner a start message. The daemon MUST NOT start a task."
  `:3010-3013` item 9 closed set → `assign`, `start`, `close`, `move`,
  `list`, `events`. ADR-062 `:134-137` replaced by:

  > **Phase BB amendment (2026-09-xx).** BA R1 is superseded. `Started` is
  > applied only by the assignee through `atm task start <id> [message]`
  > (`WriteRequest.task_op = Start`, actor = assignee). Any `assigned`
  > position may be started; the task moves to the head. A prompt
  > (`task_ready`, `task_reminder`) never transitions a task; a task that is
  > prompted and never started stays `assigned` and keeps its reminder
  > count. The daemon writes no `task_started` receipt; the assigner sees
  > the assignee's start message rendered as `task_started`.

## Paths to delete

- `herdr_task_start.rs::start_assigned_task`
- `task_ops.rs::start_reminder_was_emitted`
- `nudge_template.rs::render_task_started_template`, `templates/task_started.xml`
- writer tests asserting `task start requires atm-daemon` and the reminder gate (grep `start_reminder_was_emitted`, `requires atm-daemon`)

## Tests

Writer — `crates/atm-storage-rusqlite/tests/task_ledger_writer.rs`:

- `start_by_assignee_moves_assigned_task_to_head_and_active` — three queued tasks, start the third: it is position 1 and `active`, the others keep order, one `started` event with the start message id.
- `start_by_non_assignee_is_rejected` — assigner and third party.
- `start_on_complete_task_is_rejected`.
- `start_while_another_task_is_active_is_rejected` — `already has an active task`.
- `duplicate_start_is_rejected_nothing_delivered_one_rejected_row` — second start: the write returns `ATM_TASK_ALREADY_ACTIVE`, no mail row exists for the message, one `rejected` row `active → active`, still exactly one `started` row (plan §1 row 5).
- `rejected_start_rolls_back_report_state_and_projection_atomically` — after the rejection, `mail_messages`, `mail_message_states` and the search projection have no trace of the message while the `rejected` row is committed (the savepoint contract of `batch.rs:432-441`).
- `concurrent_starts_admit_exactly_one_started_event` — two writer transactions for the same start interleaved through the serialized writer: one `started`, one rejection.
- `start_racing_reassign_is_rejected_as_not_counterparty` — reassign to B commits first; A's start is row 13.
- `start_of_missing_task_appends_null_state_rejected_row` (row 18): `ATM_TASK_NOT_FOUND`, a `rejected` row with `from_state = to_state = NULL` and `assignee` = the requested agent.
- `move_of_active_task_appends_moved_head_to_head` (row 16, existing behaviour pinned).
- `close_of_complete_task_retains_report_and_strips_link` (row 17, `Applied(Some(outcome))`), `close_by_non_party_is_rejected_and_retained` (row 19, `RejectedReportDelivered`), `move_of_complete_task_appends_rejected_row` (row 20, `ATM_TASK_ALREADY_CLOSED`, `rejected` complete→complete) — existing behaviour pinned so plan §1 is closed.
- `start_without_prior_reminder_succeeds` — the removed gate.
- `daemon_actor_can_no_longer_start_a_task`.

CLI — `crates/atm/src/commands/task/tests`:

- `task_start_sends_to_assigner_with_start_op`.
- `task_start_defaults_message_when_omitted`.
- `task_start_by_non_assignee_fails_before_sending`.
- `task_start_prints_writer_error_for_active_task` — the daemon's
  `ATM_TASK_ALREADY_ACTIVE` error is printed verbatim with the same typed code
  and exit status 1 assigned by `main.rs`; no preflight-derived wording exists
  in the command (lead ruling, fenix, BB4-TASK-START).

Integration (colima, every roster shape):

- `task_start_line_reaches_assigner_at_write_time` — assigner busy for the whole cycle still sees `<atm task=… started agent=… message=…/>` immediately (SMK-004).
- `task_start_out_of_order_reorders_queue` — start task 3 of 3: `atm task list` shows it first and active.
- `prompted_task_never_started_stays_assigned_and_keeps_reminding` — no `started` event; `reminder="1"`, `"2"` at ≥ 60 s.

## Acceptance criteria

1. `grep -rn "start_assigned_task\|start_reminder_was_emitted\|render_task_started_template\|task_started:" crates/` returns nothing.
2. Every test above passes.
3. D4 edits landed; `just lint spell` passes.
4. No new state, event kind, counter, route, or wire field (plan P1, P8): `git diff --stat feature/bb1-transition-templates -- crates/atm-core/src/protocol.rs` is empty. The one interface addition is the error code `ATM_TASK_ALREADY_ACTIVE`, present in the surface fixtures and the ADR-061 D5 1.8.0 entry.

## Required validation

`just lint`, `just test`, `just lint boundaries`; RULE-003 via `.just/check_line_counts.py`; colima fixture run with the three integration tests.
