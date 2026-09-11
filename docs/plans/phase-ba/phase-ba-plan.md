# Phase BA: one invariant, one queue, one task command set

| Field | Value |
| --- | --- |
| Design authority | [`nudge-task-design.md`](./nudge-task-design.md) — committed verbatim at `9b5c7d876` (this branch). Where this plan and the design disagree, the design wins and this plan is the defect. |
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
| B5 | Queue order is `(position, assigned_at, task_id)`; `position` is its own column; `assigned_at` is immutable; `--head` = position 2 when a task is active | §4.3 |
| B6 | Close outcome is typed: `completed \| refused \| cancelled \| reassigned`; reassignment is close-and-create | §4 |
| B7 | `blocked` is an agent state; `refused` is a task outcome; the words never cross | §4.1 |
| B8 | `atm task` is a closed set `{assign, close, move, list, events}`; `atm send --task-id` / `--task-complete` are aliases | §5 |
| B9 | Ack never touches task state; task state never gates an ack | §5.1 |
| B10 | A close delivers its report before applying the close; unknown id blocks, already-complete informs | §5.2 |
| B11 | Escalation is terminal at `TASK_STALLED_REMINDER_THRESHOLD` (10): one message, then nudging stops until the task changes state | §6 |
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
| `complete` | reject `already complete; use a new id` | reject `already complete` | reject `already complete` — **informational** at the command layer (B10) |

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
observation of a different state (`EscalationState.blocked_since`, retained;
the `BLOCKED_RENOTIFY_MS` cooldown is deleted). Restart behaviour: **§4 R4**.

### 3.3 Ephemeral message item — a view, not a machine

An `atm queue` message is *open* while `read = 0`, or while
`acknowledged_at IS NULL` when the message `requires_ack`. It is *closed*
otherwise. There is no row, no state column, no event. For an `Idle` member the
selector emits open messages before the open task (B13). Eligibility
predicate: **§4 R3**.

## 4. Decisions required from Rand before plan approval

Each has the default the sprint docs are written to. A different answer
changes the named sprint before it opens; nothing else moves.

| id | question | default written into the sprints | alternative |
| --- | --- | --- | --- |
| **R0** | **ADR-061 classification of BA.2's schema change.** Narrowing `PRIMARY KEY (team, task_id, assignee)` → `(team, task_id)` and `task_events` likewise is MAJOR under ADR-061 D2 ("changing a constraint"). ADR-061 D3 says *"No change may require every host to upgrade together."* `STORAGE_SCHEMA_VERSION` does not exist on `develop` (ADR-061 D1), so a pre-migration binary cannot be made to refuse the database. | **MAJOR, one-way, no bridge.** Every host upgrades daemon and CLI together (they are one install); rollback is restore from the backup the migration writes. Requires Rand's recorded approval **and** a recorded ADR-061 D3 exception, both as a comment on this PR, cited in ADR-062's amendment. | Additive: keep both PKs, enforce one-row and one-active in the writer + `doctor`. MINOR, no exception; B3's "enforced by the database" becomes "enforced by the writer". |
| **R1** | **What is `start`?** Design §5: no `start` verb; "start is implicit in beginning work and produces the receipt". Once ack is decoupled (BA.1) nothing on `develop` moves `assigned` → `active`. | **Start = the queue-nudge handoff for that task.** When the runtime successfully hands the task's nudge to an `Idle` member, it applies `Started` (actor `Daemon`) and sends the receipt to the assigner in the same write. Observable, ATM-owned, needs no agent action, gives `--head` = position 2 its meaning. | An `atm task start` verb (sixth verb); or the assignee's first `atm send --task-id <id>`. |
| **R3** | **Which undischarged messages does the invariant remind?** Design §9 says the message's own state is the state, but `deferred` origin is not persisted; after handoff `nudge_pending_at` is cleared and an unread `atm queue` item is indistinguishable from any unread message. | **Any open message** (per §3.3) for an `Idle` member is remindable, regardless of origin, under the same rate limit. No new column; "idle with unread mail" is the invariant applied to messages. | One `delivery_mode` column on `mail_message_states`; only `deferred` items are remindable. |
| **R4** | **Escalation across a daemon restart.** `EscalationState` is in-RAM; after a restart a still-blocked agent looks like a new episode. | **At-least-once per daemon process.** A restart may re-report; the message body carries `blocked_since` so a reader can tell a duplicate from a new episode. Design §6.1 already accepts the mirror-image residual risk. | Persist episode start on the roster record (new state — outside the budget). |

| **R5** | **Consecutive-refusal threshold.** Design §4.2 says consecutive refusals by one agent escalate but names no number. | `TASK_CONSECUTIVE_REFUSAL_THRESHOLD = 3`, escalate once when the trailing run reaches exactly 3; a non-refused close resets the run. | 2, or make it a per-team setting (outside the budget). |

Reassignment is **not** a question: design §4 and Rand (23:01) say
close-and-create with outcome `reassigned`. No `reassign` verb.

## 5. Sprint sequence

| sprint | doc | wave | recommended |
| --- | --- | --- | --- |
| BA.1 | [Ack/task separation](./sprint-BA.1-ack-task-separation.md) | 1 | Cipher-311d / fast |
| BA.2 | [Task identity, queue position, typed outcome, mutation boundary, migration](./sprint-BA.2-task-identity-queue.md) | 2 | arch-ctm / deep-reasoning |
| BA.3 | [Nudge invariant and terminal escalation](./sprint-BA.3-nudge-invariant.md) | 3 | arch-ctm / deep-reasoning |
| BA.4 | [`atm task` closed command set](./sprint-BA.4-atm-task-commands.md) | 3 | arch-ctm / deep-reasoning |
| BA.5 | [`atm queue` as an ephemeral item](./sprint-BA.5-queue-ephemeral-item.md) | 4 | arch-ctm / deep-reasoning |
| BA.6 | [Documentation, CLAUDE.md, ADR index](./sprint-BA.6-docs.md) | 5 | Cipher-311d / fast |

```
wave 1   BA.1
wave 2   BA.2            (must_follow BA.1)
wave 3   BA.3 ∥ BA.4     (both must_follow BA.2)
wave 4   BA.5            (must_follow BA.3)
wave 5   BA.6            (must_follow all, PR completion)
```

## 6. Dependency relations

| sprint | relation | rationale |
| --- | --- | --- |
| BA.2 | `must_follow` BA.1 (dev push; branch ancestry) | both edit `atm-storage/src/task_state.rs` and `atm-storage-rusqlite/src/writer/task_ops.rs`; BA.1 removes the `Acked` coupling, BA.2 then changes identity in the same files |
| BA.3 | `must_follow` BA.2 (dev push) | reads `position` and applies `TaskOp::Start` through the boundary BA.2 ships |
| BA.4 | `must_follow` BA.2 (dev push) | writes `position` / `close_outcome` and `TaskOp::{Close, Move}` through the same boundary |
| BA.3 / BA.4 | `parallel_safe` | BA.3 owns `crates/atm-http-runtime/src/herdr_*`; BA.4 owns `crates/atm/src/commands/*`, `crates/atm-core/src/protocol.rs`, `crates/atm-core/src/send/*`. No shared file, no acceptance criterion asserting the other's behaviour. `plan-scope-reviewer` verifies. |
| BA.5 | `must_follow` BA.3 (dev push) | edits the selection pass in `herdr_queue_wake.rs` that BA.3 rewrites |
| BA.5 / BA.4 | `parallel_safe` | disjoint files as above |
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

BA.1 → BA.2 is one gh stack (branch ancestry). BA.3, BA.4 and BA.5 are
independent branches that merge `integrate/phase-ba` forward once their parent
has merged; they are not stacked because none edits a parent's files. Layers
join the stack when their PR opens. Merge commits only; never squash. Stack
discipline per `/gh-stack-view`.

## 8. ADR-061 governed interfaces

| sprint | interface | change | class |
| --- | --- | --- | --- |
| BA.2 | SQLite | PK narrowing ×2, two partial unique indexes, `position`, `close_outcome` ×2, two `CHECK`s; table rebuild migration with `VACUUM INTO` backup | **MAJOR — R0** |
| BA.4 | HTTP/peer API | `WriteRequest.task_op: Option<TaskOp>` (additive; replaces `task_complete`, which no released peer sends — verify in the older-consumer test) and `RequestEnvelope::TaskMove` / `ResponseEnvelope::TaskMove` (additive variants, local-only); `HTTP_API_VERSION` 1.4.0 → 1.5.0 | MINOR |
| BA.3, BA.5 | Herdr IPC | none; request shape and `HERDR_MINIMUM_VERSION` unchanged | none |

`schema-reviewer` reviews BA.2 and BA.4 at plan review and phase end.

## 9. Complexity budget — exhaustive additions list

Zero new tables, sealed traits, semantic storage capabilities, id types, or
state machines beyond the three in §3. Everything this phase adds, by sprint:

| sprint | addition |
| --- | --- |
| BA.1 | nothing — deletions only |
| BA.2 | columns `tasks.position`, `tasks.close_outcome`, `task_events.close_outcome`; PK `(team, task_id)` ×2; indexes `one_active_task_per_agent`, `tasks_position_per_member`; two `CHECK` constraints |
| BA.2 | `QueuePosition(NonZeroU32)` newtype; `TaskCloseOutcome`; `TaskState::Complete(TaskCloseOutcome)`; `TaskEvent::Started`, `TaskEvent::Completed(outcome)`; `TaskEventKind::{Started, Moved, Migrated}`; `TaskRejected { kind }` + `TaskRejectionKind` (5); `TaskRow.position`; `Transition(pub TaskState)` struct |
| BA.2 | `TaskOp { Start, Close, Move }`, `MoveTarget { Head, End, Before }`; `WriteRequest.task_op`, envelope `task_op` (replacing `task_complete`); `TaskCloseApplied`; `TASK_CONSECUTIVE_REFUSAL_THRESHOLD` |
| BA.2 | `AsyncTaskLedgerReader::open_tasks_for_team`; `task_migration.rs` (`migrate_task_identity`, `TaskMigrationReport`, crate-private); `DoctorFinding::TaskQueueGap` |
| BA.3 | `herdr_task_disposition.rs`: `TaskDisposition`, `EpisodeKind`, `HoldReason`, `dispose`; `EpisodeState`/`Episode` (replacing `EscalationState`); `task_started` template text (existing template class, no new nudge kind) |
| BA.4 | clap `Task(TaskCommand)` with five subcommands; `OutcomeArg`; `task_query.rs` (`TaskListQuery`, `TaskEventQuery`, `TaskPage` — copied from AZ); `task_close.rs` (`ClosePreflight`, `preflight_close`); `RequestEnvelope::TaskMove`, `ResponseEnvelope::TaskMove`, `TaskMoveRequest`, `TaskMoveOutcome`; `WriteOp::TaskMove`, `WriteOpResult::TaskMoved`; `EscalationKind::RefusalsEscalated`; `HTTP_API_VERSION` 1.5.0 |
| BA.5 | `Attention`, `select_attention`, `HoldReason::MailPending`, `is_open` |
| BA.6 | nothing |

A sprint that needs an entry not on this list stops and amends this table
first. Phase acceptance #10 gates on it.

## 10. Task-subsystem boundary tightening (BA.2, with a written ruling)

`boundaries/atm-storage/task-store.toml` already forbids `task_state_transition`
outside the writer transaction. BA.2 tightens it to the new decisions and the
plan's approval is the written ruling those edits require:

- `[contracts].notes` replay rule → per `(team, task_id)`; events
  `Assigned/Started/Completed`; `Acked` removed
- `[enforcement].review_gates` += `no_ack_task_coupling` (grep gate:
  `apply_task_acknowledgement` has no production caller),
  `no_task_state_write_outside_task_op` (the only writers of `tasks.state` are
  the `TaskOp` arms in `writer/task_ops.rs`)
- `[ownership].io_forbidden` += `task_body_dereference` (B4)
- same edits mirrored in `boundaries/atm-storage-rusqlite/task-store-sqlite.toml`
  and `async-task-ledger-reader*.toml` (`open_tasks_for_team` added to the
  read surface)

No new manifest. No manifest relaxation.

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
11. ADR-062 and ADR-054 amendments merged with BA.2 / BA.5; ADR-063 marked
    superseded; `schema-reviewer` sign-off recorded on BA.2 and BA.4; R0's
    approval and exception cited by PR comment.
12. `CLAUDE.md` and `docs/team-protocol.md` steer assignment to
    `atm task assign` / `atm send --task-id` and non-interrupting delivery to
    `atm queue`.
