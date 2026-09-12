# BA.4 — `atm task` closed command set

| Field | Value |
| --- | --- |
| Design | [`nudge-task-design.md`](./nudge-task-design.md) §4, §5, §5.1, §5.2, §7, §7.1, §9 (commit `18db5acc3`) |
| Recommended | arch-ctm / deep-reasoning — deliver-then-close and a new envelope variant |
| Depends on | `must_follow` BA.3 (dev push) — this sprint adds the `TaskMove` arm to `storage_and_nudge_router.rs::dispatch_non_write` after BA.3 has landed its `reader tick` change |
| `parallel_safe` | BA.5 — this sprint owns `crates/atm/src/commands/*`, `crates/atm-core/src/protocol.rs`, `crates/atm-core/src/api.rs`, `crates/atm-core/src/task_close.rs` (new), `crates/atm-core/src/task_query.rs` (new), `crates/atm-core/src/send/*`, the existing `WriteOp::TaskMove` arm of `writer/ops.rs`, and the `TaskMove` arms of `message_handler.rs` and `storage_and_nudge_router.rs`. Shared files with BA.5 are disjoint regions (plan §4). |
| Worktree | `feature/ba4-atm-task-commands` off `integrate/phase-ba` (merge BA.3 forward) |
| Governed interfaces | HTTP/peer API **MINOR**: `RequestEnvelope::TaskMove`, `ResponseEnvelope::TaskMove`; `HTTP_API_VERSION` `1.5.0` → `1.6.0` |

## Tasks

1. Add `Command::Task` with the five subcommands and arg structs below — `crates/atm/src/commands/task.rs` (new), `commands/mod.rs:114-142` (see "CLI").
2. Re-wire `--task-complete` to a `bool` requiring `--task-id`; route both aliases through the shared request builder with `NudgeMode::Deferred` for assign — `crates/atm/src/commands/send.rs:133-138`, `commands/queue.rs` (see "Aliases").
3. Add the local-team guard and `require_daemon_api` to every verb and alias — `commands/task.rs`, `send.rs` (see "CLI").
4. Add `ClosePreflight`, `preflight_close`, `report_recipient` — `crates/atm-core/src/task_close.rs` (new) (see "Close").
5. Copy `TaskListQuery`, `TaskEventQuery`, `TaskPage` from AZ — `crates/atm-core/src/task_query.rs` (new).
6. Add `TaskMoveRequest`, `TaskMoveOutcome`, the envelope variants, `HTTP_API_VERSION = "1.6.0"` — `crates/atm-core/src/protocol.rs` (see "Move").
7. Route `RequestEnvelope::TaskMove` to the existing `WriteOp::TaskMove`, which calls BA.2's `apply_task_move` — `crates/atm-storage-rusqlite/src/writer/ops.rs`.
8. Add the `TaskMove` arm to `dispatch_non_write`; reject it on peer ingress — `storage_and_nudge_router.rs:484`.
   The mechanical codec surface also adds `HttpRouteKind::TaskMove`,
   `TASK_MOVE_PATH = "/v1/atm/tasks/move"`, and the envelope/encode/decode arms
   in `crates/atm-core/src/api.rs` and
   `crates/atm-http-runtime/src/message_handler.rs`.
9. Render `list` / `events` (human and `--json`) — `commands/task.rs` (see "Output").
10. Delete the paths under "Paths to delete"; write the tests under "Tests".

## Phase AZ code used

| AZ artifact (`origin/integrate/phase-az:crates/atm/src/commands/task.rs`) | how |
| --- | --- |
| `TaskListCommand` (`:73-88`), `TaskEventsCommand` (`:91-103`) | **copied**, minus `--member` and `TaskListScope::Closed` |
| `TaskAssignCommand` (`:106-115`) with `MutationActor` (`:185-190`) and `MessageSource` (`:193-206`) | **copied**, minus `--priority`; `--task-id` optional |
| `TaskTerminalCommand` (`:153-160`) | **copied** as `TaskCloseCommand` with positional `outcome` / `reason` |
| `crates/atm-core/src/task_command.rs:40-70` `TaskListQuery`, `TaskEventQuery`, `TaskPage { Bounded{limit}, All }` | **copied** into `task_query.rs` |

Not used: `Start`, `Block`, `Unblock`, `Reassign`, `Reopen`, `Fail`, `Abort`,
`PriorityArg`, `AbortReasonArg`, `TaskCommandService`, `TaskMutationCommand`,
`operation_id`, `expected_revision`.

## CLI — exactly as it lands (`crates/atm/src/commands/task.rs`, new)

Design §5, verbatim contract:

```
atm task assign <agent> [message | --template <j2> --vars <json>] [--task-id <id>] [--before <other-task-id> | --head]
atm task close  <task-id> <outcome> [reason] [message]
atm task move   <task-id> --before <other> | --head | --end
atm task list [--all]
atm task events <task-id>
```

```rust
#[derive(Debug, Args)]
pub struct TaskCommand {
    #[command(subcommand)]
    command: TaskSubcommand,
}

#[derive(Debug, Subcommand)]
enum TaskSubcommand {
    /// Queue view: the caller's open tasks in (position, assigned_at, task_id) order; --all for every member.
    List(TaskListCommand),
    /// Append-only event history of one task.
    Events(TaskEventsCommand),
    /// Assign a task: sends the assignment message (deferred nudge) and creates or re-points the task row.
    Assign(TaskAssignCommand),
    /// Close a task with a typed outcome: delivers the report, then closes.
    Close(TaskCloseCommand),
    /// Reorder one member's queue.
    Move(TaskMoveCommand),
}

#[derive(Debug, Args)]
struct TaskListCommand {
    #[arg(long)] all: bool,
    #[arg(long)] json: bool,
    #[command(flatten)] caller: CallerArgs,          // --as / --team
}

#[derive(Debug, Args)]
struct TaskEventsCommand {
    task_id: TaskId,
    #[arg(long)] json: bool,
    #[command(flatten)] caller: CallerArgs,
}

#[derive(Debug, Args)]
struct TaskAssignCommand {
    assignee: AgentAddress,
    /// When omitted a ULID is generated and printed.
    #[arg(long = "task-id")] task_id: Option<TaskId>,
    #[arg(long, value_name = "OTHER_TASK_ID", group = "placement")] before: Option<TaskId>,
    #[arg(long, group = "placement")] head: bool,
    #[command(flatten)] message: MessageSourceArgs,  // AZ MessageSource, verbatim
    #[arg(long)] json: bool,
    #[command(flatten)] caller: CallerArgs,
}

impl TaskAssignCommand {
    fn placement(&self) -> Option<MoveTarget> {
        match (&self.before, self.head) {
            (Some(id), false) => Some(MoveTarget::Before { task_id: id.clone() }),
            (None, true) => Some(MoveTarget::Head),
            (None, false) => None,
            (Some(_), true) => unreachable!("clap ArgGroup"),
        }
    }
}

#[derive(Debug, Args)]
struct TaskCloseCommand {
    task_id: TaskId,
    #[arg(value_enum)] outcome: OutcomeArg,
    /// Recorded on the close event; also the report body when no source is given.
    reason: Option<String>,
    #[command(flatten)] report: MessageSourceArgs,   // every source optional; validated in `validate()`
    #[arg(long)] json: bool,
    #[command(flatten)] caller: CallerArgs,
}

impl TaskCloseCommand {
    fn validate(&self) -> Result<(), AtmError> {
        if self.report.is_present() || self.reason.is_some() { Ok(()) }
        else { Err(AtmError::validation("a close <id> <outcome> needs a reason or a report source")) }
    }
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("target").required(true).multiple(false).args(["head", "end", "before"])))]
struct TaskMoveCommand {
    task_id: TaskId,
    #[arg(long)] head: bool,
    #[arg(long)] end: bool,
    #[arg(long, value_name = "OTHER_TASK_ID")] before: Option<TaskId>,
    #[arg(long)] json: bool,
    #[command(flatten)] caller: CallerArgs,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "lowercase")]
enum OutcomeArg { Completed, Refused, Cancelled }

/// The daemon must speak the API version that introduced this verb.
/// assign/close/list/events and the aliases: min = 1.5.0; move: min = 1.6.0.
fn require_daemon_api(verdict: &CompatibilityVerdict, min: HttpApiVersion, verb: &str) -> Result<(), AtmError>;
```

An assignment message always requires an ack: the shared builder sets it
from `task_id`, as `atm send --task-id` does today, so there is no
`--requires-ack` flag.

`atm task assign` on an existing id is the same writer operation whatever
the row's state: same-agent resend, in-place reassign, or reopen (design
§3.1a); no reassign or reopen verb exists.

**Local-only guard (plan §2 R8).** Before preflight and before any
`WriteRequest`, `assignee` (and for close / `--task-id`, the recipient) is
rejected when `resolve_recipient(addr, caller_team).team != caller_team` or
`AgentAddress::host()` is `Some`, with the exact R8 error text. The check
lives in the shared request builder so the aliases inherit it.

**Generated task id:** `ulid::Ulid::new().to_string()` (the `ulid` crate
already backs `AtmMessageId`) parsed through `TaskId::from_str`; printed as
`assigned <task-id> to <agent>` and in `--json` as `{"task_id": …}`.

## Aliases — `crates/atm/src/commands/send.rs:133-138`

```rust
    #[arg(long = "task-id")]
    pub(super) task_id: Option<TaskId>,
    /// Close `--task-id` as completed after delivering this message as the report.
    #[arg(long = "task-complete", requires = "task_id")]
    pub(super) task_complete: bool,
```

`atm send <to> --task-id X …` → `assign`. `atm send <assigner> --task-id X
--task-complete …` → `close X completed`. Both reach the same
`WriteRequest { task_id, task_op, placement, nudge_mode }` as `atm task`;
there is no second code path. `atm queue` inherits both flags unchanged.

**Assignment never interrupts.** `atm task assign` and `atm send --task-id X`
build the request with `.with_nudge_mode(NudgeMode::Deferred)`
(`send/mod.rs:344`), the `atm queue` path; the deferred marker is what BA.3's
pump hands off when the assignee is `Idle`, and that handoff is the Start. A
close report keeps `Immediate` — it is a reply the counterparty is waiting for.

## Close (design §5.2) — `crates/atm-core/src/task_close.rs` (new, ≤ 120 lines)

```rust
pub enum ClosePreflight {
    /// A row exists: send the guarded task write; the writer decides.
    Proceed { row: TaskRow },
    /// No row: block before anything is sent.
    Unknown,
}

pub fn preflight_close(rows: Vec<TaskRow>, task_id: &TaskId) -> ClosePreflight;
// BA4-FIX-R1: CLI reads through daemon List; pure projection.

/// Who receives the report: the other party. A self-addressed send is invalid
/// (`send/recipient.rs:13-31`), and `assigner == assignee` cannot exist.
pub fn report_recipient(row: &TaskRow, caller: &AgentName) -> AgentName {
    if caller == &row.assignee { row.assigner.clone() } else { row.assignee.clone() }
}
```

| stage | `Proceed` | `Unknown` |
| --- | --- | --- |
| 1 preflight (reader lane, `list_tasks(team, None)` filtered) | continue; `ClosePreflight::Proceed { row }` supplies a complete row's outcome for `already_closed` rendering | exit 1: `task <id> does not exist on team <t>`; nothing sent |
| 2 deliver | one guarded write to `report_recipient`; the writer applies the close, or (row already complete) delivers plain and returns `already_closed`, or rejects | — |
| 3 result | `closed <id> (<outcome>)`, exit 0; or `task <id> was already closed (<outcome>); report delivered`, exit 0; or the writer's `AtmError` message, exit 1, nothing delivered, never retried | — |

The preflight's complete-row observation is best-effort because the writer may
close or reassign the row before stage 2. Stage 2's single transaction is what
design §5.2 means by "deliver first":
the report is never lost and never sent from the preflight read alone. The
report body is the message source when given, else the `reason` text.

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

pub const HTTP_API_VERSION: &str = "1.6.0";
```

Writer: `WriteOp::TaskMove(TaskMoveRequest)` (`writer/ops.rs:39`) →
`WriteOpResult::TaskMoved(TaskMoveOutcome)` inside the ordered writer lane.
Router: `dispatch_non_write` (`storage_and_nudge_router.rs:484`) gains the arm;
peer ingress rejects `TaskMove` explicitly (local-only, like every task op).
Compatibility: the envelopes are externally tagged serde enums with no
unknown-variant fallback, so a 1.5.0 daemon fails to decode `TaskMove`
explicitly; a 1.6.0 CLI never sends it below 1.6.0 (`require_daemon_api`);
`docs/atm-daemon/http-api.md` says exactly this.

Phase BA closeout — shipped state (2026-09-12): the existing `MESSAGE`
positional remains the report body when no `--stdin`, `--file`, or `--template`
source is supplied. Validation-family rejections use CLI exit code 3.

## Output

`atm task list` (human): `pos  state  task_id  assigned_at  reminders assigner`,
one block per member under `--all`, with the member's `RuntimeMemberState`
from `atm members` as a header line (read live, not persisted — design §7.1).
`--json`: `Vec<TaskRow>`. Closed tasks are never listed. `atm task events`:
`seq at event from→to actor detail`; `--json`: `Vec<TaskEventRow>`.

## Paths to delete

- `crates/atm/src/commands/list.rs` `--tasks` / `--task-events` flags and
  `commands/task_ledger.rs` rendering — moved under `atm task list/events`
  (CLAUDE.md / team-protocol updated in BA.6 inside the same phase PR)
- `send.rs` `task_complete: Option<TaskId>` → `bool`; BA.2's interim mapping
  in `send.rs` → the shared builder
- `apply_task_message` "cannot assign and complete at the same time" branch
  (`task_ops.rs:141-145`)

## Tests

CLI parse — `crates/atm/src/commands/task.rs` tests:

- `task_has_exactly_five_subcommands` — `__dump-cli-surface` set equals `{list, events, assign, close, move}`.
- `close_parses_positional_outcome_and_reason` — `close T1 refused "no capacity"`; `close T1 bogus` → parse error listing the three values.
- `close_without_reason_or_report_source_is_rejected` — `close T1 refused` alone → the `validate()` error.
- `close_with_source_and_no_reason_is_accepted` — `--stdin` and `--template`; reason `None`, body is the report.
- `move_requires_exactly_one_target` — 0 → error, 2 → error, each of 3 alone → ok.
- `assign_without_task_id_mints_ulid` — 26-char Crockford, printed; `--task-id X` used verbatim.
- `send_task_complete_requires_task_id`; `queue_inherits_both_task_flags`.
- `send_alias_builds_identical_write_request_to_task_command` — assign and close, byte-equal serialised `WriteRequest`; assign is `Deferred`, close is `Immediate`.
- `list_has_only_all_and_json_flags`.
- `require_daemon_api_refuses_older_daemon` — 1.4.0 → every verb fails naming both versions; 1.5.0 → `move` fails, others pass; 1.6.0 → all pass.

Close — `crates/atm/tests/task_close.rs` (fixture daemon, loopback):

- `close_open_task_delivers_and_closes_in_one_write` — report in the assigner's mailbox; row `complete(completed)`; one `completed` event; all three outcomes visible in `events --json`.
- `close_by_assigner_reports_to_assignee` — report in the assignee's mailbox only; row `complete(cancelled)`.
- `close_unknown_task_sends_nothing_and_exits_one`.
- `close_already_closed_delivers_report_without_task_event` — `already_closed` line, exit 0.
- `stale_counterparty_rejection_exits_one_without_retry` — reassign between preflight and write.
- `close_by_third_party_sends_nothing_and_exits_one` — `tasks`, `task_events` (+1 `rejected`), `mail_messages` counts as expected.
- `task_target_on_other_team_or_host_is_rejected_before_send` — `x@other-team` and `x@other.host` via `assign`, `--task-id`, `--task-complete`: exit 1, no rows, no peer dispatch.
- `refusal_releases_next_queued_task` — A has T1, T2; refuse T1 → T2 at position 1 and next nudged.

Assign — `crates/atm/tests/task_assign.rs`:

- `assign_to_active_member_persists_with_zero_prompts_until_idle` — Active recipient; row `assigned`, deferred marker, 0 prompts over 20 ticks; Idle → 1 prompt, `active`, receipt.
- `assign_existing_open_id_to_other_agent_reassigns_in_place`.
- `assign_closed_id_reopens_same_row`.

Move — `crates/atm/tests/task_move.rs`:

- `move_head_end_before_via_cli` — head with an active task lands at 2; end; before; every `assigned_at` unchanged in `list --json`.
- `task_move_to_pre_1_6_0_daemon_is_refused_before_send`.
- `peer_ingress_rejects_task_move_explicitly` — no writer call.
- `protocol_1_5_0_fixtures_decode_on_1_6_0` — every 1.5.0 fixture decodes; a `TaskMove` fixture fails on the pinned 1.5.0 enum copy with an unknown-variant error.

List/events:

- `list_default_is_callers_own_queue` — A sees only A's open tasks; closed never shown.
- `list_all_shows_every_member_grouped_with_state_header`.
- `list_orders_by_position_not_assigned_at` — T1, T2, T3; move T3 head → T3, T1, T2.
- `events_are_seq_ordered_and_include_moved_and_started`.

## Acceptance criteria

1. `atm task --help` lists exactly five subcommands with the design §5 syntax,
   including the optional `MESSAGE` report body; validation-family rejections
   exit 3 according to the CLI error-code map; `__dump-cli-surface` diff shows
   no other new entries.
2. Every test above exists by name and passes under `just test`.
3. `HTTP_API_VERSION == "1.6.0"`; ADR-061 version row added; `schema-reviewer` sign-off on the PR.
4. `grep -rn "task_complete: Option" crates/atm/src` → nothing.
5. `grep -n "NudgeMode::Immediate" crates/atm/src/commands/task.rs` → nothing.
6. `grep -rn "escalate\|AlreadyClosed" crates/atm/src crates/atm-core/src/task_close.rs` → nothing.

## Required validation

`just lint`, `just test`, `just lint boundaries`, RULE-003.
