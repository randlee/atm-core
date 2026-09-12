---
status: planned
branch: feature/bb6-prompt-handoffs
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/bb6-prompt-handoffs
---

# BB.6 — `prompt_handoffs`

| Field | Value |
| --- | --- |
| Design | [`design.md`](../nudge-transition-templates/design.md) §4.6 (SMK-005); plan P2 |
| Recommended | arch-ctm / deep-reasoning |
| Depends on | `must_follow` BB.5 (dev push) — the `task_pass` trigger is written from the task pass BB.5 rewrites; stacks on `feature/bb5-assignment-write-task-pass` |
| Worktree | `feature/bb6-prompt-handoffs` |
| Governed interfaces | SQLite MINOR (additive table); HTTP/peer API MINOR: optional `handoffs` on the task-events list response, `HTTP_API_VERSION` 1.8.0 → 1.9.0; schema-reviewer sign-off |
| Requirements / ADRs edited | ADR-062 new subsection; ADR-061 D5 (1.9.0) and D6 (additive table entry) |

One best-effort durable observation per successfully emitted **task-linked**
prompt, written by the path that emitted it after the sink reported
success, read back by `atm task events`. A storage failure after a
successful emission is logged, never retried, and never fails the emission
(plan P10, P11); exact one-to-one holds for a run whose log shows zero
record failures, which is what the colima acceptance asserts. With it the
2026-09-12 nudge test is one query.

## Deliverables

- [ ] D1 — DDL appended to `TASK_TABLES_DDL`
  (`crates/atm-storage-rusqlite/src/task_store.rs:17-52`), exactly:

```sql
CREATE TABLE IF NOT EXISTS prompt_handoffs (
    team TEXT NOT NULL,
    agent TEXT NOT NULL,
    message_key TEXT NOT NULL,
    kind TEXT NOT NULL,
    task_id TEXT NOT NULL,
    attempt INTEGER NOT NULL DEFAULT 0 CHECK(attempt >= 0),
    trigger TEXT NOT NULL CHECK(trigger IN ('steer', 'task_pass')),
    at TEXT NOT NULL,
    UNIQUE (team, agent, message_key, attempt)
);
CREATE INDEX IF NOT EXISTS prompt_handoffs_task ON prompt_handoffs(team, task_id, at);
```

  `message_key` is TEXT because `MessageKey` is a `String` newtype
  (`crates/atm-storage/src/contract.rs:29`) and `mail_messages.message_key`
  is `TEXT NOT NULL` (`mail_messages_schema.rs:33`). The identity of a
  handoff is `(team, agent, message_key, attempt)`: repeated reminders share
  one message key and differ by attempt; every non-reminder prompt is
  attempt 0. The insert is `INSERT OR IGNORE`, so a repeated record of the
  same prompt is a no-op (idempotent; plan P11). No foreign key — no task
  table has one today, and the row must survive a later task-row rewrite.
  `CREATE TABLE IF NOT EXISTS` is how every task table is created today; a
  pre-BB binary ignores the table (MINOR). No `STORAGE_SCHEMA_VERSION`
  exists yet (ADR-061 D1); the D6 entry records the addition.

- [ ] D2 — `crates/atm-storage/src/task_state.rs`:

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PromptTrigger { Steer, TaskPass }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptHandoff {
    pub team: TeamName,
    pub agent: AgentName,
    pub message_key: MessageKey,
    pub kind: BuiltInNudgeTemplateKind,
    pub task_id: TaskId,
    pub attempt: u32,
    pub trigger: PromptTrigger,
    pub at: IsoTimestamp,
}
```

  `TaskStore` (`crates/atm-storage/src/task_store.rs:87`) gains
  `fn record_prompt_handoff(&self, handoff: &PromptHandoff) -> Result<(), AtmError>`
  (returns `Ok(())` on an ignored duplicate). Its doc comment becomes "Read
  and audit capability for the task ledger, including the prompt-handoff
  audit record" — the row is task-linked by construction (plan P10), so it
  belongs to the task ledger and no new optional capability is introduced
  (ADR-054 capability count unchanged; stated in the PR body);
  `AsyncTaskLedgerReader` gains
  `async fn list_prompt_handoffs(&self, team: TeamName, task_id: TaskId, deadline: ReadDeadline) -> Result<Vec<PromptHandoff>, ReadLaneError>`
  (ordered by `at`, then rowid). Boundary manifests
  `boundaries/atm-storage/task-store.toml` and
  `boundaries/atm-storage-rusqlite/task-store-sqlite.toml` list both.

- [ ] D3 — one helper in `crates/atm-http-runtime/src/prompt_handoff_record.rs`
  (new, ≤ 80 lines), `pub(crate)`: both emit paths that can carry a task
  link live in this crate (plan P13):

```rust
pub(crate) async fn record_prompt_handoff(
    bridge: &BoundedBlockingBridge,          // router_support.rs:81
    deadline: RequestDeadline,
    store: Arc<dyn TaskStore + Send + Sync>,
    dispatch: &BuiltInPostSendDispatch,
    trigger: PromptTrigger,
    at: IsoTimestamp,
) // returns (); logs, never errors
```

  It returns without writing when `dispatch.event.task_transition` is
  `None` (not task-linked; plan P10). Otherwise it builds the row (`kind`
  via the BB.1 decision, `task_id`, `attempt` from `Reminder { attempt }`
  else `0`) and runs the synchronous store call through the caller's
  bridge exactly as `record_task_reminder` does
  (`task_pass.rs:552-580`: `bridge.run(deadline, move || store.record_prompt_handoff(&row)).await`).
  Any `Err` — storage, bridge timeout, bridge saturation — emits one
  `tracing::error!(subsystem, action = "prompt_handoff_record_failed", reason, message_id, kind, trigger)`
  and returns (plan P11). Called **after the sink reports success** at the
  two emit paths:

  | trigger | call site at `281e6f546` | bridge / deadline |
  | --- | --- | --- |
  | `Steer` | `storage_and_nudge_router.rs:487` (immediate built-in path, after the `timeout(remaining, …)` returns `Ok(Ok(_))`) | the router's `BoundedBlockingBridge`; the request's remaining deadline (`:42-49`), skipped with the failure log when already exhausted |
  | `TaskPass` | `herdr_queue_wake/task_pass.rs:483`, beside `record_task_reminder` | the task pass's `blocking_bridge`; `herdr_request_deadline()` |

  The herdr queue claim (`herdr_queue_wake.rs:646`) and the bootstrap
  `queue_drain.rs::drain_one` (idle drain and recovery sweep) are untouched:
  after BB.5 no task-linked message is deferred, so they never see a task
  link (`send_mode_for_task_request`, `send/mod.rs:381-389`, becomes
  `request.nudge_mode` for every task request). A failed sink writes no
  row. Exact lines pinned in the PR body.

- [ ] D4 — `atm task events <id>` (`crates/atm/src/commands/task.rs:437+`,
  `render_task_events`): the response (`TaskLedgerQuery` list outcome) gains
  `#[serde(default)] handoffs: Vec<PromptHandoff>`; the renderer
  interleaves handoffs with events ordered by `(at, source, rowid)` where
  `source` puts task events before prompts — the task pass stamps the
  reminder row and the prompt with one clock value, so the tie-break is
  pinned in `task_events_interleaves_handoffs_by_time`. One line each:
  `2026-09-12T15:27:34Z  prompt  task_ready  attempt=0  trigger=task_pass  msg=01M2…`.
  JSON output carries them under `handoffs`. `HTTP_API_VERSION` → `"1.9.0"`;
  `openapi.yaml` and surface baseline updated; a 1.8.0 client omits the
  field and still decodes.

- [ ] D5 — ADR-062 new subsection "Prompt handoffs (Phase BB)": the table,
  the two triggers and why only two (P13), task-linked only (P10),
  best-effort after sink success with the logged failure line (P11), and
  "`task_events.reminded` remains the task-side counter; `prompt_handoffs`
  is the emission record".
  ADR-061 D5 entry (1.9.0) and D6 entry (additive table, MINOR), each
  naming its previous-consumer proof test below (ADR-061 D3). ADR-054
  capability count: no edit, no new capability (P10).

## Tests

Storage — `crates/atm-storage-rusqlite/tests/`:

- `record_prompt_handoff_round_trips_every_trigger` — both.
- `record_prompt_handoff_ignores_duplicate_identity` — same `(team, agent, message_key, attempt)` twice: one row, `Ok(())`.
- `record_prompt_handoff_keeps_reminder_attempts_distinct` — one key, attempts 1 and 2: two rows.
- `list_prompt_handoffs_orders_by_time_then_rowid`.
- `prompt_handoff_row_with_unknown_trigger_fails_decode` — malformed-row coverage.
- `pre_bb_fixture_opens_and_gains_prompt_handoffs_table` — the BA fixture database opens, the table is created, existing rows untouched.
- `pre_bb_ddl_set_reads_and_writes_after_prompt_handoffs_created` — the ADR-061 D3 direction: a frozen copy of the BA `TASK_TABLES_DDL` + mail DDL (string fixture under `tests/fixtures/`) is applied to a database that already has `prompt_handoffs`, then assigns, starts and closes a task and reads it back through the frozen statements.

Runtime — `crates/atm-http-runtime/`:

- `task_pass_records_handoff_with_kind_and_attempt`.
- `steer_emit_records_handoff_with_trigger_steer`.
- `queue_claim_records_handoff_with_trigger_queue_claim`.
- `steer_of_non_task_message_records_no_handoff` (P10).
- `failed_sink_records_no_handoff` — one per path (steer, queue claim, task pass).
- `record_failure_logs_prompt_handoff_record_failed_and_emission_succeeds` — a failing `TaskStore` stub; the sink result is unchanged and the log line carries `reason = storage`, message id, kind, trigger.
- `record_bridge_timeout_logs_and_emission_succeeds` — a store stub that sleeps past the deadline: `reason = timeout`, sink result unchanged, the Tokio worker is not blocked (the test's own timer keeps firing).
- `record_bridge_saturated_logs_and_emission_succeeds` — bridge permits exhausted: `reason = saturated`, sink result unchanged.
- `steer_with_exhausted_deadline_skips_record_with_failure_log`.
- `deferred_task_linked_message_is_impossible_after_bb5` — every task request goes through `send_mode_for_task_request` as `Immediate`; the claim path is asserted never to see `task_transition.is_some()`.

CLI:

- `task_events_interleaves_handoffs_by_time` — including the equal-`at` tie-break.
- `task_events_decodes_response_without_handoffs_field` — a 1.9 client reads a 1.8 response.
- `frozen_1_8_task_events_response_decodes_1_9_payload_with_handoffs` — the ADR-061 D3 direction: a test-local struct copying the 1.8 response fields decodes a 1.9 fixture that carries `handoffs`.

Integration (colima):

- `task_events_shows_ready_reminders_and_started_for_prompted_task_only` — the 2026-09-12 scenario: three tasks, task 1 shows `task_ready`, `task_reminder` rows; tasks 2 and 3 show `task_queued` only.
- `disabled_task_reminder_override_yields_no_handoff_and_doctor_finding`.

## Acceptance criteria

1. Every emitted task-linked prompt on the colima run has exactly one `prompt_handoffs` row: `SELECT COUNT(*)` equals the count of task-linked terminal lines, and the daemon log has zero `prompt_handoff_record_failed` lines (plan P11).
2. Every test above passes; `just lint boundaries` passes with the manifest edits.
3. `HTTP_API_VERSION == "1.9.0"`; ADR-061 D5 and D6 entries; schema-reviewer sign-off.
4. `just lint nudge-taxonomy` passes with no inventory change (no new identifier contains `nudge`).

## Required validation

`just lint`, `just test`, `just lint boundaries`, `just lint nudge-taxonomy`; RULE-003; colima fixture run.
