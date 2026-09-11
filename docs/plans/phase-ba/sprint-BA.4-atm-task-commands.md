# BA.4 — `atm task` closed command set

| Field | Value |
| --- | --- |
| Design | [`nudge-task-design.md`](./nudge-task-design.md) §4, §5, §5.1, §5.2, §7, §7.1, §9 (commit `18db5acc3`) |
| Outcomes | B8, B10, B14 |
| Recommended | arch-ctm / deep-reasoning — three-stage close and a new envelope variant |
| Depends on | `must_follow` BA.3 (dev push) — BA.3 owns `storage_and_nudge_router.rs`; this sprint adds the `TaskMove` arm to `dispatch_non_write` in that file after BA.3 has landed its `commit_write` change (PLAN-SCOPE-001) |
| `parallel_safe` | BA.5 — this sprint owns `crates/atm/src/commands/*`, `crates/atm-core/src/protocol.rs`, `crates/atm-core/src/task_close.rs` (new), `crates/atm-core/src/task_query.rs` (new), `crates/atm-core/src/send/*`, `crates/atm-storage-rusqlite/src/writer/ops.rs` (`WriteOp::TaskMove` arm only), `crates/atm-http-runtime/src/storage_and_nudge_router.rs::dispatch_non_write` (`TaskMove` arm only, `:484`). **Shared file with BA.5:** `storage_and_nudge_router.rs` — BA.5 edits only the `PendingNudgeStore` test-fixture adapter inside `mod tests` (`:1240-1245`, method rename); the regions are disjoint and neither sprint's acceptance criteria assert the other's behaviour (PLAN-SCOPE-008). Every other file above is BA.4-only. |
| Worktree | `feature/ba4-atm-task-commands` off `integrate/phase-ba` (merge BA.3 forward) |
| Governed interfaces | HTTP/peer API **MINOR**: `RequestEnvelope::TaskMove`, `ResponseEnvelope::TaskMove`; `HTTP_API_VERSION` `1.5.0` (BA.2) → `1.6.0` |

## Scope

Exactly five subcommands, with the syntax of design §5. Two aliases on
`atm send`. Assignment is delivered without interrupting (deferred nudge —
design §1 "active → never divert", §9). Close delivers the report to the
counterparty before it applies the close. `move` is the only message-less
mutation and gets its own envelope variant. Refusal escalation is **not**
here (BA.3 emits it from the runtime; this sprint only produces the close).

## Deliverables

| id | deliverable | where |
| --- | --- | --- |
| D1 | `TaskCommand`, `TaskSubcommand` (5 variants), the five arg structs, `OutcomeArg`, `Command::Task` | `crates/atm/src/commands/task.rs` (new), `commands/mod.rs:114-142` |
| D2 | `--task-complete` → `bool` with `requires = "task_id"`; both aliases build the same `WriteRequest` as `atm task`; assignment writes use `NudgeMode::Deferred` | `crates/atm/src/commands/send.rs:133-138`, `commands/queue.rs` |
| D3 | `require_daemon_api(min)` guard on every `atm task` verb and on the aliases | `crates/atm/src/commands/task.rs` |
| D4 | `ClosePreflight`, `preflight_close`, `report_recipient` | `crates/atm-core/src/task_close.rs` (new) |
| D5 | `TaskListQuery`, `TaskEventQuery`, `TaskPage` (AZ copy) | `crates/atm-core/src/task_query.rs` (new) |
| D6 | `TaskMoveRequest`, `TaskMoveOutcome`, envelope variants, `HTTP_API_VERSION = "1.6.0"`, peer-ingress rejection | `crates/atm-core/src/protocol.rs`, `storage_and_nudge_router.rs::dispatch_non_write` |
| D7 | `WriteOp::TaskMove` → `WriteOpResult::TaskMoved` | `crates/atm-storage-rusqlite/src/writer/ops.rs:39,114` |
| D8 | list / events rendering (human + `--json`) | `crates/atm/src/commands/task.rs` |
| D9 | deletions under "Paths to delete"; tests named below | — |

## Phase AZ code used

| AZ artifact (`origin/integrate/phase-az:crates/atm/src/commands/task.rs`) | how |
| --- | --- |
| `TaskListCommand` (`:73-88`), `TaskEventsCommand` (`:91-103`) arg shapes (`--as`, `--team`, `--all`, `--limit`, `--json`) | **copied**, minus `--member` and `TaskListScope::Closed` — design §7.1 has exactly `list` and `list --all` |
| `TaskAssignCommand` (`:106-115`) with flattened `MutationActor` (`:185-190`) and `MessageSource` (`:193-206`: positional text / `--file` / `--stdin` / `--template` + `--vars`, mutually exclusive) | **copied**, minus `--priority`; `--task-id` made optional (design §5) |
| `TaskTerminalCommand` (`:153-160`) — reason + actor + message source | **copied** as `TaskCloseCommand` with positional `outcome` / `reason` (design §5) |
| `crates/atm-core/src/task_command.rs:40-70` `TaskListQuery`, `TaskEventQuery`, `TaskPage { Bounded{limit}, All }` | **copied** into `crates/atm-core/src/task_query.rs` |

Not used: `Start`, `Block`, `Unblock`, `Reassign`, `Reopen`, `Fail`, `Abort`,
`PriorityArg`, `AbortReasonArg`, `TaskCommandService`, `CoreTaskCommandService`,
`TaskMutationCommand`, `operation_id`, `expected_revision`.
Reassign/reopen are not verbs: `atm task assign` on an existing id performs the in-place transition (design §3.1a).

## CLI — exactly as it lands (`crates/atm/src/commands/task.rs`, new)

Design §5, verbatim contract this code implements:

```
atm task assign <agent> --template <j2> --vars <json> [--task-id <id>]
atm task close  <task-id> <outcome> [reason]
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
// Closed set: a Rust enum without `#[non_exhaustive]` is closed by
// construction — five variants is a compile-time fact, no sealing needed (RBP-F003).
enum TaskSubcommand {
    /// Queue view: the caller's open tasks in (position, assigned_at, task_id) order; --all for every member.
    List(TaskListCommand),
    /// Append-only event history of one task.
    Events(TaskEventsCommand),
    /// Assign a task: sends the assignment message (deferred nudge) and creates the task row.
    Assign(TaskAssignCommand),
    /// Close a task with a typed outcome: delivers the report, then closes.
    Close(TaskCloseCommand),
    /// Reorder one member's queue. Never changes assigned_at.
    Move(TaskMoveCommand),
}

#[derive(Debug, Args)]
struct TaskListCommand {
    /// Team-lead view: every member's open tasks (default: the caller's own queue). Design §7.1.
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
    /// Task id. When omitted a ULID is generated and printed (design §5: `[--task-id <id>]`).
    #[arg(long = "task-id")]
    task_id: Option<TaskId>,
    /// Optional placement; mutually exclusive, default END.
    #[arg(long, value_name = "OTHER_TASK_ID", group = "placement")]
    before: Option<TaskId>,
    #[arg(long, group = "placement")]
    head: bool,
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
    /// completed | refused | cancelled (design §4).
    #[arg(value_enum)]
    outcome: OutcomeArg,
    /// Free text recorded on the close event; also the report body when no
    /// --template/--stdin/--file is given. Required for refused/cancelled.
    reason: Option<String>,
    /// Optional richer report (template/vars, stdin, file). Delivered to the
    /// counterparty before the close is applied.
    #[command(flatten)]
    report: MessageSourceArgs,   // every source optional here; validated in `validate()`
    #[arg(long)]
    json: bool,
    #[command(flatten)]
    caller: CallerArgs,
}

impl TaskCloseCommand {
    /// clap cannot express "reason required for two of four enum values" on
    /// a positional; this runs first in `execute`.
    fn validate(&self) -> Result<(), AtmError> {
        let has_report = self.report.is_present() || self.reason.is_some();
        match self.outcome {
            OutcomeArg::Refused | OutcomeArg::Cancelled if self.reason.is_none() =>
                Err(AtmError::validation(format!("{} requires a reason", self.outcome.as_str()))),
            _ if !has_report =>
                Err(AtmError::validation("a close carries a report: give a reason or a message source")),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("target").required(true).multiple(false).args(["head", "end", "before"])))]
struct TaskMoveCommand {
    task_id: TaskId,
    /// Position 1, or 2 when the member has an active task (design §4.3).
    #[arg(long)]
    head: bool,
    #[arg(long)]
    end: bool,
    #[arg(long, value_name = "OTHER_TASK_ID")]
    before: Option<TaskId>,
    #[arg(long)]
    json: bool,
    #[command(flatten)]
    caller: CallerArgs,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "lowercase")]
enum OutcomeArg { Completed, Refused, Cancelled }
```

The `ArgGroup` makes zero targets and two targets both parse errors
(FNX-BA-CRIT-016). `atm task` has no reassign or reopen verb: `assign <agent> --task-id <id>` is a same-agent no-op, reassigns an open row in place, or reopens a closed row in place. It preserves one row and appends `reassigned` or `reopened`; placement uses `MoveTarget` (`--before`/`--head`, default END).

**Generated task id:** `ulid::Ulid::new().to_string()` (the `ulid` crate
already backs `AtmMessageId`, `inbox_message.rs:22`) parsed through
`TaskId::from_str`; printed as `assigned <task-id> to <agent>` and in
`--json` as `{"task_id": …}`. No new type, no registry.

**Daemon-version guard (FNX-BA-CRIT-004/013):** every `atm task` verb and
both aliases run the existing `CompatibilityPreflight` (`protocol.rs:190`)
and then:

```rust
/// The daemon must speak the API version that introduced this verb; an
/// older daemon (not yet restarted after upgrade) would decode `task_op`
/// as absent and read `task_id` as an assignment, or fail to decode
/// `TaskMove` at all.
fn require_daemon_api(verdict: &CompatibilityVerdict, min: HttpApiVersion, verb: &str) -> Result<(), AtmError>;
// assign/close/list/events and the aliases: min = 1.5.0 (BA.2); move: min = 1.6.0
```

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
--task-complete …` → `close X completed`. Both reach the same
`WriteRequest { task_id, task_op, nudge_mode }` as `atm task`; there is no
second code path (test asserts the built request is equal). `atm queue`
inherits both flags unchanged (`commands/queue.rs`).

**Assignment never interrupts (FNX-BA-CRIT-010):** `atm task assign` and
`atm send --task-id X` (without `--task-complete`) build the request with
`.with_nudge_mode(NudgeMode::Deferred)` (`send/mod.rs:344`) — the same
path `atm queue` uses. Design §9: "Interrupts happen today because `atm
send` defaults to `NudgeMode::Immediate` … CLAUDE.md instructs `atm send`
for assignments"; design §1: an Active agent is never diverted. The deferred
marker is what BA.3's pump hands off when the assignee is `Idle`, and that
handoff is the Start (plan §4 R1). A close report (`--task-complete`,
`atm task close`) keeps `Immediate` — it is a reply the counterparty is
waiting for, not new work.

## Close — three stages (design §5.2), `crates/atm-core/src/task_close.rs` (new, ≤ 200 lines)

```rust
pub enum ClosePreflight {
    /// Row open: deliver report and close in one write.
    Proceed { row: TaskRow },
    /// Row already closed: deliver the report as a plain message, then inform.
    ReopenViaAssign { row: TaskRow },
    /// No such task id on this team: block before anything is sent.
    Unknown,
}

pub async fn preflight_close(reader: &dyn AsyncTaskLedgerReader, team: TeamName, task_id: &TaskId, deadline: ReadDeadline) -> Result<ClosePreflight, AtmError>;

/// Who receives the mandatory report. A self-addressed send is invalid
/// (`send/recipient.rs:13-31`, enforced in `write_context.rs:129`), so the
/// report always goes to the *other* party: the assignee reports to the
/// assigner; an assigner or a lead closing someone else's task informs the
/// assignee (FNX-BA-CRIT-015). `assigner == assignee` cannot exist — the
/// assignment itself was a send.
pub fn report_recipient(row: &TaskRow, caller: &AgentName) -> AgentName {
    if caller == &row.assignee { row.assigner.clone() } else { row.assignee.clone() }
}
```

| stage | `Proceed` | `ReopenViaAssign` | `Unknown` |
| --- | --- | --- | --- |
| 1 preflight (reader lane, `list_tasks(team, None)` filtered — no new read method) | continue | continue | exit 1: `task <id> does not exist on team <t>`; nothing sent |
| 2 deliver | one `WriteRequest { to: report_recipient(row, caller), task_id, task_op: Some(Close{outcome, reason}) }` — report and close are **one transaction**; if the close arm rejects, the writer rolls back the whole write and the CLI retries once as stage-2b: plain message (`task_op: None`, `task_id: None`) then reports the rejection | `WriteRequest` with `task_id: None` to `report_recipient` — plain delivery | — |
| 3 result | `closed <id> (<outcome>)`; exit 0 | `task <id> was already closed (<outcome>) on <at>; report delivered`; exit 0 | — |

Stage 2's single transaction is what design §5.2 means by "deliver first":
the report is never lost — either both land or the report lands alone with
an explicit assignment transition. `ReopenViaAssign` directs the caller to `assign`
from BA.2 when the race (closed between stage 1 and 2) occurs: the writer
rejects, the CLI runs 2b and prints the stage-3 informational line. The
report body is the message source when given, else the `reason` text.

## Consecutive-refusal escalation (design §4.2)

Not in this sprint. The refused close is an ordinary stage-2 write; the
writer returns `WriteOutcome.task_close` (BA.2) and the Tokio runtime's
`commit_write` seam emits the escalation (BA.3 "Consecutive-refusal
escalation") — `atm-core` cannot call the escalation path (dependency
direction; FNX-BA-CRIT-014). The refused task's close has already released
the next queued task by renumbering; nothing else is needed for "release"
(requirements §15.4 item 13).

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
`WriteOpResult::TaskMoved(TaskMoveOutcome)`, calling BA.2's
`apply_task_move` inside the ordered writer lane. Router:
`dispatch_non_write` (`storage_and_nudge_router.rs:484`) gains the
`RequestEnvelope::TaskMove` arm. Peer ingress **rejects** `TaskMove` with
an explicit error (local-only; ADR-035 delivery cannot join the task
transaction — same rule as every task op).

**Compatibility contract (FNX-BA-CRIT-013):** `RequestEnvelope` /
`ResponseEnvelope` are externally tagged serde enums (`protocol.rs:53-95`)
with no unknown-variant fallback, so a 1.5.0 daemon *cannot* ignore
`TaskMove` — it fails to decode. The contract is therefore: (a) a 1.6.0
daemon decodes every 1.5.0 request unchanged (additive variant); (b) a
1.6.0 CLI never sends `TaskMove` to a daemon below 1.6.0
(`require_daemon_api(1.6.0)` above); (c) a daemon receiving a variant it
does not know fails explicitly with a decode error, never silently. ADR-061
evidence wording in `docs/http-api.md` says exactly this — not "ignored".

## Output

`atm task list` (human): `pos  state  task_id  assigned_at  reminders
assigner`, one block per member under `--all`; the member's
`RuntimeMemberState` from `atm members` is printed as a header line
(informational, read live, not persisted — design §7.1). `--json`:
`Vec<TaskRow>` (BA.2 wire shape). Closed tasks are never listed; history
is `atm task events <id>`. `atm task events`: `seq at event from→to actor
detail`. `--json`: `Vec<TaskEventRow>`.

## Paths to delete

- `crates/atm/src/commands/list.rs` `--tasks` / `--task-events` flags and
  `commands/task_ledger.rs` rendering — moved under `atm task list/events`;
  the flags are deleted in this sprint, and CLAUDE.md / team-protocol are
  updated in BA.6 inside the same phase PR
- `send.rs` `task_complete: Option<TaskId>` → `bool` (above); the BA.2
  interim mapping in `send.rs` is replaced by the shared builder
- `apply_task_message` "cannot assign and complete at the same time" branch
  (`task_ops.rs:141-145`) — the shape no longer allows it

## Tests

CLI parse — `crates/atm/src/commands/task.rs` tests:

- `task_has_exactly_five_subcommands` — enumerate `TaskSubcommand` via the
  `__dump-cli-surface` output; assert the set equals
  `{list, events, assign, close, move}`.
- `close_parses_positional_outcome_and_reason` — `close T1 refused "no
  capacity"`; `close T1 completed` (no reason, `--template` given) ok;
  `close T1 bogus` → parse error listing the four values.
- `close_refused_requires_reason`, `close_cancelled_requires_reason`,
  `close_completed_without_reason_or_source_is_rejected`,
  `assign_existing_open_id_to_other_agent_reassigns_in_place`,
  `assign_closed_id_reopens_same_row`, `assign_same_agent_open_id_is_noop`,
  `assign_with_head_places_after_active`.
- `move_requires_exactly_one_target` — 0 targets → error, 2 targets →
  error, each of the 3 alone → ok.
- `assign_without_task_id_mints_ulid` — 26-char Crockford, printed;
  `assign --task-id X` uses X verbatim.
- `send_task_complete_requires_task_id`, `queue_inherits_both_task_flags`.
- `send_alias_builds_identical_write_request_to_task_command` — assign and
  close, byte-equal serialised `WriteRequest` (including `nudge_mode`).
- `assign_request_is_deferred`, `close_request_is_immediate`.
- `list_has_only_all_and_json_flags` — `__dump-cli-surface` for `task list`.
- `require_daemon_api_refuses_older_daemon` — stub verdict 1.4.0 → each
  verb fails naming both versions; 1.5.0 → `move` fails, others pass;
  1.6.0 → all pass.

Close — `crates/atm/tests/task_close.rs` (fixture daemon, loopback):

- `close_open_task_delivers_and_closes_in_one_write` — assigner mailbox has
  the report; row `complete(completed)`; exactly one `completed` event.
- `close_by_assigner_reports_to_assignee` — assigner cancels: report in the
  assignee's mailbox, none in the assigner's, row `complete(cancelled)`
  (FNX-BA-CRIT-015).
- `close_by_unique_lead_reports_to_assignee` — lead who is neither party.
- `close_by_lead_who_is_assigner_reports_to_assignee`.
- `close_unknown_task_sends_nothing_and_exits_one` — mailbox count unchanged.
- `close_already_closed_delivers_and_informs_exit_zero`.
- `close_raced_by_concurrent_close_delivers_plain_and_informs` — close from
  two processes; second gets the informational line; two reports delivered.
- `close_rejected_for_authority_delivers_report_and_prints_rejection` —
  third party closes: report lands (2b), row untouched, exit 1.
- `close_each_outcome_roundtrips` — 4 outcomes visible in `atm task events --json`.
- `assign_existing_open_id_to_other_agent_reassigns_in_place`.
- `refusal_releases_next_queued_task` — A has T1, T2; refuse T1 → T2 is
  position 1 (`atm task list --json`) and is the next nudge (BA.3 runtime).

Assign — `crates/atm/tests/task_assign.rs`:

- `assign_to_active_member_persists_with_zero_prompts_until_idle` — the
  recipient is `Active` (heartbeat); `atm task assign` and `atm send
  --task-id` both: row `assigned`, message persisted with the deferred
  marker, **0 prompts** over 20 ticks; flip to `Idle` → 1 prompt, task
  `active`, receipt to assigner (FNX-BA-CRIT-010).
- `assign_same_id_twice_is_idempotent_resend`.

Move — `crates/atm/tests/task_move.rs`:

- `move_head_with_active_task_lands_at_two`, `move_end`, `move_before`,
  `move_by_assignee_is_rejected`, `move_leaves_assigned_at_unchanged_in_list_json`.
- `task_move_to_pre_1_6_0_daemon_is_refused_before_send` (stub verdict).
- `peer_ingress_rejects_task_move_explicitly` — authenticated peer sends
  `TaskMove` → typed rejection, no writer call.
- `protocol_1_5_0_fixtures_decode_on_1_6_0` — every 1.5.0 request/response
  JSON fixture (existing `protocol_compat.rs` pattern) decodes; a 1.6.0
  `TaskMove` fixture fails to decode on the pinned 1.5.0 enum copy with a
  serde unknown-variant error (pinned so the contract is visible).

List/events:

- `list_default_is_callers_own_queue` — caller A sees only A's open tasks;
  B's are absent (design §7.1).
- `list_all_shows_every_member_grouped` and `list_all_shows_roster_state_header`.
- `list_orders_by_position_not_assigned_at` — assign T1, T2, T3; move T3
  head → list shows T3, T1, T2.
- `list_never_shows_closed_tasks` — with and without `--all`; history via
  `events`.
- `events_are_seq_ordered_and_include_moved_and_started`.

## Acceptance criteria

1. `atm task --help` lists exactly five subcommands with the design §5
   syntax; `__dump-cli-surface` diff shows no other new top-level or `task`
   entries; `task list` has only `--all`, `--json` and the caller flags.
2. All tests above pass.
3. `HTTP_API_VERSION == "1.6.0"`; ADR-061 version record row added with the
   compatibility contract wording above; `schema-reviewer` sign-off on the PR.
4. `grep -rn "task_complete: Option" crates/atm/src` → nothing.
5. `grep -n "NudgeMode::Immediate" crates/atm/src/commands/task.rs` → nothing;
   the assign path is `Deferred`.
6. `grep -rn "escalate" crates/atm/src crates/atm-core/src/task_close.rs` → nothing.

## Required validation

`just lint`, `just test`, `just lint-boundaries`, RULE-003; `atm doctor`
on the fixture after the close tests shows no `TaskQueueGap`.

## Out of scope

Runtime disposition and every escalation (BA.3); docs (BA.6); any sixth
verb; read authorisation on `list --all` (design §7: oversight is a view;
no ATM read is role-gated today and this phase adds no authority to reads).

Additional CLI validation test: `assign_before_rejects_with_start_flag`.
