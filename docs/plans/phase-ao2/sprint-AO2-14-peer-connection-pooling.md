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
   crate must continue to never import rustls/TLS; it pools
   `Box<dyn OpaquePeerStream>`-level connections produced by the existing
   adapter seam): daemon-lifetime, bounded per-peer, idle-evicting,
   reconnect-on-failure. Keyed by the configured peer `HostName` authority —
   exactly matching `MtlsPeerStreamAdapter::client_config_for`'s
   exact-authority lookup — never by resolved IP; a pooled connection is
   never reused across peers.
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
5. **Config surface**: new `PeerPoolConfig { max_per_peer, max_total,
   idle_timeout }` in `HttpRuntimeConfig` with conservative defaults
   (proposed: 4 per peer, 32 total, 60s idle) and CLI/env plumbing analogous
   to `parse_direct_peer_port`.
6. **Error-taxonomy preservation**: the `HttpRuntimeClientFailure` variants
   (`EndpointRecord`, `Connect`, `PeerConnect`, `RequestWrite`,
   `ResponseDecode`, `Cancelled`, `Timeout`, `PeerConnectTimeout`) and their
   `into_atm_error` mapping are unchanged — existing tests assert on
   specific variants/messages; pooling adds reuse underneath, never new
   caller-visible failure shapes.
7. **Boundary-test alignment**: deliberate updates (if needed) to
   `crates/atm-architecture/tests/boundary_enforcement.rs` literal-scan
   tests (`ao2_plaintext_baseline_stays_on_the_existing_direct_peer_pipeline`,
   `ao2_peer_wire_policy_keeps_one_error_registry_and_one_http_pipeline`) so
   pool types don't trip them — reviewed as explicit diffs, never weakened
   silently. `boundary-guard` reviews this deliverable.

## Pool contract (normative)

```rust
pub struct PeerPoolConfig {
    pub max_per_peer: usize,     // default 4
    pub max_total: usize,        // default 32
    pub idle_timeout: Duration,  // default 60s; idle connections evicted
}

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

// Returned via RAII: Drop returns a healthy connection to the pool;
// a connection whose request errored is closed and discarded instead.
// One transparent redial: if a borrowed pooled connection fails on first
// write (stale socket), the pool dials fresh once before surfacing the
// existing PeerConnect/RequestWrite failure unchanged.
```

Semantics fixed by this contract: dispatch never blocks on pool exhaustion
(falls back to a one-shot dial); TLS session-ticket resumption comes free
from continuing to reuse the per-peer `Arc<rustls::ClientConfig>` already
built in `MtlsPeerStreamAdapter::from_peer_config` — connection pooling is
layered on top of, and never replaces, that config reuse.

## Acceptance criteria

1. Reuse proof: a test dispatching N≥3 writes to one peer observes exactly
   one underlying connect/handshake (counting via a test adapter), for both
   the mTLS pooled path and the shared plaintext client.
2. Isolation proof: writes to two different peers never share a connection
   (test with two test-adapter peers).
3. Idle eviction: a pooled connection past `idle_timeout` is closed and the
   next dispatch redials (deterministic clock or tokio time-pause, per
   ADR-008 no-flaky-tests).
4. Reconnect: a peer that drops its socket between dispatches causes exactly
   one transparent redial and the write succeeds; a peer that is down
   surfaces the same `PeerConnect`/`PeerConnectTimeout` variants and
   messages as today (existing tests
   `direct_peer_connection_failure_names_the_remote_authority` and
   `direct_peer_deadline_is_reported_as_a_connect_failure` pass unmodified).
5. Bounds: `max_per_peer` exceeded → unpooled one-shot dial, never queuing;
   asserted by a concurrency test.
6. All three existing router dispatch tests and
   `authenticated_peer_stream_uses_the_same_canonical_router_after_the_adapter`
   pass unmodified.
7. `cargo test` workspace + boundary-enforcement suite green on macOS and
   Windows CI lanes; any boundary-test diff is called out in the PR
   description as a deliberate change.
8. **Benchmark gate (standing no-regression constraint + D1/D3)**: live
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
- Reviewers: standard set plus `boundary-guard` (deliverable 7) and
  `rust-service-hardening` lens (pool lifecycle, timeouts, backpressure).

## Non-closure / out of scope

- Inbound/accept-side connection reuse (`accept_with_peer`) — outbound
  dispatch only.
- HTTP/2 multiplexing, request pipelining, or replacing hyper http1.
- Retry policy beyond the single transparent redial of a stale pooled
  connection — the one-attempt failure surface is intentionally preserved.
- Any change to the legacy synchronous daemon (Phase-AM deletion target).
- Baseline value changes (D3 flow, separate).

## Dependencies

- must_follow: AO2.11 / AO2.12 / AO2.13 PRs (#1013/#1014/#1015) merged to
  `integrate/phase-ao2`, and the F002–F008 ledger Resolutions recorded —
  this sprint's benchmark gate publishes through the pipeline those sprints
  deliver. PR-completion trigger.
- parallel_safe: none declared — the touch set (router, client, bootstrap
  composition root) is the runtime hot path; no other AO2 sprint may touch
  it concurrently.
