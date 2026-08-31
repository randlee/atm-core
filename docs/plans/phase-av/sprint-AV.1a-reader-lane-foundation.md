---
phase: AV
sprint: AV.1a
title: Reader-lane foundation (capability, pool, threading)
branch: fix/mailbox-read-blocking-serialization
worktree: /Users/randlee/Documents/github/atm-core-worktrees/fix/mailbox-read-blocking-serialization
integration_branch: integrate/phase-av
stack_parent: integrate/phase-av (stack bottom) — planned; stack provisioned by task AV.0 (phase plan §4)
status: complete
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

- [x] D1 — `AsyncMailboxReader` capability in `atm-storage`
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
- [x] D1a — Boundary record for the new capability (ADR-001: sealing is
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
- [x] D2 — Reader pool implementation in `atm-storage-rusqlite`: N
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
- [x] D2a — `SearchReader` re-hosted on the same pool type (one
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
- [x] D3 — Capability threading: `StorageHandles`
      (`factory.rs:12-35,89-92`), `LocalServiceRuntime`
      (`service_runtime.rs:145-166,325-344`), and
      `atm-runtime/src/composition.rs:153-170` expose the reader handle;
      no rusqlite type leaks into `atm-http-runtime`.
- [x] D4 — Read deadline enforcement inside the lane, with a defined
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
- [x] D5 — Reader-lane metrics seams, labelled per lane instance
      (`mailbox` / `search`; AV.1b's `doctor` lane reuses them): queue
      depth/saturation, in-flight count, wait vs. execution duration,
      deadline-expiry count split by outcome (expired-in-queue vs.
      interrupted-while-active vs. quarantined), current
      quarantined-worker gauge, retired/replaced worker count,
      quarantine-exhausted rejections, pool size, and WAL health (last
      checkpoint outcome, current WAL frame count) — exported so AV.4
      floors can diagnose regressions.
- [x] D6 — Async mailbox runtime port (the async core-service seam the
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

- [x] A1 — Pool concurrency unit test: with one reader worker blocked on
      a slow query, other workers service list/load calls concurrently
      within deadline.
- [x] A2 — Saturation unit test: submissions beyond pool + queue
      capacity fail explicitly with `Saturated`; deadline expiry during
      queue wait fails with `DeadlineExpired { waited }`.
- [x] A3 — RO safety: reader connections reject any write statement
      (`query_only=ON` verified by test).
- [x] A3a — Capacity reclamation (D4): deterministic test that times out
      an *active* blocked query, proves the statement is interrupted and
      the worker's capacity is reclaimed, and a subsequent independent
      read on that worker completes within budget. Existing queue-wait
      and saturation tests retained.
- [x] A3c — Adversarial quarantine test (D4): with a test-double worker
      that ignores interrupt, repeated deadline expiries prove (1) the
      worker is quarantined and receives no new jobs, (2) no replacement
      is spawned while it is alive (worker + connection counts stay
      bounded at `pool_size + max_quarantined` at most), (3) once
      `max_quarantined` is exhausted new reads are rejected explicitly
      with `Saturated`, and (4) when the blocked call finally returns
      the worker retires and capacity is restored — never before.
- [x] A3b — Boundary authorization (D1/D1a): negative tests prove a
      cross-team or cross-agent `load_message`/`list_messages` with a
      mismatched `MailboxScope` is rejected at the storage boundary, and
      an out-of-crate implementation of the sealed trait does not
      compile (compile-fail or sealed-pattern test per repo convention).
      `lint_boundaries.py` passes with the two D1a records present, and
      a scratch second implementation site (outside the authorized pool
      type / declared test double) fails `just lint` (demonstrated once,
      reverted). `atm-storage`'s `Cargo.toml` gains no new dependency;
      `ReadDeadline` is the only deadline type in the trait.
- [x] A3d — Transaction scoping + WAL health (D2): a test holds sustained
      concurrent reader load (all lanes) beside continuous writer commits
      and proves `PRAGMA wal_checkpoint(PASSIVE)` keeps progressing and
      the WAL frame count stays bounded; a scratch mutation that leaves a
      read transaction open across jobs makes the test fail
      (demonstrated once, reverted).
- [x] A3e — Connection budget (D2): composition with the default knobs
      opens exactly 12 connections at steady state and never more than
      the documented worst case of 22 under the A3c quarantine scenario
      (asserted by counting opened connections in a test build); a
      configuration whose
      sum exceeds `max_connections` fails startup with an error naming
      each contributing knob.
- [x] A3f — Search lane re-host (D2a): every existing search test passes
      unchanged; a concurrency test with one search worker blocked on a
      slow FTS query proves a second search request completes within
      deadline and that mailbox-lane capacity is unaffected (and vice
      versa); `search_reader.rs`'s single-thread loop no longer exists.
- [x] A4 — Boundary: `atm-http-runtime` compiles against the new handle
      without any rusqlite dependency; existing handler behavior is
      byte-for-byte unchanged (no handler file modified this sprint).
- [x] A5 — Metrics seams (D5) observable in a test via the exported
      counters/gauges, per lane label, including the three D4 deadline
      outcomes (expired-in-queue, interrupted-while-active, quarantined)
      as distinct counters, the quarantined-worker gauge, the
      retired/replaced worker count, and the WAL-health gauges.
- [x] A6 — Async-port parity (D6): parity tests prove the
      `AsyncMailboxRuntime` port and the existing synchronous core path
      produce identical results for read, peek, list, missing-record,
      and state-transition-visibility cases over the same store fixture
      (state-transition application itself is AV.1b; here visibility
      parity of already-applied state is asserted). Existing sync-path
      tests pass unchanged (selection extraction is delegation-only).

## Required validation

This is the authoritative validation checklist.

- [x] `just lint`
- [x] `just test`
- [ ] `just validate` — blocked by repository-wide stale pinned tools
      (`sc-compose` 1.5.0 vs 1.6.0; Wyvern 0.5.0 vs 0.6.0), unrelated to
      this sprint's reader-lane implementation.
- [x] Architecture/boundary tests green (`cargo test -p atm-architecture`);
      `python3 .just/lint_boundaries.py` green with the D1a records.

## QA round 1 closure

All AV.1a round-1 findings are implemented before this status transition:
AV1A-B2 `f81db7eb13b2ae9940ce51e17780673f34258af4`; AV1A-I1
`f00643c69ef6bcb0511903236bbcb5e8515d8b70`; AV1A-I2
`bf347f711b76331bc781fc8371324f5e37cbc249`; AV1A-I3/I4
`5e717719bfad355203a09d29b45a58fb56b21e91`; AV1A-I5
`fd2d02191ad23d229d8af8f8b0577d5cbbb2705f`; AV1A-M1
`32cde39158a31df172674d8930a34803b9ebcc3f`; AV1A-M2
`cb96ef022adec670fe1f3a4d7cc654f53f0c05b6` (with WAL-state completion in
M8); AV1A-M3 `2b6c95958975ea59894d3598efbbed7ae771a8d8`; AV1A-M4
`1517df1f57ffc98a45547912b9051612b0352739`; AV1A-M5
`7212f4a6a83f5145796a05807df639bb8333bed4`; AV1A-M6
`487a8be08114af7ed812ccd6c1ebeeeb322a7567`; AV1A-M7
`e2f172fcc929ae606a3c0d5ec2563c52ea844df1`; and AV1A-M8
`73c0a4bf1d36a87a8b31d517754a86bfb0aa76d5`.

Validation before closure: `cargo test --workspace --exclude atm-daemon`,
`cargo test -p atm-architecture`, `just lint`, and
`python3 .just/lint_boundaries.py` pass. `just validate` reaches the final
dependency-currency gate and fails only because the repository-wide
`sc-ecosystem`/Wyvern pins are stale; that external pin work is out of AV.1a
scope.

## QA round 2 closure

AV1A-M4, AV1A-I6, and AV1A-M9 are closed by the AV.1a fix-round-2 commit:
both reader boundary records mechanically forbid write-capable connections and
writer lanes; the boundary lint now scans each record's declared, tag-specific
implementation modules, including declared trait-only adapter modules. A
SQLite `Connection::open_with_flags` call is permitted only when its flags are
read-only; default opens and flagged `READ_WRITE`/`CREATE` opens fail the
guard, while `#[cfg(test)]` in-memory setup stays outside production-policy
scope. Validation reports `just validate` honestly as failing only at the
known dependency-currency gate above.

## QA round 3 closure

AV1A-I6's full trait-only I/O-policy sweep is implemented by
`0ca86ecafb342b9f68fa298b6a1bdaf81970c44f`. All 34 records with a non-empty
`io_forbidden` policy now have a non-vacuous source target: 22 derive the
production trait-contract declaration region, the pre-existing AV.1a reader
mapping remains explicit for its helper modules, and 11 retired/planned
contracts assert `no_in_repo_implementation = true`, which the lint verifies
against production `impl Trait for Type` sites. The scan fails closed for any
record lacking all three forms of coverage. The sweep surfaced one
`SocketAddr` metadata false positive in `PeerConfigStore`; it is covered by a
single exact source exception, leaving socket operations themselves guarded.
`python3 .just/lint_boundaries.py` passes. As in round 2, `just validate` is
reported honestly after the full validation run rather than described as a
clean pass while dependency-currency remains an external gate. QA owns the
finding's final closure after independent verification.

## QA round 4 closure

AV1A-I6 is completed by `eb9744b4b03c0a6a99a07bca105da4a7fffef350`: an
unmapped trait-only I/O policy now derives both the trait declaration region
and every production `impl Trait for Type` module. A declared concrete boundary
remains the single owner for its concrete adapter source, so permitted adapter
I/O is evaluated by that boundary rather than incorrectly attributed to the
abstract trait. This reaches the previously uncovered
`DrainOnTransitionSink` implementation in `queue_drain.rs`; its
`database_io`, `process_spawn`, and `daemon_request_dispatch` scan surfaced no
real violation. Both `AsyncMailboxReader` records explicitly scan
`mailbox_reader`, `reader_pool`, and `shared_db_reader_lanes` for `socket_io`
and `process_spawn`; negative fixtures prove both constructs fail the lint.

The source-target split is regenerated from the tree: 34 trait-only records
with non-empty `io_forbidden` policies comprise 19 declaration-plus-production
implementation derivations, one explicit `AsyncMailboxReader` helper mapping,
and 14 lint-verified `no_in_repo_implementation` declarations. The latter
includes the cfg(test)-only `StorageNotifier` case, completed by
`f682576081ee2e436ea250191e405880efe1fc2e`. The sweep keeps failing closed
when a contract has neither a production implementation nor that explicit
declaration. `python3 .just/lint_boundaries.py` passes. As in earlier rounds,
`just validate` is reported from its actual final run and is not described as
clean while the repository-wide dependency-currency gate remains external.

Future-round candidates only: a `write_capable_connection` flag assembled via
a variable can evade the current literal-flag matcher, and widening an AV.3
handler signature can evade its allowlist. Neither changes the AV.1a reader
lane and neither is implemented here.

## Out of scope

- Any handler change in `atm-http-runtime` — AV.1b.
- Removing `WriteOp::ListMessages` or the writer-routed read path —
  AV.1b (still has callers until the cutover).
- Requirements/ADR text — AV.2. Gates — AV.3. Benchmarks — AV.4.
- Any change to the frozen legacy synchronous daemon.
- Changing `AsyncMessageSearchStore`/`MessageSearchStore` semantics or
  DTOs — D2a re-hosts the search worker only.
