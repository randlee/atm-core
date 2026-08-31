---
title: "Phase AV — Async mailbox-read cutover completion, hardening, and read benchmarks"
phase: AV
branch: plan/phase-av
dev_branch: fix/mailbox-read-blocking-serialization (provisioned by team-lead, held clean off develop)
status: draft-ready-for-qa
owner: fenix (plan author); arch-ctm (investigations I-1..I-5, implementation on approval)
base_revision: 938767c72 (develop)
dependency_relations:
  - prerequisite: AV.1
    dependent: AV.2
    relation: parallel_safe
  - prerequisite: AV.1
    dependent: AV.3
    relation: must_follow
  - prerequisite: AV.1
    dependent: AV.4
    relation: must_follow
---

# Phase AV — Async mailbox-read cutover completion, hardening, and read benchmarks

> Evidence base: arch-ctm's read-only investigation findings I-1..I-5
> (task INVESTIGATE-PHASE-AV-MAILBOX-READ, msg 01M1AJVGB9V5WXBGS2SF03KTS8,
> 2026-08-31), verified against develop @ 938767c72.

## 1. Problem — a cutover regression, not new architecture

`atm read --team atm-dev` intermittently fails after the same-host 3.25 s
client budget while the daemon completes the request 6–7 s later
(arch-ctm RCA, msg 01M1AJBANH1C43NECYF8VNKV68, reproduced locally).

Root cause: `StorageAndNudgeRouter` funnels **every** core job through
one `BlockingCoreBridge` holding **exactly one semaphore permit**
(definition `crates/atm-http-runtime/src/storage_and_nudge_router.rs:70-136`;
single-permit construction :171-181; `spawn_blocking` execution :83-121).
The bridge waits for the permit up to the request deadline
(`RequestDeadline`), then runs the job **non-cancellable** to completion —
elapsed time is observability, not enforcement. One slow job therefore
head-of-line blocks the entire read family past every other caller's
deadline; concrete interleaving: a deferred marker / doctor / clear starts
first, and an unrelated `read` waits behind it until its own 3 s request
budget expires with no DB contention of its own.

Complete bridge-client inventory (I-1), classified:

| Call site (`storage_and_nudge_router.rs`) | Operation | Class |
|---|---|---|
| :305-315 | deferred nudge marker (`PreparedWrite::mark_pending_if_deferred`) | post-write mutation/housekeeping |
| :493-511 | list (`list_mail_with_runtime`) | read |
| :514-533 | peek (`peek_mail_with_runtime`) | read |
| :536-555 | receive/read (`read_mail_with_runtime_impl`) | read **+ hidden state mutation** |
| :558-576 | clear (`clear_mail_with_runtime`) | mutation |
| :579-637 | doctor (`run_doctor_with_runtime[_ports]`) | control-plane read |
| :648-674 | heartbeat | roster-validation read |
| :677-700 | queue-get-next | roster read + in-memory FIFO drain |
| :703-811 | graft register/refresh/unregister/lookup | mutations + one read |

The hidden mutation in the read flow: `read_mail_with_runtime_impl` →
`resolve_read_display` (`crates/atm-core/src/read/mod.rs:188-209`) →
`apply_display_mutations_to_store` (:354-365) plus an optional
seen-watermark file write (:211-225). These belong on the ordered writer
lane after read-only selection.

### 1.1 Git archaeology — how the regression happened

- **AL3 (`bd7a45130`, 2026-08-06):** the single permit is born as
  `WriteAdmission::new(NonZeroUsize::new(1))`, expect-message
  **"one SQLite writer"** — a deliberate, correct bound *on the write
  path only*.
- **AL13-G7 (`1142c0ffe`, 2026-08-08):** `WriteAdmission` is renamed
  `BlockingCoreBridge` and split from a new `StorageWriterIngress`. The
  commit's own doc comment says the split exists to stop "read, doctor,
  and heartbeat bridging" from redefining "the storage writer's batching
  capacity" — yet the read-side bridge was instantiated with the same
  capacity 1, and the expect-message was reworded to "one non-storage
  core bridge operation". The writer's bound was rationalized into a
  read-path bound instead of being replaced with a read concurrency
  model.
- The legacy sync daemon was thread-per-request: each request ran its
  sync core call on its own thread with its own storage handle, so reads
  **were** naturally concurrent WAL readers. The Tokio cutover kept the
  sync calls, bridged them, and made the replacement *less* concurrent on
  reads than the daemon it replaced. Phase AV completes the cutover the
  AL phase left unfinished.

Second serialization layer: the nominally-async read path is a write
queue in disguise — `WriteOp::ListMessages`
(`crates/atm-storage-rusqlite/src/writer/ops.rs:37,106`) is submitted to
the single SQLite writer thread by `SharedDb::submit_list_messages_async`
(`shared_db.rs:482-501`), which `AsyncMessageStore::list_messages_async`
delegates to (`lib.rs:612-615`). Reads queue behind writes by
construction. Separately, the existing `SearchReader` is a single worker
(`search_reader.rs:40-75`) — its bounded mpsc/oneshot/deadline shape is
the right pattern, but one worker cannot serve mailbox fan-out.

### 1.2 Design intent this violates (Rand, 2026-08-30)

The message schema was specifically designed with an **immutable primary
message** and **race-tolerant state**: a read racing a state change may
return either value ("don't care"). Therefore reads require **zero**
coordination with the writer lane — no read-your-writes, no freshness
guarantee, no snapshot pinning, no fencing. SQLite WAL natively supports
N concurrent readers beside one writer; the runtime serializes what both
the schema contract and the storage engine explicitly permit. The reader
pool is pure resource management (bound + deadline), never consistency
machinery. Any design or QA argument that mailbox reads must be ordered
through or fenced against the writer contradicts stated schema intent.

## 2. Acceptance contract (Rand, binding for the phase)

1. Axum handlers use async mailbox APIs end-to-end; no
   `spawn_blocking(read/list/peek/...)` and no mailbox read enters a
   blocking bridge.
2. Multiple `read`, `peek`, `list`, and `doctor` requests are serviced
   concurrently across large team/agent fan-out; a slow operation for
   one mailbox/team cannot delay another.
3. A bounded multi-reader async mailbox-query capability: separate
   reader connections/lanes with an explicit concurrency bound and
   deadline/backpressure — not a single writer queue hidden behind an
   async signature (the existing search reader's single thread is also
   insufficient for fan-out).
4. Only narrowly scoped state mutations ride the ordered async writer
   lane, after parallel selection/query work.
5. Doctor is independently async/schedulable; it never occupies the
   mailbox-query capacity.
6. Regression proof: many concurrent cross-team read/peek/list/doctor
   calls while a deliberately stalled housekeeping operation and
   unrelated writer activity run — all independent reads complete within
   budget; bounded overload fails explicitly rather than serializing
   indefinitely.

Additional phase mandates (Rand, 2026-08-30/31):

7. **Hardening:** architecture/requirements amended so the current mode
   of operation is *mechanically non-compliant* — hard gates, not review
   vigilance (§ AV.2/AV.3).
8. **Read + query benchmarks:** benchmark targets proving reads and
   queries execute in a massively parallel manner, with ratcheted floors
   (§ AV.4).

## 3. Sprints

### AV.1 — Reader-lane implementation (cutover completion)

Implement the bounded multi-reader capability on
`fix/mailbox-read-blocking-serialization`:

- **Reader pool** (I-2): bounded pool of N independent read-only worker
  connections in `atm-storage-rusqlite`, generalizing `SearchReader`'s
  bounded mpsc/oneshot/deadline worker shape (`search_reader.rs:40-75`)
  to N workers. Connection substrate is the existing secure RO
  precedent — analyst `open_defensive_connection`
  (`analyst_query.rs:206-221`): `SQLITE_OPEN_READ_ONLY | NO_MUTEX` with
  `query_only=ON; trusted_schema=OFF; defensive=ON`. WAL is already
  established by writer startup (`shared_db.rs:645-650`, setup
  :594-623), so RO readers get native WAL concurrency for free.
- **New async mailbox-read capability** (I-4): a separately named async
  read trait/handle added to `atm-storage` (`contract.rs` — today's
  `AsyncMessageStore` is write-oriented, :573-617), threaded through
  `StorageHandles` (`factory.rs:12-35,89-92`), `LocalServiceRuntime`
  (`service_runtime.rs:145-166,325-344`), and composition
  (`atm-runtime/src/composition.rs:153-170`). Implemented only by
  `atm-storage-rusqlite`; no rusqlite types leak into
  `atm-http-runtime`. Surface: metadata projection (list/peek) +
  record-body load. Per ADR-036 storage topology.
- **Read-family handlers off the bridge** (I-1): list (:493-511), peek
  (:514-533), read (:536-555), and doctor (:579-637) in
  `storage_and_nudge_router.rs` move onto the reader lane end-to-end.
  Sync fresh-connection read paths (`service_runtime_store.rs:170-191`
  `list_messages`, :201-213 `load_message`) are subsumed.
- **Hidden mutation split** (I-1): the read flow's
  `apply_display_mutations_to_store` (`read/mod.rs:354-365`) and
  seen-watermark write (:211-225) become explicit state-transition
  operations on the writer capability, enqueued after the read-only
  selection returns (acceptable-race per §1.2). `persist_message_state`
  (`service_runtime_store.rs:273-295`) stays writer-lane.
- **Doctor decomposition** (I-3): core doctor projection
  (`doctor/mod.rs:130-170,173-230`; roster :368-385,653-662; peer-config
  reads :269-273) becomes an async, independently bounded control-plane
  composition; the router's runtime-health supplement
  (`storage_and_nudge_router.rs:612+`) and the async Herdr-presence leg
  (:623-637) remain separately timed. Doctor acquires neither mailbox
  reader permits nor the writer lane.
- **Deadline enforcement**: read jobs are cancellable at the request
  deadline — reads are abandonable; only durable writes retain
  run-to-completion semantics.
- **Writer purity** (I-2): `WriteOp::ListMessages`
  (`writer/ops.rs:37,106`), `submit_list_messages_async`
  (`shared_db.rs:482-501`), and the writer-routed
  `list_messages_async` delegation (`lib.rs:612-615`) are removed, not
  preserved — the writer lane carries only ordered mutations.
- **Metrics seams for AV.4** (I-2): reader-lane queue depth /
  saturation, in-flight count, wait vs. execution duration,
  deadline-expiry count, and pool size — without these, AV.4's
  concurrency and latency-under-write-storm floors cannot diagnose a
  regression.

**Acceptance:** contract points 1–6, including the deterministic
stalled-housekeeping + read-storm liveness proof and the explicit
bounded-overload failure proof, running in the standard `just test`
gate (test home per I-5: the real-HTTP router fixture at
`storage_and_nudge_router.rs:1053+`, fixture :1300-1450).

### AV.2 — Requirements + ADR hardening

- Amend `docs/requirements.md`: read-family operations MUST be
  concurrent; MUST NOT share a concurrency bound with, or be ordered
  behind, any write/housekeeping lane; state-read races are defined
  "don't care" (schema contract codified so it cannot be re-fenced
  "for safety" later).
- New ADR recording the reader/writer lane architecture, the deadline
  semantics split (reads cancellable, writes run-to-completion), and the
  AL13-G7 regression as motivating history.
- Phase-AM deletion ledger gains the sync read-bridge remnants.

### AV.3 — Mechanical hard gates

Strongest first; grounded on the I-5 enforcement-seam inventory:

1. **Uncompilable, not linted:** delete `BlockingCoreBridge` at cutover
   completion — a future "quick bridge" fails the build. I-5 found no
   existing boundary TOML governing the handler→writer edge; the
   architecture guard (item 2) is the primary mechanism, with a narrow
   TOML rule added only if sc-lint-boundary supports semantic call-edge
   policy. Remnants go on the Phase-AM deletion ledger (AV.2).
2. **Architecture guard:** extend the existing http-runtime scan in
   `crates/atm-architecture/tests/boundary_enforcement.rs:3389-3431`
   with a read-family rule: the handler region
   (`storage_and_nudge_router.rs:493-637`) must not reference
   `BlockingCoreBridge`, `spawn_blocking`, sync `*_with_runtime`
   read/list/doctor APIs, `MessageStore::list_messages`, or writer
   ingress. The existing direct-SQLite prohibition stays.
3. **WriteOp purity gate:** a small `.just` source deny-list checker
   (alongside the existing Python checks, `justfile:112+` / `.just/`)
   asserting `WriteOp` has no pure-read variant and the read-handler
   file has no bridge/spawn-blocking strings; a Rust architecture test
   covers the semantic call paths.
4. **Liveness test as permanent CI** (delivered in AV.1, owned as a gate
   here): in the router fixture (`storage_and_nudge_router.rs:1053+`),
   stall a housekeeping/mutation test seam, fire 10× concurrent
   list/peek/read/doctor across distinct teams, assert each returns
   within its request budget with writer-state commit separately
   awaited; plus the explicit bounded-overload failure case. Runs in
   standard `just test`.

### AV.4 — Read and query benchmarks (massively parallel proof)

New benchmark family beside send-message-benchmark:

- **Read targets:** concurrent `read`/`peek`/`list` against a seeded
  mailbox corpus at high reader counts; p50 throughput + tail latency.
- **Query targets:** search/filtered-list (FTS path) under the same
  parallel load.
- **Mixed mode:** read/query performance while sustained writer activity
  runs — the exact scenario that exposed this defect.
- Ratcheted floors in `baselines.json` per host label, standard
  3-clean-run rules — a read-serialization regression becomes a FAIL
  campaign, not an anecdote.
- Harness/report/schema extensions follow the shared-contract rules
  (separate PR, team-lead visibility, macOS/Windows impact stated).

## 4. Execution notes

- All daemon work targets the Tokio+Axum `atm-http-runtime` path only;
  the frozen legacy sync daemon is untouched (AGENTS.md hard rule).
- Implementation branch: `fix/mailbox-read-blocking-serialization`
  (held clean by team-lead); PRs target `develop`; merge-commit only.
- Adjacent-work sequencing: the #1030 WPERF plan touches the same writer
  path — coordinate worktrees/merge order with team-lead.
- Plan QA: quality-mgr review before any implementation dispatch.

## 5. QA history

| Round | Date | Reviewer | Result | Notes |
|---|---|---|---|---|
| — | 2026-08-31 | — | pending | I-1..I-5 evidence incorporated (msg 01M1AJVGB9V5WXBGS2SF03KTS8); awaiting quality-mgr round 1. |
