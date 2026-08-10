---
title: AO.1 — Re-home the existing mTLS peer module
status: planned
recommended_agent: arch-ctm
---

# AO.1 — Re-home the existing mTLS peer module

## Scope

Move the already implemented mTLS transport behavior from frozen reference
material into one production-contained crate, planned as `crates/atm-peer-tls`.
This is a preservation/refactoring sprint, not a new TLS implementation.

The source inventory begins with
`crates/atm-daemon/src/https_transport.rs`; it already contains PEM/key
loading, Rustls client/server construction, exact fingerprint pinning,
client-certificate verification, TLS HTTP framing, trusted-peer refresh, and
bounded shutdown.

## Dependencies

- **must_follow:** accepted Tokio/Axum-only runtime baseline. Parent
  development must be pushed before AO.1 begins; merge the parent into the
  AO.1 branch before every development/fix round. AO.1 must not use a frozen
  `atm-daemon` runtime as its base or test target.
- **parallel_safe:** none. The module boundary is public to AO.2 and must be
  settled before runtime activation starts.
- **unblocks:** AO.2.

## Deliverables

1. An `atm-peer-tls` crate containing the archived mTLS transport behavior and
   no dependency on `atm-daemon`. It owns all TLS-side database/control-plane
   work through the existing storage traits: certificate/key reference lookup,
   `HttpsInterface` and `TrustedPeer` lookup/refresh, authentication,
   provenance construction, canonical TLS-side request processing, and all
   required message/roster/nudge storage calls.
2. A narrow, explicit module API that keeps TLS state private. The active
   runtime receives an opaque module handle, not certificate material, Rustls
   configuration, storage traits, or a TLS business-logic capability. The
   composition root supplies existing sealed ports once to the module; no new
   public extension trait is introduced:

   ```rust
   pub struct TlsStoragePorts { /* private existing sealed storage ports */ }
   pub struct TlsPeerTransport { /* private TLS + storage + business state */ }

   impl TlsPeerTransport {
       pub async fn start(ports: TlsStoragePorts) -> Result<Self, AtmError>;
       pub async fn process(&self, request: WriteRequest, peer: HostName,
           deadline: RequestDeadline) -> Result<ResponseEnvelope, AtmError>;
       pub async fn shutdown(self) -> Result<(), AtmError>;
   }
   ```

   `TlsStoragePorts` is constructible only at the approved composition root and
   is never retained by or exposed to the active HTTP runtime. Exact names may
   vary, but ownership and opacity may not.
3. Preserved/adapted unit and integration tests for PEM/key pairing,
   fingerprint mismatch, hostname/SNI and pin mismatch, missing/rejected
   client certificate, disabled/unknown peer, canonical HTTP round-trip, and
   bounded shutdown.
4. Architecture guard tests proving no production dependency edge to
   `atm-daemon` or `atm-peer-tls-interop`, and a source guard proving the
   active Tokio/Axum runtime contains neither TLS business/storage logic nor
   Rustls/certificate implementation.
5. One module-owned, explicit route decision: after successful key
   exchange/provisioning, normal peer delivery selects mTLS. The module also
   exposes a deliberately named diagnostic override for plaintext test,
   benchmark, and debugging runs. The override is observable in status/report
   metadata and is never selected automatically after a TLS failure.

## Acceptance criteria

- The new crate implements the archive's current security behavior without a
  second TLS implementation or changed trust rule.
- Certificate/key material, peer/interface records, and TLS business behavior
  are read and retained only inside the module through its storage ports; key
  bytes cannot appear in serialized diagnostics, logs, or errors.
- The active HTTP runtime has neither a `PeerConfigStore` nor message/roster/
  nudge storage call in its TLS branch; it has only the opaque module handle.
- Key exchange/provisioning completion makes mTLS the default peer route;
  plaintext requires an explicit diagnostic mode and cannot be a fallback.
- The crate exposes no open extension point that lets other crates bypass its
  verification before dispatch.
- The frozen legacy daemon is untouched.

## Required validation

- Focused `cargo test -p atm-peer-tls` including all delivered negative cases.
- `cargo test -p atm-peer-tls-interop` to ensure fixture compatibility remains
  intact.
- Dependency/architecture check demonstrating no production edge from
  `atm-peer-tls` to either frozen or fixture crate, and that no TLS business
  logic/storage call exists in the active Tokio/Axum daemon path.
- `just lint` and `just test` on the accepted runtime baseline.

## Non-closure

AO.1 does not activate TLS on any live listener or direct peer connector.
It does not modify the canonical HTTP runtime except for any compile-time
crate registration required to build the isolated module.
