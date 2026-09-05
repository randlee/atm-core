---
phase: AX
sprint: AX.4a
title: Task state machine and completion
branch: feature/ax4a-task-state-machine
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/ax4a-task-state-machine
integration_branch: integrate/phase-ax
status: draft
recommended_agent: arch-ctm
recommended_model: deep-reasoning
dependency_relations:
  - related: AX.1
    relation: must_follow
    rationale: both edit crates/atm-storage/src/contract.rs and docs/user-documents; the completion message relies on the AX.1 kind mapping (no task_id => Delivery/Queue family).
  - related: AX.2
    relation: parallel_safe
    rationale: AX.2 owns hook.rs, HerdrNudgeTarget, atm-herdr, the bootstrap selector, and herdr_queue_wake.rs; this sprint owns the write pipeline, ack path, task storage, and CLI flags. No shared files, contracts, or artifacts.
  - related: AX.4b
    relation: must_follow
    rationale: AX.4b's pump step reads the tasks table and TaskStore delivered here.
---

# AX.4a — Task state machine and completion

Persist task state as one explicit, backend-agnostic state machine
applied inside the existing message-write transactions, with an
append-only audit log. This sprint changes **no nudge behaviour**: a
task-tagged send still steers or queues exactly as today (rendering the
AX.1 `Task` body). It adds the ack gate, the completion command, and the
inspection surfaces. The nudge cycle is AX.4b.

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
| ∅ (no row) | → `Assigned` | Reject `no open task <id> for <assignee>` | Reject `no open task <id> for <assignee>` |
| `Assigned` | → `Assigned` (re-send: `assignment_message_id` updated) | → `Active` (admit guard G1) | → `Complete` |
| `Active` | → `Active` (re-send: `assignment_message_id` updated) | → `Active` (no change) | → `Complete` |
| `Complete` | Reject `task <id> already complete; use a new id` | Reject `task <id> already complete` | Reject `task <id> already complete` |

Cross-row admit guards, evaluated before `transition` in the same
transaction:

| Guard | Event | Rule | Rejection detail |
| --- | --- | --- | --- |
| G1 | `Acked` from `Assigned` | assignee has no other `Active` row in the team | `task <other> is active; complete it first` |
| G2 | `Completed` | actor is the row's assignee or assigner (phase plan §2.1 default) | `task <id> is not assigned to or by <actor>` |

A rejected `Acked` leaves the assignment message pending-ack. Completing
from `Assigned` (ack skipped) is allowed; the skipped ack is visible in
the event log.

Row resolution, done by the writer before `admit`:

| Event | Key |
| --- | --- |
| `Assigned` | (`message.team`, `envelope.task_id`, `message.agent`); `assigner` is fixed at first assignment and not changed by a re-send |
| `Acked` | (`source.team`, `source.envelope.task_id`, `source.agent`) |
| `Completed` | (`message.team`, `envelope.task_complete`, actor) when that row exists; otherwise the single open row with that team and task id whose `assigner == actor`; zero or more than one candidate is Reject `no open task <id> for <actor>` |

Idempotency: a transition is applied only when the writer actually
inserts the message. `save_message_if_absent` returning an existing
record (cross-host duplicate, retried write) applies nothing.

Where each event originates:

| Event | Origin | Actor | Message |
| --- | --- | --- | --- |
| `Assigned` | writer persists a message with `task_id` set, `is_ack == false`, `task_complete == None` | envelope `from` | the assignment |
| `Acked` | writer commits `acknowledge_message_atomically` for a source with `task_id`, or `save_messages_atomically` receives an `is_ack` envelope with `task_id` | envelope `from` of the reply (the assignee) | the ack reply |
| `Completed` | writer persists a message with `task_complete == Some(id)` | envelope `from` | the completion |

The daemon pump (AX.4b) is never an event origin.

## Audit

Append-only table `task_events`. Every accepted transition, every
rejection, and (from AX.4b) every nudge and lead notification is one row
written in the same transaction as the action it records. Replaying the
`Assigned`/`Acked`/`Completed` rows in `seq` order through `transition`
must reproduce the `tasks` row.

## Deliverables

This is the authoritative deliverable checklist. Every listed deliverable
lands production-ready for the scope this sprint claims; partial or
shape-only completion fails the sprint.

- [ ] D1 — task types and pure state machine in `atm-storage`
  (`crates/atm-storage/src/task_state.rs`, re-exported from `contract`):
  code contract C1. `transition` and `admit` are pure functions with no
  storage access.
- [ ] D2 — `TaskStore` sealed trait (`crates/atm-storage/src/contract.rs`,
  code contract C2) and boundary file
  `boundaries/atm-storage/task-store.toml` modelled on
  `pending-nudge-store.toml` (`io_owns = ["task_state_transition",
  "task_event_audit"]`, `io_forbidden = ["direct_sqlite_io",
  "message_delivery", "process_spawn"]`, `forbidden = ["rusqlite::Connection"]`
  outside the owner crate).
- [ ] D3 — `MessageEnvelope` gains `task_complete: Option<TaskId>`
  (`crates/atm-storage/src/contract.rs`, serde default, skipped when
  `None`); `SendRequest`/`WriteRequest` in `crates/atm-core/src/send/mod.rs`
  carry it; JSON output of `atm send` includes `task_complete`.
- [ ] D4 — rusqlite implementation
  (`crates/atm-storage-rusqlite/src/task_store.rs`, schema in
  `crates/atm-storage-rusqlite/src/shared_db.rs`): tables per code
  contract C3; the writer ops `UpsertMessage`, `UpsertMessages`, and the
  acknowledgement op call row resolution, `admit`, and `transition`
  inside their existing transaction whenever an inserted envelope carries
  `task_id` or `task_complete`, write the `task_events` row, and roll
  back the whole op on `Reject`. The `MessageStore` trait doc states this
  obligation for every backend; `atm-storage-sqlserver-proof` is exempt
  as a proof crate.
- [ ] D5 — CLI `atm send --task-complete <TASK_ID>` (`crates/atm/src/commands/send.rs`,
  conflicts with `--task-id`); `atm queue` gains the same flag. The
  completion message has no `task_id`, so AX.1 kind selection renders the
  Delivery/Queue family and `requires_ack` is not forced.
- [ ] D6 — CLI `atm list --tasks [--member <name>]` and
  `atm list --task-events <TASK_ID>` (`crates/atm/src/commands/list.rs`),
  human and `--json` output per code contract C4.
- [ ] D7 — `docs/adr/ADR-061-task-state-machine.md` recording the states,
  events, transition table, guards, in-transaction application rule, and
  the phase plan §2.1 defaults; `docs/adr/INDEX.md` entry.
- [ ] D8 — `docs/requirements.md` §15.4 "Task Metadata Rule" extended with
  the state machine, the ack gate, `--task-complete`, and the audit rule;
  §6.2/§6.6 list `--task-complete` and the `task_complete` output field;
  `docs/team-protocol.md` completion step names
  `atm send <assigner> --task-complete <id> "task complete: <summary>"`;
  `docs/user-documents/nudge-templates.md` cross-references ADR-061 from
  the `task` kind.
- [ ] D9 — tests listed under Required validation.

### Paths to delete

None.

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
    pub assigned_at: IsoTimestamp,
    pub updated_at: IsoTimestamp,
    pub last_task_nudge_at: Option<IsoTimestamp>, // written by AX.4b only
    pub task_nudge_count: u32,                     // written by AX.4b only
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
pub enum TaskEventKind { Assigned, Acked, Completed, Rejected, Nudged, LeadNotified }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskEventRow {
    pub team: TeamName,
    pub task_id: TaskId,
    pub assignee: AgentName,
    pub seq: u64,
    pub at: IsoTimestamp,
    pub event: TaskEventKind,
    pub from_state: Option<TaskState>,
    pub to_state: Option<TaskState>, // == from_state for Rejected/Nudged/LeadNotified
    pub actor: TaskActor,
    pub message_id: Option<AtmMessageId>,
    pub detail: Option<String>,
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
    /// Audit rows for one task in `seq` order.
    fn list_task_events(&self, team: &TeamName, task_id: &TaskId)
        -> Result<Vec<TaskEventRow>, AtmError>;
    /// AX.4b: increments `task_nudge_count`, sets `last_task_nudge_at`,
    /// appends a `Nudged` row; returns the updated row.
    fn record_nudge(&self, team: &TeamName, task_id: &TaskId, assignee: &AgentName,
        at: IsoTimestamp, message_id: &AtmMessageId) -> Result<TaskRow, AtmError>;
    /// AX.4b: appends a `LeadNotified` row.
    fn record_lead_notified(&self, team: &TeamName, task_id: &TaskId, assignee: &AgentName,
        at: IsoTimestamp, lead: &AgentName, message_id: &AtmMessageId) -> Result<(), AtmError>;
}
```

State-changing events have **no** trait method: they are applied only by
the message writer inside its transaction (D4). That is the boundary
rule that makes "a rejected ack leaves the message pending-ack" hold
without a compensating write. A `DummyTaskStore` test double lives in
`atm_storage::contract` beside `DummyPendingNudgeStore`.

### C3 — schema

```sql
CREATE TABLE IF NOT EXISTS tasks (
    team TEXT NOT NULL,
    task_id TEXT NOT NULL,
    assignee TEXT NOT NULL,
    assigner TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('assigned','active','complete')),
    assignment_message_id TEXT NOT NULL,
    assigned_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_task_nudge_at TEXT NULL,
    task_nudge_count INTEGER NOT NULL DEFAULT 0,
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
columns `task_id state assignee assigner assigned_at nudges` and
`seq at event from→to actor detail` respectively.

### Unchanged surfaces

`PendingNudgeStore`; `PostSendHookEvent`; `NudgeKind`; all nudge emission
paths; `atm read` / `atm ack` argument shapes.

## Acceptance criteria

1. Two `--task-id` sends to one member create two `Assigned` rows;
   `atm ack` of the second exits 3 naming the first once the first is
   `Active`; the second message stays pending-ack.
2. `--task-complete` from the assignee moves `Assigned` or `Active` to
   `Complete`; from the assigner likewise; from anyone else, or for an
   unknown or `Complete` task, exits 3 and writes no completion message.
3. Re-sending an open task id updates `assignment_message_id`, leaves
   the state unchanged, and appends one `Assigned` event row.
4. `task_events` for each scenario above replays through `transition` to
   the stored `tasks` row.
5. `grep -rn 'UPDATE task_events\|DELETE FROM task_events' crates` is
   empty; `boundary-guard` review of `task-store.toml` passes; ADR-061
   and requirements §15.4 merged; `just validate` green.

## Required validation

- `crates/atm-storage/src/task_state.rs` unit tests: every one of the
  twelve (state, event) cells of `transition`; `admit` G1 accept/reject;
  G2 accept for assignee and assigner, reject for a third member.
- `crates/atm-storage-rusqlite/src/task_store.rs` tests: assign, ack,
  complete happy path; rejected ack rolls back the reply and leaves the
  source pending-ack; rejected completion writes no message; re-send
  updates the message id; `seq` is gapless; replay test (AC 4).
- `crates/atm-core/tests/task_state.rs` (new, `nudge_mode.rs` style):
  end-to-end through `write_mail_with_runtime` and
  `ack_mail_with_runtime` for AC 1–3 on a tmux-backed member and a
  Herdr-backed member; nudge dispatch for a task send is unchanged from
  AX.1 behaviour.
- CLI tests in `crates/atm/src/commands/send.rs` and `list.rs`:
  `--task-complete` conflicts with `--task-id`; `--tasks` and
  `--task-events` JSON shapes.
- `just validate`; quality-mgr Final Quality Report on the PR;
  `boundary-guard` on the new TOML; `arch-qa` on ADR-061.

## Out of scope

Idle re-nudge, lead notification, doctor codes, Herdr steer suppression
for task sends (all AX.4b); cross-team tasks; task priority,
reassignment, expiry; `--task-cancel` (phase plan §2.1 alternative).
