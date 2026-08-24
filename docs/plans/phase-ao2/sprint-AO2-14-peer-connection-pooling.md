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
   ends its driver task (no task leaks; asserted in tests).
2. **Plaintext client reuse**: one shared daemon-lifetime `reqwest::Client`
   (its built-in pool then engages) replacing per-dispatch construction in
   `DirectPeerTcpConnector`. No custom pool needed on this path.
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
7. **New ADR — peer-write redial and delivery-attempt invariant**:
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
    /// durable write behind a pool slot).
    pub async fn acquire(&self, peer: &HostName, port: u16)
        -> Result<PooledPeerConnection, HttpRuntimeClientFailure>;
}

/// RAII borrow guard. The caller performs the request THROUGH the guard —
/// it never gets the raw sender — so the guard, not the caller, records
/// health: no separate mark_failed() exists or is needed, because
/// `exchange` observes the I/O result itself and sets the guard's health
/// state before returning. Drop is sync and non-blocking: a healthy guard
/// pushes its live sender back onto a std::sync::Mutex-guarded slot map
/// (no .await, no tokio::spawn in Drop); a failed guard — or one dropped
/// without a completed exchange — just drops the sender, which closes the
/// connection and ends its driver task. Idle eviction runs on the pool's
/// own timer task, never in Drop.
pub struct PooledPeerConnection {
    peer: HostName,
    sender: hyper::client::conn::http1::SendRequest<RuntimeBody>,
    driver: tokio::task::JoinHandle<()>,
    health: GuardHealth,          // Healthy only after a successful exchange
    pool: Weak<PoolShared>,
}

impl PooledPeerConnection {
    /// One request/response cycle over the pooled sender, preserving the
    /// HttpRuntimeClientFailure taxonomy. Success ⇒ guard Healthy
    /// (returned to pool at Drop). Failure ⇒ guard Failed (closed at
    /// Drop), error surfaced unchanged.
    pub async fn exchange(&mut self, request: HttpRuntimeRequest)
        -> Result<HttpRuntimeResponse, HttpRuntimeClientFailure>;
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
9. **Benchmark gate (standing no-regression constraint + D1/D3)**: live
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
  benchmark pair from AC #8, plus one real cross-host mTLS dispatch burst
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

## Dependencies

- must_follow: AO2.11 / AO2.12 / AO2.13 PRs (#1013/#1014/#1015) merged to
  `integrate/phase-ao2`, and the F002–F008 ledger Resolutions recorded —
  this sprint's benchmark gate publishes through the pipeline those sprints
  deliver. PR-completion trigger.
- parallel_safe: none declared — the touch set (router, client, bootstrap
  composition root) is the runtime hot path; no other AO2 sprint may touch
  it concurrently.
