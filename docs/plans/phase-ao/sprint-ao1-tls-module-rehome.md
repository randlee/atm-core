---
title: AO.1 — Create the bounded peer-tls adapter
status: complete
recommended_agent: arch-ctm
branch: feature/pao-s1-tls-module-rehome
worktree: ../atm-core-worktrees/feature/pao-s1-tls-module-rehome
---

# AO.1 — Create the bounded peer-tls adapter

## Scope

Create the new production `peer-tls` crate around the already-landed
`PeerConfigStore` and `atm_storage::tls` helpers. It owns Tokio-Rustls client
and server configuration plus stream handshakes. It does not change key
exchange, configure a live listener, encode ATM HTTP requests, or copy the
inactive `atm-peer-tls-interop` fixture.

## Dependencies

- **must_follow:** accepted Tokio/Axum runtime baseline. AO.1 never builds or
  tests the frozen daemon.
- **parallel_safe:** none. AO.2 consumes its sealed adapter contract.
- **unblocks:** AO.2.

## Deliverables

1. A `peer-tls` crate that reads `Arc<dyn PeerConfigStore>` and uses
   `TlsIdentity`, `PinnedClientVerifier`, `certificate_fingerprint`, and
   `install_tls_provider` from `atm_storage::tls`; no duplicate certificate
   parser, fingerprint normalizer, or trust verifier is allowed.
2. A sealed, object-safe `PeerIoAdapter` port with `peer-tls` as its one
   authorized implementation. The adapter turns an inbound or outbound Tokio
   TCP stream into `BoxedPeerIo`; it owns every concrete Rustls type.
3. The explicit boundary-TOML and architecture-test transition listed in the
   Phase AO plan, including the separate active `peer-tls` record and retained
   fixture-only `atm-peer-tls-interop` record.
4. Typed, non-secret failures for missing/disabled interface, missing local
   certificate, invalid key/certificate, missing or wrong client certificate,
   hostname/pin mismatch, and deadline/handshake failure.
5. Focused Tokio tests for a valid mutual-TLS byte stream and every delivered
   negative case. The HTTP payload is opaque in this sprint.

## Acceptance criteria

- Given the existing configured certificate and trusted peers, two Tokio
  adapters complete mutual TLS and exchange opaque bytes.
- A mismatched/disabled/missing configuration or certificate fails before
  yielding `BoxedPeerIo`.
- Source/dependency guards prove only `peer-tls` imports concrete TLS APIs or
  reads `PeerConfigStore` for transport configuration.
- `peer-tls` has no dependency edge to `crates/atm-daemon` or
  `atm-peer-tls-interop`; the fixture remains test/proof-only.

## Required validation

- `cargo test -p peer-tls` with positive and negative in-memory/configuration
  fixtures.
- Architecture tests for every new allowed/forbidden edge and the ADR-001
  authorized sealed-trait implementation.
- `just lint` and `just test`.

## Non-closure

AO.1 does not install the adapter in `atm-http-runtime`, bind a production
listener, or claim cross-host delivery proof.
