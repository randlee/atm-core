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
atm task list [<agent> | --as <agent>] [--team <team>] [--closed] [--json]
atm task events <task-id> [--team <team>] [--as <agent>] [--json]

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
it is never part of task identity.

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
    Fail(HandoffInput),
    Abort { reason: AbortInput, handoff: HandoffInput },
}

pub struct HandoffInput {
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

- Initial `assign`: an authenticated roster member may assign a distinct
  resolvable member in the same team, using the existing canonical address and
  self-send rules.
- `start`, `block`, and `unblock`: current assignee only.
- `reassign`: current assigner, current assignee, or the team's unique lead;
  the target must be a resolvable team member. Reassign always creates a new
  attempt and leaves the task `Assigned`. It is legal from `Assigned`,
  `Blocked`, or `Closed` (where it has reopen semantics), never from
  `Active`.
- `reopen`: prior assigner, prior assignee, or unique team lead. It creates a
  new attempt from `Closed` and never enters `Active` directly.
- `complete` and `fail`: current assignee only, and only from `Active`.
- `abort(cancelled)`: current assignee, current assigner, or unique team lead.
  It is legal from every open state: `Assigned`, `Active`, or `Blocked`.
- `abort(superseded)`: current assigner or unique team lead; successor id must
  differ and be unused, and successor assignee must resolve before admission.
  It is likewise legal from every open state, including `Blocked`; aborting a
  blocked task never requires a meaningless unblock first.
- The terminal handoff recipient must be a resolvable roster member other than
  the actor. All authorization is evaluated before entering the writer
  transaction and rechecked against the current revision inside it.

## Closure and handoff semantics

Every terminal command—`complete`, `fail`, or either `abort` form—requires a
durable handoff message. The message is addressed to another agent, contains the
closed `TaskId` and terminal outcome as typed envelope metadata, and is
persisted atomically with the task event and projection. A failure to compose,
authorize, resolve, or persist the handoff leaves the task open and emits no
partial message. A retry with the same operation id returns the original
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

## Legacy migration

- `atm send <assignee> --task-id <id> <source>` remains accepted for one
  compatibility window, emits a deprecation warning, and translates to
  `TaskAction::Assign`. Existing open ids translate to reassign only when the
  actor is authorized; they are never silent resends that overwrite an attempt.
  Like canonical assignment, this adapter persists assignment mail/task
  metadata but creates no immediate post-send nudge and no ordinary
  message-key pending-queue entry.
- `atm send <recipient> --task-complete <id> <source>` emits a deprecation
  warning and translates to `TaskAction::Complete`, treating the target/message
  as the required handoff. It therefore retains atomic closure plus message
  persistence.
- `atm list --tasks` and `atm list --task-events` remain deprecated query
  adapters to `atm task list/events` and return the same rows/order. Their
  existing `--member` option maps to canonical agent scoping; `--member` is a
  compatibility spelling, not the sole public form.
- `atm ack` acknowledges task-linked mail but never starts or otherwise
  transitions the task. Help and recovery text directs the assignee to
  `atm task start <id>`.
- Legacy adapters contain no storage calls or transition logic. Removal is a
  separately versioned future decision, not an AZ.3 deletion.

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
  ids.
- [ ] D3 — Implement the complete clap surface, human tables, JSON responses,
  command-specific validation, help text, and installed user documentation.
  `task list` defaults open, `--closed` selects terminal history, and event
  history follows stable task identity across attempts.
- [ ] D4 — Implement template-first assignment/handoff composition and every
  authorization rule. Prove close/handoff and supersession/successor assignment
  use one durable transaction and exact idempotent response.
- [ ] D5 — Convert all legacy task send/list flags and task-linked
  acknowledgement to delegating compatibility adapters with warnings and no
  state mutation on acknowledgement.
- [ ] D6 — Amend product, CLI, core, runtime, API, error/recovery, team-protocol,
  and user-facing documentation for the command grammar, output, authorization,
  explicit-start rule, handoff requirement, Beads-id boundary, and deprecation
  window. Add end-to-end tests through the real CLI-to-Tokio/Axum-to-SQLite
  route using in-process transport and temporary storage.

## Affected paths

```text
crates/atm-core/src/api.rs
crates/atm-core/src/task_command.rs
crates/atm-core/src/lib.rs
crates/atm-http-runtime/src/client.rs
crates/atm-http-runtime/src/client_tail.rs
crates/atm-http-runtime/src/lib.rs
crates/atm-http-runtime/src/storage_and_nudge_router.rs
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
   a new attempt, all terminal outcomes, and linked supersession.
3. Unauthorized actors, self-handoffs, unknown members/tasks, illegal states,
   stale revisions, malformed operation ids, and conflicting retries fail
   without any message, task, attempt, event, or queue mutation.
4. Every terminal command persists exactly one handoff to another member in the
   same transaction as closure. Template and plain-text variants pass; injected
   message failure rolls back closure, and exact retry returns the same message
   and response.
5. Open list ordering and closed history are correct and body-free; events show
   all attempts under one stable `TaskId`. Beads-shaped ids work without any
   Beads dependency or copied task detail.
6. Legacy send/list flags emit actionable deprecation warnings and produce
   byte-equivalent service results. Task mail acknowledgement no longer starts
   a task; only `atm task start` can enter `Active`. Canonical and legacy
   assignment create neither an immediate nudge nor an ordinary message-key
   pending-queue entry.
7. Production CLI traffic uses the maintained Tokio/Axum API and injected
   service/storage boundaries. No direct SQLite access or legacy synchronous
   daemon edit exists.
8. CLI, core, API, crate, boundary, protocol, error, help, and user documents
   describe the same grammar, auth matrix, output, transition, and migration
   contract.

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
9. `cargo fmt --check`
10. `cargo clippy --workspace --all-targets -- -D warnings`
11. `python3 .just/run_lint.py boundaries`
12. `python3 .just/run_lint.py nudge-taxonomy`
13. `git diff --check`

## Non-closure

- AZ.3 does not replace the idle scheduler or claim fair message/task
  interleaving; AZ.4 owns that behavior.
- AZ.3 does not remove deprecated legacy task flags.
- AZ.3 does not modify the legacy synchronous daemon, run a live/test daemon, or
  perform a tag, release, package publish, or installation.
