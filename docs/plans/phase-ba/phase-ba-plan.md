# Phase BA: one invariant, one queue, one task command set

| Field | Value |
| --- | --- |
| Design authority | [`nudge-task-design.md`](./nudge-task-design.md) — committed verbatim at `18db5acc3` (this branch). Where this plan and the design disagree, the design wins and this plan is the defect. |
| Supersedes | Phase AZ (`integrate/phase-az`, PR #1394) — retired unmerged 2026-09-11; branch retained, plan docs retained as history |
| Base | `develop` |
| Integration branch | `integrate/phase-ba` |
| Sprints | 6 |
| Widest parallel wave | 2 |
| Status | draft — blocked on the five decisions in **§4** |
| Exactness | every significant trait, struct, enum, newtype and constant is written in its sprint doc exactly as it lands; QA diffs source against the doc. Test names and corner cases in the sprint docs are the acceptance evidence. |

## 1. The two problems

Rand, verbatim (design §0):

> a) "agent stops for no acceptable reason."
> b) "team-lead/orchestrator interrupts current tasks in process which wreck
> context by diverting an agent to a different task/branch."

Every deliverable is judged against those two.

## 2. Binding outcomes

One line each; the design section is the full statement.

| # | outcome | design |
| --- | --- | --- |
| B1 | An idle agent holding an open task is nudged (rate-limited); an active agent is never diverted; a blocked or offline agent is escalated once and never nudged | §1 |
| B2 | Agent state SSOT is the ephemeral roster record; the nudge path consumes exact `RuntimeMemberState`, never `PickerMemberStatus` | §2 |
| B3 | One task id is one row; at most one `active` task per agent, enforced by the database; `assigned` rows are the queue | §3.1 |
| B4 | ATM resolves nothing but a task id; no provider, no body dereference | §3 |
| B5 | Queue order is `(position, assigned_at, task_id)`; `position` is its own column; `assigned_at` is reset only by reassign/reopen; `--head` = position 2 when a task is active | §4.3 |
| B6 | Close outcome is typed: `completed \| refused \| cancelled`; reassignment and reopen are explicit `assign` transitions on the same id | §3.1a, §4 |
| B7 | `blocked` is an agent state; `refused` is a task outcome; the words never cross | §4.1 |
| B8 | `atm task` is a closed set `{assign, close, move, list, events}`; `atm send --task-id` / `--task-complete` are aliases | §5 |
| B9 | Ack never touches task state; task state never gates an ack | §5.1 |
| B10 | A close delivers its report before applying the close; unknown id blocks, and an already-complete id can be explicitly reopened by `assign` | §5.2 |
| B11 | Escalation is terminal at `TASK_STALLED_REMINDER_THRESHOLD` (10): one message, then nudging stops until the task changes state (start or close) or is reassigned or reopened; a change in the assignee's runtime state alone does not resume nudging. | §6 |
| B12 | Escalation is a message to lead + configured recipients, never a new structure | §6.1, §6.2 |
| B13 | `atm queue` items are a scheduling view over the message: closed on read (on ack when `requires_ack`), selected before tasks, never `active` | §9 |
| B14 | Agent state and task state are queryable: `atm task list [--all]`, `atm task events <id>`, `atm members` | §7 |

## 3. State machines

Three, and only three. Each is a table; each is independently testable in
isolation from storage and from the runtime.

### 3.1 Task state — `atm-storage/src/task_state.rs::transition`

States: `assigned` → `active` → `complete`. Events: `Assigned`, `Started`,
`Completed(outcome)`. `Acked` is **not** a task event (B9).

| state | `Assigned` | `Started` | `Completed(o)` |
| --- | --- | --- | --- |
| none | → `assigned` | reject `no open task` | reject `no open task` |
| `assigned` | → `assigned` (idempotent resend) | → `active`; reject `ActiveElsewhere` when the member's one-active index is already held | → `complete(o)` |
| `active` | → `active` (idempotent resend) | → `active` (idempotent) | → `complete(o)` |
| `complete` | → `assigned` (reopen in place; event `reopened`) | reject `already complete` | reject `already complete` — **informational** at the command layer (B10) |

Row invariants enforced by the database (BA.2): one row per `(team, task_id)`;
at most one `active` row per `(team, assignee)`; `position` unique per open
`(team, assignee)` and `>= 1`; `close_outcome` non-null iff `complete`.
Queue contiguity is a transaction invariant verified by test and `doctor`,
because SQLite cannot express it.

Who may cause each event (design 23:01, 00:42): `Assigned` — any authorized
sender (unchanged); `Started` — the daemon (**§4 R1**); `Completed` — the
assignee, the assigner, or the team lead.

### 3.2 Agent disposition — `atm-http-runtime/src/herdr_queue_wake*.rs`

Input: exact `RuntimeMemberState` from the roster snapshot, joined in memory to
that member's lowest-`position` open task. Evaluated every poll tick; no
transition tracking.

| `RuntimeMemberState` | open task? | action |
| --- | --- | --- |
| `Idle` | yes | nudge, if `now - last_reminded_at >= TASK_REMINDER_INTERVAL_MS` and `lead_notified_count == 0` |
| `Idle` | no | nothing |
| `Active` | any | nothing — never divert |
| `Blocked` | any | one escalation message per blocked episode; zero nudges |
| `Offline` | any | one escalation message per offline episode; zero nudges |
| `Unknown` | any | nothing (no observation accepted yet) |
| `IdentityConflict` | any | nothing; `doctor` already reports it |

Episode = the span from first observation of the state to the first
observation of a different state. `EscalationState` is retained (design §8
correction) and reshaped to `{episodes}`; `Episode.since` comes from the
roster's `state_changed_at` and bounds the mailbox check; the `BLOCKED_RENOTIFY_MS` cooldown and
the breaker gate are deleted. Restart behaviour: **§4 R4**.

### 3.3 Ephemeral message item — a view, not a machine

An `atm queue` message is *open* while `read = 0`, or while
`acknowledged_at IS NULL` when the message `requires_ack`. It is *closed*
otherwise. There is no row, no state column, no event. The existing deferred
marker `mail_message_states.nudge_pending_at` is the item, read as "next
prompt due at" and kept until the item closes. For an `Idle` member the
existing pending drain discharges open messages before the task pass (B13).
Marker lifecycle: **§4 R3**.

## 4. Decisions required from Rand before plan approval

Each has the default the sprint docs are written to. A different answer
changes the named sprint before it opens; nothing else moves.

| id | question | default written into the sprints | alternative |
| --- | --- | --- | --- |
| **R0** | **ADR-061 classification of BA.2's schema change.** Narrowing `PRIMARY KEY (team, task_id, assignee)` → `(team, task_id)` and `task_events` likewise is MAJOR under ADR-061 D2 ("changing a constraint"). ADR-061 D3 says *"No change may require every host to upgrade together."* `STORAGE_SCHEMA_VERSION` does not exist on `develop` (ADR-061 D1), so a pre-migration binary cannot be made to refuse the database. | **MAJOR, one-way, no bridge.** Every host switches daemon and CLI together via `/daemon-switch` (they are one install); rollback is restore from the backup the migration writes. Requires Rand's recorded approval **and** a recorded ADR-061 D3 exception: a comment on this PR **and** an ADR-061 D6 approval entry for Phase BA committed in this docs PR (FNX-BA-CRIT-003 — a PR comment alone is not the durable record D6 requires). BA.2 is blocked until the entry exists. **Entry text, committed verbatim on approval:** *"On &lt;date&gt;, Rand approved Phase BA's SQLite MAJOR change — `PRIMARY KEY (team, task_id)` on `tasks` and `(team, task_id, seq)` on `task_events`, the `position` / `close_outcome` columns and their `CHECK`s — as one-way with no compatibility bridge, with a D3 exception: daemon and CLI are one install and always switch together via `/daemon-switch`; rollback is restoring the `VACUUM INTO` backup the migration writes; a pre-BA binary against the migrated database operates read-only-safe: reads and ordinary mail work, legacy task-bearing acks still run the old transition, assignment writes fail on the new constraints (BA.2 'Rollback and the pre-BA binary'). Recorded on PR #1398."* (ATM-QA-001 / SCHEMA-SQLITE-ROLLBACK). | Additive: keep both PKs, enforce one-row and one-active in the writer + `doctor`. MINOR, no exception; B3's "enforced by the database" becomes "enforced by the writer". |
| **R1** | **What is `start`?** Design §5: no `start` verb; "start is implicit in beginning work and produces the receipt". Once ack is decoupled (BA.1) nothing on `develop` moves `assigned` → `active`. | **Start = the queue-nudge handoff for that task.** When the runtime successfully hands the task's nudge to an `Idle` member, it applies `Started` (actor `Daemon`) and sends the receipt to the assigner in the same write. A successful handoff of the task's nudge — the deferred assignment message via the queue drain, or a task reminder — applies Started and sends the receipt. Observable, ATM-owned, needs no agent action, gives `--head` = position 2 its meaning. | An `atm task start` verb (sixth verb); or the assignee's first `atm send --task-id <id>`. |
| **R3** | **How does an undischarged `atm queue` item stay remindable?** Design §9 says the message's own state is the state and that `PendingNudgeStore` already holds the item; but on `develop` the marker (`nudge_pending_at`) is cleared on the first successful handoff, after which an unread queue item is indistinguishable from an immediately-sent message. | **The marker lives until the item closes.** `nudge_pending_at` is read as "next prompt due at": admission → now; successful handoff → now + `TASK_REMINDER_INTERVAL_MS`; read (or ack when `requires_ack`) → `NULL`. Claim eligibility adds `nudge_pending_at <= now`. Only messages that carried the marker are ever reminded — an immediate `atm send` never is (design §9: "Interrupts happen today because atm send defaults to `NudgeMode::Immediate`"). No new column. | One `delivery_mode` column on `mail_message_states` (design-excluded new state). |
| **R4** | **Escalation across a daemon restart.** `EscalationState` is in-RAM; after a restart a still-blocked agent looks like a new episode. | **Each target's mailbox is the record (design §6.1, §6.2).** Once per episode start, per target (unique lead and every configured recipient), the runtime reads that mailbox for a daemon-sent message whose `summary` equals `escalation:<kind>:<agent>@<team>` (one constructor, `escalation_summary`) and whose timestamp is at or after `Episode.since` (the roster's `state_changed_at`). A target holding one is skipped; every other target is written. A cleared-then-new episode has a later `since`, so it is reported again. Host-qualified recipients have no local row (develop router refuses local echo for cross-host writes); they are suppressed only by the in-RAM episode, so a daemon restart during an open episode may re-report them once. Accepted bound; no durable receipt (design §6.1). No `cleared_at`; the only in-RAM state is the current episode per member. | Persist episode start on the roster record (new state — design-excluded). |

| **R5** | **Consecutive-refusal threshold.** Design §4.2 says consecutive refusals by one agent escalate but names no number. | `TASK_CONSECUTIVE_REFUSAL_THRESHOLD = 3`; derive the trailing refusal run from closed task rows each tick, escalate at 3, then hold task prompts while retrying only missing refusal mail; a non-refused close or reassign/reopen resets the derived run. | 2, or make it a per-team setting (outside the budget). |
| **R6** | **One id for the life of a task.** Rand's amendment §3.1a makes reassignment and reopen explicit in-place `assign` transitions. | Decided: one `(team, task_id)` row can be reassigned or reopened any number of times; every transition is an event row under that id. | Require a new id for reassignment. |

R2 is intentionally unused: the question it named (a `start` verb) is settled
by design §5 and folded into R1.

Reassignment is **not** a question: design §4 and Rand (23:01) say
Reassignment and reopen are explicit `assign` transitions on the existing id;
there is no `reassign` or `reopen` verb and no close outcome named reassigned.

## 5. Sprint sequence

| sprint | doc | wave | recommended |
| --- | --- | --- | --- |
| BA.1 | [Ack/task separation](./sprint-BA.1-ack-task-separation.md) | 1 | Cipher-311d / fast |
| BA.2 | [Task identity, queue position, typed outcome, mutation boundary, migration](./sprint-BA.2-task-identity-queue.md) | 2 | arch-ctm / deep-reasoning |
| BA.3 | [Nudge invariant and terminal escalation](./sprint-BA.3-nudge-invariant.md) | 3 | arch-ctm / deep-reasoning |
| BA.4 | [`atm task` closed command set](./sprint-BA.4-atm-task-commands.md) | 4 | arch-ctm / deep-reasoning |
| BA.5 | [`atm queue` as an ephemeral item](./sprint-BA.5-queue-ephemeral-item.md) | 4 | arch-ctm / deep-reasoning |
| BA.6 | [Documentation, CLAUDE.md, ADR index](./sprint-BA.6-docs.md) | 5 | Cipher-311d / fast |

```
wave 1   BA.1
wave 2   BA.2            (must_follow BA.1)
wave 3   BA.3            (must_follow BA.2)
wave 4   BA.4 ∥ BA.5     (both must_follow BA.3)
wave 5   BA.6            (must_follow all, PR completion)
```

## 6. Dependency relations

| sprint | relation | rationale |
| --- | --- | --- |
| BA.2 | `must_follow` BA.1 (dev push; branch ancestry) | both edit `atm-storage/src/task_state.rs` and `atm-storage-rusqlite/src/writer/task_ops.rs`; BA.1 removes the `Acked` coupling, BA.2 then changes identity in the same files |
| BA.3 | `must_follow` BA.2 (dev push) | reads `position` and applies `TaskOp::Start` through the boundary BA.2 ships |
| BA.4 | `must_follow` BA.3 (dev push) | BA.4's `TaskMove` router arm lands in `storage_and_nudge_router.rs::dispatch_non_write`, the file whose `reader tick` BA.3 changes for refusal escalation (PLAN-SCOPE-001); BA.4 also consumes `RefusalRun` and the `escalate_mail` seam BA.3 ships |
| BA.5 | `must_follow` BA.3 (dev push) | edits `complete_successful_claim` in `herdr_queue_wake.rs`, which BA.3 rewrites |
| BA.4 / BA.5 | `parallel_safe` | BA.4 owns `crates/atm/src/commands/*`, `crates/atm-core/src/{protocol.rs,task_close.rs,task_query.rs,send/*}`, the `TaskMove` arm of `writer/ops.rs` and of `storage_and_nudge_router.rs::dispatch_non_write` (`:484`). BA.5 owns `pending_nudge_store.rs`, the `PendingNudgeStore` trait block in `contract.rs`, `nudge_dispatch.rs`, `herdr_queue_wake.rs::complete_successful_claim`, and the handoff-helper rename sites (`atm-daemon-bootstrap/src/{lib.rs,queue_drain.rs,received_hook_selector.rs}`, `atm-core/tests/nudge_mode.rs`, `atm-architecture/tests/boundary_enforcement.rs`). One shared file, disjoint regions: `storage_and_nudge_router.rs` (BA.4 `:484` arm; BA.5 test-fixture adapter `:1240-1245`). No acceptance criterion asserts the other's behaviour. `plan-scope-reviewer` verifies. |
| BA.6 | `must_follow` every other sprint (PR completion) | documents the shipped surface |

Merge-forward trigger for every `must_follow`: parent development pushed, not QA.
Merge parent → child before every dev/fix round.

## 7. Worktrees and stack

```bash
/sc-git-worktree --create integrate/phase-ba develop
/sc-git-worktree --create feature/ba1-ack-task-separation   integrate/phase-ba
/sc-git-worktree --create feature/ba2-task-identity-queue   feature/ba1-ack-task-separation   # stacked on BA.1
/sc-git-worktree --create feature/ba3-nudge-invariant       integrate/phase-ba
/sc-git-worktree --create feature/ba4-atm-task-commands     integrate/phase-ba
/sc-git-worktree --create feature/ba5-queue-ephemeral-item  integrate/phase-ba
/sc-git-worktree --create docs/ba6-task-nudge-documentation integrate/phase-ba
```

BA.1 → BA.2 is one gh stack (branch ancestry). BA.3 branches off
`integrate/phase-ba` once BA.2 has merged; BA.4 and BA.5 branch off it once
BA.3 has merged and merge it forward before every dev/fix round. They are not
stacked because none edits a parent's files after the parent has merged.
Layers join the stack when their PR opens. Merge commits only; never squash. Stack
discipline per `/gh-stack-view`.

## 8. ADR-061 governed interfaces

| sprint | interface | change | class |
| --- | --- | --- | --- |
| BA.2 | SQLite | PK narrowing ×2, two partial unique indexes, `position`, `close_outcome` ×2, two `CHECK`s; table rebuild migration with `VACUUM INTO` backup | **MAJOR — R0** |
| BA.2 | HTTP/peer API | `WriteRequest.task_op: Option<TaskOp>` and `placement: Option<MoveTarget>` added; absent placement decodes as `None` (END); `task_complete` retained decode-only; `TaskRow`/`TaskEventRow` adds optional `close_outcome` and `position`; `HTTP_API_VERSION` 1.4.0 → 1.5.0. | MINOR |
| BA.4 | HTTP/peer API | `RequestEnvelope::TaskMove` / `ResponseEnvelope::TaskMove` (additive variants, local-only; peer ingress rejects explicitly); `HTTP_API_VERSION` 1.5.0 → 1.6.0; `atm task move` requires 1.6.0. Fixtures: verbatim 1.5.0 payloads decode on 1.6.0. | MINOR |
| BA.3, BA.5 | Herdr IPC | none; request shape and `HERDR_MINIMUM_VERSION` unchanged | none |

`schema-reviewer` reviews BA.2 and BA.4 at plan review and phase end. BA.2's
SQLite change additionally needs the ADR-061 D6 Phase BA entry (R0) before it
opens.

## 9. Complexity budget — exhaustive additions list

Zero new tables, sealed traits, semantic storage capabilities, id types, or
state machines beyond the three in §3. Everything this phase adds, by sprint:

| sprint | addition |
| --- | --- |
| BA.1 | nothing — deletions only |
| BA.2 | columns `tasks.position`, `tasks.close_outcome`, `task_events.close_outcome`; PK `(team, task_id)` ×2; indexes `one_active_task_per_agent`, `tasks_position_per_member`; two `CHECK` constraints |
| BA.2 | `QueuePosition(NonZeroU32)` newtype; `TaskCloseOutcome`; `TaskState::Complete(TaskCloseOutcome)`; transition input `TaskEvent::{Assigned, Started, Completed(outcome)}`; audit `TaskEventKind::{Assigned, Started, Reassigned, Reopened, Completed, Refused, Cancelled, Moved, Migrated}`; `TaskRejected { kind }` + six `TaskRejectionKind` variants (NoOpenTask, NotAuthorized, ActiveElsewhere, UnknownTarget, StaleCounterparty, AlreadyComplete); `TaskRow.position`; `Transition(pub TaskState)` struct |
| BA.2 | `TaskStateTag`, `TaskRowWire`, `TaskEventRowWire`, `TaskOp { Start, Close }`, `MoveTarget { Head, End, Before }`; `WriteRequest.task_op` plus optional `placement` copied unchanged through the persisted envelope (None = END); `RefusalRun`, `RefusalRun`, and `refusal_run(team, assignee)`; `HTTP_API_VERSION` 1.5.0 |
| BA.2 | `AsyncTaskLedgerReader::open_tasks_for_team`; `task_migration.rs` (`migrate_task_identity`, `TaskMigrationReport`, crate-private); `DoctorFinding::TaskQueueGap` |
| BA.3 | `herdr_task_disposition.rs`: `TaskDisposition`, `EpisodeKind` (+ `as_str`), `HoldReason` (incl. `NoDeliveryChannel`, `RefusalsEscalated`), `dispose` with derived `consecutive_refusals`, `reminder_due`; `TASK_REMINDER_INTERVAL_MS` moved to `atm-storage/src/task_store.rs` (`i64`); `EscalationState` reshaped to `{episodes}` with `Episode { kind, since, notified }`, `observe`, `mark_notified`; `escalation_summary(kind, member, task)` (the one summary constructor), `episode_already_reported(target, summary, since)` with an unbounded mailbox scan narrowed by daemon sender; `escalate_mail(…, suppress_since)` used by every terminal escalation (episodes, stalled, refusals); `EscalationKind::OfflineEscalated` + `From<EpisodeKind>`; `crates/atm-architecture/tests/escalation_ownership.rs` (path visitor + shape assertions `runtime_state_only_constructs`, `still_idle_is_a_single_comparison`); `runtime_state(Option<HerdrAgentStatus>)` widened; `still_idle(runtime, member) -> bool`; start-owed retry of `TaskOp::Start` for reminded `assigned` heads; refusal-run SQL derivation and refusal-hold tick retry tests; `state_write_from_other_module_fails_boundary_gate`; `EscalationKind::RefusalsEscalated` (`BreakerOpened` deleted); `MemberObservation`; `task_started` template text (existing template class, no new nudge kind) |
| BA.4 | clap `Task(TaskCommand)` with five subcommands (design §5 syntax; `list` has only `--all`/`--json`); `OutcomeArg`; `task_query.rs` (`TaskListQuery`, `TaskEventQuery`, `TaskPage` — copied from AZ); `task_close.rs` (`ClosePreflight`, `preflight_close`, `report_recipient`); `require_daemon_api`; ULID minting for an omitted `--task-id`; `RequestEnvelope::TaskMove`, `ResponseEnvelope::TaskMove`, `TaskMoveRequest`, `TaskMoveOutcome`; `WriteOp::TaskMove`, `WriteOpResult::TaskMoved`; `HTTP_API_VERSION` 1.6.0 |
| BA.3 | `complete_task_handoff(runtime, member, head, now)` is the sole Start-write helper; BA.3 reminders and BA.5 head-task queue handoffs call it; head-only activation and failed-start retry tests; `herdr_task_start.rs` / `complete_task_handoff` |
| BA.5 | `OPEN_ITEM_SQL` (in `claim_next_pending` and `list_pending_members`); `PendingNudgeStore::rearm_pending_after_handoff` (rename of `clear_pending_on_handoff`); `clear_pending_on_read` deleted; `mark_message_read` close-or-rearm `CASE`; `requeue_pending` interval back-off at `MAX_NUDGE_ATTEMPTS`; `nudge_dispatch::rearm_queue_marker_after_handoff` (rename); claim predicate `nudge_pending_at <= now`; `HoldReason::MailPending`; deferred assignment handoff calls BA.3's `complete_task_handoff` only for the assigned head |
| BA.6 | nothing |

A sprint that needs an entry not on this list stops and amends this table
first. Phase acceptance #10 gates on it.

### 9.1 Failure modes this phase adds (RBP-F001)

Every new failure reuses an existing `AtmError` constructor / code; no new
error code is added.

| failure | surfaced as | where |
| --- | --- | --- |
| task op on a missing / foreign task; lead authority on a team with ≠ 1 leads; second active task; `--before` naming another member's task | `TaskRejected { kind: TaskRejectionKind::{NoOpenTask, NotAuthorized, ActiveElsewhere, UnknownTarget} }`; a complete task is reopened by explicit `assign` | BA.2 writer |
| close message recipient is no longer the current counterparty (task reassigned between preflight and write) | `StaleCounterparty`: atomic rejection, nothing written; CLI re-preflights and recomposes once; a second `StaleCounterparty` → exit 1 `task <id> changed hands twice during close; retry` |
| close repeated after completion | `AlreadyComplete`: informational at the command layer — report delivered as plain mail, `task <id> was already closed (<outcome>) on <at>`, exit 0, no task event |
| `task_id` and legacy `task_complete` name different tasks | `AtmError::validation_with_recovery` (`error.rs:377-382`) | BA.2 `task_op_normalized` |
| `state`/`close_outcome`/`position` wire combination impossible (`from_parts`, complete row with position, terminal→open) | `AtmError::validation` on decode | BA.2 wire types |
| migration cannot complete (duplicate-group conflict it cannot resolve, I/O) | transaction rolled back; startup fails with the existing storage error naming the backup path | BA.2 migration |
| queue not contiguous | `DoctorFinding::TaskQueueGap` (doctor report, not an error) | BA.2 doctor |
| refused/cancelled close without a reason; close without a report; move with ≠ 1 target | clap `ArgGroup` / `validate()` usage error before any request | BA.4 CLI |
| CLI verb needs a newer daemon | existing `CompatibilityVerdict` refusal via `require_daemon_api` | BA.4 CLI |
| `TaskMove` envelope arrives on peer ingress | existing envelope rejection (explicit, never silent) | BA.4 router |
| member has no dispatchable backend | `Hold(NoDeliveryChannel)`, one `warn` per tick, no error | BA.3 runtime |
| escalation mail write fails | logged `warn`; repaired by the verify-and-retry path on the next tick / refusal | BA.3 runtime |
| consecutive refusal run reaches 3 | `Hold(RefusalsEscalated)`; the tick sends no task prompt and retries only missing refusal escalation mail | BA.3 runtime |
| escalation mailbox verification read fails | logged `warn`; target remains incomplete with no duplicate or notification stamp, and is retried on the next tick | BA.3 runtime |

## 10. Task-subsystem boundary tightening (BA.2, with a written ruling)

`boundaries/atm-storage/task-store.toml` already forbids `task_state_transition`
outside the writer transaction. BA.2 tightens it to the new decisions and the
plan's approval is the written ruling those edits require:

- `[contracts].notes` replay rule → per `(team, task_id)`; events
  `Assigned/Started/Reassigned/Reopened/Completed`; `Acked` removed. The same
  `(team, task_id)` row is retained for reassignment and reopen.
- `[enforcement].review_gates` += `no_ack_task_coupling` (grep gate:
  `apply_task_acknowledgement` has no production caller),
  `no_task_state_write_outside_task_ops` (the only writers of `tasks.state` are
  the `TaskOp` arms in `writer/task_ops.rs`). The single exception is `task_migration.rs::migrate_task_identity`, executed only from `shared_db::ensure_schema`, which writes `tasks.state` and the `migrated` audit kind.
- `[ownership].io_forbidden` += `task_body_dereference` (B4)
- same edits mirrored in `boundaries/atm-storage-rusqlite/task-store-sqlite.toml`
  and `async-task-ledger-reader*.toml` (`open_tasks_for_team` and `refusal_run(team, assignee)` added to the
  existing read surface)

No new manifest. No manifest relaxation.

BA.5 additionally edits the ADR-054 frozen-inventory gate
(`scripts/check-nudge-taxonomy.py` `ALLOWED_NUDGE_IDENTIFIERS`): rename
`clear_pending_on_handoff` → `rearm_pending_after_handoff`, delete
`clear_pending_on_read`. This is a rename of two `PendingNudgeStore` method
names already in the inventory — no new nudge kind, no new identifier
family — and the ADR-054 Phase-BA amendment (`:292-303`) is its recorded
authority. This paragraph is the written ruling for that edit (RBQA-F004).

## 11. Phase AZ code: what is used, and how

Phase AZ was abandoned as too complicated: it grew unauthorised subcommands,
tables and state variables. Several pieces nonetheless landed exactly as the
design specifies. Those are used — cherry-picked, copied, or cited as the
pattern — and each sprint doc has a "Phase AZ code used" table with
`file:line` on `origin/integrate/phase-az` (051652e15). Summary:

| AZ artifact | landed as designed? | use | sprint |
| --- | --- | --- | --- |
| `c99664acc` ack mail-only (−43 lines) | yes | apply by hand (AZ's file rename defeats cherry-pick) | BA.1 |
| `schema_version.rs:284-286` `one_active_task_per_agent` partial unique index | yes | copied verbatim | BA.2 |
| `schema_version.rs:288-321` duplicate-group winner ranking; `:339-380` deterministic active-conflict demotion with audit row | yes | copied, adapted to in-place rebuild | BA.2 |
| `schema_version.rs:383-414` one-IMMEDIATE-transaction migration shape | yes | shape reused | BA.2 |
| `task_command/service.rs:698-750` lead-authority helpers (`require_unique_lead` …) | yes | copied into the writer authority check | BA.2 |
| `herdr_attention_scheduler.rs:120-147` eligibility from the accepted `Idle` observation only | yes (problem b by construction) | pattern kept in `dispose()` | BA.3 |
| `commands/task.rs:73-115, 153-206` `List`/`Events`/`Assign`/terminal arg structs, `MutationActor`, `MessageSource` | yes, as a subset | copied minus priority/scope | BA.4 |
| `task_command.rs:40-70` `TaskListQuery`, `TaskEventQuery`, `TaskPage` | yes | copied | BA.4 |

**Not used, deliberately** (outside the design; listed so the drop is visible):

- eleven-verb `atm task` (Start, Block, Unblock, Reassign, Reopen, Fail, Abort), `PriorityArg`, `AbortReasonArg`
- `tasks_v2`, `task_assignment_attempts`, `task_operations`, `storage_schema_versions`, v1↔v2 bridge triggers, `STORAGE_SCHEMA_VERSION` 2.x
- `AsyncTaskMutationStore`, `AttentionScheduleStore`, `TaskOperationId`, `TaskPriority`, `AssignmentAttempt`, `RosterStateRevision` revalidation, `IdleOpportunity`, lane cursors, two-lane selector, supersession, ADR-063's capability growth
- AZ.1's bounded nudge-title contract (`fc81d97cc`, `4682bbbc0`) — a separate concern; propose as its own small phase if still wanted

Nothing from that list exists on `develop`, so nothing is deleted and no
removal review is required. ADR-063 is marked superseded in this PR.

## 12. Already built — do not design again

Verified on `origin/develop` (design §2, §6.2, §7): `escalation_recipients` +
`atm escalation add|remove|list` + `EscalationScope` + `MAX_ESCALATION_RECIPIENTS`;
`EscalationTargets { lead, recipients }` resolved independently
(`herdr_escalation.rs:204-243`); doctor `RosterNoLead` / `RosterMultipleLeads`;
`Blocked` end to end (`RuntimeMemberState::Blocked`, `HerdrError::AgentBlocked`,
`AtmErrorCode::MemberBlocked`); blocked candidates already skip the nudge and
call `escalate_blocked` (`herdr_queue_wake_reminders.rs:123-132`);
`atm queue` = `NudgeMode::Deferred`; `atm list --tasks | --task-events`; no
`DELETE FROM tasks` anywhere.

## 13. Phase acceptance

1. B1 holds under test: idle+open → nudge; active → zero nudges; blocked and
   offline → one escalation message per episode and zero nudges.
2. No task accumulates more than `TASK_STALLED_REMINDER_THRESHOLD` reminders
   without a task state change: one escalation, then silence.
3. One agent holds at most one `active` task — rejected by the database.
4. One task id is one row — rejected by the database. The 14 live duplicate
   groups are resolved by BA.2's migration or its abort report.
5. `atm task move` reorders without changing any `assigned_at`.
6. A close whose transition fails never loses its report.
7. Acking any message never reads or writes `tasks` / `task_events`.
8. `atm task` has exactly five subcommands; `atm send --task-id` and
   `--task-complete` reach the same writer path.
9. An `Idle` member with an open `atm queue` message is reminded of the
   message before its next task; a read (or acked) message is never reminded.
10. §9's additions table matches the shipped diff exactly.
11. One task id remains one row across reassignment and reopen; the acceptance
    suite proves both transitions and every transition has an event under that
    id (design §3.1a).
12. ADR-062 and ADR-054 amendments merged with BA.2 / BA.5; ADR-063 marked
    superseded; `schema-reviewer` sign-off recorded on BA.2 and BA.4; R0's
    approval and D3 exception recorded as an ADR-061 D6 entry and cited by PR
    comment.
13. `CLAUDE.md` and `docs/team-protocol.md` steer assignment to
    `atm task assign` / `atm send --task-id` and non-interrupting delivery to
    `atm queue`.

The state-write boundary is module-owned: `writer/task_ops.rs` alone writes
`tasks.state` through assignment, start, and close; move and renumber are
state-neutral. The review gate is `no_task_state_write_outside_task_ops`.
14. Reassignment and reopen preserve one task id and record their event kinds; close outcomes remain completed, refused, or cancelled.

Boundary test: `state_write_from_other_module_fails_boundary_gate`.

The writer matrix includes `StaleCounterparty` and recomposes the counterparty once before retry.

| **R7** | Stalled hold reset | task change or reassign/reopen; runtime state alone does not resume nudging |
