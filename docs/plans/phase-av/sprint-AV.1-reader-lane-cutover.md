---
phase: AV
sprint: AV.1
title: Async mailbox reader-lane cutover
branch: fix/mailbox-read-blocking-serialization
integration_branch: integrate/phase-av
status: planned
recommended_agent: arch-ctm
recommended_model: deep-reasoning
dependency_relations:
  - related: AV.2
    relation: parallel_safe
    rationale: AV.2 edits docs/requirements.md and ADR files only; AV.1 edits
      crates only. No file, contract, or artifact intersection.
  - related: AV.3
    relation: must_follow
    rationale: AV.3's gates assert the post-cutover state (BlockingCoreBridge
      deleted, WriteOp pure). They cannot pass until AV.1 lands; merge-forward
      AV.1 → AV.3 before every AV.3 round.
  - related: AV.4
    relation: must_follow
    rationale: AV.4 benchmarks drive the reader lane and consume its metrics
      seams delivered here.
---

# AV.1 — Async mailbox reader-lane cutover

Complete the AL-phase Tokio cutover on the mailbox read path: replace the
single-permit `BlockingCoreBridge` read routing with a bounded pool of
read-only WAL reader workers, split hidden read-flow mutations onto the
writer lane, and make doctor independently schedulable. Evidence base and
root cause: [phase-av-plan.md](./phase-av-plan.md) §1.

## Deliverables

This is the authoritative deliverable checklist.

- [ ] D1 — `AsyncMailboxReader` capability in `atm-storage`
      (`contract.rs`): a separately named async read-only trait/handle,
      distinct from the write-oriented `AsyncMessageStore`, exposing
      mailbox metadata projection (list/peek) and record-body load.
- [ ] D2 — Reader pool implementation in `atm-storage-rusqlite`: N
      independent read-only worker connections generalizing the
      `SearchReader` bounded mpsc/oneshot/deadline worker shape
      (`search_reader.rs:40-75`), using the analyst RO connection
      substrate (`analyst_query.rs:206-221`: `SQLITE_OPEN_READ_ONLY |
      NO_MUTEX`, `query_only=ON; trusted_schema=OFF; defensive=ON`).
      Explicit pool-size bound and per-request deadline with explicit
      saturation error.
- [ ] D3 — Capability threading: `StorageHandles`
      (`factory.rs:12-35,89-92`), `LocalServiceRuntime`
      (`service_runtime.rs:145-166,325-344`), and
      `atm-runtime/src/composition.rs:153-170` expose the reader handle;
      no rusqlite type leaks into `atm-http-runtime`.
- [ ] D4 — Read-family handler cutover in
      `storage_and_nudge_router.rs`: list (:493-511), peek (:514-533),
      read (:536-555), doctor (:579-637) serve from the reader lane;
      none acquires the `BlockingCoreBridge` permit.
- [ ] D5 — Hidden-mutation split: `apply_display_mutations_to_store`
      (`atm-core/src/read/mod.rs:354-365`) and the seen-watermark write
      (:211-225) become explicit writer-lane state transitions enqueued
      after read-only selection returns (race-tolerant per phase plan
      §1.2).
- [ ] D6 — Doctor decomposition: core doctor projection
      (`doctor/mod.rs:130-170,173-230`) is an async, independently
      bounded control-plane composition; Herdr-presence leg stays
      separately timed; doctor acquires neither reader-pool permits nor
      the writer lane.
- [ ] D7 — Writer purity: `WriteOp::ListMessages`
      (`writer/ops.rs:37,106`), `SharedDb::submit_list_messages_async`
      (`shared_db.rs:482-501`), and the writer-routed
      `list_messages_async` delegation (`lib.rs:612-615`) removed.
- [ ] D8 — Read deadline enforcement: read jobs are cancelled/abandoned
      at the request deadline; durable writes retain run-to-completion.
- [ ] D9 — Reader-lane metrics seams: queue depth/saturation, in-flight
      count, wait vs. execution duration, deadline-expiry count, pool
      size — exported so AV.4 floors can diagnose regressions.
- [ ] D10 — Deterministic liveness tests in the router fixture
      (`storage_and_nudge_router.rs:1053+`): stalled housekeeping seam +
      concurrent read storm within budget; bounded-overload explicit
      failure. Runs in standard `just test`.

## Code contracts

Indicative signatures; final naming may vary, semantics may not.

```rust
// atm-storage/src/contract.rs — new read-only capability (D1).
// Implementations MUST NOT acquire the writer lane, any write-capacity
// permit, or a read-write connection.
#[async_trait]
pub trait AsyncMailboxReader: Send + Sync {
    async fn list_messages(
        &self,
        query: MailboxQuery,
        deadline: RequestDeadline,
    ) -> Result<Vec<MessageMetadata>, ReadLaneError>;

    async fn load_message(
        &self,
        id: MessageId,
        deadline: RequestDeadline,
    ) -> Result<MessageRecord, ReadLaneError>;
}

// Bounded resource-management errors (D2, D8). Saturation and deadline
// expiry are explicit outcomes, never silent serialization.
pub enum ReadLaneError {
    DeadlineExpired { waited: Duration },
    Saturated { pool_size: NonZeroUsize, queue_depth: usize },
    Storage(StorageError),
}

// atm-storage-rusqlite — reader pool bound (D2).
pub struct ReaderPoolConfig {
    pub pool_size: NonZeroUsize,   // N independent RO WAL workers
    pub queue_depth: usize,        // bounded submission queue
}

// Writer-lane state transition replacing the hidden read-flow mutation
// (D5). Enqueued after the read returns; loss/reorder races are
// acceptable per the race-tolerant state contract.
// (Extends the existing WriteOp enum in writer/ops.rs.)
WriteOp::ApplyReadDisplayState {
    mailbox: MailboxId,
    message_ids: Vec<MessageId>,
    seen_watermark: Option<Watermark>,
}
```

## Acceptance criteria

This is the authoritative acceptance checklist (phase contract points
1–6 mapped to testable statements).

- [ ] A1 — No mailbox read/peek/list/doctor path references
      `BlockingCoreBridge`, `spawn_blocking`, or sync `*_with_runtime`
      read APIs (verified by grep + architecture test).
- [ ] A2 — Liveness test: with one housekeeping/mutation job stalled and
      writer activity running, ≥10 concurrent list/peek/read/doctor
      calls across distinct teams each complete within their request
      budget.
- [ ] A3 — Overload test: reads beyond pool + queue capacity fail
      explicitly with `Saturated`/`DeadlineExpired`, not by queuing
      indefinitely.
- [ ] A4 — Read flows perform zero writer-lane work before returning;
      display/seen mutations are observed on the writer lane afterward.
- [ ] A5 — Doctor completes while both the reader pool and writer lane
      are saturated.
- [ ] A6 — `WriteOp` contains no pure-read variant; the writer queue
      receives no read traffic under a read-only workload (asserted via
      writer metrics in a test).
- [ ] A7 — All existing mailbox read/clear/graft behavior tests pass
      unchanged except where they asserted serialization.

## Required validation

This is the authoritative validation checklist.

- [ ] `just lint`
- [ ] `just test` (includes D10 liveness tests)
- [ ] `just validate`
- [ ] Architecture/boundary tests green (`cargo test -p atm-architecture`)
- [ ] Live manual proof on a local daemon build: `atm read` under
      induced housekeeping stall returns within budget (gate feature —
      live proof before QA dispatch).

## Out of scope

- Deleting `BlockingCoreBridge` remnants used by mutation paths and the
  enforcement gates — AV.3.
- Requirements/ADR text — AV.2.
- Benchmark harness/report work — AV.4.
- Any change to the frozen legacy synchronous daemon.
