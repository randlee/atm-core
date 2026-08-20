---
title: AO.2 — Isolated mTLS Stream Adapter
status: planned
branch: feature/pao-s2-isolated-mtls-stream-adapter
target: integrate/phase-ao2
worktree: ../atm-core-worktrees/feature/pao-s2-isolated-mtls-stream-adapter
external_blockers: []
---

# AO.2 — Isolated mTLS Stream Adapter

**recommended_agent:** arch-ctm/deep-reasoning.
**must_follow:** AO.1 development pushed. Merge AO.1's integration tip before
every AO.2 development or fix round; AO.1 PR must merge before AO.2 PR
completion.
**parallel_safe:** none. AO.2 owns the sealed stream contract and the sole
new authorized TLS consumer.
**unblocks:** AO.3.

**traceability:** ADR-047; ADR-034; `REQ-CORE-TRANSPORT-002B`;
`boundaries/atm-storage/tls.toml`; `boundaries/atm-storage/peer-config-store.toml`.

## Goal

Provide the only concrete mTLS stream wrapper without allowing TLS policy or
application behavior to leak into the HTTP runtime.

## Scope Summary

One bounded `peer-tls` crate, its concrete facade, exact dependency/boundary
records, and stream-level evidence. No daemon wiring or HTTP behavior changes.

## Governing Requirements

`REQ-CORE-TRANSPORT-002B` and the accepted AO.1 reconciliation of
`REQ-CORE-TRANSPORT-002B1`.

## Governing ADRs

ADR-034 and ADR-047.

## Governing Boundaries

`boundaries/atm-storage/tls.toml`,
`boundaries/atm-storage/peer-config-store.toml`, and new `peer-tls` boundary
records introduced in this sprint.

## Prerequisites

AO.1's ADR/requirement and typed-error changes are merged forward into this
worktree before every development or fix round.

## Hard Dependencies

AO.1 development pushed; AO.1 PR merged before AO.2 PR completion.

## Deliverables

1. Add the narrow production `peer-tls` crate. It is the only concrete
   Rustls/Tokio-Rustls owner and uses the canonical `atm_storage::tls`
   parsing/fingerprint/pin helpers plus `PeerConfigStore` only after the
   selected mTLS mode is composed.
2. Define one concrete adapter facade in `peer-tls`; do **not** add a public
   transport trait or trait-object registry. Its input/output are byte streams,
   never ATM messages or HTTP DTOs. The facade is equivalent to:

   ```rust
   pub struct MtlsPeerStreamAdapter { /* private TLS/config state */ }

   impl MtlsPeerStreamAdapter {
       pub async fn connect(
           &self,
           tcp: TcpStream,
           peer: &PeerAuthority,
       ) -> Result<impl AsyncRead + AsyncWrite + Send + Unpin, AtmError>;

       pub async fn accept(
           &self,
           tcp: TcpStream,
       ) -> Result<impl AsyncRead + AsyncWrite + Send + Unpin, AtmError>;
   }
   ```

   `atm-http-runtime` receives the result only through private generic
   HTTP-over-stream helpers (`S: AsyncRead + AsyncWrite + Send + Unpin`), so it
   neither names a Rustls type nor owns a dynamic extension point. This avoids
   a non-object-safe `async` transport trait and keeps the implementer set
   concrete and finite.
3. Amend the `TlsHelpers` and `PeerConfigStore` boundary records, manifests,
   and architecture tests to authorize exactly this dependency direction. No
   other crate may import Rustls, Tokio-Rustls, `PeerConfigStore`, or certificate
   policy for transport use.
4. Add focused positive mutual-auth byte-stream tests and negatives for
   missing/disabled interface, missing identity, bad key, expired/untrusted
   client certificate, hostname mismatch, pin mismatch, deadline, and
   handshake failure. Each negative proves failure before Hyper connection,
   HTTP decode, router, persistence, or hook.

## Acceptance Criteria

- A valid configured pair exchanges opaque bytes through mTLS.
- Every invalid trust/identity case returns a typed, non-secret error before
  application processing and never falls back to plaintext.
- `atm-http-runtime`, CLI, graft, and bootstrap do not import concrete TLS or
  certificate-policy APIs.
- No AO.2 production code is reachable from `plaintext-test` mode.

## Required Validation

- `cargo test -p peer-tls -p agent-team-mail-core -p atm-storage`
- `cargo test -p atm-architecture --test boundary_enforcement`
- `just lint`
- `just test`

## Required Document Updates

- `Cargo.toml`/lockfile, the three affected boundary manifests, storage
  boundary prose, ADR-047 implementation notes, and typed-error documentation.

## Split Recommendation

Do not split: the concrete stream facade and its dependency guards must land
atomically so no ungoverned Rustls consumer is temporarily authorized.

## Error Inventory

| Failure mode | Stable code ownership | Required recovery |
| --- | --- | --- |
| Disabled/missing interface or local identity | Reuse `ATM_PEER_CONFIG_VALIDATION_FAILED` when its semantics fit; otherwise AO.2 adds a specific central code. | Enable the registered interface and install a valid local identity, then restart in mTLS mode. |
| Invalid key/certificate or untrusted/expired client certificate | Reuse `ATM_CERTIFICATE_OPERATION_FAILED` only if its documented recovery remains precise; otherwise add a distinct central code. | Replace the invalid certificate/trust record and retry after daemon reload/restart. |
| Hostname or pin mismatch | AO.2 adds or reserves one fail-closed central peer-authentication code. | Correct the registered hostname/pin; do not use plaintext-test as a recovery. |
| Handshake or deadline failure | Reuse documented transport timeout/protocol code only if present in the registry, otherwise register the exact code. | Check reachability and peer TLS configuration; retry only when the code's recoverability permits it. |

Errors preserve a typed cause, must not expose private key/certificate bytes,
and must be asserted by stable code in both unit and integration tests.

## Paths To Delete

None. `atm-peer-tls-interop` remains a quarantined fixture and is not reused.

## Non-Goals

AO.2 does not alter daemon bootstrap, `HttpRuntime`, the direct connector,
the direct listener, benchmark runners, or any application route.

## Risks And Watchouts

The facade must not become an accidental plugin surface. It is a concrete,
finite adapter; generic HTTP-over-stream handling remains private to
`atm-http-runtime` and all certificate/key errors remain non-secret.
