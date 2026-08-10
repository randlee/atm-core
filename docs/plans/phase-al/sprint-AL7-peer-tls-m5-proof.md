---
title: AL.7 Authenticated Peer TLS Adapter and M5 Proof
status: abandoned
branch: feature/pal-s7-peer-tls-m5-proof
worktree: ../atm-core-worktrees/feature/pal-s7-peer-tls-m5-proof
target: integrate/phase-al
---

# AL.7 — Authenticated Peer TLS Adapter and M5 Proof

## Status: abandoned

AL.7's mTLS/TLS-adapter scope was explicitly removed from the Phase AL MVP
before implementation. The retained `atm-peer-tls-interop` material remains
quarantined reference only; it is neither linked into the active Tokio/Axum
runtime nor evidence for direct-peer delivery. AL.9/AL.13-AL.15 instead prove
the selected direct-peer HTTP path. Reopening authenticated peer TLS requires
a separately approved phase and plan.

**recommended_agent:** arch-ctm/deep-reasoning; M5 team executes the remote
clean-checkout proof.
**must_follow:** AL.2, AL.4, AL.5, AL.6, and accepted existing TLS/trust
policy. Merge the pushed integration commits before each development/fix round;
their PR merges are not required. AL.7 is the final physical-adapter round, so
it also owns retiring AL.4's temporary synchronous local-client compatibility
path before AL.8 activates the replacement runtime.
**unblocks:** AL.8.
**parallel_safe:** none. It owns TLS connection/authentication setup and the
final local-client retirement gate, which requires the accepted AL.5 and AL.6
adapters.

**traceability:** `REQ-CORE-TRANSPORT-001/002/002A/002B/002B1/002C/004/005`,
`REQ-DAEMON-TRANSPORT-001/003/005`, ADR-033, ADR-040 (proposed input only),
ADR-041.

## Deliverables

1. Configure the existing durable trust/authority view and Rustls mTLS
   connector/listener around the shared AL.4 client and AL.2 router. The
   adapter contributes authenticated peer provenance only after certificate
   and exact allowlist verification.
2. Use the existing hostname/port/pin policy without persisting observed IPs,
   adding peer delivery state, or treating socket address as identity.
3. Keep the explicit plaintext-test profile process-local and untrusted. It
   reaches the same handler only for smoke diagnosis; it cannot authorize a
   recipient or count as mTLS proof.
4. Provide the M5 team a pinned commit, clean-checkout command, fixture setup,
   expected unchanged JSON artifact, and machine-readable pass/fail artifact.
   This artifact is the single M5 proof for AL.7 and AL.9 unless a later
   change alters the route, client, TLS configuration, or composition binary.
5. Remove AL.4's temporary synchronous local-client compatibility path once
   AL.5, AL.6, and this sprint's TLS connector are available. This is a
   mandatory AL.7 closure item, not a TODO for AM: migrate the CLI and graft
   call sites to `HttpRuntimeClient<Connector>` through `DaemonApiClient`, then
   delete `atm_daemon_client::exchange_request`,
   `atm_daemon_client::try_connect`, and the compatibility preflight/dispatch
   wrapper that depends on them. The pre-removal inventory is deliberately
   small and exhaustive:

   - `crates/atm/src/composition.rs`: runtime reload, probe, and the retained
     `LocalIpcClientTransportAdapter`;
   - `crates/atm-graft/src/transport.rs`: probe and
     `GraftLocalIpcClientTransport`;
   - `crates/atm-daemon-client/src/compatibility.rs`: the wrapper's internal
     dispatch and preflight calls; and
   - `crates/atm-daemon-client/src/lib.rs`: the two public synchronous
     function definitions and their private transport helpers.

   AL.7's PR must carry the replacement call graph and deletion together.
   It may not leave a source-level compatibility shim, a `TODO`, or an
   allowlist for either symbol. This retires only the legacy local-client
   operations; AM remains responsible for deleting now-unreferenced legacy
   source and its boundary records after AL.9 freezes the live-reference
   ledger.

## Acceptance criteria

- One direct M5 cross-host write uses the exact existing `POST /messages`
  body/result and the same `ApiRouter`/storage/hook call path as local traffic.
- No peer listener/client/decoder/router, `PeerMessageArray`, peer header
  application protocol, retry, replay, queue, or coordinator is introduced.
- An untrusted/mismatched peer cannot reach application dispatch; a direct
  connection/DNS/TLS failure returns the normal typed direct-send outcome and
  starts no background work.
- No source import may name `atm-peer-tls-interop` or
  `atm-storage/src/tls.rs`; both remain quarantined reference material.
- Before AL.7 can close, a production-source search for
  `atm_daemon_client::{exchange_request, try_connect}` and their aliased
  imports/call sites is empty, and the CLI, graft, compatibility preflight,
  and runtime-reload paths all await the one `DaemonApiClient` operation.
  The AL.7 tests must cover each of the four inventory groups above so a
  future direct synchronous call cannot be reintroduced silently.

## Required validation

- mTLS allowlist positive and negative integration tests
- plaintext-test isolation test
- direct failure test with task-accounting assertion
- M5 clean-checkout cross-host proof, including artifact and SHA

## Non-closure

This is direct-send proof only. It creates no replay/reconciliation mechanism
and does not delete legacy peer server/listener code; it does retire the
explicitly listed synchronous local-client operations as required for AL.8's
sole-runtime cutover.
