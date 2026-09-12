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

One durable row per emitted prompt, written by the path that emitted it,
read back by `atm task events`. With it the 2026-09-12 nudge test is one
query.

## Deliverables

- [ ] D1 — DDL appended to `TASK_TABLES_DDL`
  (`crates/atm-storage-rusqlite/src/task_store.rs:17-52`), exactly:

```sql
CREATE TABLE IF NOT EXISTS prompt_handoffs (
    team TEXT NOT NULL,
    agent TEXT NOT NULL,
    message_key INTEGER NOT NULL,
    kind TEXT NOT NULL,
    task_id TEXT NULL,
    attempt INTEGER NULL CHECK(attempt IS NULL OR attempt >= 0),
    trigger TEXT NOT NULL CHECK(trigger IN ('steer', 'queue_claim', 'task_pass', 'recovery_sweep')),
    at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS prompt_handoffs_task ON prompt_handoffs(team, task_id, at);
```

  `CREATE TABLE IF NOT EXISTS` is how every task table is created today; a
  pre-BB binary ignores the table (MINOR). No `STORAGE_SCHEMA_VERSION`
  exists yet (ADR-061 D1); the D6 entry records the addition.

- [ ] D2 — `crates/atm-storage/src/task_state.rs`:

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PromptTrigger { Steer, QueueClaim, TaskPass, RecoverySweep }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptHandoff {
    pub team: TeamName,
    pub agent: AgentName,
    pub message_key: MessageKey,
    pub kind: BuiltInNudgeTemplateKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    pub trigger: PromptTrigger,
    pub at: IsoTimestamp,
}
```

  `TaskStore` (`crates/atm-storage/src/task_store.rs`) gains
  `fn record_prompt_handoff(&self, handoff: &PromptHandoff) -> Result<(), AtmError>`;
  `AsyncTaskLedgerReader` gains
  `async fn list_prompt_handoffs(&self, team: TeamName, task_id: TaskId, deadline: ReadDeadline) -> Result<Vec<PromptHandoff>, ReadLaneError>`
  (ordered by `at`, then rowid). Boundary manifests
  `boundaries/atm-storage/task-store.toml` and
  `boundaries/atm-storage-rusqlite/task-store-sqlite.toml` list both.

- [ ] D3 — one helper in `crates/atm-http-runtime/src/prompt_handoff_record.rs`
  (new, ≤ 80 lines): `pub(crate) async fn record_prompt_handoff(pump_or_runtime, dispatch: &BuiltInPostSendDispatch, trigger: PromptTrigger, now)`
  building the row from `dispatch.event` (`kind` via the BB.1 decision,
  `task_id`, `attempt` from `Reminder { attempt }` else `0` for `Ready`,
  else `NULL`). Called **after the sink reports success** at the four emit
  paths: steer (the immediate built-in path in
  `storage_and_nudge_router.rs`), `queue_claim`
  (`herdr_queue_wake.rs::complete_successful_claim`), `task_pass`
  (`task_pass.rs`, beside `record_task_reminder`), `recovery_sweep`
  (`requeue_pending` re-emit). A failed sink writes no row. Exact lines
  pinned in the PR body.

- [ ] D4 — `atm task events <id>` (`crates/atm/src/commands/task.rs:437+`,
  `render_task_events`): the response (`TaskLedgerQuery` list outcome) gains
  `#[serde(default)] handoffs: Vec<PromptHandoff>`; the renderer
  interleaves handoffs with events by time, one line each:
  `2026-09-12T15:27:34Z  prompt  task_ready  attempt=0  trigger=task_pass  msg=01M2…`.
  JSON output carries them under `handoffs`. `HTTP_API_VERSION` → `"1.9.0"`;
  `openapi.yaml` and surface baseline updated; a 1.8.0 client omits the
  field and still decodes.

- [ ] D5 — ADR-062 new subsection "Prompt handoffs (Phase BB)": the table,
  the four triggers, and "`task_events.reminded` remains the task-side
  counter; `prompt_handoffs` is the emission record". ADR-061 D5 entry
  (1.9.0) and D6 entry (additive table, MINOR).

## Tests

Storage — `crates/atm-storage-rusqlite/tests/`:

- `record_prompt_handoff_round_trips_every_trigger`.
- `list_prompt_handoffs_orders_by_time`.
- `pre_bb_fixture_opens_and_gains_prompt_handoffs_table` — the BA fixture database opens, the table is created, existing rows untouched.

Runtime — `crates/atm-http-runtime/`:

- `task_pass_records_handoff_with_kind_and_attempt`.
- `steer_emit_records_handoff_with_trigger_steer`.
- `queue_claim_records_handoff_with_trigger_queue_claim`.
- `failed_sink_records_no_handoff`.

CLI:

- `task_events_interleaves_handoffs_by_time`.
- `task_events_decodes_response_without_handoffs_field` — 1.8.0 compatibility.

Integration (colima):

- `task_events_shows_ready_reminders_and_started_for_prompted_task_only` — the 2026-09-12 scenario: three tasks, task 1 shows `task_ready`, `task_reminder` rows; tasks 2 and 3 show `task_queued` only.
- `disabled_task_reminder_override_yields_no_handoff_and_doctor_finding`.

## Acceptance criteria

1. Every emitted prompt on the colima run has exactly one `prompt_handoffs` row; `SELECT COUNT(*)` equals the terminal line count.
2. Every test above passes; `just lint boundaries` passes with the manifest edits.
3. `HTTP_API_VERSION == "1.9.0"`; ADR-061 D5 and D6 entries; schema-reviewer sign-off.
4. `just lint nudge-taxonomy` passes with no inventory change (no new identifier contains `nudge`).

## Required validation

`just lint`, `just test`, `just lint boundaries`, `just lint nudge-taxonomy`; RULE-003; colima fixture run.
