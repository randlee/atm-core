---
status: planned
branch: feature/bb1-transition-templates
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/bb1-transition-templates
---

# BB.1 — Transition templates and kind decision

| Field | Value |
| --- | --- |
| Design | [`design.md`](../nudge-transition-templates/design.md) §3, §4.1, §4.2, §9 sprint 1 |
| Recommended | arch-ctm / deep-reasoning — the kind decision and three emit sites span `atm-core`, `atm-storage`, `atm-http-runtime`, `atm-graft` |
| Depends on | none (wave 1); `parallel_safe` with BB.2, BB.3 |
| Worktree | `feature/bb1-transition-templates` off `integrate/phase-bb` |
| Governed interfaces | HTTP/peer API MINOR: `PostSendHookEvent.task_transition` (plan §6, P6); `HTTP_API_VERSION` 1.7.0 → 1.8.0; schema-reviewer sign-off |
| Requirements / ADRs edited | `docs/requirements.md` 1368–1383, 4971; ADR-054 (a) + Phase-BB amendment; ADR-061 D5 |

No behavior changes in WHEN a prompt fires. Every prompt that fires today
fires at the same moment; only its body and its recorded kind change.

## Tasks

1. Replace the `BuiltInNudgeTemplateKind` variants and parser (D1).
2. Add `TaskTransition`, `TaskClosedOutcome`, and the `PostSendHookEvent` field (D2).
3. Rewrite the kind decision (D3).
4. Add the six default bodies and five render values (D4).
5. Populate `task_transition` at the three builders (D5).
6. Doctor: stale and disabled task-kind override reporting; `clear-nudge-template` accepts the retired names (D6).
7. Both-sides decode in `atm-graft` and `atm-graft-python`; version bump (D7).
8. Requirements and ADR edits (D8).
9. Tests.

## Deliverables

- [ ] D1 — `crates/atm-storage/src/contract.rs:130-178`, exactly:

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum BuiltInNudgeTemplateKind {
    Delivery,
    DeliveryAck,
    Queue,
    QueueAck,
    Acknowledge,
    TaskQueued,
    TaskReady,
    TaskReminder,
    TaskStarted,
    TaskComplete,
    TaskClosed,
}
// as_str: "delivery" | "delivery_ack" | "queue" | "queue_ack" | "acknowledge"
//       | "task_queued" | "task_ready" | "task_reminder" | "task_started"
//       | "task_complete" | "task_closed"
// from_str: the eleven above; and
//   "task" | "acknowledge_task" | "delivery_task" | "delivery_task_ack" => Err(AtmError::validation(format!(
//       "template kind `{value}` was retired; use one of task_queued, task_ready, task_reminder, task_started, task_complete, task_closed"
//   )))
```

  `Task` and `AcknowledgeTask` are deleted; every match on the enum in the
  workspace is updated (grep `K::Task\b`, `AcknowledgeTask`).

- [ ] D2 — `crates/atm-storage/src/task_state.rs` (beside `TaskCloseOutcome`):

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskClosedOutcome {
    Cancelled,
    Reassigned,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "transition")]
pub enum TaskTransition {
    Queued { position: u32 },
    Ready,
    Reminder { attempt: u32 },
    Started,
    Complete { outcome: TaskCloseOutcome },
    Closed { outcome: TaskClosedOutcome },
}
```

  `crates/atm-core/src/boundary/mod.rs:114-133` `PostSendHookEvent` gains,
  after `task_id`:

```rust
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_transition: Option<TaskTransition>,
```

  Re-exported from `atm_core::boundary`. Not a new nudge-family identifier
  (`just lint nudge-taxonomy` unchanged).

- [ ] D3 — `boundary/mod.rs:148-167`
  `built_in_nudge_template_kind_from_post_send_event` returns
  `Result<BuiltInNudgeTemplateKind, AtmError>`:

```rust
match (event.is_ack, event.task_transition, event.task_id.is_some(), event.requires_ack, delivery_kind) {
    (true, _, _, _, _) => Ok(K::Acknowledge),
    (false, Some(TaskTransition::Queued { .. }), _, _, _) => Ok(K::TaskQueued),
    (false, Some(TaskTransition::Ready), _, _, _) => Ok(K::TaskReady),
    (false, Some(TaskTransition::Reminder { .. }), _, _, _) => Ok(K::TaskReminder),
    (false, Some(TaskTransition::Started), _, _, _) => Ok(K::TaskStarted),
    (false, Some(TaskTransition::Complete { .. }), _, _, _) => Ok(K::TaskComplete),
    (false, Some(TaskTransition::Closed { .. }), _, _, _) => Ok(K::TaskClosed),
    (false, None, _, false, NudgeKind::Steer) => Ok(K::Delivery),
    (false, None, _, true, NudgeKind::Steer) => Ok(K::DeliveryAck),
    (false, None, _, false, NudgeKind::Queue) => Ok(K::Queue),
    (false, None, _, true, NudgeKind::Queue) => Ok(K::QueueAck),
}
```

  A task-linked event with no transition is **not** an error: it renders the
  ordinary non-task kind. That case is reachable only for a pre-BB queued
  assignment claimed before BB.5 D6's open-time normalization, or a task-op
  message sent through `atm queue`; both must render, not warn (plan P14).
  The `Result` return remains for the retired-kind parse path only. The
  `task_id.is_some()` column is therefore unused by the match and is
  dropped from the tuple.

- [ ] D4 — `crates/atm-core/src/send/nudge_template.rs:77-101` default bodies
  for the six kinds, byte-for-byte from design §3 (the `task_ready` and
  `task_reminder` bodies carry the three `<action>` lines and the
  `<console announce="concise" pause="false"/>` line; the four informational
  kinds are one self-closing element). Render values (`:60-75`) gain
  `position`, `attempt`, `assignee`, `outcome`, `by`; every value is set
  from the event: `position`/`attempt` from the transition, `assignee` =
  `event.recipient` for `Started`/`Complete` (the counterparty of the
  assigner) and `event.sender` otherwise, `outcome` from the transition,
  `by` = `event.sender`. An unknown placeholder stays a validation error.
  `render_task_started_template` (`:50-58`) and
  `crates/atm-core/templates/task_started.xml` stay until BB.4 (they render
  the daemon receipt's *message body*, not the nudge).

- [ ] D5 — the three builders set `task_transition`:
  1. Claim path, `crates/atm-core/src/nudge_dispatch.rs` (the builder above
     `build_task_reminder_dispatch`, fed by
     `herdr_queue_wake.rs::load_received_hook_dispatch_message`): a
     task-linked message with `task_op = None` → `Ready` when the row is
     `assigned` at position 1 (the same test
     `queue_prompt_is_head_assignment` makes today, `task_pass.rs:148-169`),
     else `Queued { position }` from the task row (`TaskStore::load_task`);
     `task_op = Some(Start)` →
     `Started` — this builder only ever sees a start the writer applied: a
     rejected start (duplicate, non-assignee, complete, another task
     active) fails the write and is rolled back before post-write routing
     (plan P12, `batch.rs:432-438`); `task_op = Some(Close { outcome, .. })`
     → `Complete` when `envelope.from == row.assignee`, else
     `Closed { Cancelled }`; a rejected close (`RejectedReportDelivered`) and
     an `already_closed` admission are retained with the link stripped by
     `with_task_rejection` / `with_already_closed`
     (`crates/atm-core/src/send/delivery_persistence.rs:76-95`) and render
     `delivery` (plan §1 rows 17, 19). This is the **immediate** builder; the
     claim path (`load_received_hook_dispatch_message`) sets no transition at
     all — after BB.5 no task-linked message is deferred, and a legacy one
     renders the non-task kind (D3).
  2. Task pass, `nudge_dispatch.rs:157-189` `build_task_reminder_dispatch`:
     `Ready` when `row.reminder_count == 0`, else
     `Reminder { attempt: row.reminder_count }`.
  3. Immediate path, `crates/atm-core/src/send/hook.rs` (event built from the
     write result): same rule as 1 for `Close`; an immediate assignment does
     not exist today (assignments are deferred until BB.5), so `Queued` is
     set from the write result's position when BB.5 lands and is `None` →
     ordinary `delivery` until then (plan P14; test pins it).
  Line numbers are pinned in the PR body at task start; QA diffs.

- [ ] D6 — doctor, `crates/atm-core/src/doctor/mod.rs:691-700` (beside the
  disabled-delivery finding):
  - `NudgeTemplateOverrideStore::list_stale_template_override_kinds(team) -> Result<Vec<(String, IsoTimestamp)>, AtmError>`
    (raw `kind` text of rows that no longer parse) in
    `crates/atm-storage/src/contract.rs` and
    `crates/atm-storage-rusqlite/src/nudge_template_override_store.rs`;
  - finding `stale_nudge_template_override` per row, remediation
    `atm teams clear-nudge-template <kind>`; the row is otherwise ignored
    (never re-mapped);
  - finding `disabled_task_nudge_template` per disabled row whose kind is one
    of the six task kinds (a disabled `task_reminder` silences nags);
  - `atm teams clear-nudge-template` (`crates/atm/src/commands/teams.rs`)
    accepts `task` and `acknowledge_task` for deletion only: the command
    parses the kind with `BuiltInNudgeTemplateKind::from_str` and, on the
    retired-kind error, calls `clear_template_override` with the raw text.
    `clear_template_override`'s `kind` parameter becomes `&str`.

- [ ] D6a — boundary manifests for the widened sealed trait:
  `boundaries/atm-storage/nudge-template-override-store.toml` `[contracts]`
  `request_types` gains `"retired kind name (&str, deletion only)"`,
  `response_types` gains `"Vec<(String, IsoTimestamp)> (stale override kinds)"`,
  and `[public] notes` records the two additions;
  `boundaries/atm-storage-rusqlite/nudge-template-override-store-sqlite.toml`
  `request_types` and `response_types` gain the same entries. The trait stays
  sealed and implemented only by `atm-storage-rusqlite`
  (`[implementation] visibility = "trait_only"` unchanged); `just lint
  boundaries` and the `no_cli_sqlite_lookup` review gate cover the edit.

- [ ] D7 — `crates/atm-graft/src/nudge_sink.rs`, `crates/atm-graft/src/runtime/mod.rs`,
  `crates/atm-graft-python/src/lib.rs`: decode `PostSendHookEvent` with the
  new optional field (no `deny_unknown_fields` anywhere on the path; the
  receivers render `template.rendered` and never read the kind). Python
  callback shape unchanged (ADR-054 (g)). `HTTP_API_VERSION` → `"1.8.0"`
  (`crates/atm-core/src/protocol.rs:103`); `openapi.yaml` and the surface
  baseline updated.

- [ ] D8 — `docs/requirements.md:1368-1383` (eleven kinds; delete
  `acknowledge_task`; "task-tagged messages select `task` in either family"
  → "a task-linked message selects the kind named by its `task_transition`");
  `:4971` seven → eleven; ADR-054 (a) lists the eleven kinds and gains a
  "Phase-BB amendment (2026-09-xx)" recording the retirement and the (g)
  both-sides change; ADR-061 D5 entry "Phase BB.1: 1.7.0 → 1.8.0".

## Paths edited outside `crates/`

- `boundaries/atm-storage/nudge-template-override-store.toml`
- `boundaries/atm-storage-rusqlite/nudge-template-override-store-sqlite.toml`
- `docs/requirements.md`, `docs/adr/ADR-054-…`, `docs/adr/ADR-061-…` (D8)

## Paths to delete

- `BuiltInNudgeTemplateKind::{Task, AcknowledgeTask}` and their `as_str`/`from_str` arms
- default bodies for `task` and `acknowledge_task` in `nudge_template.rs`
- `crates/atm/src/commands/internal_nudge.rs:347,382` tests naming `acknowledge_task`
- the `"task"` row in every fixture/test table of kinds (`grep -rn '"task"' crates/ --include='*.rs'` restricted to kind tables)

## Tests

Unit — `crates/atm-core/src/boundary/mod.rs` (kind decision):

- `kind_decision_covers_every_transition` — twelve arms, parametrised, no `_` arm in the test.
- `task_linked_event_without_transition_is_a_validation_error`.

Unit — `crates/atm-storage/src/contract.rs`:

- `eleven_kinds_round_trip_as_str_from_str`.
- `retired_kinds_parse_to_hint_naming_the_six` — `task`, `acknowledge_task`, `delivery_task`, `delivery_task_ack`.

Unit — `crates/atm-core/src/send/nudge_template.rs`:

- `every_default_body_renders_with_its_placeholders` — all eleven.
- `informational_kinds_carry_no_action_element` — `task_queued`, `task_started`, `task_complete`, `task_closed`.
- `ready_and_reminder_bodies_name_atm_task_start`.
- `unknown_placeholder_is_a_validation_error`.

Unit — `crates/atm-core/src/nudge_dispatch.rs`:

- `task_pass_builder_sets_ready_at_zero_reminders_then_reminder_with_count`.
- `claim_builder_sets_ready_for_head_assignment_and_queued_otherwise` — positions 1, 2, 3.
- `claim_builder_sets_started_for_daemon_receipt`.
- `close_builder_sets_complete_for_assignee_and_closed_cancelled_for_assigner`.

Unit — `crates/atm-core/src/doctor/mod.rs`:

- `doctor_reports_stale_task_override_row_with_clear_remediation`.
- `doctor_reports_disabled_task_reminder_override`.

CLI — `crates/atm/src/commands/teams.rs`:

- `clear_nudge_template_accepts_retired_task_kind_for_deletion`.
- `set_nudge_template_rejects_retired_task_kind_with_hint`.

Compatibility — `crates/atm-graft/tests/` and `crates/atm-graft-python`:

- `graft_decodes_pre_1_8_event_without_task_transition`.
- `graft_python_callback_shape_unchanged_with_task_transition_present`.
- `frozen_1_7_event_shape_decodes_1_8_payload_with_task_transition` — the
  ADR-061 D3 direction: a test-local struct that copies the 1.7
  `PostSendHookEvent` field set verbatim (no `task_transition`, no
  `deny_unknown_fields`) decodes a 1.8 JSON fixture that carries
  `task_transition`; the same fixture is fed to the `atm-graft-python`
  callback path. Recorded as the D5 evidence for 1.8.0 (plan §7).

Storage — `crates/atm-storage-rusqlite/tests/`:

- `override_row_with_retired_kind_is_listed_stale_and_never_loaded`.

## Acceptance criteria

1. `grep -rn "AcknowledgeTask\|K::Task\b\|\"acknowledge_task\"" crates/` returns only the `from_str` retired arm and its test.
2. Every test above passes; `just lint nudge-taxonomy` passes with no inventory change.
3. `HTTP_API_VERSION == "1.8.0"`; ADR-061 D5 row; schema-reviewer sign-off on the PR.
4. On the prerelease built from this sprint, the nudge-test of 2026-09-12 (three assignments to cipher) shows the *same three prompts as today* but rendered as `task_ready` (head, claimed) and `task_queued` (positions 2 and 3), and the receipt to team-lead rendered as `task_started`. Timing is unchanged (that is BB.5).
5. D8 edits landed; `just lint spell` passes.

## Required validation

`just lint`, `just test`, `just lint nudge-taxonomy`, `just lint boundaries`; RULE-003 via `.just/check_line_counts.py`.
