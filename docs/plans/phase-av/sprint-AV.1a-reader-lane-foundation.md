---
phase: AV
sprint: AV.1a
title: Reader-lane foundation (capability, pool, threading)
branch: fix/mailbox-read-blocking-serialization
integration_branch: integrate/phase-av
stack_parent: integrate/phase-av (stack bottom)
status: planned
recommended_agent: arch-ctm
recommended_model: deep-reasoning
dependency_relations:
  - related: AV.1b
    relation: must_follow
    rationale: AV.1b flips the read handlers onto the capability delivered
      here. AV.1b is stacked on this branch; restack propagates changes.
  - related: AV.2
    relation: parallel_safe
    rationale: AV.2 edits docs/requirements.md and ADR files only; this
      sprint edits crates only.
---

# AV.1a — Reader-lane foundation

Deliver the bounded read-only reader lane end-to-end through the storage
boundary — **without changing any handler behavior**. This sprint is
deliberately inert at runtime: the new capability is constructed, tested,
and exposed, but `atm-http-runtime` does not consume it until AV.1b. That
split keeps the atomic behavior change (AV.1b) small while this sprint
carries the cross-crate surface work. Evidence base and root cause:
[phase-av-plan.md](./phase-av-plan.md) §1.

## Deliverables

This is the authoritative deliverable checklist. Every listed
deliverable is expected to land at a production-ready level for the
scope this sprint claims; partial or shape-only completion fails the
sprint.

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
- [ ] D4 — Read deadline enforcement inside the lane: reader jobs are
      cancelled/abandoned at the request deadline; saturation beyond
      pool + queue capacity fails explicitly.
- [ ] D5 — Reader-lane metrics seams: queue depth/saturation, in-flight
      count, wait vs. execution duration, deadline-expiry count, pool
      size — exported so AV.4 floors can diagnose regressions.

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

// Bounded resource-management errors (D2, D4). Saturation and deadline
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
```

## Acceptance criteria

This is the authoritative acceptance checklist.

- [ ] A1 — Pool concurrency unit test: with one reader worker blocked on
      a slow query, other workers service list/load calls concurrently
      within deadline.
- [ ] A2 — Saturation unit test: submissions beyond pool + queue
      capacity fail explicitly with `Saturated`; deadline expiry during
      queue wait fails with `DeadlineExpired { waited }`.
- [ ] A3 — RO safety: reader connections reject any write statement
      (`query_only=ON` verified by test).
- [ ] A4 — Boundary: `atm-http-runtime` compiles against the new handle
      without any rusqlite dependency; existing handler behavior is
      byte-for-byte unchanged (no handler file modified this sprint).
- [ ] A5 — Metrics seams (D5) observable in a test via the exported
      counters/gauges.

## Required validation

This is the authoritative validation checklist.

- [ ] `just lint`
- [ ] `just test`
- [ ] `just validate`
- [ ] Architecture/boundary tests green (`cargo test -p atm-architecture`)

## Out of scope

- Any handler change in `atm-http-runtime` — AV.1b.
- Removing `WriteOp::ListMessages` or the writer-routed read path —
  AV.1b (still has callers until the cutover).
- Requirements/ADR text — AV.2. Gates — AV.3. Benchmarks — AV.4.
- Any change to the frozen legacy synchronous daemon.
