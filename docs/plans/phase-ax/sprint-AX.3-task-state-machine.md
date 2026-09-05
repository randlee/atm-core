---
phase: AX
sprint: AX.3
title: Task state machine and storage
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
    rationale: this sprint changes no nudge behaviour; the overlap is additive edits in different regions of crates/atm-storage/src/contract.rs.
  - prerequisite: AX.2
    dependent: AX.3
    relation: parallel_safe
    rationale: no functional dependency; both add lines to crates/atm-core/src/boundary/mod.rs. Before opening its PR this sprint merges integrate/phase-ax forward (after track A lands) and resolves that overlap; the AX.3 PR merges after the AX.2 PR.
  - prerequisite: AX.3
    dependent: AX.4
    relation: must_follow
    rationale: AX.4 adds the CLI flags and user docs over the SendRequest builder, TaskStore, and tables delivered here.
---

# AX.3 — Task state machine and storage

Persist task state as one explicit, backend-agnostic state machine
applied inside the existing message-write transactions, with an
append-only audit log. This sprint changes **no nudge behaviour** and
adds **no CLI flag**: it delivers the types, the store, the writer-side
application, the ack gate, the library-level completion request, the
boundary record, and ADR-061. The CLI and user docs are AX.4; the
reminder cycle is AX.5. This sprint **executes in parallel with AX.1 and
AX.2** on its own gh-stack rooted on `integrate/phase-ax`.

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
enclosing message write is rolled back and the caller receives
`ATM_MESSAGE_VALIDATION_FAILED` (CLI exit 3) with the `detail` text.
`No-op` means the enclosing write proceeds and no task row or event row
is written.

| From \ Event | `Assigned` | `Acked` | `Completed` |
| --- | --- | --- | --- |
| ∅ (no row) | → `Assigned` | **No-op** (the ack proceeds; the machine never blocks an ack it has no row for: pre-upgrade mail, peer-delivered tasks) | Reject `no open task <id> for <actor>` |
| `Assigned` | → `Assigned` (re-send: `assignment_message_id` and `description` updated; event marker `resend`) | → `Active` (admit guard G1) | → `Complete` (assignment message marked acknowledged in the same transaction, code contract C7) |
| `Active` | → `Active` (re-send, as above) | → `Active` (no change, one `Acked` event row) | → `Complete` |
| `Complete` | Reject `task <id> already complete; use a new id` | Reject `task <id> already complete` | Reject `task <id> already complete` |

Cross-row admit guards, evaluated before `transition` in the same
transaction:

| Guard | Event | Rule | Rejection detail |
| --- | --- | --- | --- |
| G1 | `Acked` from `Assigned` | assignee has no other `Active` row in the team | `task <other> is active; complete it first` |
| G2 | `Completed` | actor is the row's assignee or assigner (phase plan §2.1 default) | `task <id> is not assigned to or by <actor>` |

A rejected `Acked` leaves the assignment message pending-ack. Completing
from `Assigned` (ack skipped) is allowed; the skipped ack is visible in
the event log and the assignment message is marked acknowledged (C7) so
it does not remain pending-ack forever.

Row resolution, done by the writer before `admit`:

| Event | Key |
| --- | --- |
| `Assigned` | (`message.team`, `envelope.task_id`, `message.agent`) — one row per recipient; a fan-out send creates N rows; `assigner` is fixed at first assignment and not changed by a re-send |
| `Acked` | (`source.team`, `source.envelope.task_id`, `source.agent`) from the pending-ack source loaded inside the acknowledgement op — the acked message's recipient is the assignee |
| `Completed` | (`message.team`, `envelope.task_complete`, `envelope.from`) when that row exists (assignee completing); otherwise (`message.team`, `envelope.task_complete`, `message.agent`) when that row exists and its `assigner == envelope.from` (assigner completing the recipient's task); otherwise Reject `no open task <id> for <actor>` |

Exactly one application site per event (code contract C6):

| Event | Only application site | Actor |
| --- | --- | --- |
| `Assigned` | rusqlite message-insert ops with `MessageWriteOrigin::Local`, when the record was actually inserted and its envelope has `task_id`, `acknowledges_message_id == None`, `task_complete == None` | envelope `from` |
| `Acked` | rusqlite `WriteOp::Acknowledge` (`execute_acknowledgement`), when the loaded source envelope has `task_id` | envelope `from` of the reply (the assignee) |
| `Completed` | rusqlite message-insert ops with `MessageWriteOrigin::Local`, when the record was actually inserted and its envelope has `task_complete` | envelope `from` |

Idempotency and provenance:

- A transition is applied only when the writer actually inserts the
  record (`inserted == true`). A duplicate-key admission applies nothing.
- Writes with `MessageWriteOrigin::Peer` (authenticated peer receipts,
  decided by `has_authenticated_peer_provenance` in
  `crates/atm-core/src/write/pipeline.rs`) never create, change, or
  reject task state. A peer-delivered ack reply arrives as a `Peer`
  message insert, so it is never an `Acked` origin either. Cross-host
  tasks are out of scope (phase plan §5); their acks succeed by the
  ∅/`Acked` no-op cell.

The daemon pump (AX.5) is never an event origin.

## Audit

Append-only table `task_events`. Every accepted transition, every
rejection, and (from AX.5 and AX.6) every reminder and lead notification
is one row written in the same transaction as the action it records.

Replay claim, stated precisely: for one (team, task_id, assignee), the
`Assigned`/`Acked`/`Completed` rows in `seq` order replayed through
`transition` reproduce `tasks.state`; the `message_id` of the latest
`Assigned` row equals `tasks.assignment_message_id`; the count of
`Reminded` rows equals `tasks.reminder_count`; the latest `Reminded.at`
equals `tasks.last_reminded_at`. `tasks.description` is **not**
replayable (a re-send may change it; the log records only `resend`).
ADR-061 and the boundary record state the same four columns.

## Deliverables

This is the authoritative deliverable checklist. Every listed deliverable
lands production-ready for the scope this sprint claims; partial or
shape-only completion fails the sprint.

- [ ] D1 — retire the unused newtype `pub struct TaskState(String)` in
  `crates/atm-storage/src/contract.rs` and its three re-exports
  (`crates/atm-storage/src/lib.rs`, `crates/atm-core/src/boundary/mod.rs`,
  `crates/atm-core/src/lib.rs`); add task types and the pure state
  machine in `crates/atm-storage/src/task_state.rs` per code contract
  C1; add the `TaskStore` trait, `DummyTaskStore`, `MessageWriteOrigin`
  and `DAEMON_ACTOR_NAME` in new `crates/atm-storage/src/task_store.rs`
  per code contracts C2 and C6 (the `sealed` module in `contract.rs`
  becomes `pub(crate)` so sibling modules can seal). `contract.rs` gains
  only `pub use crate::task_store::*` plus the two defaulted provenance
  methods, so every existing `atm_storage::contract::X` path keeps
  working and the daemon-runtime crates import through
  `atm_core::boundary` (same pattern as `PendingNudgeStore`). All new identifiers avoid the `nudge` word
  family so `scripts/check-nudge-taxonomy.py` needs no allowlist change.
- [ ] D2 — `TaskStore` sealed trait (`crates/atm-storage/src/contract.rs`,
  code contract C2), `DummyTaskStore` double beside
  `DummyPendingNudgeStore`, boundary file
  `boundaries/atm-storage/task-store.toml` and its implementation
  companion `boundaries/atm-storage-rusqlite/task-store-sqlite.toml` per
  code contract C5, and a `## TaskStore` section in
  `docs/atm-storage/boundaries.md`.
- [ ] D3 — wiring, one site per file, mirroring `PendingNudgeStore`:
  `crates/atm-storage/src/factory.rs` (`StorageHandleParts.task_store`
  field at line 50, `StorageHandles::task_store()` accessor beside
  `pending_nudge_store()` at line 133, and the `StorageHandles::from_parts`
  mapping at line 90; the `StorageFactory` trait at line 172 is
  unchanged); `crates/atm-storage-rusqlite/src/lib.rs`
  (`SqliteStorageBackend` field near line 667 with its `task_store()`
  accessor, and the `StorageHandleParts` literal in
  `SqliteStorageFactory::open` near line 746);
  `crates/atm-core/src/service_runtime.rs`
  (`LocalServiceRuntime::with_task_store` builder and `task_store()`
  accessor returning `Result<Arc<dyn TaskStore + Send + Sync>, AtmError>`
  with the same not-installed error shape as `pending_nudge_store()` at
  line 448–470); `crates/atm-runtime/src/composition.rs` (beside line
  189); `crates/atm-http-runtime/src/storage_and_nudge_router.rs`
  (assembly field near line 1401 and installation near line 1515).
  Consumers (AX.4 list command, AX.5 pump, AX.6 doctor) obtain the store
  only through `LocalServiceRuntime::task_store()`.
- [ ] D4 — envelope and request fields. `MessageEnvelope` in
  `crates/atm-storage/src/schema/inbox_message.rs` gains
  `task_complete: Option<TaskId>` with
  `#[serde(rename = "taskComplete", default, skip_serializing_if = "Option::is_none")]`
  on the public struct (line 138), on the private `RawMessageEnvelope`
  mirror (line 193), in the `From<RawMessageEnvelope>` impl (line 244),
  and in the manual `Deserialize` (line 272) so it never lands in
  `extra`. `SendRequest` (`crates/atm-core/src/send/mod.rs`) gains
  `task_complete: Option<TaskId>` with builder `with_task_complete`;
  `build_send_envelope` (send/mod.rs line 520) copies it; both prepare
  paths (`prepare_persisted_write` line 515 and
  `prepare_persisted_write_async` line 586 in
  `crates/atm-core/src/write/pipeline.rs`) and both persistence
  functions (`persist_send_message` in `crates/atm-core/src/send/mod.rs`
  line 461 and `persist_send_message_async` in
  `crates/atm-core/src/send/async_persistence.rs` line 51) carry it. A request
  with both `task_id` and `task_complete` is rejected at validation.
- [ ] D5 — write provenance carrier, code contract C6:
  `MessageWriteOrigin` in `crates/atm-storage/src/contract.rs`; two new
  defaulted methods, one per insert path:
  `MessageStore::save_message_if_absent_with_provenance` (sync trait,
  contract.rs line 612, delegates to `save_message_if_absent`) and
  `AsyncMessageStore::save_message_if_absent_with_provenance_async`
  (`#[async_trait]` trait, line 668, delegates to
  `save_message_if_absent_async`); `TemplateMessageAdmission.provenance` field
  (`crates/atm-storage/src/template_catalog.rs` line 267; the type has
  no constructor, so the two struct-literal sites set it:
  `crates/atm-core/src/send/async_persistence.rs` line 168 with the
  caller's value and the test literal in
  `crates/atm-storage-rusqlite/src/lib.rs` line 2539 with `Local`).
  Both prepare paths compute the value with
  `has_authenticated_peer_provenance` and thread it down: the sync path
  `prepare_persisted_write` → `persist_send_message` (send/mod.rs line
  461) → `mirror_message_to_store` (send/persistence.rs line 204) →
  `RetainedMailboxRuntime::admit_message_record` →
  `save_message_if_absent_with_provenance`; the async path
  `prepare_persisted_write_async` → `persist_send_message_async`
  (async_persistence.rs line 51) → `mirror_message_to_store_async`
  (persistence.rs line 250) →
  `save_message_if_absent_with_provenance_async`. The sync path matters
  because `write/pipeline.rs` lines 433–435 route an authenticated-peer
  ack receipt onto it. Only `SqliteMessageStore`
  (`crates/atm-storage-rusqlite/src/lib.rs` line 625) overrides either
  method. Every other `MessageStore` implementor keeps the sync default
  (`crates/atm-storage/src/contract.rs` `DummyStore`,
  `crates/atm-core/src/doctor/mod.rs` `UnusedMailStore`,
  `crates/atm-storage-sqlserver-proof/src/lib.rs`
  `SqlServerMessageStore`, plus the three below) and every other
  `AsyncMessageStore` implementor keeps the async default
  (`crates/atm-runtime/src/mailbox_runtime.rs` `TestOnlyWriterLane`,
  `crates/atm-runtime-test-support/src/lib.rs` `RecordingWriter`,
  `crates/atm-core/src/ack/admission_tests.rs` `InMemoryAsyncStore`);
  none applies transitions, and both trait docs state this.
- [ ] D6 — rusqlite implementation
  (`crates/atm-storage-rusqlite/src/task_store.rs`, schema in
  `crates/atm-storage-rusqlite/src/shared_db.rs`, application in
  `crates/atm-storage-rusqlite/src/writer/ops.rs`): tables per code
  contract C3; `WriteOp::UpsertMessage` becomes `UpsertMessage { record,
  provenance }`; both construction sites gain a `provenance` parameter
  forwarded from the new store methods: `submit_upsert_message`
  (shared_db.rs line 244, from `save_message_if_absent_with_provenance`)
  and `submit_upsert_message_async` (line 291, from
  `save_message_if_absent_with_provenance_async`); the existing
  un-suffixed store methods call them with `Local`;
  `WriteOp::AdmitTemplateMessage` reads `admission.provenance`;
  `WriteOp::UpsertMessages` and `WriteOp::AdmitDecomposedMessage` stay
  `Local` because their only callers are the local atomic-ack path
  (`persist_message_records_atomically`) and local template
  decomposition, never a peer receipt; `ApplyReadDisplayState`
  and `RegisterTemplate` apply nothing. Application per the
  "one site per event" table: row resolution, `admit`, `transition`, the
  `task_events` row, and rollback on `Reject`, all inside the op's
  existing transaction. `WriteOp::Acknowledge` (`execute_acknowledgement`
  line 413) is the only `Acked` site and uses the loaded source. C7 for
  completion from `Assigned`.
- [ ] D7 — `docs/adr/ADR-061-task-state-machine.md` recording the
  states, events, transition table (including the ∅/`Acked` no-op),
  guards, row resolution, provenance rule and carrier, the one-site
  rule, the precise replay claim, the seventh-capability-trait
  justification under ADR-018 §3 as re-counted by ADR-054, and the phase
  plan §2.1 defaults; `docs/adr/INDEX.md` entry; dated amendment
  appended to `docs/adr/ADR-054-nudge-taxonomy-and-queue-mechanism.md`
  re-counting the capability traits to seven; `docs/requirements.md`
  §22.1 (SQLite mail and roster ownership) gains a "Task storage"
  subsection listing `TaskStore`, the two tables, and
  `MessageWriteOrigin` (§7 is queue inspection and is not touched).
- [ ] D8 — tests listed under Required validation.
- [ ] D10 — supersede the Phase-AC task-storage deferral. `docs/requirements.md`
  lines 52–64 ("Phase-AC supersession note") and `docs/architecture.md`
  lines 2871–2880 ("Task Storage (Deferred)") say a later task store
  "starts from Claude-code task schema plus Pydantic validation". Both
  get a dated Phase-AX amendment directly below the existing text (the
  historical note is kept, marked superseded, not rewritten): task
  storage is approved in Phase AX (Rand, 2026-09-04, phase plan §2); the
  canonical model is ADR-061's daemon-owned, message-derived state
  machine implemented in Rust inside `atm-storage` / `atm-storage-rusqlite`;
  the Claude-code-schema-plus-Pydantic direction is withdrawn because ATM
  tasks are derived from messages the daemon already persists, the write
  path is Rust with no Python in it, and a Claude Code task list is a
  per-session harness artifact rather than a cross-host record. The
  `AC.6` deletion stands: ADR-061 is a fresh design and revives none of
  the deleted scaffolding. Gate: `grep -n Pydantic docs/requirements.md
  docs/architecture.md` returns only lines inside the two superseded
  notes, each followed by the Phase-AX amendment.
- [ ] D9 — `crates/atm-storage/src/contract.rs` decomposition (arch-qa
  RULE-003): the file is already 1133 non-test lines (tests start at
  line 1134) and AX.1, AX.3 and AX.6 all add to it. Extract the graft
  receiver and peer-configuration section (lines 760–1003:
  `GraftReceiverRegistration`, `GraftReceiverLease`,
  `GraftEndpointStoreError`, `GraftReceiverEndpointStore`,
  `CertificateFingerprint`, `PrivateKeyRef`, `HttpsInterface`,
  `LocalCertificate`, `TrustedPeer`, `PeerConfigStore`) into new
  `crates/atm-storage/src/peer_contract.rs` with `pub use` re-exports
  from `contract.rs`, so no import path outside the crate changes and
  no boundary record moves. After this sprint `contract.rs` has at most
  1000 non-test lines (AC 6 gate) with room for AX.6's additions; no
  behaviour change, existing tests untouched.

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
/// The one string for the daemon actor and the reserved sender name; AX.6
/// consumes this constant instead of defining its own.
pub const DAEMON_ACTOR_NAME: &str = "atm-daemon";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskRow {
    pub team: TeamName,
    pub task_id: TaskId,
    pub assignee: AgentName,
    pub assigner: AgentName,
    pub state: TaskState,
    pub assignment_message_id: AtmMessageId,
    pub description: String,               // snapshot of the assignment description; not replayable
    pub assigned_at: IsoTimestamp,
    pub updated_at: IsoTimestamp,
    pub last_reminded_at: Option<IsoTimestamp>, // written by AX.5 only
    pub reminder_count: u32,                    // written by AX.5 only
    pub lead_notified_count: u32,               // written by AX.6 only
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskRejected { pub detail: String }
// Every `detail` ends with the remediation the AX.6 doctor codes also use:
// "Run: atm list --task-events <task_id> --member <assignee>"

/// Outcome of one row-local step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition { To(TaskState), NoOp }

/// Row-local machine: the twelve-cell table above. Pure.
pub fn transition(state: Option<TaskState>, event: TaskEvent) -> Result<Transition, TaskRejected>;

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
    pub outcome: Option<ReminderOutcome>, // Reminded rows only; typed, never free text
    pub marker: Option<TaskEventMarker>,  // Assigned(resend) / Completed(assignment_missing); typed
    pub detail: Option<String>,      // Rejected rows only: the TaskRejected prose; None otherwise
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskEventMarker { Resend, AssignmentMissing }
```

`Reject` maps to `AtmError` with code `MessageValidationFailed` and the
`detail` text as the message. `transition(None, Acked)` returns
`Ok(NoOp)`; every other `Ok` is `To(state)`.

### C2 — store contract

```rust
// crates/atm-storage/src/task_store.rs (new; `pub use`d from contract.rs)
pub trait TaskStore: sealed::Sealed + Send + Sync {
    /// Current row, if any.
    fn load_task(&self, member: &MemberKey, task_id: &TaskId)
        -> Result<Option<TaskRow>, AtmError>;
    /// Non-Complete rows for one member, oldest `assigned_at` first.
    fn open_tasks(&self, member: &MemberKey) -> Result<Vec<TaskRow>, AtmError>;
    /// All rows for a team, optionally one member; newest first.
    fn list_tasks(&self, team: &TeamName, member: Option<&AgentName>)
        -> Result<Vec<TaskRow>, AtmError>;
    /// Audit rows for one task, optionally one assignee, in `seq` order.
    fn list_task_events(&self, team: &TeamName, task_id: &TaskId, assignee: Option<&AgentName>)
        -> Result<Vec<TaskEventRow>, AtmError>;
    /// AX.5: increments `reminder_count`, sets `last_reminded_at`,
    /// appends a `Reminded` row with `outcome` set (typed column); returns the updated row.
    fn record_reminder(&self, member: &MemberKey, task_id: &TaskId,
        at: IsoTimestamp, outcome: ReminderOutcome) -> Result<TaskRow, AtmError>;
    /// AX.6: increments `lead_notified_count` and appends a `LeadNotified` row.
    fn record_lead_notified(&self, member: &MemberKey, task_id: &TaskId,
        at: IsoTimestamp, lead: &AgentName, message_id: &AtmMessageId) -> Result<(), AtmError>;
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReminderOutcome { Emitted, Unrenderable, Blocked }
// Blocked: the assignee was observed at an interactive prompt; no input was
// attempted (Herdr rejects it) but the reminder is counted so the stall is visible.
```

`MemberKey` is (team, agent) as in `PendingNudgeStore`. State-changing
events have **no** trait method: they are applied only by the message
writer inside its transaction (D6). `record_reminder` and
`record_lead_notified` are implemented here and first called in AX.5
and AX.6.

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
    lead_notified_count INTEGER NOT NULL DEFAULT 0,
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
    outcome TEXT NULL CHECK(outcome IN ('emitted', 'unrenderable', 'blocked')),
    marker TEXT NULL CHECK(marker IN ('resend', 'assignment_missing')),
    detail TEXT NULL,             -- Rejected rows only
    PRIMARY KEY (team, task_id, assignee, seq)
);
```

`seq` is `1 + max(seq)` for the key, assigned inside the writing
transaction. No `UPDATE` or `DELETE` on `task_events` anywhere in the
workspace (grep gate below).

### C4 — library completion request

```rust
// crates/atm-core/src/send/mod.rs
impl SendRequest {
    pub fn with_task_complete(mut self, task_id: TaskId) -> Self;   // validation rejects when task_id is also set
}
```

The completion message has no `task_id`, so kind selection renders the
Delivery/Queue family and `requires_ack` is not forced. The CLI flag is
AX.4.

### C5 — boundary record

```toml
# boundaries/atm-storage/task-store.toml  (modelled on pending-nudge-store.toml)
boundary_id = "BOUNDARY-TaskStore"
owner_package = "atm-storage"
owner_crate_path = "atm_storage"
name = "TaskStore"

[public]
trait = "TaskStore"
notes = "storage-neutral task ledger: read rows and events, append reminder and lead-notification audit rows; state transitions are applied only inside the backend's message-writer transaction, never through this trait"

[implementation]
visibility = "trait_only"
constructor = "none"

[composition]
roots = []

[ownership]
io_owns = ["task_event_audit_append", "task_row_read"]
io_forbidden = ["direct_sqlite_io", "message_delivery", "process_spawn", "task_state_transition"]

[dependencies]
allowed_dependents = ["atm", "atm-core", "atm-daemon-bootstrap", "atm-runtime", "atm-storage-rusqlite"]
allowed_dependencies = []
forbidden_edges = ["atm-storage -> atm-core", "atm-storage -> atm-storage-rusqlite", "atm-storage -> atm-daemon", "atm-http-runtime -> atm-storage"]

[references]
scope = "outside_owner_crate"
forbidden = ["rusqlite::Connection"]

[contracts]
request_types = ["MemberKey", "TaskId", "IsoTimestamp", "ReminderOutcome", "MessageWriteOrigin"]
response_types = ["Option<TaskRow>", "Vec<TaskRow>", "Vec<TaskEventRow>"]
error_types = ["AtmError"]
notes = [
  "state change: only the message writer transaction applies Assigned/Acked/Completed; no TaskStore method changes tasks.state",
  "audit: task_events is append-only; every accepted transition, rejection, reminder, and lead notification is one row in the acting transaction",
  "replay: per (team, task_id, assignee) the Assigned/Acked/Completed rows replayed through transition reproduce tasks.state; latest Assigned.message_id == assignment_message_id; count(Reminded) == reminder_count; max(Reminded.at) == last_reminded_at; count(LeadNotified) == lead_notified_count; description is not replayable",
  "provenance: transitions apply only to MessageWriteOrigin::Local inserts and to the acknowledgement op",
  "CLI (atm) reads TaskRow/TaskEventRow through atm_storage::contract, the same path nudge-template-override-store.toml grants it; daemon crates import through atm_core::boundary",
]

[testing]
allowed_test_double_paths = ["crate::contract::DummyTaskStore"]
forbidden_test_bypasses = ["rusqlite::Connection"]

[enforcement]
lint_rules = ["LINT-BOUNDARY-TASK-STORE-REFERENCES"]
review_gates = ["no_cli_sqlite_lookup", "no_backend_to_core_edge_for_task_store", "no_task_state_write_outside_writer_transaction"]

[status]
state = "planned"
notes = ["AX.3 introduces the trait, the rusqlite implementation and the schema in one sprint", "AX.4 (atm list), AX.5 (record_reminder) and AX.6 (record_lead_notified, doctor) consume it"]
```

```toml
# boundaries/atm-storage-rusqlite/task-store-sqlite.toml  (modelled on pending-nudge-store-sqlite.toml)
boundary_id = "BOUNDARY-TaskStore-Sqlite"
owner_package = "atm-storage-rusqlite"
owner_crate_path = "atm_storage_rusqlite"
name = "SqliteTaskStore"

[public]
trait = "TaskStore"

[implementation]
type = "SqliteTaskStore"
module = "atm_storage_rusqlite::task_store"
visibility = "private"
constructor = "pub(crate)"

[composition]
roots = ["atm_storage_rusqlite::SqliteStorageBackend::new"]

[ownership]
io_owns = ["sqlite", "task_event_audit_append", "task_state_transition_in_writer_transaction"]
io_forbidden = ["message_delivery", "process_spawn"]

[dependencies]
allowed_dependents = ["atm-daemon-bootstrap", "atm-runtime-test-support"]
allowed_dependencies = ["atm-storage", "rusqlite"]
forbidden_edges = ["atm-storage-rusqlite -> atm-core", "atm-storage-rusqlite -> atm-runtime"]

[references]
scope = "outside_owner_crate"
forbidden = ["SqliteTaskStore", "rusqlite::Connection"]

[contracts]
request_types = ["MemberKey", "TaskId", "IsoTimestamp", "ReminderOutcome", "MessageWriteOrigin"]
response_types = ["Option<TaskRow>", "Vec<TaskRow>", "Vec<TaskEventRow>"]
error_types = ["AtmError"]
notes = ["transitions are applied by writer/ops.rs inside the UpsertMessage / AdmitTemplateMessage / Acknowledge transaction; task_store.rs itself only reads and appends"]

[testing]
allowed_test_double_paths = []
forbidden_test_bypasses = ["rusqlite::Connection"]

[enforcement]
lint_rules = ["LINT-BOUNDARY-TASK-STORE-SQLITE"]
review_gates = ["private_sqlite_impl", "no_cli_sqlite_lookup"]

[status]
state = "planned"
```

Both lint rules are enforced by an `atm-architecture` test modelled on
`crates/atm-architecture/tests/pending_nudge_store_boundary.rs`.
`atm-http-runtime` has `atm-storage` only under `[dev-dependencies]`
(`crates/atm-http-runtime/Cargo.toml` line 43) and must not gain a
production dependency on it; the pump (AX.5) imports through
`atm_core::boundary` and `boundaries/atm-http-runtime/http-runtime.toml`
is not relaxed. `crates/atm` already depends on `atm-storage`
(`crates/atm/Cargo.toml` line 41) and is listed as an allowed dependent
for the AX.4 list command.

### C6 — write provenance carrier

```rust
// crates/atm-storage/src/task_store.rs (enum); the two trait methods below stay in contract.rs
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum MessageWriteOrigin { #[default] Local, Peer }
// Distinct from `atm_core::provenance::WriteProvenance<'a>` (the write-admission
// field bundle re-exported by atm_core::write and constructed in write/pipeline.rs
// lines 377/412/654), which is unchanged. No module imports both unqualified.

pub trait MessageStore: sealed::Sealed + Send + Sync {           // sync trait, line 612
    /* existing methods unchanged */
    /// Like `save_message_if_absent`, carrying the write's origin so task
    /// transitions apply only to local writes. Default: delegate.
    fn save_message_if_absent_with_provenance(
        &self,
        message: &Message,
        provenance: MessageWriteOrigin,
    ) -> Result<Option<Message>, AtmError> {
        let _ = provenance;
        self.save_message_if_absent(message)
    }
}

#[async_trait::async_trait]
pub trait AsyncMessageStore: MessageStore {                        // line 668
    /* existing methods unchanged */
    async fn save_message_if_absent_with_provenance_async(
        &self,
        message: Message,
        provenance: MessageWriteOrigin,
    ) -> Result<Option<Message>, AtmError> {
        let _ = provenance;
        self.save_message_if_absent_async(message).await
    }
}

// crates/atm-storage/src/template_catalog.rs
pub struct TemplateMessageAdmission {
    /* existing fields */
    pub provenance: MessageWriteOrigin,   // Local unless the caller says Peer
}

// crates/atm-storage-rusqlite/src/writer/ops.rs
pub(crate) enum WriteOp {
    UpsertMessage { record: Box<Message>, provenance: MessageWriteOrigin },
    /* other variants unchanged */
}
```

`acknowledge_message_atomically(_async)` carries no provenance: it is
always a local act.

### C7 — completion from `Assigned`

```rust
// crates/atm-storage-rusqlite/src/writer/ops.rs
/// Extracted from execute_acknowledgement (today's lines 422–425) and used by
/// both the acknowledgement op and the completion path.
fn mark_source_acknowledged(source: &mut Message, now: IsoTimestamp) {
    source.envelope.read = true;
    source.envelope.pending_ack_at = None;
    source.envelope.acknowledged_at = Some(now);
}
```

When `Completed` moves a row from `Assigned`, the completion op loads
the message `assignment_message_id`, applies `mark_source_acknowledged`,
and re-upserts it in the same transaction. Intentionally bypassed:
`load_pending_ack_source`'s successor invariants and reply building,
because the completion message is the reply and no acknowledgement-of-
an-ack is wanted. If the assignment message no longer exists, the
completion still succeeds and the `Completed` event row carries
`marker = Some(TaskEventMarker::AssignmentMissing)` (C1); `detail` stays
`None`, since it is reserved for `TaskRejected`.

### C8 — CLI-visible output of `atm send --json`

The `task_complete` field is serialised on `SendOutcome`
(`crates/atm-core/src/send/outcome.rs`) beside `task_id`, skipped when
`None`.

### Unchanged surfaces

`PendingNudgeStore`; `PostSendHookEvent`; `NudgeKind`; all nudge emission
paths; `atm read` / `atm ack` argument shapes; `DeliveryRecipientSnapshot`;
`MessageStore` methods other than the added default method.

## Acceptance criteria

1. Two `--task-id` sends to one member create two `Assigned` rows; after
   the first is acked (`Active`), an ack of the second is rejected
   naming the first; the second message stays pending-ack; one ack
   writes exactly one `Acked` event row.
2. A completion request (`SendRequest::with_task_complete`) from the
   assignee moves `Assigned` or `Active` to `Complete` (from `Assigned`
   the assignment message reads as acknowledged on `atm read`, `atm
   list`, and the pending-ack count); from the assigner likewise; from
   anyone else, or for an unknown or `Complete` task, is rejected and
   writes no completion message.
3. Re-sending an open task id updates `assignment_message_id` and
   `description`, leaves the state unchanged, and appends one `Assigned`
   event row with marker `resend`.
4. A `Peer` write carrying a `task_id` creates no task row; a local ack
   of that message succeeds and creates no row and no event; a local ack
   of a task message that predates the `tasks` table succeeds the same
   way. `task_complete` and `task_id` reach the store identically on the
   sync and async prepare paths and survive an envelope
   serialise/deserialise round trip under the key `taskComplete`.
5. The replay claim holds for every scenario above (state,
   `assignment_message_id`, `reminder_count`, `last_reminded_at`,
   `lead_notified_count`).
6. Gates: `grep -rn 'UPDATE task_events\|DELETE FROM task_events' crates`
   empty; `grep -rn 'TaskState(' crates/atm-storage/src/contract.rs`
   empty; `grep -rn 'atm_storage::.*Task' crates/atm-http-runtime/src crates/atm-daemon-bootstrap/src`
   empty; `python scripts/check-nudge-taxonomy.py` passes with an
   unchanged allowlist; `boundary-guard` review of `task-store.toml`
   and `task-store-sqlite.toml` passes; ADR-061, the ADR-054 amendment, and requirements §7 merged;
   `just validate` green; contract.rs size gate:
   `awk '/^#\[cfg\(test\)\]/{exit} {n++} END{exit n>1000}' crates/atm-storage/src/contract.rs`
   succeeds (non-test lines ≤ 1000).

## Required validation

- `crates/atm-storage/src/task_state.rs` unit tests: every one of the
  twelve (state, event) cells of `transition` including the `NoOp`
  cell; `admit` G1 accept/reject; G2 accept for assignee and assigner,
  reject for a third member.
- `crates/atm-storage-rusqlite/src/task_store.rs` tests (rusqlite
  runtime): assign, ack, complete happy path; rejected ack rolls back
  the reply and leaves the source pending-ack; rejected completion
  writes no message; re-send updates message id and description;
  complete from `Assigned` acknowledges the assignment via C7 and the
  `assignment_missing` case; `seq` is gapless per key; fan-out send
  creates one row per recipient; `Peer` insert creates nothing; ack with
  no row is a no-op; one ack → one `Acked` row; replay test (AC 5).
- `crates/atm-storage/src/schema/inbox_message.rs` tests: `taskComplete`
  round trip, absent field deserialises as `None`, and the key never
  appears in `extra`.
- `crates/atm-core/tests/task_state.rs` (new, `nudge_mode.rs` style):
  end-to-end through `write_mail_with_runtime` / the async pipeline and
  `ack_mail_with_runtime` for AC 1–4 on a tmux-backed member and a
  Herdr-backed member, on both prepare paths; nudge dispatch for a task
  send is unchanged from AX.1 behaviour.
- `crates/atm-architecture/tests/task_store_boundary.rs`: both C5 lint
  rules (`atm-storage` record and `atm-storage-rusqlite` companion).
- `crates/atm-storage-rusqlite/src/lib.rs` tests: `Peer` through the sync
  `save_message_if_absent_with_provenance` and through the async variant
  each create no task row; the un-suffixed methods behave as `Local`.
- Composition tests: `LocalServiceRuntime::task_store()` returns the
  rusqlite store after `atm-runtime` composition and after
  `storage_and_nudge_router` assembly; the not-installed error shape.
- `just validate`; quality-mgr Final Quality Report on the PR;
  `boundary-guard` on the new TOML; `arch-qa` on ADR-061 and the ADR-054
  amendment.

## Out of scope

CLI flags and user docs (AX.4); idle reminders, lead notification,
doctor codes, Herdr marker exception (AX.5, AX.6); cross-host and
cross-team tasks; task priority, reassignment, expiry; `--task-cancel`
(phase plan §2.1 alternative).
