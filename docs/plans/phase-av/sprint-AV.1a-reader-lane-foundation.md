---
phase: AV
sprint: AV.1a
title: Reader-lane foundation (capability, pool, threading)
branch: fix/mailbox-read-blocking-serialization
integration_branch: integrate/phase-av
stack_parent: integrate/phase-av (stack bottom) — planned; stack provisioned by task AV.0 (phase plan §4)
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
- [ ] D4 — Read deadline enforcement inside the lane, with a defined
      cancellation protocol (not abandonment): the request deadline
      propagates into the worker; on expiry of an *active* query the
      pool interrupts the running SQLite statement via the connection's
      interrupt handle (`rusqlite::InterruptHandle`), the worker maps
      `SQLITE_INTERRUPT` to `DeadlineExpired` and returns to service
      (capacity reclaimed, connection reusable). Cooperative interrupt is
      the ONLY reclamation path — a thread inside a blocked synchronous
      SQLite call is never force-terminated.

      **Non-responsive worker lifecycle (normative):** if a worker has
      not returned within the grace period after interrupt, it is
      *quarantined*: removed from the scheduler so it receives no new
      jobs, but left alive. A quarantined worker *retires* only when its
      blocked call actually returns (its connection is then closed, and
      only then may a replacement be spawned). Effective capacity is
      `pool_size − quarantined_count`; the plan never claims configured
      capacity is restored while a quarantined worker is still alive.
      Quarantine is a finite zombie-resource budget
      (`ReaderPoolConfig::max_quarantined`, default `pool_size`); once
      it is exhausted the lane **fails closed** — new reads are rejected
      with `ReadLaneError::Saturated { .. }` (reason recorded as
      quarantine-exhausted) rather than spawning further workers or
      connections. Queue-wait expiry (never dispatched) is tracked
      separately from active-query interruption and from quarantine.
      Saturation beyond pool + queue capacity fails explicitly.
- [ ] D5 — Reader-lane metrics seams: queue depth/saturation, in-flight
      count, wait vs. execution duration, deadline-expiry count split by
      outcome (expired-in-queue vs. interrupted-while-active vs.
      quarantined), current quarantined-worker gauge, retired/replaced
      worker count, quarantine-exhausted rejections, pool size —
      exported so AV.4 floors can diagnose regressions.
- [ ] D6 — Async mailbox runtime port (the async core-service seam the
      cutover consumes): a Tokio-only `AsyncMailboxRuntime` port in
      `atm-runtime` exposing `list_mail` / `peek_mail` / `read_mail`,
      composing (a) the `AsyncMailboxReader` storage capability, (b) the
      pure selection/display logic, and (c) a writer-lane handle for the
      AV.1b mutation split. The pure selection/display/authorization
      logic currently embedded in the synchronous read flow
      (`atm-core/src/read/mod.rs`) is extracted into a side-effect-free
      module (`atm-core::read::selection`) consumed by BOTH the existing
      synchronous path (behavior unchanged, delegation only) and the new
      async port — business semantics exist exactly once. The port
      preserves the existing team/agent authorization and visibility
      filters exactly as `LocalServiceRuntime` applies them today. The
      frozen legacy synchronous daemon is not modified. Runtime-inert
      this sprint: no `atm-http-runtime` handler consumes the port until
      AV.1b.

## Code contracts

Indicative signatures; final naming may vary, semantics may not.

```rust
// atm-storage/src/contract.rs — new read-only capability (D1).
// Sealed per the repository backend-capability pattern: only authorized
// in-repo implementations (the rusqlite reader pool, plus the in-memory
// test double) may implement it. Implementations MUST NOT acquire the
// writer lane, any write-capacity permit, or a read-write connection.
#[async_trait]
pub trait AsyncMailboxReader: sealed::Sealed + Send + Sync {
    async fn list_messages(
        &self,
        scope: MailboxScope,          // team + agent authorization scope,
        query: MailboxQuery,          // enforced at the storage boundary
        deadline: RequestDeadline,
    ) -> Result<Vec<MessageMetadata>, ReadLaneError>;

    async fn load_message(
        &self,
        scope: MailboxScope,          // cross-team/cross-agent lookup is
        id: MessageId,                // rejected here, not by convention
        deadline: RequestDeadline,
    ) -> Result<MessageRecord, ReadLaneError>;
}

// atm-runtime — Tokio-only async core-service seam (D6). AV.1b routes
// every read-family handler through this port; it owns composition of
// storage reads, pure selection, and writer-lane state handoff.
#[async_trait]
pub trait AsyncMailboxRuntime: Send + Sync {
    async fn list_mail(&self, scope: MailboxScope, req: ListRequest, deadline: RequestDeadline)
        -> Result<ListResponse, MailboxReadError>;
    async fn peek_mail(&self, scope: MailboxScope, req: PeekRequest, deadline: RequestDeadline)
        -> Result<PeekResponse, MailboxReadError>;
    async fn read_mail(&self, scope: MailboxScope, req: ReadRequest, deadline: RequestDeadline)
        -> Result<ReadResponse, MailboxReadError>;
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
    pub interrupt_grace: Duration, // interrupt → quarantine threshold
    pub max_quarantined: usize,    // zombie budget; exhausted ⇒ fail closed
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
- [ ] A3a — Capacity reclamation (D4): deterministic test that times out
      an *active* blocked query, proves the statement is interrupted and
      the worker's capacity is reclaimed, and a subsequent independent
      read on that worker completes within budget. Existing queue-wait
      and saturation tests retained.
- [ ] A3c — Adversarial quarantine test (D4): with a test-double worker
      that ignores interrupt, repeated deadline expiries prove (1) the
      worker is quarantined and receives no new jobs, (2) no replacement
      is spawned while it is alive (worker + connection counts stay
      bounded at `pool_size + max_quarantined` at most), (3) once
      `max_quarantined` is exhausted new reads are rejected explicitly
      with `Saturated`, and (4) when the blocked call finally returns
      the worker retires and capacity is restored — never before.
- [ ] A3b — Boundary authorization (D1): negative tests prove a
      cross-team or cross-agent `load_message`/`list_messages` with a
      mismatched `MailboxScope` is rejected at the storage boundary, and
      an out-of-crate implementation of the sealed trait does not
      compile (compile-fail or sealed-pattern test per repo convention).
- [ ] A4 — Boundary: `atm-http-runtime` compiles against the new handle
      without any rusqlite dependency; existing handler behavior is
      byte-for-byte unchanged (no handler file modified this sprint).
- [ ] A5 — Metrics seams (D5) observable in a test via the exported
      counters/gauges, including the interrupted vs. abandoned split and
      worker-replacement count.
- [ ] A6 — Async-port parity (D6): parity tests prove the
      `AsyncMailboxRuntime` port and the existing synchronous core path
      produce identical results for read, peek, list, missing-record,
      and state-transition-visibility cases over the same store fixture
      (state-transition application itself is AV.1b; here visibility
      parity of already-applied state is asserted). Existing sync-path
      tests pass unchanged (selection extraction is delegation-only).

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
