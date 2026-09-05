---
phase: AX
sprint: AX.3
title: Task state machine and completion
branch: feature/ax3-task-state-machine
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/ax3-task-state-machine
integration_branch: integrate/phase-ax
status: draft
recommended_agent: arch-ctm
recommended_model: deep-reasoning
execution_track: B
parallel_with: [AX.1, AX.2]
dependency_relations:
  - prerequisite: AX.1
    dependent: AX.3
    relation: parallel_safe
    rationale: this sprint changes no nudge behaviour and its completion message renders with either six or seven kinds; the overlap is additive edits in different regions of crates/atm-storage/src/contract.rs.
  - prerequisite: AX.2
    dependent: AX.3
    relation: parallel_safe
    rationale: no functional dependency; both add lines to crates/atm-core/src/boundary/mod.rs. Before opening its PR this sprint merges integrate/phase-ax forward (after track A lands) and resolves that overlap; the AX.3 PR merges after the AX.2 PR.
  - prerequisite: AX.3
    dependent: AX.4
    relation: must_follow
    rationale: AX.4's pump step reads the tasks table and TaskStore delivered here.
---

# AX.3 — Task state machine and completion

Persist task state as one explicit, backend-agnostic state machine
applied inside the existing message-write transactions, with an
append-only audit log. This sprint changes **no nudge behaviour**: a
task-tagged send still steers or queues exactly as today (rendering the
AX.1 `Task` body, or today's `DeliveryTask` body if AX.1 has not merged
yet). It adds the ack gate, the completion flag, and the inspection
surfaces. The reminder cycle is AX.4. This sprint **executes in parallel
with AX.1 and AX.2** on its own gh-stack rooted on `integrate/phase-ax`.

## State machine

One row per (`team`, `task_id`, `assignee`). States and events are
closed enums; the transition function is pure and table-tested.

```
        Assigned              Acked                Completed
  ∅ ──────────────► ASSIGNED ──────────► ACTIVE ──────────────► COMPLETE
                       │  ▲ Assigned        │  ▲ Assigned          ▲
                       │  └── (re-send)     │  └── (re-send)       │
                       └────────────── Completed ──────────────────┘
```

| State | Meaning |
| --- | --- |
| `Assigned` | assignment message persisted; assignee has not acked it |
| `Active` | assignee acked the assignment; this is the assignee's current work |
| `Complete` | a `Completed` event was accepted |

Transition table, all twelve (state, event) cells. `Reject` means the
enclosing message write is rolled back and the CLI exits 3
`ATM_MESSAGE_VALIDATION_FAILED` with the `detail` text.

| From \ Event | `Assigned` | `Acked` | `Completed` |
| --- | --- | --- | --- |
| ∅ (no row) | → `Assigned` | Reject `no open task <id> for <assignee>` | Reject `no open task <id> for <actor>` |
| `Assigned` | → `Assigned` (re-send: `assignment_message_id` and `description` updated; event detail `resend`) | → `Active` (admit guard G1) | → `Complete` (assignment message marked acknowledged in the same transaction, no reply message) |
| `Active` | → `Active` (re-send, as above) | → `Active` (no change) | → `Complete` |
| `Complete` | Reject `task <id> already complete; use a new id` | Reject `task <id> already complete` | Reject `task <id> already complete` |

Cross-row admit guards, evaluated before `transition` in the same
transaction:

| Guard | Event | Rule | Rejection detail |
| --- | --- | --- | --- |
| G1 | `Acked` from `Assigned` | assignee has no other `Active` row in the team | `task <other> is active; complete it first` |
| G2 | `Completed` | actor is the row's assignee or assigner (phase plan §2.1 default) | `task <id> is not assigned to or by <actor>` |

A rejected `Acked` leaves the assignment message pending-ack. Completing
from `Assigned` (ack skipped) is allowed; the skipped ack is visible in
the event log and the assignment message is marked acknowledged so it
does not remain pending-ack forever (requirements §15.4 amendment, D8).

Row resolution, done by the writer before `admit`:

| Event | Key |
| --- | --- |
| `Assigned` | (`message.team`, `envelope.task_id`, `message.agent`) — one row per recipient; a fan-out send creates N rows; `assigner` is fixed at first assignment and not changed by a re-send |
| `Acked` | (`source.team`, `source.envelope.task_id`, `source.agent`) — the acked message's recipient is the assignee |
| `Completed` | (`message.team`, `envelope.task_complete`, `envelope.from`) when that row exists (assignee completing); otherwise (`message.team`, `envelope.task_complete`, `message.agent`) when that row exists and its `assigner == envelope.from` (assigner completing the recipient's task); otherwise Reject `no open task <id> for <actor>` |

Idempotency and provenance:

- A transition is applied only when the writer actually inserts the
  message. `save_message_if_absent` returning an existing record (retried
  write) applies nothing.
- Transitions are applied only for locally originated writes. Writes with
  authenticated peer provenance (`has_authenticated_peer_provenance` in
  `crates/atm-core/src/write/pipeline.rs`) never create, change, or
  reject task state; cross-host tasks are out of scope (phase plan §5).
  The gate is passed to the store as `apply_task_transitions: bool` on
  the write op.

Where each event originates:

| Event | Origin | Actor | Message |
| --- | --- | --- | --- |
| `Assigned` | writer persists a message with `task_id` set, `is_ack == false`, `task_complete == None` | envelope `from` | the assignment |
| `Acked` | writer commits `acknowledge_message_atomically` for a source with `task_id`, or `save_messages_atomically` inserts an `is_ack` envelope whose `task_id` is set | envelope `from` of the reply (the assignee) | the ack reply |
| `Completed` | writer persists a message with `task_complete == Some(id)` | envelope `from` | the completion |

The daemon pump (AX.4) is never an event origin.

## Audit

Append-only table `task_events`. Every accepted transition, every
rejection, and (from AX.4 and AX.5) every reminder and lead notification
is one row written in the same transaction as the action it records.
Replaying the `Assigned`/`Acked`/`Completed` rows for one
(team, task_id, assignee) in `seq` order through `transition` must
reproduce that `tasks` row.

## Deliverables

This is the authoritative deliverable checklist. Every listed deliverable
lands production-ready for the scope this sprint claims; partial or
shape-only completion fails the sprint.

- [ ] D1 — retire the unused newtype `pub struct TaskState(String)` in
  `crates/atm-storage/src/contract.rs` and its three re-exports
  (`crates/atm-storage/src/lib.rs`, `crates/atm-core/src/boundary/mod.rs`,
  `crates/atm-core/src/lib.rs`); add task types and the pure state
  machine in `crates/atm-storage/src/task_state.rs` per code contract
  C1, re-exported from `atm_storage::contract` and, for the
  daemon-runtime crates, through `atm_core::boundary` (same pattern as
  `PendingNudgeStore`). All new identifiers avoid the `nudge` word
  family so `scripts/check-nudge-taxonomy.py` needs no allowlist change.
- [ ] D2 — `TaskStore` sealed trait (`crates/atm-storage/src/contract.rs`,
  code contract C2), `DummyTaskStore` double beside
  `DummyPendingNudgeStore`, boundary file
  `boundaries/atm-storage/task-store.toml` per code contract C5, and a
  `## TaskStore` section in `docs/atm-storage/boundaries.md`. ADR-061
  (D7) records `TaskStore` as the seventh optional storage capability
  trait under ADR-018 §3 as re-counted by ADR-054, and ADR-054 gains a
  dated amendment pointing at it.
- [ ] D3 — `MessageEnvelope` gains `task_complete: Option<TaskId>`
  (`crates/atm-storage/src/contract.rs`; `#[serde(default,
  skip_serializing_if = "Option::is_none")]`); `SendRequest` gains
  `task_complete` with builder `with_task_complete` in
  `crates/atm-core/src/send/mod.rs`; the envelope builder in
  `crates/atm-core/src/write/pipeline.rs` (the site that copies
  `request.task_id`, around line 525) copies it; `atm send --json`
  output includes `task_complete`.
- [ ] D4 — rusqlite implementation
  (`crates/atm-storage-rusqlite/src/task_store.rs`, schema in
  `crates/atm-storage-rusqlite/src/shared_db.rs`): tables per code
  contract C3; in `crates/atm-storage-rusqlite/src/writer/ops.rs` the
  message-insert and acknowledgement ops call row resolution, `admit`,
  and `transition` inside their existing transaction whenever
  `apply_task_transitions` is set and an inserted envelope carries
  `task_id` or `task_complete`, write the `task_events` row, and roll
  back the whole op on `Reject`. `SqliteMessageStore` is the only
  `MessageStore` implementation that applies transitions; the trait doc
  states the obligation for production backends and names the exempt
  test doubles (`TestOnlyWriterLane`, `InMemoryAsyncStore`,
  `RecordingWriter`, `DummyStore`, `UnusedMailStore`) and the proof crate
  `atm-storage-sqlserver-proof`.
- [ ] D5 — CLI `atm send --task-complete <TASK_ID>`
  (`crates/atm/src/commands/send.rs`, `conflicts_with = "task_id"`).
  `atm queue` inherits the flag through the flattened `SendCommand` in
  `crates/atm/src/commands/queue.rs`; no separate change. The completion
  message has no `task_id`, so AX.1 kind selection renders the
  Delivery/Queue family and `requires_ack` is not forced.
- [ ] D6 — CLI `atm list --tasks [--member <name>]` and
  `atm list --task-events <TASK_ID> [--member <name>]`
  (`crates/atm/src/commands/list.rs`, which owns both parsing and
  rendering today), `conflicts_with_all` against every mailbox filter
  including the existing `--task <id>`; human and `--json` output per
  code contract C4.
- [ ] D7 — `docs/adr/ADR-061-task-state-machine.md` recording the
  states, events, transition table, guards, row resolution, provenance
  rule, in-transaction application rule, the seventh-capability-trait
  justification, and the phase plan §2.1 defaults; `docs/adr/INDEX.md`
  entry; dated amendment appended to
  `docs/adr/ADR-054-nudge-taxonomy-and-queue-mechanism.md` re-counting the capability traits to seven.
- [ ] D8 — `docs/requirements.md`: §15.4 "Task Metadata Rule" extended
  with the state machine, the ack gate, `--task-complete`, the
  completion-from-Assigned ack rule, and the audit rule; §6.5 (`atm
  send` flags) and §6.6 (`atm list` flags) list the new flags; §7
  (storage) lists `TaskStore` and the two tables. `docs/team-protocol.md`
  completion step names
  `atm send <assigner> --task-complete <id> --stdin`;
  `docs/user-documents/nudge-templates.md` cross-references ADR-061 from
  the `task` kind.
- [ ] D9 — tests listed under Required validation.

### Paths to delete

None (one type and three `pub use` lines removed in place).

## Code contracts

### C1 — types and pure state machine

```rust
// crates/atm-storage/src/task_state.rs
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskState { Assigned, Active, Complete }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskEvent { Assigned, Acked, Completed }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskActor { Member(AgentName), Daemon }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskRow {
    pub team: TeamName,
    pub task_id: TaskId,
    pub assignee: AgentName,
    pub assigner: AgentName,
    pub state: TaskState,
    pub assignment_message_id: AtmMessageId,
    pub description: String,               // snapshot of the assignment description
    pub assigned_at: IsoTimestamp,
    pub updated_at: IsoTimestamp,
    pub last_reminded_at: Option<IsoTimestamp>, // written by AX.4 only
    pub reminder_count: u32,                    // written by AX.4 only
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskRejected { pub detail: String }

/// Row-local machine: the twelve-cell table above. Pure.
pub fn transition(state: Option<TaskState>, event: TaskEvent) -> Result<TaskState, TaskRejected>;

/// Cross-row guards G1 and G2. `open` is every non-Complete row for the
/// assignee. Pure.
pub fn admit(
    row: Option<&TaskRow>,
    open: &[TaskRow],
    event: TaskEvent,
    actor: &AgentName,
) -> Result<(), TaskRejected>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskEventKind { Assigned, Acked, Completed, Rejected, Reminded, LeadNotified }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskEventRow {
    pub team: TeamName,
    pub task_id: TaskId,
    pub assignee: AgentName,
    pub seq: u64,
    pub at: IsoTimestamp,
    pub event: TaskEventKind,
    pub from_state: Option<TaskState>,
    pub to_state: Option<TaskState>, // == from_state for Rejected/Reminded/LeadNotified
    pub actor: TaskActor,
    pub message_id: Option<AtmMessageId>,
    pub detail: Option<String>,      // "resend" on a re-send; rejection text on Rejected
}
```

`Reject` maps to `AtmError` with code `MessageValidationFailed` and the
`detail` text as the message.

### C2 — store contract

```rust
// crates/atm-storage/src/contract.rs
pub trait TaskStore: sealed::Sealed + Send + Sync {
    /// Current row, if any.
    fn load_task(&self, team: &TeamName, task_id: &TaskId, assignee: &AgentName)
        -> Result<Option<TaskRow>, AtmError>;
    /// Non-Complete rows for one member, oldest `assigned_at` first.
    fn open_tasks(&self, member: &MemberKey) -> Result<Vec<TaskRow>, AtmError>;
    /// All rows for a team, optionally one member; newest first.
    fn list_tasks(&self, team: &TeamName, member: Option<&AgentName>)
        -> Result<Vec<TaskRow>, AtmError>;
    /// Audit rows for one task, optionally one assignee, in `seq` order.
    fn list_task_events(&self, team: &TeamName, task_id: &TaskId, assignee: Option<&AgentName>)
        -> Result<Vec<TaskEventRow>, AtmError>;
    /// AX.4: increments `reminder_count`, sets `last_reminded_at`,
    /// appends a `Reminded` row with `detail` = outcome; returns the updated row.
    fn record_reminder(&self, team: &TeamName, task_id: &TaskId, assignee: &AgentName,
        at: IsoTimestamp, outcome: ReminderOutcome) -> Result<TaskRow, AtmError>;
    /// AX.5: appends a `LeadNotified` row.
    fn record_lead_notified(&self, team: &TeamName, task_id: &TaskId, assignee: &AgentName,
        at: IsoTimestamp, lead: &AgentName, message_id: &AtmMessageId) -> Result<(), AtmError>;
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReminderOutcome { Emitted, Unrenderable }
```

State-changing events have **no** trait method: they are applied only by
the message writer inside its transaction (D4). That is the boundary
rule that makes "a rejected ack leaves the message pending-ack" hold
without a compensating write. `record_reminder` and
`record_lead_notified` are implemented in this sprint (so the trait is
complete and the double exists) and first called in AX.4 and AX.5.

### C3 — schema

```sql
CREATE TABLE IF NOT EXISTS tasks (
    team TEXT NOT NULL,
    task_id TEXT NOT NULL,
    assignee TEXT NOT NULL,
    assigner TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('assigned','active','complete')),
    assignment_message_id TEXT NOT NULL,
    description TEXT NOT NULL,
    assigned_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_reminded_at TEXT NULL,
    reminder_count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (team, task_id, assignee)
);
CREATE INDEX IF NOT EXISTS tasks_open_by_member
    ON tasks(team, assignee, assigned_at) WHERE state <> 'complete';

CREATE TABLE IF NOT EXISTS task_events (
    team TEXT NOT NULL,
    task_id TEXT NOT NULL,
    assignee TEXT NOT NULL,
    seq INTEGER NOT NULL,
    at TEXT NOT NULL,
    event TEXT NOT NULL,
    from_state TEXT NULL,
    to_state TEXT NULL,
    actor TEXT NOT NULL,          -- member name or 'atm-daemon'
    message_id TEXT NULL,
    detail TEXT NULL,
    PRIMARY KEY (team, task_id, assignee, seq)
);
```

`seq` is `1 + max(seq)` for the key, assigned inside the writing
transaction. No `UPDATE` or `DELETE` on `task_events` anywhere in the
workspace (grep gate below).

### C4 — CLI output

`atm list --tasks --json`: array of `TaskRow`. `atm list --task-events
<id> --json`: array of `TaskEventRow`. Human output: one line per row,
columns `task_id state assignee assigner assigned_at reminders` and
`seq at event from→to actor detail` respectively.

### C5 — boundary record

```toml
# boundaries/atm-storage/task-store.toml
[boundary]
id = "atm-storage.task-store"
owner_crate = "atm-storage"
path = "crates/atm-storage/src/contract.rs"
symbols = ["TaskStore", "TaskRow", "TaskEventRow", "TaskState", "TaskEvent", "TaskEventKind", "TaskActor", "ReminderOutcome", "DummyTaskStore"]
introduced_by = "AX.3"
adr = "docs/adr/ADR-061-task-state-machine.md"

[io]
owns = ["task_state_transition", "task_event_audit"]
forbidden = ["direct_sqlite_io", "message_delivery", "process_spawn", "nudge_emission"]

[dependencies]
allowed_dependents = ["atm-core", "atm-daemon-bootstrap", "atm-runtime", "atm-storage-rusqlite", "atm-http-runtime"]
forbidden_edges = ["atm-storage -> atm-core"]
forbidden_symbols_outside_owner = ["rusqlite::Connection"]

[contracts]
state_change = "Only the message writer transaction applies Assigned/Acked/Completed; no TaskStore method changes state."
audit = "task_events is append-only; every accepted transition, rejection, reminder, and lead notification is one row in the acting transaction."
replay = "Assigned/Acked/Completed rows per (team, task_id, assignee) replayed through transition reproduce the tasks row."
provenance = "Transitions apply only to writes without authenticated peer provenance."
test_double = "crate::contract::DummyTaskStore"

[lint]
id = "LINT-BOUNDARY-TASK-STORE-REFERENCES"
rule = "atm-http-runtime and atm-daemon-bootstrap reference TaskStore types only via atm_core::boundary."

[status]
state = "planned"
note = "AX.3 introduces; AX.4 and AX.5 consume record_reminder / record_lead_notified."
```

The `[lint]` rule is enforced by an `atm-architecture` test modelled on
`crates/atm-architecture/tests/pending_nudge_store_boundary.rs`.
`atm-http-runtime` already depends on `atm-storage` in `Cargo.toml`, but
`boundaries/atm-http-runtime/http-runtime.toml` does not list it, so the
pump (AX.4) imports through `atm_core::boundary` and the boundary file is
not relaxed.

### Unchanged surfaces

`PendingNudgeStore`; `PostSendHookEvent`; `NudgeKind`; all nudge emission
paths; `atm read` / `atm ack` argument shapes; `DeliveryRecipientSnapshot`.

## Acceptance criteria

1. Two `--task-id` sends to one member create two `Assigned` rows; after
   the first is acked (`Active`), `atm ack` of the second exits 3 naming
   the first; the second message stays pending-ack.
2. `--task-complete` from the assignee moves `Assigned` or `Active` to
   `Complete` (from `Assigned` the assignment message is also
   acknowledged); from the assigner likewise; from anyone else, or for an
   unknown or `Complete` task, exits 3 and writes no completion message.
3. Re-sending an open task id updates `assignment_message_id` and
   `description`, leaves the state unchanged, and appends one `Assigned`
   event row with detail `resend`.
4. A write admitted with authenticated peer provenance carrying a
   `task_id` creates no task row and is never rejected by the task
   machine.
5. `task_events` for each scenario above replays through `transition` to
   the stored `tasks` row.
6. `grep -rn 'UPDATE task_events\|DELETE FROM task_events' crates` is
   empty; `grep -rn 'TaskState(String)\|TaskState(' crates/atm-storage/src/contract.rs`
   is empty; `python scripts/check-nudge-taxonomy.py` passes with an
   unchanged allowlist; `boundary-guard` review of `task-store.toml`
   passes; ADR-061, the ADR-054 amendment, and requirements §6.5/§6.6/§7/§15.4
   merged; `just validate` green.

## Required validation

- `crates/atm-storage/src/task_state.rs` unit tests: every one of the
  twelve (state, event) cells of `transition`; `admit` G1 accept/reject;
  G2 accept for assignee and assigner, reject for a third member.
- `crates/atm-storage-rusqlite/src/task_store.rs` tests (rusqlite
  runtime): assign, ack, complete happy path; rejected ack rolls back the
  reply and leaves the source pending-ack; rejected completion writes no
  message; re-send updates message id and description; complete from
  `Assigned` acknowledges the assignment; `seq` is gapless per key;
  fan-out send creates one row per recipient; replay test (AC 5).
- `crates/atm-core/tests/task_state.rs` (new, `nudge_mode.rs` style):
  end-to-end through `write_mail_with_runtime` and
  `ack_mail_with_runtime` for AC 1–4 on a tmux-backed member and a
  Herdr-backed member; nudge dispatch for a task send is unchanged from
  AX.1 behaviour; peer-provenance case (AC 4).
- `crates/atm-architecture/tests/task_store_boundary.rs`: the C5 lint
  rule; `grep -rn 'atm_storage::' crates/atm-http-runtime/src crates/atm-daemon-bootstrap/src`
  shows no `TaskStore` import outside `atm_core::boundary`.
- CLI tests in `crates/atm/src/commands/send.rs`, `queue.rs`, and
  `list.rs`: `--task-complete` conflicts with `--task-id` on both
  commands; `--tasks` / `--task-events` conflict with `--task` and the
  other mailbox filters; JSON shapes.
- `just validate`; quality-mgr Final Quality Report on the PR;
  `boundary-guard` on the new TOML; `arch-qa` on ADR-061 and the ADR-054
  amendment.

## Out of scope

Idle reminders, lead notification, doctor codes, Herdr steer suppression
for task sends (AX.4, AX.5); cross-host and cross-team tasks; task
priority, reassignment, expiry; `--task-cancel` (phase plan §2.1
alternative).
