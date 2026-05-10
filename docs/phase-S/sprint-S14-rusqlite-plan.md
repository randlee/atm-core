# Sprint S.14 Rusqlite Write-Worker Plan

**Branch**: feature/pS-s15-rusqlite-hardening  
**Base**: integrate/phase-S @ 77badd5  
**PR target**: integrate/phase-S  
**Status**: Planning

## Goal

Define the next `atm-rusqlite` hardening step after the current Phase S
transport work. The write path is still built around ad-hoc connection opens
plus one transaction per logical store call. That shape is simple, but it
re-pays connection setup cost on every write, lets concurrent writers contend
at SQLite’s single-writer boundary, and keeps the hot mailbox append path on a
probe-then-upsert sequence that is heavier than the conceptual data model.

This plan uses the Opus architect analysis as input, not as an unreviewed
specification. The accepted design keeps the good core idea, one dedicated
writer plus bounded batching, while correcting the parts that do not match the
current crate contracts.

## Current Write Path Shape

Today `SharedDb`:
- opens a fresh SQLite connection for each production operation
- enforces a handle budget of `4`
- applies WAL / foreign-keys / busy-timeout per connection
- uses `with_transaction(...)` as the common mutation helper

Current high-frequency writes in `crates/atm-rusqlite/src/lib.rs`:

1. `MailStore::upsert_message`
- writes `mail_messages`
- writes `ack_state`
- currently does a `SELECT 1 ...` probe first to synthesize `inserted`

2. `MailStore::upsert_visibility_state`
- writes `mail_visibility_states`
- writes `ack_state`
- fires on read/ack visibility updates

3. `MailStore::record_ingest_replay_state`
- single-row upsert, lower frequency

4. task writes
- `create_task`
- `update_task`
- `attach_message_link`
- `detach_message_link`
- `record_ack_transition`

5. roster writes
- `replace_roster`
- touches both `rosters` and `team_roster`

6. daemon replay writes
- `record_remote_replay_state`
- `delete_remote_replay_state`
- `purge_expired_remote_replay_states`

The hot path is clearly mailbox append plus ack projection, not task or roster
maintenance.

## Assessment Of The Opus Recommendations

### Adopted As-Is

- one dedicated `std::thread` write worker, not a Tokio task
- one long-lived write `Connection` owned by that worker
- one bounded submission channel for backpressure
- one transaction per drained batch
- a no-flag-day migration order starting with `upsert_message` and
  `upsert_visibility_state`
- `writer` remaining `pub(crate)` only
- `1` permanent writer handle plus `3` reader handles inside the existing
  `1..=4` connection budget

### Adopted With Changes

#### 1. `INSERT OR IGNORE` only under an explicit immutability rule

The Opus proposal is directionally correct, but only if the mailbox message
row is treated as append-only after first insert.

S.14 adopts the following invariant:
- `mail_messages` is conceptually immutable after the initial insert
- live ack/read state is owned by `ack_state` and
  `mail_visibility_states`, not by later edits to `mail_messages`
- deleting a message remains legal; mutating message text, legacy id,
  parent linkage, or other payload fields after insert remains prohibited

Under that invariant:
- `mail_messages` may use `INSERT OR IGNORE`
- `ack_state` still uses `INSERT ... ON CONFLICT DO UPDATE`
- the `inserted` bit comes from `rows_changed()`, not a pre-write `SELECT`

If any future caller needs mutable message-row semantics, that must be a new
design change, not an accidental consequence of preserving the current hot-path
SQL shape.

#### 2. The generic `with_transaction(...)` contract is not fully actorized in S.14

The Opus recommendation says “signature unchanged” while also moving writes
through a single dedicated thread. Those two goals are only partially
compatible.

The accepted S.14 position is:
- `SharedDb::with_transaction(...)` keeps its current callable shape for
  remaining cold-path internal callers
- migrated hot-path writes stop depending on arbitrary closure submission and
  instead use typed writer shortcuts
- S.14 does not attempt to transparently ship arbitrary borrowed-transaction
  closures across thread boundaries

That keeps the current internal API stable enough for incremental migration
without forcing a boxed-dyn-closure design into the worker message protocol.

#### 3. Reply channels should stay in `std`, not add a Tokio-shaped oneshot dependency

The current crate has no Tokio dependency and should not gain one just to model
single-response delivery. The plan therefore uses a per-request standard reply
channel:
- `WriterMessage = (WriteOp, ReplyTx)`
- `ReplyTx` is a single-use standard-library sender

The worker owns the write result until it either:
- delivers it successfully, or
- detects the caller dropped the receiver

Dropped reply receivers are not a store failure. The worker should log and
continue rather than surfacing a synthetic error path to a caller that is
already gone.

#### 4. Batch draining is opportunistic, not a CPU spin loop

The accepted drain loop is:
1. block on `recv()` for the first write
2. start a short batch deadline
3. opportunistically `try_recv()` additional queued writes
4. stop when:
   - the queue is empty
   - `BATCH_SIZE_MAX` is reached
   - `BATCH_TIME_BUDGET` expires

The worker must not busy-spin for the full time budget.

#### 5. Per-op isolation is accepted, but SAVEPOINT cost is a real tradeoff

The Opus proposal uses one transaction per batch plus one SAVEPOINT per op so a
single malformed write does not abort the entire batch.

That is acceptable for S.14 because correctness matters more than squeezing the
last bit of write throughput from the first implementation. But the plan treats
SAVEPOINT cost as explicit overhead, not free performance. If later
measurement shows meaningful cost on homogeneous mailbox batches, a narrower
batch specialization can be a follow-up.

#### 6. Known logical/schema violations should be rejected before SQL

The worker must not treat avoidable schema faults as normal hot-path control
flow.

Accepted S.14 rule:
- validate known ATM-owned invariants before executing SQL for an op
- use SQL constraint handling as a backstop, not as the primary branch

Examples:
- message-key validity remains enforced before writer submission
- if the schema allows only one successor per parent, that logical check should
  be made in writer-owned preflight before attempting the `mail_messages`
  insert
- task existence and similar crate-owned invariants should continue to return
  typed validation failures instead of generic SQLite write faults

This does not remove the need for SAVEPOINT isolation. Preflight reduces
avoidable constraint faults, while per-op isolation prevents the remaining bad
row cases from poisoning unrelated writes in the same batch.

### Rejected Or Deferred

#### 1. `submit_record_heartbeat`

The current `atm-rusqlite` crate has no heartbeat write API. Adding a
`submit_record_heartbeat` shortcut would invent new scope rather than migrate
real existing writes. S.14 rejects that item.

#### 2. hard throughput claims as planning facts

“8-15x throughput uplift” and “tail latency from 5s busy_timeout to 2ms”
should be treated as hypotheses, not acceptance criteria. The implementation
sprint may cite those expectations as rationale, but only measured results
should be reported as outcomes.

#### 3. WAL checkpoint tuning in S.14

`wal_autocheckpoint=0` plus periodic passive checkpointing is a plausible
follow-up, but it is not required to land the single-writer design itself.
That remains a Phase S+1 tuning task unless real testing proves it is blocking.

## Writer Design

### `SqliteWriter`

`SqliteWriter` is a crate-private actor that owns:
- one dedicated writer thread
- one long-lived write `Connection`
- one bounded submission channel
- one shutdown/drain contract

It is not a public extension surface and must not be re-exported outside
`atm-rusqlite`.

### Message protocol

New crate-private files:
- `crates/atm-rusqlite/src/writer/mod.rs`
- `crates/atm-rusqlite/src/writer/ops.rs`
- `crates/atm-rusqlite/src/writer/stmt_cache.rs`

Core message shape:
- `WriteOp`
- `WriteOpResult`
- `WriterMessage = (WriteOp, ReplyTx)`

Representative `WriteOp` cases:
- `UpsertMessage`
- `UpsertVisibilityState`
- `RecordIngestReplayState`
- `CreateTask`
- `UpdateTask`
- `AttachMessageLink`
- `DetachMessageLink`
- `RecordAckTransition`
- `ReplaceRoster`
- `RecordRemoteReplayState`
- `DeleteRemoteReplayState`
- `PurgeExpiredRemoteReplayStates`

The worker result enum should stay typed enough to preserve current responses,
for example:
- `UpsertMessage { inserted: bool }`
- `Unit`
- `PurgeExpiredRemoteReplayStates { purged: usize }`

### Thread and transaction model

- one `std::thread` owns the write loop for the full lifetime of `SharedDb`
- the loop keeps one write `Connection` open
- the loop prepares and reuses cached statements on that connection
- one SQLite transaction wraps each drained batch
- one SAVEPOINT isolates each operation within that batch

### Panic and closed-channel behavior

- no `unwrap()`
- per-op execution must convert failures into typed `AtmError`
- if an op panics, the worker catches it, converts it to a typed internal
  store write failure, rolls back that op’s savepoint, and continues draining
- if the submission channel is closed, the worker drains already-queued writes
  before exiting
- if a reply receiver has already been dropped, the worker logs and continues

## SharedDb Integration

### Shared ownership changes

`SharedDb` gains:
- `writer: Arc<SqliteWriter>`

Connection budget changes from:
- `4` ad-hoc handles

to:
- `1` permanent writer handle
- `3` concurrent reader handles

This keeps the crate inside the documented `1..=4` budget while making the
writer’s cost explicit instead of hiding it inside transient connection opens.

### Submission shortcuts

S.14 implementation should add typed submission helpers on `SharedDb` rather
than making every caller construct `WriteOp` directly. The adopted shortcut
set is:
- `submit_upsert_message`
- `submit_upsert_visibility`
- `submit_replace_roster`
- `submit_create_task`
- `submit_update_task`
- `submit_attach_link`
- `submit_detach_link`
- `submit_record_ack_transition`
- `submit_record_ingest_replay`
- `submit_record_remote_replay`
- `submit_delete_remote_replay`
- `submit_purge_expired_remote_replay`

Not adopted:
- `submit_record_heartbeat`

### In-memory test mode

The current in-memory mode keeps one retained connection alive so the shared
cache URI remains durable for the life of the test assembly.

Under the writer design:
- the permanent writer connection becomes that retained anchor
- reader paths may still open additional shared-cache connections for tests
- no special in-memory-only public API is required

## SQL Plan

### Mailbox append hot path

Current shape:
- open connection
- `SELECT` probe
- `mail_messages` upsert
- `ack_state` upsert
- commit

Planned shape:
- writer receives `UpsertMessage`
- writer runs preflight validation for known logical invariants that can be
  checked before SQL
- `INSERT OR IGNORE` into `mail_messages`
- detect insertion by affected row count
- `INSERT ... ON CONFLICT DO UPDATE` into `ack_state`
- commit once per batch

Important invariant:
- `mail_messages.envelope_json` is not the live source of truth for later ack
  transitions
- `ack_state` remains canonical for post-insert ack updates

### Visibility hot path

`UpsertVisibilityState` remains:
- `mail_visibility_states` upsert
- `ack_state` upsert

This is still a write-worker win because it avoids writer contention and
amortizes commit cost even though the SQL shape itself stays as an upsert pair.

### Task and roster writes

Task and roster writes are lower frequency and more structurally mutable. They
should migrate after the mailbox path, but they still benefit from:
- serialized writer ownership
- fewer transient connections
- batched commit amortization under bursts

## Migration Order

No flag-day cutover:

1. `MailStore::upsert_message`
2. `MailStore::upsert_visibility_state`
3. `MailStore::record_ingest_replay_state`
4. task-store writes
5. roster-store writes
6. daemon replay helpers in `SqliteBoundaryAssembly`

That order follows actual write frequency and risk:
- first migrate the append-heavy mailbox path
- then the lower-frequency single-row upserts
- finally the more stateful task and roster updates

## Error Contract

The writer must preserve the current crate error posture:
- no raw `rusqlite` error leakage across store boundaries
- SQLite failures still map through the existing mailbox/store error vocabulary
- worker availability failures must become typed store-write failures, not
  silent drops

The plan does not adopt “closed oneshot -> `AtmError::daemon_unavailable`” as
the primary contract. That is too daemon-specific for a crate that is supposed
to translate store failures. A dead writer thread or closed submission queue is
better modeled as a typed durable-store unavailability/write failure.

## Tests And Validation

Required regression coverage for the implementation sprint:
- single-writer correctness under concurrent callers
- hot-path inserted/not-inserted behavior without a `SELECT` probe
- a single row failure inside one drained batch does not cause unrelated queued
  rows to fail
- known invariant violations are rejected in preflight before SQL is attempted
- batch drain executes multiple queued writes under one worker transaction
- closed submission channel drains pending work before shutdown
- in-memory shared-cache tests still preserve reopen semantics
- on-disk temp-db tests still cover migration/bootstrap and reopen behavior

Required validation:
- `cargo fmt --all --check`
- `cargo test -p atm-rusqlite`
- `cargo xwin check --workspace --target x86_64-pc-windows-msvc`
- `just lint`

## Risks And Follow-Ups

### Accepted S.14 risks

- per-op SAVEPOINT overhead may reduce the theoretical top-end write gain
- batching introduces bounded write queueing latency
- permanent writer ownership makes worker health a more visible runtime
  dependency

### Follow-up, not blocking S.14

- WAL autocheckpoint tuning and periodic passive checkpoint policy
- measured throughput and tail-latency benchmarking
- any future optimization that specializes homogeneous mailbox append batches
- any future need to re-evaluate immutable-message assumptions

## ADR Impact

S.14 should add `docs/adr/ADR-ATM-RUSQLITE-002.md`:
- title: Single In-Process SQLite Write Worker
- rationale: SQLite WAL allows concurrent readers but still serializes writers;
  making that serialization explicit at the app layer reduces self-contention
  and lets the hot path batch commits without widening public store contracts
