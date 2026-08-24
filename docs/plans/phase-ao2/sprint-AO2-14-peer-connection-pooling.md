# Sprint AO2.14 — Daemon-Owned Peer Connection Pooling

Status: draft · Branch: `feature/ao2-14-peer-connection-pooling` off
`integrate/phase-ao2` (after the AO2.10–AO2.13 chain merges) · PR target:
`integrate/phase-ao2`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

## Problem

Every canonical cross-host dispatch is one-shot. In
`crates/atm-http-runtime/src/storage_and_nudge_router.rs`,
`dispatch_resolved_peer_write` builds a fresh client per write and drops it
after one request:

- **Plaintext TCP**: `DirectPeerTcpConnector::new` constructs a brand-new
  `reqwest::Client` per dispatch (`client.rs`, locate by
  `direct_peer_tcp_client`), so reqwest's internal pool never engages.
- **mTLS opaque-stream**: `PeerStreamConnector::exchange` performs
  `TcpStream::connect` → adapter mTLS handshake → one hyper
  `http1::handshake` → drop, per write.

Neither path has daemon-lifetime, bounded per-peer connection ownership.
Result: connect + TLS-handshake cost on every peer write.

## Deliverables

1. **`PeerConnectionPool`** in `crates/atm-http-runtime` (TLS-opaque — the
   crate must continue to never import rustls/TLS). The pool stores
   **post-handshake HTTP senders, not raw streams**: each pooled entry is a
   negotiated `hyper::client::conn::http1::SendRequest` plus the
   `JoinHandle` of its spawned connection-driver task, created once from
   the `EstablishedPeerStream` (`Box<dyn AuthenticatedPeerStream>`, the
   real adapter seam in `peer_stream.rs`) that `PeerStreamAdapter::connect`
   returns. Reusing the raw pre-handshake stream would force a fresh
   `http1::handshake` (and orphan a driver task) per write — that is
   explicitly not the design. Daemon-lifetime, bounded per-peer,
   idle-evicting. Keyed by the configured peer `HostName` authority —
   exactly matching `MtlsPeerStreamAdapter::client_config_for`'s
   exact-authority lookup — never by resolved IP; a pooled entry is never
   reused across peers. Dropping/evicting an entry closes the sender, which
   ends its driver task (driver-task completion is observed by AC #9 —
   sync Drop cannot join, so tests await a completion signal).
2. **Plaintext client reuse**: one shared daemon-lifetime `reqwest::Client`
   (its built-in pool then engages) replacing per-dispatch construction in
   `DirectPeerTcpConnector`. No custom pool needed on this path. Wiring
   contract mirrors deliverable 1's: `StorageAndNudgeRouter` gains
   `with_shared_direct_peer_client(client: reqwest::Client) -> Self`
   (builder style of `with_peer_stream_adapter`), and
   `build_replacement_handler` in `atm-daemon-bootstrap` constructs the one
   shared client at daemon scope and injects it —
   `DirectPeerTcpConnector::new(client: reqwest::Client, ...)` takes the
   shared client as a parameter instead of building its own (constructor
   signature change, call sites updated). The bootstrap edits for this
   deliverable and deliverable 4's pool wiring land as ONE coordinated
   change to `build_replacement_handler`, not two conflicting edits.
3. **Router integration**: `StorageAndNudgeRouter` gains a pool handle
   (builder method mirroring `with_peer_stream_adapter`);
   `dispatch_resolved_peer_write` changes to acquire → execute → release,
   with a failed connection discarded (not returned) and one transparent
   redial for a connection that died while pooled (see contract).
4. **Composition-root wiring** in `crates/atm-daemon-bootstrap/src/lib.rs`
   (`build_replacement_handler` / `run_replacement_daemon_with_selector`):
   construct and inject the pool at daemon scope, threaded the same way
   `peer_stream_adapter` is today.
5. **Config surface**: new `PeerPoolConfig { max_per_peer,
   max_pooled_total, idle_timeout }` in `HttpRuntimeConfig` with the
   defaults fixed in the normative contract block below (4 per peer, 32
   pooled total, 60s idle) and CLI/env plumbing analogous to
   `parse_direct_peer_port` (validated by AC #8).
6. **Error-taxonomy preservation**: the `HttpRuntimeClientFailure` variants
   (`EndpointRecord`, `Connect`, `PeerConnect`, `RequestWrite`,
   `ResponseDecode`, `Cancelled`, `Timeout`, `PeerConnectTimeout`) and their
   `into_atm_error` mapping are unchanged — existing tests assert on
   specific variants/messages; pooling adds reuse underneath, never new
   caller-visible failure shapes.
7. **New ADR — peer-write redial and delivery-attempt invariant**
   (`docs/adr/ADR-0NN-peer-write-redial-and-delivery-attempt-invariant.md`,
   NN = next available number at authoring time, added to `docs/adr/INDEX.md`):
   documents the redial-safety invariant fixed in the contract block
   (staleness detected only at acquire time via sender liveness; no retry
   of any failure after a request is handed to `exchange`; delivery-attempt
   count for a possibly-delivered request stays exactly one; no
   receiving-side idempotency assumed). Referenced from this doc and from
   the router's `dispatch_resolved_peer_write` comment.
8. **Boundary-test alignment**: deliberate updates (if needed) to
   `crates/atm-architecture/tests/boundary_enforcement.rs` literal-scan
   tests (`ao2_plaintext_baseline_stays_on_the_existing_direct_peer_pipeline`,
   `ao2_peer_wire_policy_keeps_one_error_registry_and_one_http_pipeline`) so
   pool types don't trip them — reviewed as explicit diffs, never weakened
   silently. `boundary-guard` reviews this deliverable.

## Pool contract (normative)

```rust
pub struct PeerPoolConfig {
    pub max_per_peer: usize,       // default 4
    pub max_pooled_total: usize,   // default 32 — hard ceiling on POOLED
                                   // entries (idle + borrowed) daemon-wide
    pub idle_timeout: Duration,    // default 60s; idle entries evicted
}
// max_pooled_total bounds only what the pool retains. Fallback one-shot
// dials (pool full or per-peer cap reached) are unpooled and deliberately
// NOT bounded by this config — exactly today's per-write-dial behavior,
// already bounded upstream by runtime admission limits. Stated risk, not
// an accident: a durable write is never queued or rejected by pool limits.

pub struct PeerConnectionPool { /* bounded, keyed by HostName authority */ }

impl PeerConnectionPool {
    pub fn new(config: PeerPoolConfig, adapter: Arc<dyn PeerStreamAdapter>) -> Self;

    /// Borrow a live connection to `peer`, dialing (TCP + adapter handshake)
    /// only when none is pooled. At most `max_per_peer` concurrent
    /// connections per peer; excess acquirers dial unpooled (never queue a
    /// durable write behind a pool slot). The caller's remaining per-write
    /// budget bounds EVERYTHING here — the liveness check, any redial, and
    /// the dial itself — producing today's PeerConnectTimeout when it
    /// elapses, exactly like PeerStreamConnector::exchange does now.
    /// Locking rule: the candidate entry is popped out of the
    /// std::sync::Mutex-guarded slot map first; is_closed()/ready() runs on
    /// the owned entry OUTSIDE the lock (never .await while holding it).
    pub async fn acquire(&self, peer: &HostName, port: u16, deadline: RequestDeadline)
        -> Result<PooledPeerConnection, HttpRuntimeClientFailure>;
}

/// RAII borrow guard. The caller performs the request THROUGH the guard —
/// it never gets the raw sender — so the guard, not the caller, records
/// health: no separate mark_failed() exists or is needed, because
/// `exchange` observes the I/O result itself and sets the guard's health
/// state before returning. Drop is sync and non-blocking: a healthy
/// **Pooled-origin** guard pushes its live sender back onto a
/// std::sync::Mutex-guarded slot map (no .await, no tokio::spawn in Drop);
/// a failed guard, one dropped without a completed exchange, or ANY
/// Overflow-origin guard (healthy or not — see counter discipline below)
/// just drops the sender, which closes the connection and ends its driver
/// task. Idle eviction runs on the pool's own timer task, never in Drop.
/// Set at acquire time, BEFORE any .await, under the slot-map mutex:
/// Pooled = a slot was reserved (per-peer and total counters incremented
/// pre-dial, so a concurrent acquirer can never over-commit the ceiling);
/// Overflow = capacity was already full, this dial is unpooled.
pub enum ConnectionOrigin { Pooled, Overflow }

pub struct PooledPeerConnection {
    peer: HostName,
    origin: ConnectionOrigin,
    // Real types only — the same ones execute_opaque_peer_request drives
    // today: axum::body::Body request bodies over the http1 sender, and
    // axum::http::Response<Vec<u8>> back out. No new wrapper types.
    sender: hyper::client::conn::http1::SendRequest<axum::body::Body>,
    driver: tokio::task::JoinHandle<()>,
    health: GuardHealth,          // Healthy only after a successful exchange
    pool: Weak<PoolShared>,
}

// Counter discipline (all under the slot-map mutex, never across .await).
// The reservation is PER-CONNECTION-LIFETIME, not per-borrow:
// - Pooled origin: the counter increments exactly once, at reservation
//   (pre-dial). Re-acquiring an idle entry from the slot map touches NO
//   counter — the connection still holds its original reservation. Drop
//   decrements ONLY when the sender is actually closed/discarded (failed
//   exchange, incomplete exchange, idle eviction, pool teardown); a
//   healthy return-to-pool never decrements. A failed dial releases its
//   reservation before surfacing the error. Invariant: across a pooled
//   connection's full lifetime, total increments == total decrements ==
//   1, regardless of borrow-cycle count. The one transparent redial
//   replaces the connection UNDER THE SAME reservation — no counter
//   change.
// - Overflow origin: counters are never touched, and Drop NEVER pushes an
//   overflow sender into the slot map — healthy or not, it is closed on
//   Drop. Overflow connections exist for exactly one exchange, preserving
//   AC #5's ceiling invariant even when slots free mid-flight.
//
// Pool teardown: dropping/shutting down PeerConnectionPool closes every
// pooled sender and drains their driver tasks (bounded await on the
// pool's own shutdown path — never inside a Drop impl of the guard),
// so daemon shutdown/reconfig leaks no tasks (AC #9).

impl PooledPeerConnection {
    /// One request/response cycle over the pooled sender, preserving the
    /// HttpRuntimeClientFailure taxonomy. Takes the same request type the
    /// connector layer consumes today (atm_core::api::HttpRequest) and the
    /// caller's RequestDeadline; the send is bounded by
    /// tokio::time::timeout(deadline.remaining(), ...) producing
    /// HttpRuntimeClientFailure::Timeout, exactly as
    /// execute_opaque_peer_request does now. Success ⇒ guard Healthy
    /// (returned to pool at Drop). Failure ⇒ guard Failed (closed at
    /// Drop), error surfaced unchanged.
    pub async fn exchange(&mut self, request: HttpRequest, deadline: RequestDeadline)
        -> Result<axum::http::Response<Vec<u8>>, HttpRuntimeClientFailure>;
}

// Redial-safety invariant (durable-write double-send safety):
// staleness is detected ONLY at acquire time, pre-request — acquire()
// checks the pooled sender's liveness via SendRequest::is_closed() /
// ready() and, if dead, discards it and dials fresh (this replacement
// dial is the "one transparent redial"; if it also fails, ITS failure is
// surfaced using exactly today's PeerConnect/PeerConnectTimeout variant
// and message shape). Once a request has been handed to `exchange`, NO
// failure is ever retried — not RequestWrite, not ResponseDecode — even
// though a kept-alive race means such an error can occur when a peer
// died after acquire. That residual failure surfaces unchanged, exactly
// as today's one-shot path; the delivery-attempt count for a request
// that may have reached the peer remains exactly one. This invariant is
// recorded in the new ADR (deliverable 7).
```

Semantics fixed by this contract: dispatch never blocks on pool exhaustion
(falls back to a one-shot dial); TLS session-ticket resumption comes free
from continuing to reuse the per-peer `Arc<rustls::ClientConfig>` already
built in `MtlsPeerStreamAdapter::from_peer_config` — connection pooling is
layered on top of, and never replaces, that config reuse.

## Acceptance criteria

1. Reuse proof: a test dispatching N≥3 writes to one peer observes exactly
   one underlying connect/handshake — mTLS path counted via a test
   `PeerStreamAdapter` (the existing seam); plaintext path counted via a
   local TCP listener spy asserting exactly one accepted connection
   (reqwest exposes no adapter seam, so accepted-connection counting is
   the named mechanism).
2. Isolation proof: writes to two different peers never share a connection
   (test with two test-adapter peers).
3. Idle eviction: a pooled connection past `idle_timeout` is closed and the
   next dispatch redials (deterministic clock or tokio time-pause, per
   ADR-008 no-flaky-tests).
4. Reconnect: a peer that drops its socket between dispatches is caught at
   acquire time (sender liveness check), causing exactly one transparent
   redial and a successful write; a peer that is down surfaces the same
   `PeerConnect`/`PeerConnectTimeout` variants and messages as today
   (existing tests `direct_peer_connection_failure_names_the_remote_authority`
   and `direct_peer_deadline_is_reported_as_a_connect_failure` pass
   unmodified). A companion test kills the peer AFTER acquire, mid-request:
   the failure surfaces unchanged and NO retry occurs (delivery-attempt
   count stays one — asserted via the test adapter's dial count).
5. Bounds: `max_per_peer` exceeded → unpooled one-shot dial, never queuing
   (single-peer concurrency test); `max_pooled_total` exceeded across
   multiple peers → dispatches still succeed via unpooled dials while the
   pool's retained-entry count never exceeds the ceiling (multi-peer test).
   Includes the overflow-Drop case: an Overflow-origin connection whose
   exchange succeeds is closed at Drop and does NOT enter the slot map,
   even when a slot has freed in the meantime (test frees a slot mid-flight
   and asserts the retained count and the closed overflow connection).
6. All three existing router dispatch tests and
   `authenticated_peer_stream_uses_the_same_canonical_router_after_the_adapter`
   pass unmodified.
7. `cargo test` workspace + boundary-enforcement suite green on macOS and
   Windows CI lanes; any boundary-test diff is called out in the PR
   description as a deliberate change.
8. Config plumbing: unit tests for the `PeerPoolConfig` CLI/env parsing
   (valid values, rejection of zero/negative/garbage for `max_per_peer`,
   `max_pooled_total`, `idle_timeout`, and defaults applied when unset),
   mirroring how `parse_direct_peer_port` is tested.
9. Driver-task completion (no leaks): tests observe the connection-driver
   task actually complete — via a completion signal (e.g. the driver task
   resolving a oneshot/notify observed with a bounded `tokio::time::timeout`,
   never an unbounded join) — for each of: idle eviction, Drop after a
   failed exchange, Drop with no completed exchange, overflow-Drop after a
   successful exchange, overflow-Drop after a FAILED exchange (distinct
   branch per the never-pushes rule), and pool teardown with healthy
   still-pooled connections (daemon shutdown/reconfig drains every driver
   task). A counter-lifetime test runs N borrow/return cycles on one
   pooled connection and asserts the retained-count and reservation
   counters are unchanged (no per-borrow drift), then closes it and
   asserts exactly one decrement.
10. ADR delivery gate: the ADR file from deliverable 7 exists, is listed
   in `docs/adr/INDEX.md`, and contains the redial/delivery-attempt
   invariant, the per-connection-lifetime counter rule, and the teardown
   drain semantics (grep-gated on those phrases — a stub ADR fails).
11. **Benchmark gate (standing no-regression constraint + D1/D3)**: live
   `just benchmark` on rand-m5 before/after on the same revision pair; tcp
   and tcp-tls f8 p50 must not regress below `baselines.json` floors, and
   the after-run is expected to improve tcp/tcp-tls p50 (pooling removes
   per-write connect+handshake). Results published through the AO2.10–13
   pipeline (v4 campaign JSON, phase report, wyvern review step). Any
   baseline ratchet increase that follows is a separate quality-mgr-approved
   `baselines.json` revision per D3 — not part of this sprint's merge gate.

## Required validation

- Unit/integration tests above; full workspace test + clippy; both CI lanes.
- Live-verify gate before quality-mgr dispatch: the m5 before/after
  benchmark pair from AC #11, plus one real cross-host mTLS dispatch burst
  demonstrating reuse in daemon logs.
- Reviewers: standard set plus `boundary-guard` (deliverable 8) and
  `rust-service-hardening` lens (pool lifecycle, timeouts, backpressure —
  in particular validating acquire-time liveness via
  `SendRequest::is_closed()`/`ready()` per the redial-safety invariant).

## Non-closure / out of scope

- Inbound/accept-side connection reuse (`accept_with_peer`) — outbound
  dispatch only.
- HTTP/2 multiplexing, request pipelining, or replacing hyper http1.
- Retry policy beyond the single transparent redial of a stale pooled
  connection — the one-attempt failure surface is intentionally preserved.
- Any change to the legacy synchronous daemon (Phase-AM deletion target).
- Baseline value changes (D3 flow, separate).

## Plan QA history

| Round | Reviewer | Commit | Result | Disposition |
|-------|----------|--------|--------|-------------|
| 1 | plan-scope-reviewer (sonnet) | `ed23eba5` | FAIL — 2 Important (no `PooledPeerConnection` signature; config CLI/env plumbing had no AC), 1 minor (defaults hedged "proposed") | Fixed in round-1 fix commit: full guard signature with through-the-guard `exchange`; AC #8 added; defaults fixed. |
| 1 | critical-plan-reviewer (sonnet) | `ed23eba5` | FAIL — 3 Blocking (pool stored raw streams, incompatible with hyper http1 keep-alive and AC #1; nonexistent `OpaquePeerStream` trait name; redial-on-write-failure risked durable-write double-send), 4 Important (`max_total` semantics contradiction; no plaintext test-adapter seam; guard health-signaling undefined; missing ADR), 2 minor | Fixed in round-1 fix commit: pool stores post-handshake `SendRequest` + driver handle keyed off `EstablishedPeerStream`; real trait names; redial-safety invariant = acquire-time liveness only, no post-`exchange` retry ever, redial's own failure surfaced with today's variant shape; `max_pooled_total` defined as pooled-entry ceiling with unbounded-fallback risk stated + multi-peer AC; plaintext reuse proven via TCP listener spy (AC #1); health recorded by `exchange` itself, sync Drop; new ADR deliverable 7; reviewer pointer to `SendRequest::is_closed()/ready()`. |
| 2 | plan-scope-reviewer (sonnet) | `82252f3b` | FAIL — round-1 closures confirmed; 1 Important (live-verify gate cited stale AC #8 after renumbering), 1 minor (ADR lacked path/number convention) | Fixed in round-2 commit: gate re-pointed to AC #9; ADR path + INDEX.md convention stated. |
| 2 | critical-plan-reviewer (sonnet) | `82252f3b` | FAIL — round-1 closures verified against real code; 2 new Blocking (contract used undefined types `RuntimeBody`/`HttpRuntimeRequest`/`HttpRuntimeResponse`; no `RequestDeadline` threaded through `acquire`/`exchange`, silently dropping the per-write timeout contract deliverable 6 claims unchanged), 1 minor (liveness check vs mutex-across-await) | Fixed in round-2 commit: real types (`axum::body::Body`, `atm_core::api::HttpRequest`, `axum::http::Response<Vec<u8>>`); `RequestDeadline` threaded through both `acquire` and `exchange` with today's Timeout/PeerConnectTimeout trigger conditions stated; pop-entry-then-check-outside-the-lock rule added. |
| 3 | both reviewers (sonnet) | `21241a01` | **PASS** — all round-2 closures verified against real code (types, deadline semantics incl. RequestDeadline being Copy over a fixed Instant, lock discipline); zero findings | Hardening complete; ready for quality-mgr gate. |
| 5 | quality-mgr gate round 2 (PR #1018) | `1be4a46b`+r4 fixes | FAIL — 1 Blocking (counter discipline contradictory: "Drop decrements on BOTH paths" vs one-time reservation — reuse cycles would monotonically drain the counter), 3 Important (pool-teardown driver drain untested; ADR deliverable had no AC; overflow-Drop-after-failure branch missing from AC #9), 3 minor | Fixed in round-5 commit: reservation defined as per-connection-lifetime (reuse-acquire touches no counter; Drop decrements only on actual close/discard; increments == decrements == 1 per lifetime; redial replaces under the same reservation); teardown drain semantics in contract + AC #9; new AC #10 ADR non-stub grep gate (benchmark gate → #11); AC #9 gains overflow-Drop-after-failure and N-cycle counter-drift test; DirectPeerTcpConnector signature spelled out; deliverables 2+4 declared one coordinated bootstrap edit. |
| 4 | quality-mgr gate (PR #1018) | `1be4a46b` | FAIL — 1 Blocking (AC #5 unimplementable: no Pooled/Overflow origin distinction, so a healthy overflow dial would re-enter the slot map on Drop and breach the ceiling), 2 Important (deliverable-1 "no task leaks; asserted in tests" had no observing AC — sync Drop can't join; deliverable 2 lacked a composition-root wiring contract) | Fixed in round-4 commit: `ConnectionOrigin { Pooled, Overflow }` set pre-await under the mutex with explicit counter increment (pre-dial reservation) / decrement (both Drop paths) discipline and overflow-never-re-enters rule + mid-flight slot-free test in AC #5; new AC #9 (driver-task completion via bounded-timeout signal for eviction/failed/incomplete/overflow Drop paths, benchmark gate renumbered #10); `with_shared_direct_peer_client` builder + bootstrap injection point for the shared reqwest::Client. Discarded false positive (nonexistent-seam claim from a plan-doc-only worktree) noted, no action. |

## Dependencies

- must_follow: AO2.11 / AO2.12 / AO2.13 PRs (#1013/#1014/#1015) merged to
  `integrate/phase-ao2`, and the F002–F008 ledger Resolutions recorded —
  this sprint's benchmark gate publishes through the pipeline those sprints
  deliver. PR-completion trigger.
- parallel_safe: none declared — the touch set (router, client, bootstrap
  composition root) is the runtime hot path; no other AO2 sprint may touch
  it concurrently.
