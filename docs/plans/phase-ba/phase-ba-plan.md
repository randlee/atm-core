# Phase BA: one invariant, one queue, one task command set

| Field | Value |
| --- | --- |
| Design authority | [`nudge-task-design.md`](./nudge-task-design.md) — committed verbatim at `18db5acc3` (this branch). Where this plan and the design disagree, the design wins and this plan is the defect. |
| Supersedes | Phase AZ (`integrate/phase-az`, PR #1394) — retired unmerged 2026-09-11; branch retained, plan docs retained as history |
| Base | `develop` |
| Integration branch | `integrate/phase-ba` |
| Sprints | 6 |
| Widest parallel wave | 2 |
| Status | ready for scope + critical review, then QA approval |
| Exactness | every significant trait, struct, enum, newtype and constant is written in its sprint doc exactly as it lands; QA diffs source against the doc. Test names in the sprint docs are the acceptance evidence. |

## 1. Task state — `atm-storage/src/task_state.rs::transition`

States: `assigned` → `active` → `complete`. Events: `Assigned`, `Started`,
`Completed(outcome)`. `Acked` is **not** a task event (design §5.1).

| state | `Assigned` | `Started` | `Completed(o)` |
| --- | --- | --- | --- |
| none | → `assigned` | reject `no open task` | reject `no open task` |
| `assigned` | → `assigned` (same agent: resend; other agent: reassign, event `reassigned`) | → `active`; reject when the member's one-active index is already held | → `complete(o)` |
| `active` | → `active` (same agent: resend; other agent: reassign) | → `active` (idempotent) | → `complete(o)` |
| `complete` | → `assigned` (reopen in place; event `reopened`) | ordinary mail write returns `already_closed` | ordinary mail write returns `already_closed` — informational at the command layer (design §5.2) |

Row invariants enforced by the database (BA.2): one row per `(team, task_id)`;
at most one `active` row per `(team, assignee)`; `position` unique per open
`(team, assignee)` and `>= 1`; `close_outcome` non-null iff `complete`.
Queue contiguity is a transaction invariant verified by test, because SQLite
cannot express it. `assigned_at` is the time of the current assignment: set by
assign, reassign, and reopen, never by move or start (design §3.1a, §4.3).

Who may cause each event: `Assigned` — any sender (unchanged); `Started` —
the daemon (§2 R1); `Completed` — the assignee or the assigner (develop's
existing rule, unchanged). Nothing else checks a caller.

Agent disposition (BA.3) and the ephemeral queue item (BA.5) are not state
machines: the first is one pure function over the roster record, the second is
a SQL predicate over the message's own columns.

## 2. Decisions

Each has the default the sprint docs are written to.

| id | question | decision |
| --- | --- | --- |
| **R0** | ADR-061 classification of BA.2's schema change (PK narrowing, new columns and `CHECK`s). | **MAJOR, one-way, no bridge.** Approved by Rand 2026-09-11; ADR-061 D6 entry landed in this PR (`ab44564bc`); PR #1398 comment posted. Daemon and CLI are one install and switch together via `/daemon-switch`; rollback is restoring the `VACUUM INTO` backup the migration writes. |
| **R1** | What is `start`? Design §5: no `start` verb. | **Start = the queue-nudge handoff for that task.** When the runtime hands the head task's nudge (deferred assignment message or task reminder) to an `Idle` member, it applies `Started` (actor `atm-daemon`) and sends the `task_started` receipt to the assigner. |
| **R3** | How does an undischarged `atm queue` item stay remindable? | **The deferred marker lives until the item closes.** `mail_message_states.nudge_pending_at` is read as "next prompt due at": admission → now; handoff → now + `TASK_REMINDER_INTERVAL_MS`; read (or ack when `requires_ack`) → `NULL`. Only messages that carried the marker are ever reminded; an immediate `atm send` never is. No new column. |
| **R4** | Escalation across a daemon restart. | **Each target's mailbox is the record (design §6.1).** When an episode is first seen, the runtime reads each target's mailbox for a daemon-sent message with summary `escalation:<kind>:<agent>@<team>` timestamped at or after the roster's `state_changed_at`; a target holding one is skipped, every other target is written. The only in-RAM state is `HashMap<MemberKey, EpisodeKind>`. |
| **R5** | Consecutive-refusal threshold (design §4.2 names no number). | `TASK_CONSECUTIVE_REFUSAL_THRESHOLD = 3`. The run is derived each tick from `task_events` (BA.2 `refusal_run`); at 3 the member is escalated once and held from task prompts until a non-refused close, reassign, or reopen. |
| **R6** | One id for the life of a task (design §3.1a). | One `(team, task_id)` row is reassigned or reopened any number of times by `assign`; every transition is an event row under that id. No `reassign`/`reopen` verb. |
| **R8** | Host-qualified task targets. | **Task commands are local-team only in Phase BA.** `atm task assign|close` and `atm send --task-id` reject a target whose resolved team differs from the caller's or whose host is set, at CLI preflight: `AtmError::validation("task commands are local-team only; <addr> resolves to another team or host — send a plain message or assign the local alias")`, exit 1, nothing sent. The writer applies the same check. |

R2 and R7 are unused (folded into R1 and design §6).

## 3. Sprint sequence

| sprint | doc | wave | recommended |
| --- | --- | --- | --- |
| BA.1 | [Ack/task separation](./sprint-BA.1-ack-task-separation.md) | 1 | Cipher-311d / fast |
| BA.2 | [Task identity, queue position, typed outcome, migration](./sprint-BA.2-task-identity-queue.md) | 2 | arch-ctm / deep-reasoning |
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

## 4. Dependency relations

| sprint | relation | rationale |
| --- | --- | --- |
| BA.2 | `must_follow` BA.1 (branch ancestry) | both edit `atm-storage/src/task_state.rs` and `atm-storage-rusqlite/src/writer/task_ops.rs` |
| BA.3 | `must_follow` BA.2 (dev push) | reads `position`, `open_tasks_for_team`, `refusal_run`; applies `TaskOp::Start` |
| BA.4 | `must_follow` BA.3 (dev push) | adds the `TaskMove` arm to `storage_and_nudge_router.rs::dispatch_non_write`, a file BA.3 rewrites |
| BA.5 | `must_follow` BA.3 (dev push) | edits `herdr_queue_wake.rs::complete_successful_claim`, which BA.3 rewrites |
| BA.4 / BA.5 | `parallel_safe` | BA.4 owns `crates/atm/src/commands/*`, `crates/atm-core/src/{protocol.rs,task_close.rs,task_query.rs,send/*}`. BA.5 owns `pending_nudge_store.rs`, the `PendingNudgeStore` trait block, `nudge_dispatch.rs`, `complete_successful_claim`, and the handoff-helper rename sites. Two shared files, disjoint regions: `storage_and_nudge_router.rs` (BA.4 `:484` arm; BA.5 test adapter `:1240-1245`) and `writer/ops.rs` (BA.4 `TaskMove` arm; BA.5 `execute_read_display_state` `:264-284`). |
| BA.6 | `must_follow` every other sprint (PR completion) | documents the shipped surface |

Merge-forward trigger for every `must_follow`: parent development pushed, not QA.
Merge parent → child before every dev/fix round.

## 5. Worktrees and stack

```bash
/sc-git-worktree --create integrate/phase-ba develop
/sc-git-worktree --create feature/ba1-ack-task-separation   integrate/phase-ba
/sc-git-worktree --create feature/ba2-task-identity-queue   feature/ba1-ack-task-separation   # stacked on BA.1
/sc-git-worktree --create feature/ba3-nudge-invariant       integrate/phase-ba
/sc-git-worktree --create feature/ba4-atm-task-commands     integrate/phase-ba
/sc-git-worktree --create feature/ba5-queue-ephemeral-item  integrate/phase-ba
/sc-git-worktree --create docs/ba6-task-nudge-documentation integrate/phase-ba
```

BA.1 → BA.2 is one gh stack. BA.3 branches off `integrate/phase-ba` once BA.2
has merged; BA.4 and BA.5 branch off it once BA.3 has merged and merge it
forward before every dev/fix round. Merge commits only; never squash. Stack
discipline per `/gh-stack-view`.

## 6. ADR-061 governed interfaces

| sprint | interface | change | class |
| --- | --- | --- | --- |
| BA.2 | SQLite | PK narrowing ×2, two partial unique indexes, `position`, `close_outcome` ×2, two `CHECK`s; table rebuild migration with `VACUUM INTO` backup | **MAJOR — R0** |
| BA.2 | HTTP/peer API | `WriteRequest.task_op: Option<TaskOp>` and `placement: Option<MoveTarget>` added; `task_complete` retained decode-only; `TaskRow`/`TaskEventRow` add optional `close_outcome` and `position`; `HTTP_API_VERSION` 1.4.0 → 1.5.0 | MINOR |
| BA.4 | HTTP/peer API | `RequestEnvelope::TaskMove` / `ResponseEnvelope::TaskMove` (additive, local-only; peer ingress rejects explicitly); `HTTP_API_VERSION` 1.5.0 → 1.6.0 | MINOR |
| BA.3, BA.5 | Herdr IPC | none | none |

`schema-reviewer` reviews BA.2 and BA.4 at plan review and phase end.

## 7. Additions list

Zero new tables, sealed traits, semantic storage capabilities, id types, or
state machines beyond §1. Everything this phase adds:

| sprint | addition |
| --- | --- |
| BA.1 | nothing — deletions only |
| BA.2 | columns `tasks.position`, `tasks.close_outcome`, `task_events.close_outcome`; PK `(team, task_id)` ×2; indexes `one_active_task_per_agent`, `tasks_position_per_member`; two `CHECK`s; `task_migration.rs` (`migrate_task_identity`, `TaskMigrationReport`, crate-private) |
| BA.2 | `QueuePosition(NonZeroU32)`; `TaskCloseOutcome`; `TaskState::Complete(TaskCloseOutcome)`; `TaskStateTag`; `TaskEvent::{Assigned, Started, Completed(outcome)}`; `TaskEventKind::{Started, Reassigned, Reopened, Refused, Cancelled, Moved, Migrated}`; `TaskRejected { detail }`; `Transition(pub TaskState)`; `TaskRow.position`; `TaskRowWire`, `TaskEventRowWire` |
| BA.2 | `TaskOp { Start, Close }`, `MoveTarget { Head, End, Before }`; `WriteRequest.task_op`, `.placement`; `SendOutcome.already_closed`; `RefusalRun`; `AsyncTaskLedgerReader::{open_tasks_for_team, refusal_run}`; `TaskStore::load_task(team, task_id)`; `TASK_CONSECUTIVE_REFUSAL_THRESHOLD`; `HTTP_API_VERSION` 1.5.0 |
| BA.3 | `herdr_task_disposition.rs`: `TaskDisposition { Nudge, EscalateStalled, EscalateEpisode(EpisodeKind), Hold(&'static str) }`, `EpisodeKind`, `dispose` (including the `Hold("mail pending")` arm), `reminder_due`; `TASK_REMINDER_INTERVAL_MS` moved to `atm-storage/src/task_store.rs` (`i64`); `EscalationState { episodes: HashMap<MemberKey, EpisodeKind> }` with `observe`; `escalation_summary`, `episode_already_reported`, `escalate_mail(…, suppress_since)`; `EscalationKind::{OfflineEscalated, RefusalsEscalated}` (`BreakerOpened` deleted); `MemberObservation`; `runtime_state(Option<HerdrAgentStatus>)`; `still_idle`; `herdr_task_start.rs::complete_task_handoff`; `task_started` template (existing class); `crates/atm-architecture/tests/escalation_ownership.rs` |
| BA.4 | clap `Task(TaskCommand)` with five subcommands; `OutcomeArg`; `task_query.rs` (`TaskListQuery`, `TaskEventQuery`, `TaskPage`); `task_close.rs` (`ClosePreflight { Proceed, Unknown }`, `preflight_close`, `report_recipient`); `require_daemon_api`; ULID minting for an omitted `--task-id`; `RequestEnvelope::TaskMove`, `ResponseEnvelope::TaskMove`, `TaskMoveRequest`, `TaskMoveOutcome`; `WriteOp::TaskMove`, `WriteOpResult::TaskMoved`; `HTTP_API_VERSION` 1.6.0 |
| BA.5 | `OPEN_ITEM_SQL`; `PendingNudgeStore::rearm_pending_after_handoff` (rename of `clear_pending_on_handoff`); `clear_pending_on_read` deleted; `mark_message_read` close-or-rearm `CASE`; `requeue_pending` interval back-off at `MAX_NUDGE_ATTEMPTS`; `nudge_dispatch::rearm_queue_marker_after_handoff` (rename); claim predicate `nudge_pending_at <= now` |
| BA.6 | nothing |

A sprint that needs an entry not on this list stops and amends this table
first. Phase acceptance 10 gates on it.

## 8. Task-subsystem boundary tightening (BA.2, with a written ruling)

`boundaries/atm-storage/task-store.toml` already forbids `task_state_transition`
outside the writer transaction. BA.2 tightens it and this plan's approval is
the written ruling those edits require:

- `[contracts].notes` replay rule → per `(team, task_id)`; events
  `Assigned/Started/Reassigned/Reopened/Completed`; `Acked` removed.
- `[enforcement].review_gates` += `no_ack_task_coupling`,
  `no_task_state_write_outside_task_ops` (the only writers of `tasks.state`
  are the `TaskOp` arms in `writer/task_ops.rs`, plus
  `task_migration.rs::migrate_task_identity` at schema-ensure time).
- `[ownership].io_forbidden` += `task_body_dereference` (design §3).
- Same edits mirrored in `boundaries/atm-storage-rusqlite/task-store-sqlite.toml`
  and `async-task-ledger-reader*.toml` (`open_tasks_for_team`, `refusal_run`
  added to the read surface).

No new manifest. No manifest relaxation.

BA.5 additionally edits the ADR-054 frozen-inventory gate
(`scripts/check-nudge-taxonomy.py` `ALLOWED_NUDGE_IDENTIFIERS`): rename
`clear_pending_on_handoff` → `rearm_pending_after_handoff`, delete
`clear_pending_on_read`. A rename of two names already in the inventory — no
new nudge kind. This paragraph is the written ruling for that edit.

## 9. Phase AZ code: what is used

Each sprint doc has a "Phase AZ code used" table with `file:line` on
`origin/integrate/phase-az` (051652e15). Summary:

| AZ artifact | use | sprint |
| --- | --- | --- |
| `c99664acc` ack mail-only (−43 lines) | apply by hand | BA.1 |
| `schema_version.rs:284-286` `one_active_task_per_agent` index | copied verbatim | BA.2 |
| `schema_version.rs:288-321` winner ranking; `:339-380` active-conflict demotion with audit row | copied, adapted to in-place rebuild | BA.2 |
| `schema_version.rs:383-414` one-IMMEDIATE-transaction migration shape | shape reused | BA.2 |
| `herdr_attention_scheduler.rs:120-147` eligibility from the accepted `Idle` observation only | pattern kept in `dispose()` | BA.3 |
| `commands/task.rs:73-115, 153-206` arg structs, `MutationActor`, `MessageSource` | copied minus priority/scope | BA.4 |
| `task_command.rs:40-70` `TaskListQuery`, `TaskEventQuery`, `TaskPage` | copied | BA.4 |

Not used: the eleven-verb `atm task`, `tasks_v2`, `task_assignment_attempts`,
`task_operations`, `storage_schema_versions`, bridge triggers,
`AsyncTaskMutationStore`, `AttentionScheduleStore`, lane cursors, supersession,
ADR-063's capability growth. Nothing from that list exists on `develop`.
ADR-063 is marked superseded in this PR.

## 10. Phase acceptance

1. Idle + open task → nudge; active → zero nudges; blocked and offline → one
   escalation message per episode and zero nudges (BA.3 tests).
2. No task accumulates more than `TASK_STALLED_REMINDER_THRESHOLD` reminders
   without a task state change: one escalation, then silence.
3. One agent holds at most one `active` task — rejected by the database.
4. One task id is one row — rejected by the database. The 14 live duplicate
   groups are resolved by BA.2's migration or its abort report.
5. No operation changes `assigned_at` after the first assign.
6. A close whose transition fails never loses its report.
7. Acking any message never reads or writes `tasks` / `task_events`.
8. `atm task` has exactly five subcommands; `atm send --task-id` and
   `--task-complete` reach the same writer path.
9. An `Idle` member with an open `atm queue` message is reminded of the
   message before its next task; a read (or acked) message is never reminded.
10. §7's additions table matches the shipped diff exactly.
11. Reassignment and reopen preserve one task id and record `reassigned` /
    `reopened` events under it.
12. ADR-062 and ADR-054 amendments merged with BA.2 / BA.5; ADR-063 marked
    superseded; `schema-reviewer` sign-off recorded on BA.2 and BA.4; R0's
    approval recorded as an ADR-061 D6 entry.
13. `CLAUDE.md` and `docs/team-protocol.md` steer assignment to
    `atm task assign` / `atm send --task-id` and non-interrupting delivery to
    `atm queue`.
