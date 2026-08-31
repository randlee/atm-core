# ADR-059 — Async Mailbox-Read Concurrency And State Handoff

| Field | Value |
| --- | --- |
| ID | ADR-059 |
| Status | Proposed (Phase AV.2) |
| Scope | Mailbox read-family scheduling, SQLite reader-lane ownership, and read-side state handoff |
| Relates to | ADR-001, ADR-036, phase-av-plan §1.1–§1.2, AV.1a D1/D1a, AV.1b D2, AV.3 D1/D2/D2b/D3/D4, `av-closeout-record.md` |

## Context

The AL3 implementation admitted every core job through one single-permit
`WriteAdmission`. AL13-G7 renamed the resulting runtime wrapper to
`BlockingCoreBridge` but retained that permit for mailbox reads. The result is
head-of-line blocking: a slow write or housekeeping operation can consume the
only permit while an unrelated `atm read` waits past its client deadline. The
history and the affected current paths are recorded in
[`phase-av-plan.md`](../phase-av-plan.md) §1.1 and
[`av-closeout-record.md`](../av-closeout-record.md).

The storage topology remains the one in ADR-036: backend-owned SQLite
connections and transactions stay behind storage traits, while
`atm-runtime` composes those traits and `atm-http-runtime` routes requests.
This ADR changes scheduling ownership, not that topology.

## Decision

### D1. Independent bounded reader lane

AV.1a defines the storage-owned `AsyncMailboxReader` capability and
`ReadDeadline` value in the reader-lane contract described by
[`sprint-AV.1a-reader-lane-foundation.md`](../sprint-AV.1a-reader-lane-foundation.md)
§D1/D1a. The selected backend owns a bounded pool of read-only WAL
connections. Each job has its own read transaction and cancellable deadline;
no cursor or transaction outlives the job.

The composition path is:

1. `atm-runtime` provides the `AsyncMailboxReader` handle and translates the
   runtime request deadline to storage-owned `ReadDeadline` once at the
   boundary.
2. `atm-storage-rusqlite` owns the pool and its read-only SQLite connections.
   Its existing `search_reader.rs` is re-hosted as a separate instance of
   the same bounded-pool shape, so search capacity cannot consume mailbox
   capacity.
3. `atm-http-runtime` read handlers consume only the reader capability (and
   the dedicated doctor projection for `doctor`). They do not acquire a
   writer permit and do not submit pure reads to the writer lane.

The reader lane is bounded by explicit pool and queue capacities. Saturation
is an explicit error, not an unbounded wait. Cancellation releases capacity
after the job stops, and read deadlines are enforced independently of writer
work.

### D2. Ordered writer lane and non-blocking state handoff

Durable message admission and mutable state transitions remain ordered on the
single writer lane. A read response is eligible after read-only selection and
body loading; its read/seen transition is then offered synchronously with a
non-blocking `try_push` to the bounded `StateHandoffSupervisor` specified by
AV.1b D2. The read path never awaits writer admission or retry.

`R-STATE-HANDOFF-1` is a product decision. The supervisor is readiness-gated,
monitored, and owns retry. On task fault it atomically enters `Unavailable`,
preserves buffered work, and attempts restart within a bounded budget.
Handoff-buffer overflow (counted and surfaced by `atm doctor`) and process
exit are the only permitted loss cases. Both are fail-safe: the message stays
unread/unseen and is re-presented. Restart exhaustion or permanent writer
failure fails the runtime closed. `R-STATE-RACE-1` describes observation
under concurrent state changes; it never authorizes dropping a transition.

The concrete AV.1b contract types are `StateHandoffSupervisor` and
`HandoffConfig`, with the transition represented by
`WriteOp::ApplyReadDisplayState`. They are documented in
[`sprint-AV.1b-read-handler-cutover.md`](../sprint-AV.1b-read-handler-cutover.md)
§D2 and are not a second reader/writer queue.

### D3. Explicit bridge retirement boundary

AV.3 renames the residual synchronous wrapper
`BlockingCoreBridge` to `ControlPathSyncBridge`. The rename is intentionally
mechanical and scope-limited: it prevents a read path from silently restoring
the old name while leaving the eight non-read/control-path call sites
enumerated in [`av-closeout-record.md`](../av-closeout-record.md).

AV.3 also removes the pure-read `WriteOp::ListMessages` variant,
`SharedDb::submit_list_messages_async`, and the bespoke
`crates/atm-storage-rusqlite/src/search_reader.rs` loop in favor of the
reader-lane capability. Its D1 exact-call-site gate, D2 dependency allowlist,
D2b instrumented-writer behavior gate, D3 deny-list checker, and D4 liveness
tests are the enforcement surface. Any new bridge, semaphore, blocking spawn,
or writer submission on a read-family path is non-compliant.

### D4. Concrete current and target modules

The current revision (`db08f4591`) contains the following symbols and paths;
AV.3 changes their ownership as stated, without changing ADR-036's storage
boundary:

| Concern | Current path/symbol | AV target |
| --- | --- | --- |
| HTTP control wrapper | `crates/atm-http-runtime/src/storage_and_nudge_router.rs:BlockingCoreBridge` | `ControlPathSyncBridge`, residual control paths only |
| Pure-read writer op | `crates/atm-storage-rusqlite/src/writer/ops.rs:WriteOp::ListMessages` | removed; reader lane owns projection |
| Pure-read async submission | `crates/atm-storage-rusqlite/src/shared_db.rs:SharedDb::submit_list_messages_async` and `crates/atm-storage-rusqlite/src/lib.rs:SqliteMessageStore::list_messages_async` | removed/repointed to `AsyncMailboxReader` |
| Search reader loop | `crates/atm-storage-rusqlite/src/search_reader.rs` | second bounded reader-pool instance |
| Reader capability/deadline | AV.1a D1/D1a contract in `crates/atm-storage/src/contract.rs` | `AsyncMailboxReader` + storage-owned `ReadDeadline` |

### D5. Product alternatives and operator consequences

Two alternatives were considered and rejected:

* **Permanent drop:** return the read response and discard a rejected
  read/seen transition. This can permanently hide a message and makes
  operator state unverifiable; it violates the fail-safe unread/unseen
  consequence.
* **Synchronous write-through:** await the writer before returning the read
  response. This restores head-of-line blocking, couples read latency to
  writer health, and defeats the independent reader deadline.

The chosen bounded handoff exposes overflow, supervisor state, restart count,
retry-deadline exhaustion, and buffered depth to `atm doctor`. Operators can
therefore distinguish a safely re-presented message from a runtime that has
failed closed.

## Consequences

* Read/list/peek/doctor/query capacity is independently bounded and can make
  progress while the writer lane is busy.
* Read observations are intentionally race-tolerant; callers cannot rely on
  read-your-writes, snapshot pinning, or reader/writer fencing.
* Durable admission and state transitions retain ordered writer semantics.
* The residual synchronous control-path work remains visible and bounded
  until the explicit `AV-FU-1` follow-up migrates it; it is not mislabeled as
  a completed bridge deletion.

## Verification and references

The normative requirements are `R-READ-CONC-1`, `R-READ-CONC-2`,
`R-STATE-RACE-1`, and `R-STATE-HANDOFF-1` in
[`docs/requirements.md`](../requirements.md). AV.3's mechanical gates are
defined in [`sprint-AV.3-mechanical-hard-gates.md`](../sprint-AV.3-mechanical-hard-gates.md).
The phase-owned path inventory is
[`docs/plans/phase-av/av-closeout-record.md`](../plans/phase-av/av-closeout-record.md);
the Phase-AM ledger remains frozen and is not edited by AV.
