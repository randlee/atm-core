---
status: in-progress
branch: feature/bb5-assignment-write-task-pass
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/bb5-assignment-write-task-pass
---

# BB.5 — Assignment write and task pass

| Field | Value |
| --- | --- |
| Design | [`design.md`](../nudge-transition-templates/design.md) §3.1, §4.3, §4.5, §4.7 (R9); plan §1 rows 1–3, 7–10; plan P9 |
| Recommended | arch-ctm / deep-reasoning |
| Depends on | `must_follow` BB.4 (dev push) — both edit `task_pass.rs` and `herdr_task_start.rs`; stacks on `feature/bb4-task-start` |
| Worktree | `feature/bb5-assignment-write-task-pass` |
| Governed interfaces | none — deletions on the write path and the task pass; no wire or DDL change |
| Requirements / ADRs edited | `docs/requirements.md` 1585, 1591, 2975, §15.4 item 15; ADR-062 "Reminder and escalation"; ADR-054 "Phase-BA amendment" |

Everything here is a deletion or a one-line emission. After this sprint an
assignment is an ordinary unread message plus one informational line, the
task pass is the only source of `task_ready`/`task_reminder`, and every
`reminded` event names the task that was prompted.

## Tasks

1. Ack and task independent (D1).
2. Assignment write: immediate `task_queued`, no pending marker (D2).
3. Task pass owns ready/reminder; delete the claim-time handoff (D3).
4. Reassign line to the old assignee (D4).
5. Requirements and ADR edits (D5).
6. Tests.

## Deliverables

- [x] D1 — `crates/atm/src/commands/send.rs:367`
  `self.requires_ack || assigning_task` → `self.requires_ack`; delete
  `assigning_task` (`:358`) if nothing else reads it. The `--requires-ack`
  arg gains `conflicts_with = "task_id"` (clap error: `the argument
  '--requires-ack' cannot be used with '--task-id <TASK_ID>'`).
  `atm task assign` keeps having no such flag. Because `pending_ack_at` is
  created only from `requires_ack` (`docs/requirements.md:1956`), an
  assignment now writes `requires_ack = false`, `pending_ack_at = NULL`.
  `SendCommand::for_task` (`task.rs:259-268`) is shared by both spellings,
  so both get it.

- [x] D2 — `crates/atm-core/src/send/mod.rs:381` `send_mode_for_task_request`
  returns `NudgeMode::Immediate` for an assignment (`task_op = None`), as it
  already does for closes. No pending-nudge marker is written (the marker is
  written only for `Deferred`, `pending_nudge_store.rs`). The writer's
  internal admission result `MessageAdmissionOutcome`
  (`crates/atm-storage/src/contract.rs:236-241`, not a wire type) gains
  `pub queued_position: Option<u32>`, set by `apply_task_assignment` from the
  landed position; the immediate builder (`send/hook.rs`, BB.1 D5.3) sets
  `TaskTransition::Queued { position }` from it at **every** position,
  including 1 (plan P9: one line per assignment, always; `task_ready`
  follows from the task pass).

- [x] D3 — `crates/atm-http-runtime/src/herdr_queue_wake/task_pass.rs`:
  delete `record_queue_prompt_reminders` (`:105-146`) and
  `queue_prompt_is_head_assignment` (`:148-169`) and their call sites in
  `herdr_queue_wake.rs`; delete `herdr_task_start.rs` entirely (BB.4 left
  only `complete_task_handoff`, whose sole job was the reminder record the
  task pass already writes through `record_task_reminder`). The task pass
  keeps its existing flow: `dispose` → `build_task_reminder_dispatch`
  (BB.1 D5.2 sets `Ready`/`Reminder`) → emit → `record_task_reminder(row)`
  for **that** row. `build_task_reminder_dispatch` (`nudge_dispatch.rs:182`)
  sets `requires_ack: false`.

- [x] D4 — `crates/atm-storage-rusqlite/src/writer/task_ops.rs::apply_task_assignment`
  (`:151+`), reassign branch (existing open row, different assignee): inside
  the same transaction, after the `reassigned` event, insert one message to
  the old assignee through a writer-internal primitive
  `insert_message_canonical(record, connection, cache, target) -> Result<bool, AtmError>`
  extracted from `execute_upsert_message` (`writer/ops.rs:686-745`): it is
  exactly the existing `insert_message_row` + `sync_inserted_message_projection`
  + `insert_initial_message_state` sequence, and `execute_upsert_message`
  becomes that call followed by task admission. The notice therefore gets
  its `mail_message_states` row and search projection like every other
  message (read marks it read; `atm list`/search see it); it bypasses only
  `apply_task_message`, which runs on the incoming record alone. Notice
  fields: from `record.envelope.from` (the assigner), summary
  `task_closed:{task_id}`, body `task {task_id} was reassigned to
  {new_assignee}`, `task_id` set, `task_op = None`, `requires_ack = false`,
  a fresh message id and key. `MessageAdmissionOutcome` gains
  `pub reassign_notice: Option<Message>`; the router dispatches one extra
  immediate built-in nudge for it with `TaskTransition::Closed { outcome:
  Reassigned }` (the router already dispatches for the admitted record; this
  is the same call with the notice message and that transition). Nothing
  crosses the wire: `SendOutcome` is unchanged.

- [x] D6 — pre-BB durable assignments (plan P14). New
  `crates/atm-storage-rusqlite/src/task_assignment_migration.rs::normalize_legacy_assignment_markers(connection: &Connection, target: &SharedDbTarget) -> Result<u64, AtmError>`
  called from `ensure_schema` beside
  `migrate_template_override_kinds_to_seven` (`shared_db.rs:756-770`), one
  statement, idempotent, run at every open:

```sql
UPDATE mail_message_states
   SET pending_ack_at = NULL, nudge_pending_at = NULL, nudge_attempts = 0
 WHERE acknowledged_at IS NULL
   AND (team, agent, message_key) IN (
        SELECT m.team, m.agent, m.message_key
          FROM mail_messages m
          JOIN tasks t ON t.team = m.team AND t.assignee = m.agent
                      AND t.assignment_message_id = m.message_id
         WHERE t.state IN ('assigned', 'active'));
```

  (column names pinned against `mail_messages_schema.rs:250` and
  `pending_nudge_store.rs:40-72` in the PR body). Envelopes are immutable
  and keep `requires_ack = true`; that is harmless because the pending
  predicate (`pending_nudge_store.rs:13`) and the `Pending-Ack:` header read
  state columns only. Once BB.5 writes no markers the statement matches
  nothing. Logged once per open with the affected-row count when non-zero.
  ADR-061 D6 gets a note row (no DDL change).

- [x] D5 — `docs/requirements.md:1585` delete "require acknowledgement for
  any task-linked message"; `:1591` delete "imply `--requires-ack`", add
  "`--requires-ack` conflicts with `--task-id`"; `:2975` → "a task-linked
  message never requires acknowledgement; readiness is signalled by the
  task pass (`task_ready`), start by `atm task start`"; §15.4 item 15 gains
  "the first prompt is `task_ready`, later prompts are `task_reminder` with
  a rising attempt; every assignment produces one `task_queued` line at
  write time". ADR-062 "Reminder and escalation" gains "a `reminded` event
  is recorded only against the task the prompt was rendered for". ADR-054
  "Phase-BA amendment" gains "an assignment carries no pending-nudge
  marker; the task pass owns every task prompt (Phase BB)".

## Paths to delete

- `send.rs` `assigning_task` binding
- `task_pass.rs::record_queue_prompt_reminders`, `::queue_prompt_is_head_assignment`
- `herdr_task_start.rs` (whole file) and its `mod` line
- nothing in `nudge_dispatch.rs`: BB.1 D5.1's claim path already sets no transition
- tests asserting an assignment is pending-ack or claimed by the pump (grep `pending_ack_at` in task tests; `queue_prompt_is_head_assignment`)

## Tests

Unit / writer:

- `assignment_write_leaves_requires_ack_false_and_pending_ack_null`.
- `assignment_write_creates_no_pending_marker` — `nudge_pending_at IS NULL`.
- `requires_ack_conflicts_with_task_id` — clap.
- `assignment_at_every_position_emits_task_queued_with_position` — positions 1, 2, 3.
- `reassign_inserts_closed_reassigned_message_to_old_assignee_in_same_transaction` — and a forced failure after the notice insert rolls back the `reassigned` event, the notice and its state row together.
- `reassign_notice_row_is_never_admitted_as_an_assignment` — the old assignee's task list has no row for the id; only one `assigned` row for the new assignee.
- `reassign_notice_has_state_row_and_projection` — `atm read` returns it once, mark-read persists in `mail_message_states`, message search finds its body.
- `insert_message_canonical_is_the_only_insert_path` — `execute_upsert_message` and the notice both go through it; a duplicate message key returns `false` and inserts nothing.
- `same_assignee_reassign_refreshes_row_and_emits_task_queued` (plan §1 row 15).
- `ba_fixture_with_pending_assignments_opens_with_zero_pending_ack_and_no_markers` — the BA live-ledger fixture (three open assignments with `pending_ack_at` and `nudge_pending_at` set) opened by the BB.5 binary: `Pending-Ack 0`, no pending markers, envelopes byte-identical, the three tasks still `assigned` in order, a second open changes nothing (D6).
- `legacy_assignment_claimed_before_normalization_renders_delivery` — the same fixture with the pump claiming a marker before `ensure_schema` runs (unit-level, claim path invoked directly): the event has no transition and the kind is `delivery`, no error (BB.1 D3, plan P14).
- `pump_never_claims_a_bb_assignment_and_loses_no_prompt` — three assignments on a normalized fixture: exactly one `task_ready` after the next pass, no duplicate, no `queue` line.
- `reopen_complete_task_emits_task_queued` (row 9).
- `ack_of_assignment_writes_no_task_event` (row 14).

Runtime — `crates/atm-http-runtime/src/herdr_queue_wake/`:

- `task_pass_first_prompt_is_ready_then_reminder_with_attempt`.
- `task_pass_records_reminder_against_prompted_task_only` — two open tasks, the head is prompted, the other's `reminder_count` stays 0 (SMK-006).
- `pump_never_claims_an_assignment` — assignment written, pump ticks, no claim.

Integration (colima, every roster shape):

- `three_assignments_show_queued_1_2_3_then_one_ready` — terminal shows exactly `queued="1"`, `queued="2"`, `queued="3"`, one `ready` for task 1, no `execute` line for tasks 2 and 3.
- `pending_ack_header_stays_zero_across_assignments` — `atm read` header `Unread 3 / Pending-Ack 0`.
- `close_shows_ready_for_next_task_within_one_pass`.
- `reassign_shows_closed_reassigned_to_old_and_queued_to_new`.
- `move_to_head_while_idle_shows_ready_next_pass_and_no_extra_line`.
- `cancel_shows_closed_cancelled_to_assignee`.
- `busy_assigner_receives_started_and_complete_at_write_time`.

## Acceptance criteria

1. `grep -rn "queue_prompt_is_head_assignment\|record_queue_prompt_reminders\|herdr_task_start" crates/` returns nothing.
2. Every test above passes.
3. D5 edits landed; `just lint spell` passes.
4. No new column, state, event kind or counter (plan P8); D6 changes data, not DDL.

## Required validation

`just lint`, `just test`, `just lint boundaries`; RULE-003 via `.just/check_line_counts.py`; colima fixture run with the seven integration tests.
