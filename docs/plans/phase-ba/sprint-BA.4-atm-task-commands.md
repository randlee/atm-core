# BA.4 — `atm task` closed command set

| Field | Value |
| --- | --- |
| Design | [`nudge-task-design.md`](./nudge-task-design.md) §4, §5, §5.2, §7 (commit `9b5c7d876`) |
| Outcomes | B8, B10, B14 |
| Recommended | arch-ctm / deep-reasoning — three-stage close and a new envelope variant |
| Depends on | `must_follow` BA.2 (dev push) |
| `parallel_safe` | BA.3 — this sprint owns `crates/atm/src/commands/*`, `crates/atm-core/src/protocol.rs`, `crates/atm-core/src/send/*`, `crates/atm-storage-rusqlite/src/writer/ops.rs` (`WriteOp::TaskMove` only) |
| Worktree | `feature/ba4-atm-task-commands` off `integrate/phase-ba` (merge BA.2 forward) |
| Governed interfaces | HTTP/peer API **MINOR**: `WriteRequest.task_op` (BA.2 field, first wire use), `RequestEnvelope::TaskMove`, `ResponseEnvelope::TaskMove`; `HTTP_API_VERSION` `1.4.0` → `1.5.0` |

## Scope

Exactly five subcommands. Two aliases on `atm send`. Close delivers the
report before it applies the close. `move` is the only message-less
mutation and gets its own envelope variant.

## Phase AZ code used

| AZ artifact (`origin/integrate/phase-az:crates/atm/src/commands/task.rs`) | how |
| --- | --- |
| `TaskListCommand` (`:73-88`), `TaskEventsCommand` (`:91-103`) arg shapes (`--as`, `--team`, `--all`, `--limit`, `--json`) | **copied**, minus `TaskListScope::Closed` (`--all` covers it) |
| `TaskAssignCommand` (`:106-115`) with flattened `MutationActor` (`:185-190`) and `MessageSource` (`:193-206`: positional text / `--file` / `--stdin` / `--template` + `--vars`, mutually exclusive) | **copied**, minus `--priority` |
| `TaskTerminalCommand` (`:153-160`) — reason + actor + message source | **copied** as `TaskCloseCommand` with `--outcome` |
| `crates/atm-core/src/task_command.rs:40-70` `TaskListQuery`, `TaskEventQuery`, `TaskPage { Bounded{limit}, All }` | **copied** into `crates/atm-core/src/task_query.rs` |

Not used: `Start`, `Block`, `Unblock`, `Reassign`, `Reopen`, `Fail`, `Abort`,
`PriorityArg`, `AbortReasonArg`, `TaskCommandService`, `CoreTaskCommandService`,
`TaskMutationCommand`, `operation_id`, `expected_revision`.

## CLI — exactly as it lands (`crates/atm/src/commands/task.rs`, new)

```rust
#[derive(Debug, Args)]
pub struct TaskCommand {
    #[command(subcommand)]
    command: TaskSubcommand,
}

#[derive(Debug, Subcommand)]
enum TaskSubcommand {
    /// Queue view: open tasks per member in (position, assigned_at, task_id) order.
    List(TaskListCommand),
    /// Append-only event history of one task.
    Events(TaskEventsCommand),
    /// Assign a task: sends the assignment message and creates the task row.
    Assign(TaskAssignCommand),
    /// Close a task with a typed outcome: delivers the report, then closes.
    Close(TaskCloseCommand),
    /// Reorder one member's queue. Never changes assigned_at.
    Move(TaskMoveCommand),
}

#[derive(Debug, Args)]
struct TaskListCommand {
    /// Restrict to one member (default: whole team).
    #[arg(long)]
    member: Option<AgentName>,
    /// Include closed tasks.
    #[arg(long)]
    all: bool,
    #[arg(long)]
    json: bool,
    #[command(flatten)]
    caller: CallerArgs,          // --as / --team, existing CallerContextOverrides
}

#[derive(Debug, Args)]
struct TaskEventsCommand {
    task_id: TaskId,
    #[arg(long)]
    json: bool,
    #[command(flatten)]
    caller: CallerArgs,
}

#[derive(Debug, Args)]
struct TaskAssignCommand {
    assignee: AgentAddress,
    #[arg(long = "task-id")]
    task_id: TaskId,
    #[command(flatten)]
    message: MessageSourceArgs,  // AZ MessageSource, verbatim
    #[arg(long = "requires-ack")]
    requires_ack: bool,
    #[arg(long)]
    json: bool,
    #[command(flatten)]
    caller: CallerArgs,
}

#[derive(Debug, Args)]
struct TaskCloseCommand {
    task_id: TaskId,
    #[arg(long, value_enum, default_value_t = OutcomeArg::Completed)]
    outcome: OutcomeArg,
    /// Free text recorded on the close event (required for refused/cancelled).
    #[arg(long, required_if_eq_any([("outcome", "refused"), ("outcome", "cancelled")]))]
    reason: Option<String>,
    /// Report delivered to the assigner before the close is applied. Mandatory.
    #[command(flatten)]
    report: MessageSourceArgs,
    #[arg(long)]
    json: bool,
    #[command(flatten)]
    caller: CallerArgs,
}

#[derive(Debug, Args)]
struct TaskMoveCommand {
    task_id: TaskId,
    #[arg(long, conflicts_with_all = ["end", "before"])]
    head: bool,
    #[arg(long, conflicts_with_all = ["head", "before"])]
    end: bool,
    #[arg(long, conflicts_with_all = ["head", "end"])]
    before: Option<TaskId>,
    #[arg(long)]
    json: bool,
    #[command(flatten)]
    caller: CallerArgs,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutcomeArg { Completed, Refused, Cancelled, Reassigned }
```

Exactly one of `--head | --end | --before` is required (clap group
`required = true`). `Reassigned` is accepted on `close` so the
close-and-create sequence can be expressed; `atm task` has **no** reassign
verb — reassignment is `atm task close <id> --outcome reassigned …` followed
by `atm task assign <new> --task-id <new-id> …`. Documented, not automated.

Top-level: `Command::Task(TaskCommand)` added to
`crates/atm/src/commands/mod.rs:114-142`.

## Aliases — `crates/atm/src/commands/send.rs:133-138`

```rust
    #[arg(long = "task-id")]
    pub(super) task_id: Option<TaskId>,
    /// Close `--task-id` as completed after delivering this message as the report.
    #[arg(long = "task-complete", requires = "task_id")]
    pub(super) task_complete: bool,
```

`atm send <to> --task-id X …` → `assign`. `atm send <assigner> --task-id X
--task-complete …` → `close X --outcome completed`. Both reach the same
`WriteRequest { task_id, task_op }` as `atm task`; there is no second
code path (test asserts the built request is equal). `atm queue` inherits
both flags unchanged (`commands/queue.rs`).

## Close — three stages (design §5.2), `crates/atm-core/src/task_close.rs` (new, ≤ 200 lines)

```rust
pub enum ClosePreflight {
    /// Row open: deliver report and close in one write.
    Proceed { row: TaskRow },
    /// Row already closed: deliver the report as a plain message, then inform.
    AlreadyComplete { row: TaskRow },
    /// No such task id on this team: block before anything is sent.
    Unknown,
}

pub async fn preflight_close(reader: &dyn AsyncTaskLedgerReader, team: TeamName, task_id: &TaskId, deadline: ReadDeadline) -> Result<ClosePreflight, AtmError>;
```

| stage | `Proceed` | `AlreadyComplete` | `Unknown` |
| --- | --- | --- | --- |
| 1 preflight (reader lane, `list_tasks(team, None)` filtered — no new read method) | continue | continue | exit 1: `task <id> does not exist on team <t>`; nothing sent |
| 2 deliver | one `WriteRequest { to: assigner, task_id, task_op: Some(Close{..}) }` — report and close are **one transaction**; if the close arm rejects, the writer rolls back the whole write and the CLI retries once as stage-2b: plain message (`task_op: None`, `task_id: None`) then reports the rejection | `WriteRequest` with `task_id: None` — plain delivery | — |
| 3 result | `closed <id> (<outcome>)`; exit 0 | `task <id> was already closed (<outcome>) on <at>; report delivered`; exit 0 | — |

Stage 2's single transaction is what design §5.2 means by "deliver first":
the report is never lost — either both land or the report lands alone with
an explicit rejection. `AlreadyComplete` uses `TaskRejectionKind::AlreadyComplete`
from BA.2 when the race (closed between stage 1 and 2) occurs: the writer
rejects, the CLI runs 2b and prints the stage-3 informational line.

## Consecutive-refusal escalation (design §4.2)

The daemon's post-write step (the same place assignment nudges are
triggered after a successful write in `crates/atm-core/src/send/`) reads
`SendOutcome.task_close` and, when `consecutive_refusals ==
TASK_CONSECUTIVE_REFUSAL_THRESHOLD` (exactly equal — once per run, not on
every later refusal), sends one escalation message to the team's
`EscalationTargets` via the existing `escalate(...)` path with
`EscalationKind::RefusalsEscalated` (one new variant of the existing enum;
body lists the refused task ids and reasons). The refused task's close has
already released the next queued task by renumbering; nothing else is needed
for "release" (requirements §15.4 item 13).

## Move — message-less envelope (MINOR)

```rust
// crates/atm-core/src/protocol.rs
pub enum RequestEnvelope { …, TaskMove(TaskMoveRequest), … }
pub enum ResponseEnvelope { …, TaskMove(TaskMoveOutcome), … }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskMoveRequest {
    pub caller_identity: AgentName,
    pub caller_team: TeamName,
    pub task_id: TaskId,
    pub target: MoveTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskMoveOutcome {
    pub task_id: TaskId,
    pub assignee: AgentName,
    pub from: QueuePosition,
    pub to: QueuePosition,
}

pub const HTTP_API_VERSION: &str = "1.5.0";
```

Writer: `WriteOp::TaskMove(TaskMoveRequest)` (`writer/ops.rs:39`) →
`WriteOpResult::TaskMoved(TaskMoveOutcome)`, calling BA.2's
`apply_task_move` inside the ordered writer lane. Peer ingress rejects
`TaskMove` (local-only; ADR-035 delivery cannot join the task transaction —
same rule as every task op). An older 1.4.0 consumer test
(`crates/atm-core/tests/protocol_compat.rs`, existing pattern) proves the
new variants are ignored.

## Output

`atm task list` (human): one block per member, `pos  state  task_id
assigned_at  reminders  assigner`, blocked column from `atm members`'
`RuntimeMemberState` (informational, design §7). `--json`: `Vec<TaskRow>`.
`atm task events`: `seq at event from→to actor detail`. `--json`:
`Vec<TaskEventRow>`.

## Paths to delete

- `crates/atm/src/commands/list.rs` `--tasks` / `--task-events` flags and
  `commands/task_ledger.rs` rendering — moved under `atm task list/events`;
  the flags are deleted in this sprint, and CLAUDE.md / team-protocol are
  updated in BA.6 inside the same phase PR
- `send.rs` `task_complete: Option<TaskId>` → `bool` (above)
- `apply_task_message` "cannot assign and complete at the same time" branch
  (`task_ops.rs:141-145`) — the shape no longer allows it

## Tests

CLI parse — `crates/atm/src/commands/task.rs` tests:

- `task_has_exactly_five_subcommands` — enumerate `TaskSubcommand` via the
  `__dump-cli-surface` output; assert the set equals
  `{list, events, assign, close, move}`.
- `close_refused_requires_reason`, `close_completed_reason_optional`,
  `move_requires_exactly_one_target` (0 targets and 2 targets both fail),
  `assign_requires_task_id`, `send_task_complete_requires_task_id`,
  `queue_inherits_both_task_flags`.
- `send_alias_builds_identical_write_request_to_task_command` — assign and
  close, byte-equal serialised `WriteRequest`.

Close — `crates/atm/tests/task_close.rs` (fixture daemon, loopback):

- `close_open_task_delivers_and_closes_in_one_write` — assigner mailbox has
  the report; row `complete(completed)`; exactly one `completed` event.
- `close_unknown_task_sends_nothing_and_exits_one` — mailbox count unchanged.
- `close_already_closed_delivers_and_informs_exit_zero`.
- `close_raced_by_concurrent_close_delivers_plain_and_informs` — close from
  two processes; second gets the informational line; two reports delivered.
- `close_rejected_for_authority_delivers_report_and_prints_rejection` —
  third party closes: report lands (2b), row untouched, exit 1.
- `close_each_outcome_roundtrips` — 4 outcomes visible in `atm task events --json`.
- `close_reassigned_then_assign_new_id_yields_two_rows_two_histories`.
- `third_consecutive_refusal_escalates_once` — 3 refused closes → 1
  escalation mail to lead + recipients; a 4th refused close → no second mail;
  a `completed` then 3 more refused → one more mail.
- `refusal_releases_next_queued_task` — A has T1, T2; refuse T1 → T2 is
  position 1 and is the next nudge (BA.3 runtime) — asserted via
  `atm task list --json`.

Move — `crates/atm/tests/task_move.rs`:

- `move_head_with_active_task_lands_at_two`, `move_end`, `move_before`,
  `move_by_assignee_is_rejected`, `move_leaves_assigned_at_unchanged_in_list_json`,
  `move_is_visible_to_a_1_4_0_consumer_as_unknown_variant` (protocol compat).

List/events:

- `list_orders_by_position_not_assigned_at` — assign T1, T2, T3; move T3
  head → list shows T3, T1, T2.
- `list_hides_closed_without_all`, `list_all_shows_close_outcome`,
  `events_are_seq_ordered_and_include_moved_and_started`.

## Acceptance criteria

1. `atm task --help` lists exactly five subcommands; `__dump-cli-surface`
   diff shows no other new top-level or `task` entries.
2. All tests above pass.
3. `HTTP_API_VERSION == "1.5.0"`; ADR-061 version record row added;
   `schema-reviewer` sign-off on the PR.
4. `grep -rn "task_complete: Option" crates/` → nothing.

## Required validation

`just lint`, `just test`, `just lint-boundaries`, RULE-003; `atm doctor`
on the fixture after the close tests shows no `TaskQueueGap`.

## Out of scope

Runtime disposition (BA.3); docs (BA.6); any sixth verb.
