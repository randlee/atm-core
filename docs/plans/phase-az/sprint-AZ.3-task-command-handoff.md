---
phase: AZ
sprint: AZ.3
title: Task command service and durable handoffs
branch: feature/az3-task-command-handoff
integration_branch: feature/az2-task-domain-storage
final_integration_branch: develop
status: planned
recommended_agent: arch-ctm
recommended_model: deep-reasoning
execution_track: stacked
dependency_relations:
  - prerequisite: AZ.2
    dependent: AZ.3
    relation: must_follow
    rationale: The public command service consumes AZ.2's final lifecycle, mutation transaction, revision, attempt, and queue-cleanup contracts; merge AZ.2 forward before every AZ.3 round and merge its PR first.
---

# AZ.3 — Task command service and durable handoffs

## Goal

Ship one canonical `atm task` command/service family for task query and every
legal lifecycle transition. Keep clap and HTTP adapters thin, enforce actor
authorization and idempotency in shared policy, persist terminal handoff mail in
the same AZ.2 transaction as closure, and route legacy task flags through this
service with explicit deprecation warnings.

Every command listed here must be production-ready in this sprint. AZ.3 does
not claim the fair idle-reminder scheduler, which belongs only to AZ.4.

## Public command contract

```text
atm task list [<agent> | --as <agent>] [--team <team>] [--closed]
  [--limit <1..10000> | --all] [--json]
atm task events <task-id> [--team <team>] [--as <agent>]
  [--limit <1..10000> | --all] [--json]

atm task assign <to> <task-id> [--priority high|normal|low] <message-source>
atm task start <task-id>
atm task block <task-id> --reason <text>
atm task unblock <task-id> --resolution <text>
atm task reassign <task-id> <assignee> <message-source>
atm task reopen <task-id> <assignee> <message-source>

atm task complete <task-id> --handoff <agent> <message-source>
atm task fail <task-id> --handoff <agent> <message-source>
atm task abort <task-id> --handoff <agent>
  --reason cancelled <message-source>
atm task abort <task-id> --handoff <agent>
  --reason superseded --successor-task-id <new-id>
  --assign-to <agent> [--successor-priority high|normal|low]
  --assignment-template <path> --assignment-vars <path>
  --handoff-template <path> --handoff-vars <path>
```

For assignment and handoff content, `<message-source>` means exactly one of:
positional plain text, `--stdin`, `--file`, or `--template <path> --vars
<path>`. Template plus variables is documented first and uses the existing
compose/admission pipeline; plain text remains fully supported. A command
previews templates only through `atm compose`, never by persisting source paths
as message content. Supersession has two messages, so its sources are explicitly
namespaced: template-first
`--assignment-template/--assignment-vars` and
`--handoff-template/--handoff-vars`, or plain-text
`--assignment-text` and `--handoff-text`. It does not accept ambiguous
positional text or two competing stdin readers.

`task list` returns open rows by default. `atm task list <agent>` and
`atm task list --as <agent>` are equivalent supported forms for a specific
agent; parser tests keep both forms aligned. `--closed` returns closed history
instead of mixing it into the actionable default. Rows sort per the AZ.2
contract: active first, assigned by priority and original assignment time, then
blocked; closed history sorts newest terminal event first. `task events`
returns the complete stable-`TaskId` event/attempt history. Optional
agent scope is a presentation filter over the assignee recorded on attempts;
it is never part of task identity. Both `--closed` and `events` default to at
most 200 metadata rows at the storage query, not after materialization.
`--limit` selects a smaller or larger bounded page up to 10,000; explicit
`--all` is the ADR-009 opt-out and may not be combined with `--limit`.

## API and service contract

```rust
pub enum TaskCommandRequest {
    List(TaskListQuery),
    Events(TaskEventQuery),
    Mutate(TaskMutationCommand),
}

pub enum TaskListScope {
    Open,
    Closed,
}

pub struct TaskMutationCommand {
    pub operation_id: TaskOperationId,
    pub actor: MemberKey,
    pub task_id: TaskId,
    pub expected_revision: Option<u64>,
    pub action: TaskAction,
}

pub enum TaskAction {
    Assign(AssignmentInput),
    Start,
    Block { reason: NonEmptyText },
    Unblock { resolution: NonEmptyText },
    Reassign(AssignmentInput),
    Reopen(AssignmentInput),
    Complete(HandoffInput),
    LegacyComplete(LegacyCompletionNoticeInput),
    Fail(HandoffInput),
    Abort { reason: AbortInput, handoff: HandoffInput },
}

pub struct HandoffInput {
    pub recipient: MemberKey,
    pub message: ComposedMessageInput,
}

pub struct LegacyCompletionNoticeInput {
    pub recipient: MemberKey,
    pub message: ComposedMessageInput,
}

pub enum AbortInput {
    Cancelled,
    Superseded {
        successor_task_id: TaskId,
        successor: AssignmentInput,
    },
}

#[async_trait::async_trait]
pub trait TaskCommandService: sealed::Sealed + Send + Sync {
    async fn execute(
        &self,
        request: TaskCommandRequest,
    ) -> Result<TaskCommandResponse, TaskCommandError>;
}
```

`atm-core` owns validation, state-policy authorization, message composition,
prepared-assignment/handoff construction, and translation to
`AsyncTaskMutationStore`. `atm` owns clap parsing and stable human/JSON
rendering. The HTTP runtime owns only route/request dispatch to the injected
service. No layer above `atm-storage-rusqlite` opens SQLite or reconstructs a
second transition table.

All mutating JSON responses include `operation_id`, `task_id`, prior/new
state, revision, current attempt, current assignee, outcome/reason when closed,
assignment or handoff `message_id`, and successor id when applicable. Typed
conflicts retain machine-readable error codes and recovery guidance.

## Authorization

- Every operation that creates an assignment attempt—initial `assign`,
  `reassign`, `reopen`, and the successor side of `abort(superseded)`—must
  resolve its recipient to a same-host roster member. A host-qualified
  other-host recipient rejects before mutation with
  `ATM_TASK_HANDOFF_CROSS_HOST_UNSUPPORTED`; Phase AZ cannot atomically join
  an ADR-035 remote delivery to the local task transaction.
- Initial `assign`: an authenticated roster member may assign a distinct
  resolvable same-host member in the same team, using the existing canonical
  address and self-send rules.
- `start`, `block`, and `unblock`: current assignee only.
- `reassign`: current assigner, current assignee, or the team's unique lead;
  the target must be a resolvable same-host team member. Reassign always creates
  a new attempt and leaves the task `Assigned`. It is legal from `Assigned`,
  `Blocked`, or `Closed` (where it has reopen semantics), never from
  `Active`.
- `reopen`: prior assigner, prior assignee, or unique team lead. It creates a
  new attempt for a same-host assignee from `Closed` and never enters
  `Active` directly.
- `complete` and `fail`: current assignee only, and only from `Active`.
- `abort(cancelled)`: current assignee, current assigner, or unique team lead.
  It is legal from every open state: `Assigned`, `Active`, or `Blocked`.
- `abort(superseded)`: current assigner or unique team lead; successor id must
  differ and be unused, and successor assignee must resolve as same-host before
  admission. It is likewise legal from every open state, including `Blocked`;
  aborting a blocked task never requires a meaningless unblock first.
- Any operation relying on lead authority resolves exactly one roster member
  whose `agent_type` is `lead`. Zero matches reject with
  `ATM_TASK_LEAD_MISSING`; two or more reject with
  `ATM_TASK_LEAD_AMBIGUOUS`. Neither condition guesses an actor or suppresses
  an otherwise unauthorized mutation.
- The canonical terminal handoff recipient must be a resolvable **same-host**
  roster member other than the actor. A host-qualified other-host recipient
  rejects before mutation with `ATM_TASK_HANDOFF_CROSS_HOST_UNSUPPORTED` and
  guidance to choose a same-host member; Phase AZ does not pull ADR-035 remote
  post-commit semantics into task closure. All authorization is evaluated
  before entering the writer transaction and rechecked against the current
  revision inside it.

## Closure and handoff semantics

Every canonical terminal command—`complete`, `fail`, or either `abort`
form—requires a durable same-host handoff message. The message is addressed to
another local roster member, contains the closed `TaskId` and terminal outcome
as typed envelope metadata, and is persisted in the same SQLite transaction as
the task event and projection. A cross-host target, or a failure to compose,
authorize, resolve, or persist the local handoff, leaves the task open and emits
no partial message. A retry with the same operation id returns the original
response and message id.

Supersession is one operation: it closes the old task as
`Aborted(Superseded)`, links the new `TaskId`, creates the successor's first
assignment attempt/message, and persists the terminal handoff. It does not edit
the old objective in place.

Canonical `atm task assign` persists assignment mail and task/attempt metadata,
but it creates neither an immediate post-send nudge nor an ordinary
message-key pending-queue entry. The assignment remains durable and body-free
in task projections; only AZ.4's derived attention selector may emit a task
nudge, and only when that attempt is the top runnable task and its assignee is
idle.

Every canonical assignment and every legacy task-linked message sets
`requires_ack = true`. Until acknowledged, it remains visible through
`atm read` and `atm clear` must refuse to remove it, exactly as the existing
task-linked mail obligation requires. Suppressing immediate/ordinary-queue
nudges changes notification scheduling only; it does not weaken durable read,
acknowledgement, or clear protection.

## Legacy migration

- `atm send <assignee> --task-id <id> <source>` remains accepted for one
  compatibility window, emits a deprecation warning, and translates to
  `TaskAction::Assign`. Existing open ids translate to reassign only when the
  actor is authorized; they are never silent resends that overwrite an attempt.
  Like canonical assignment, this adapter requires a same-host assignee,
  persists assignment mail/task metadata, and creates no immediate post-send
  nudge or ordinary message-key pending-queue entry.
- `atm send <recipient> --task-complete <id> <source>` emits a deprecation
  warning and translates to a typed `LegacyComplete` compatibility action.
  It preserves the historical actor set—current assigner or current
  assignee—and is legal from `Assigned` or `Active`. Its named recipient is the
  historical completion-notice recipient and may equal the assignee/actor;
  this narrow adapter exemption is not canonical handoff authorization. The
  notice and closure still persist atomically, and every other actor/state
  rejects. This intentionally widens the shared service policy only for typed
  legacy provenance; canonical `atm task complete` remains assignee-only,
  Active-only, same-host, and non-self.
- `atm list --tasks` and `atm list --task-events` remain deprecated query
  adapters to `atm task list/events` and return the same rows/order. Their
  existing `--member` option maps to canonical agent scoping; `--member` is a
  compatibility spelling, not the sole public form.
- `atm ack` acknowledges task-linked mail but never starts or otherwise
  transitions the task. This semantic switch lands in the same AZ.3 commit as
  the usable `atm task start <id>` CLI/API path; before that commit, the AZ.2
  compatibility adapter still activates on acknowledgement. Help and recovery
  text direct the assignee to explicit start.
- Legacy adapters contain no storage calls or transition logic. Removal is a
  separately versioned future decision, not an AZ.3 deletion.

## Governed HTTP/peer interface change

AZ.3's additive task routes, request variants, and optional response fields are
an ADR-061 minor change. In the same implementation change,
`HTTP_API_VERSION` moves from `1.3.0` to `1.4.0`; both maintained OpenAPI files,
the CLI surface baseline, HTTP/peer ICD, and ADR-061 D5 version record are
updated. A retained 1.3.0 client/daemon fixture proves the older consumer still
uses every pre-AZ route and ignores additive task response fields. New task
requests sent to a 1.3.0 daemon fail with the existing typed unsupported-route/
version response before mutation. Herdr IPC and SQLite versions do not change
in this sprint.

## Deliverables

This is the sole authoritative deliverables list for AZ.3. Every item must land
at a production-ready level; parsing-only, route-only, or happy-path-only
completion is insufficient.

- [ ] D1 — Add the `TaskCommandRequest`, action/input/result DTOs, typed errors,
  authorization matrix, and `TaskCommandService` in `atm-core`; route
  prepared mutations only through AZ.2's storage-neutral async mutation
  boundary.
- [ ] D2 — Add canonical HTTP request/response routing and replacement-runtime
  composition for task queries and mutations. Update the API schema/ICD and
  client mapping; preserve structured errors, deadlines, and retry operation
  ids. Bump `HTTP_API_VERSION` to 1.4.0, update both OpenAPI documents and the
  surface baseline, append the ADR-061 D5 record, and prove a 1.3.0 consumer's
  pre-AZ surface remains compatible.
- [ ] D3 — Implement the complete clap surface, human tables, JSON responses,
  command-specific validation, help text, and installed user documentation.
  `task list` defaults open, `--closed` selects terminal history, and event
  history follows stable task identity across attempts. Closed/event reads
  default to 200 rows and require explicit `--all` to remove the bound. Every
  task-linked message retains `requires_ack`, read visibility, and protection
  from `atm clear` until acknowledged.
- [ ] D4 — Implement template-first assignment/handoff composition and every
  authorization rule. Prove close/handoff and supersession/successor assignment
  use one durable transaction and exact idempotent response. Assignment,
  reassign, reopen, successor assignment, and canonical handoff resolution
  admit only same-host roster members (handoff additionally requires non-self);
  cross-host, missing-lead, and ambiguous-lead cases return their typed errors
  before mutation.
- [ ] D5 — Convert all legacy task send/list flags and task-linked
  acknowledgement to delegating compatibility adapters with warnings. Preserve
  assigner-or-assignee `--task-complete` from Assigned/Active via typed legacy
  provenance. Land acknowledgement's mail-only behavior atomically with the
  working explicit-start path. In `PreparedWrite`, suppress assignment's
  immediate post-send dispatch and ordinary pending-queue marker while
  retaining `requires_ack`; no parallel send path is introduced.
- [ ] D6 — Amend product, CLI, core, runtime, API, error/recovery, team-protocol,
  and user-facing documentation for the command grammar, output, authorization,
  explicit-start rule, handoff requirement, Beads-id boundary, and deprecation
  window. Add end-to-end tests through the real CLI-to-Tokio/Axum-to-SQLite
  route using in-process transport and temporary storage. Update the
  `ATM_TASK_STALLED` recovery hint to canonical `atm task` syntax and test the
  legacy completion, same-host handoff, task-linked read/clear, and bounded
  history contracts.

## Affected paths

```text
crates/atm-core/src/api.rs
crates/atm-core/src/protocol.rs
crates/atm-core/src/task_api.rs
crates/atm-core/src/task_command.rs
crates/atm-core/src/lib.rs
crates/atm-core/src/send/mod.rs
crates/atm-http-runtime/src/client.rs
crates/atm-http-runtime/src/client_tail.rs
crates/atm-http-runtime/src/lib.rs
crates/atm-http-runtime/src/storage_and_nudge_router.rs
crates/atm-http-runtime/tests/http_v1_3_compat.rs
crates/atm/src/main.rs
crates/atm/src/commands/mod.rs
crates/atm/src/commands/task.rs
crates/atm/src/commands/task_ledger.rs
crates/atm/src/commands/list.rs
crates/atm/src/commands/send.rs
crates/atm/src/commands/ack.rs
crates/atm/src/commands/api.rs
crates/atm/openapi.yaml
crates/atm/tests/task_ledger_cli.rs
crates/atm/tests/openapi_surface.rs
crates/atm/tests/openapi_surface_baseline.json
crates/atm-storage/src/error_catalog.rs
crates/atm-storage/src/error_codes.rs
boundaries/atm-error/error-codes.toml
docs/requirements.md
docs/architecture.md
docs/atm/requirements.md
docs/atm/architecture.md
docs/atm/boundaries.md
docs/atm-core/requirements.md
docs/atm-core/architecture.md
docs/atm-core/boundaries.md
docs/atm-http-runtime/architecture.md
docs/atm-http-runtime/openapi.yaml
docs/atm-daemon/http-api.md
docs/adr/ADR-061-governed-interface-schema-versioning.md
docs/team-protocol.md
docs/atm-error-codes.md
docs/user-documents/tasks.md
docs/user-documents/README.md
boundaries/atm-core/task-command-service.toml
boundaries/atm-http-runtime/http-runtime.toml
docs/plans/phase-az/phase-az-plan.md
docs/plans/phase-az/issues.md
docs/project-plan.md
```

The implementation agent must resolve the actual clap root module name before
editing; the list above uses `crates/atm/src/main.rs` as the current command
registration entry and does not authorize creation of a parallel CLI root.

### Paths to delete

None. Legacy flags are deprecated adapters in this sprint.

### Paths that must not change

- `crates/atm-daemon/**` legacy synchronous runtime/dispatch implementation.
  Updating the maintained HTTP API documentation/schema does not authorize
  legacy runtime code changes.
- AZ.1 metadata/title behavior.
- AZ.2 schema/state semantics except corrections required by a promoted QA
  finding; no duplicate command-owned state machine.
- Herdr queue wake/reminder selection and fairness; AZ.4 owns it.
- Beads implementation or storage.

## Acceptance criteria

This is the sole authoritative acceptance list for AZ.3.

1. Help and parser tests enumerate `list`, `events`, `assign`, `start`,
   `block`, `unblock`, `reassign`, `reopen`, `complete`, `fail`, and
   `abort`, with `assign <to> <task-id>` ordering, both specific-agent list
   forms, mutually exclusive message sources, and stable JSON fields.
2. End-to-end tests prove every legal transition and authorization role,
   including pre-start block, explicit unblock to assigned, reopen/reassign to
   a new attempt, all terminal outcomes, linked supersession, and legacy
   assigner/assignee completion from Assigned or Active.
3. Unauthorized actors, self-handoffs, unknown members/tasks, illegal states,
   cross-host assignment/reassignment/reopen/supersession/handoff recipients,
   missing/ambiguous lead authority, stale revisions, malformed operation ids,
   and conflicting retries fail without any message, task, attempt, event, or
   queue mutation. Every cross-host case uses
   `ATM_TASK_HANDOFF_CROSS_HOST_UNSUPPORTED`.
4. Every canonical terminal command persists exactly one handoff to another
   same-host member in the same SQLite transaction as closure. Template and
   plain-text variants pass; injected message failure rolls back closure, exact
   retry returns the same message/response, and cross-host targets receive the
   typed unsupported error. The legacy completion-notice exemption is tested
   separately and cannot authorize canonical self-handoff.
5. Open list ordering and closed history are correct, bounded at the query, and
   body-free; `--all` is the explicit opt-out. Events show all attempts under
   one stable `TaskId` through bounded pages. Beads-shaped ids work without any
   Beads dependency or copied task detail.
6. Legacy send/list flags emit actionable deprecation warnings, delegate to
   the same task service, and preserve the historical completion actor/state
   set through typed compatibility provenance. Task mail acknowledgement no
   longer starts a task; only `atm task start` can enter `Active`. Canonical and legacy
   assignment create neither an immediate nudge nor an ordinary message-key
   pending-queue entry. The ack switch and start command land atomically.
   Every task-linked message still requires acknowledgement, remains readable,
   and cannot be cleared before acknowledgement.
7. Production CLI traffic uses the maintained Tokio/Axum API and injected
   service/storage boundaries. No direct SQLite access or legacy synchronous
   daemon edit exists.
8. CLI, core, API, crate, boundary, protocol, error, help, and user documents
   describe the same grammar, auth matrix, output, transition, and migration
   contract.
9. The HTTP API reports 1.4.0; both OpenAPI files, surface baseline, and ADR-061
   D5 record agree, and a retained 1.3.0 consumer passes its old-route
   compatibility suite.

## Required validation

This is the sole authoritative validation list for AZ.3. Use in-process
transport and temporary stores; do not start a daemon.

1. `cargo test -p agent-team-mail-core task_command`
2. `cargo test -p agent-team-mail task`
3. `cargo test -p agent-team-mail --test task_ledger_cli`
4. `cargo test -p atm-http-runtime task`
5. `cargo test -p atm-storage-rusqlite task`
6. `cargo test -p agent-team-mail-core`
7. `cargo test -p agent-team-mail`
8. `cargo test -p atm-http-runtime`
9. `cargo test -p atm-http-runtime --test http_v1_3_compat`
10. `cargo fmt --check`
11. `cargo clippy --workspace --all-targets -- -D warnings`
12. `python3 .just/run_lint.py boundaries`
13. `python3 .just/run_lint.py nudge-taxonomy`
14. `python3 .just/check_line_counts.py`
15. `git diff --check`

## Non-closure

- AZ.3 does not replace the idle scheduler or claim fair message/task
  interleaving; AZ.4 owns that behavior.
- AZ.3 does not remove deprecated legacy task flags.
- AZ.3 does not modify the legacy synchronous daemon, run a live/test daemon, or
  perform a tag, release, package publish, or installation.
