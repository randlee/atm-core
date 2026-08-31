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
      **Deadline type is storage-owned:** the trait takes a
      `ReadDeadline` defined in `atm-storage` (same shape and rationale
      as the existing `SearchDeadline`, `atm-storage/src/search.rs:480-489`
      — a non-zero remaining `Duration`), never `atm_core::RequestDeadline`.
      `atm-storage → atm-core` is a forbidden edge
      (`boundaries/atm-storage/async-message-store.toml`
      `forbidden_edges`); the translation `RequestDeadline → ReadDeadline`
      happens once, at the runtime boundary inside the D6 port
      implementation in `atm-runtime`, exactly as search does today.
- [ ] D1a — Boundary record for the new capability (ADR-001: sealing is
      lint-enforced across crates, not compiler-enforced):
      `boundaries/atm-storage/async-mailbox-reader.toml` (public trait
      `AsyncMailboxReader`; `visibility = "trait_only"`; forbidden edges
      identical to `async-message-store.toml`; `io_forbidden` includes
      any write-capable connection) and its adapter sibling
      `boundaries/atm-storage-rusqlite/async-mailbox-reader-sqlite.toml`
      (naming the reader pool type as the single authorized
      implementation site, `references.forbidden` = the concrete pool
      type outside the owner crate), plus a section in
      `docs/atm-storage/boundaries.md`. `lint_boundaries.py` (already in
      `just lint`) must consume both records; the in-memory test double
      is declared under `[testing].allowed_test_double_paths`.
- [ ] D2 — Reader pool implementation in `atm-storage-rusqlite`: N
      independent read-only worker connections generalizing the
      `SearchReader` bounded mpsc/oneshot/deadline worker shape
      (`search_reader.rs:40-75`), using the analyst RO connection
      substrate (`analyst_query.rs:206-221`: `SQLITE_OPEN_READ_ONLY |
      NO_MUTEX`, `query_only=ON; trusted_schema=OFF; defensive=ON`).
      Explicit pool-size bound and per-request deadline with explicit
      saturation error.

      **Defaults and knob location (normative):** `pool_size` default
      **4**, `queue_depth` default **16** (4 × pool), `interrupt_grace`
      default 250 ms, `max_quarantined` default `pool_size`. The knobs
      live in the same runtime configuration section AV.1b D3 uses for
      `doctor_pool_size`/`doctor_queue_depth` (one `[reader_lanes]`
      surface, one place to reason about), threaded through
      `StorageHandles`/composition like every other storage setting —
      never a hard-coded constant in a handler. The production default
      is deliberately independent of AV.4's benchmark fan-out: ≥32 is
      the number of concurrent *client requests* the harness issues
      against the production-default pool; the pool size in effect is
      recorded per campaign (AV.4 D7) and is not raised to make a floor.

      **Per-job transaction scoping (normative):** every reader job runs
      in its own read transaction that is closed before the worker
      returns to the queue; a worker connection never holds an open read
      transaction between jobs, and no API exposes a cursor/iterator
      that outlives the job. Rationale: a long-held read transaction on
      a WAL reader pins the WAL end mark and starves checkpointing, so
      unbounded WAL growth under sustained reader+writer load would be a
      regression of a different shape. Bounded pagination (limit/cursor
      *values*, not held statements) is the only way a large result set
      spans calls.

      **Connection budget (normative):** the total number of SQLite
      connections the daemon may open is stated in one place and
      asserted at startup: `1` writer + `pool_size` mailbox readers +
      `search_pool_size` (D2a) + `doctor_pool_size` (AV.1b D3) +
      `max_quarantined` (transient zombie budget) + the existing analyst
      RO connection. `max_quarantined` is **per lane** (each lane's
      default equals its own `pool_size`, per D4/A3c), so the transient
      term is the sum over lanes: mailbox 4 + search 2 + doctor 4 = 10.
      With all defaults the worst-case total is therefore
      `1 + 4 + 2 + 4 + 10 + 1 = 22` (steady state, no quarantined
      workers: 12). A `max_connections` knob (default 32, leaving 10
      of headroom over the worst case) caps the sum;
      composition fails closed at startup with an actionable error if
      the configured sum exceeds it, so no combination of knobs can
      silently exceed the per-process fd/connection ceiling.
- [ ] D2a — `SearchReader` re-hosted on the same pool type (one
      canonical reader-lane owner, per acceptance-contract point 3: "the
      existing search reader's single thread is also insufficient for
      fan-out"). The FTS/search lane becomes a second *instance* of the
      D2 pool with its own bound (`search_pool_size` default **2**,
      `search_queue_depth` default 8) so query load never consumes
      mailbox-read capacity and vice versa; `AsyncMessageSearchStore`
      semantics, `SearchDeadline`, and every existing search test are
      unchanged (delegation only). `search_reader.rs`'s bespoke
      single-thread loop is deleted; the reader-pool metrics (D5) are
      emitted per lane instance (`lane = mailbox | search | doctor`).
      AV.4 D2's parallel query benchmarks measure this lane.
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
- [ ] D5 — Reader-lane metrics seams, labelled per lane instance
      (`mailbox` / `search`; AV.1b's `doctor` lane reuses them): queue
      depth/saturation, in-flight count, wait vs. execution duration,
      deadline-expiry count split by outcome (expired-in-queue vs.
      interrupted-while-active vs. quarantined), current
      quarantined-worker gauge, retired/replaced worker count,
      quarantine-exhausted rejections, pool size, and WAL health (last
      checkpoint outcome, current WAL frame count) — exported so AV.4
      floors can diagnose regressions.
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
// atm-storage has no atm-core dependency (forbidden edge), so the
// deadline is storage-owned, mirroring `SearchDeadline`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadDeadline { remaining: Duration }   // new() rejects zero
#[async_trait]
pub trait AsyncMailboxReader: sealed::Sealed + Send + Sync {
    async fn list_messages(
        &self,
        scope: MailboxScope,          // team + agent authorization scope,
        query: MailboxQuery,          // enforced at the storage boundary
        deadline: ReadDeadline,
    ) -> Result<Vec<MessageMetadata>, ReadLaneError>;

    async fn load_message(
        &self,
        scope: MailboxScope,          // cross-team/cross-agent lookup is
        id: MessageId,                // rejected here, not by convention
        deadline: ReadDeadline,
    ) -> Result<MessageRecord, ReadLaneError>;
}

// atm-runtime — Tokio-only async core-service seam (D6). AV.1b routes
// every read-family handler through this port; it owns composition of
// storage reads, pure selection, and writer-lane state handoff. It is
// the single place `RequestDeadline` (atm-core) is translated into
// `ReadDeadline` (atm-storage) — the same boundary translation the
// search path performs for `SearchDeadline` today.
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

// atm-storage-rusqlite — reader pool bound (D2). One config per lane
// instance (mailbox, search; doctor in AV.1b), all under one
// `[reader_lanes]` runtime-config section.
pub struct ReaderPoolConfig {
    pub pool_size: NonZeroUsize,   // N independent RO WAL workers (mailbox default 4, search 2)
    pub queue_depth: usize,        // bounded submission queue (default 4 × pool)
    pub interrupt_grace: Duration, // interrupt → quarantine threshold (default 250 ms)
    pub max_quarantined: usize,    // zombie budget; exhausted ⇒ fail closed (default pool_size)
}

// Composition-time connection budget (D2). Fails closed at startup.
pub struct ConnectionBudget { pub max_connections: NonZeroUsize /* default 32 */ }
// asserted: 1 (writer) + mailbox.pool_size + search.pool_size
//         + doctor.pool_size + Σ_lanes max_quarantined + 1 (analyst RO)
//         <= max_connections
// defaults: 1 + 4 + 2 + 4 + (4 + 2 + 4) + 1 = 22  <= 32
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
- [ ] A3b — Boundary authorization (D1/D1a): negative tests prove a
      cross-team or cross-agent `load_message`/`list_messages` with a
      mismatched `MailboxScope` is rejected at the storage boundary, and
      an out-of-crate implementation of the sealed trait does not
      compile (compile-fail or sealed-pattern test per repo convention).
      `lint_boundaries.py` passes with the two D1a records present, and
      a scratch second implementation site (outside the authorized pool
      type / declared test double) fails `just lint` (demonstrated once,
      reverted). `atm-storage`'s `Cargo.toml` gains no new dependency;
      `ReadDeadline` is the only deadline type in the trait.
- [ ] A3d — Transaction scoping + WAL health (D2): a test holds sustained
      concurrent reader load (all lanes) beside continuous writer commits
      and proves `PRAGMA wal_checkpoint(PASSIVE)` keeps progressing and
      the WAL frame count stays bounded; a scratch mutation that leaves a
      read transaction open across jobs makes the test fail
      (demonstrated once, reverted).
- [ ] A3e — Connection budget (D2): composition with the default knobs
      opens exactly 12 connections at steady state and never more than
      the documented worst case of 22 under the A3c quarantine scenario
      (asserted by counting opened connections in a test build); a
      configuration whose
      sum exceeds `max_connections` fails startup with an error naming
      each contributing knob.
- [ ] A3f — Search lane re-host (D2a): every existing search test passes
      unchanged; a concurrency test with one search worker blocked on a
      slow FTS query proves a second search request completes within
      deadline and that mailbox-lane capacity is unaffected (and vice
      versa); `search_reader.rs`'s single-thread loop no longer exists.
- [ ] A4 — Boundary: `atm-http-runtime` compiles against the new handle
      without any rusqlite dependency; existing handler behavior is
      byte-for-byte unchanged (no handler file modified this sprint).
- [ ] A5 — Metrics seams (D5) observable in a test via the exported
      counters/gauges, per lane label, including the three D4 deadline
      outcomes (expired-in-queue, interrupted-while-active, quarantined)
      as distinct counters, the quarantined-worker gauge, the
      retired/replaced worker count, and the WAL-health gauges.
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
- [ ] Architecture/boundary tests green (`cargo test -p atm-architecture`);
      `python3 .just/lint_boundaries.py` green with the D1a records.

## Out of scope

- Any handler change in `atm-http-runtime` — AV.1b.
- Removing `WriteOp::ListMessages` or the writer-routed read path —
  AV.1b (still has callers until the cutover).
- Requirements/ADR text — AV.2. Gates — AV.3. Benchmarks — AV.4.
- Any change to the frozen legacy synchronous daemon.
- Changing `AsyncMessageSearchStore`/`MessageSearchStore` semantics or
  DTOs — D2a re-hosts the search worker only.
