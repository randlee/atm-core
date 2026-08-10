---
title: AO.2 — Activate the optional mTLS module on the canonical runtime
status: planned
recommended_agent: arch-ctm
---

# AO.2 — Activate the optional mTLS module on the canonical runtime

## Scope

Enable the AO.1 module on the active Tokio/Axum runtime. The module, not the
runtime, uses existing `HttpsInterface`, `LocalCertificate`, and `TrustedPeer`
records through the storage traits. The canonical runtime selects a transport
and delegates; it must not gain TLS implementation, database, or business
logic.

The required shape is equivalent to:

```rust
match peer.transport_mode() {
    PeerTransportMode::PlainDirect => plain.process(request).await,
    PeerTransportMode::MutualTls => tls.process(request, peer, deadline).await,
}
```

Incoming TLS ownership is likewise delegated to the module (`tls.start(...)`)
with its module-owned storage/business composition already constructed. There
is one canonical request shape; TLS wraps the connection only.

## Dependencies

- **must_follow:** AO.1 development must be pushed before AO.2 begins. Merge
  AO.1 into the AO.2 branch before every AO.2 development/fix round so the
  opaque module API and its guards are current.
- **parallel_safe:** none. This sprint consumes the module's public boundary
  and owns all active call sites.
- **unblocks:** AO.3.

## Deliverables

1. One exhaustive persisted/configured transport choice equivalent to
   `PlainDirect | MutualTls`; selection is explicit and is never inferred from
   a port, IP address, or hostname suffix. Successful key
   exchange/provisioning selects `MutualTls` as the normal route. `PlainDirect`
   remains only an explicit, observable test/benchmark/debug override.
2. A single inbound activation call to the TLS module and a single outbound
   `MutualTls` delegation call. TLS code in the active runtime is limited to
   selection, delegation, lifecycle handle storage, and typed error
   propagation; it does not read TLS certificate, interface, or trusted-peer
   state from storage and it does not invoke message, roster, or nudge storage
   on the TLS branch.
3. Fail-closed activation: incomplete certificate/interface/peer setup fails
   before bind/publication; TLS connection, DNS, certificate, pin, or
   handshake failure never attempts plaintext.
4. Lifecycle/doctor wiring through the module handle, without exposing keys or
   creating a second endpoint owner.

## Acceptance criteria

- A peer without successful TLS key exchange/provisioning retains its existing
  plaintext behavior; a provisioned peer uses mTLS by default.
- Once the module reports successful key exchange/provisioning for a peer, the
  normal route is `MutualTls`; a plaintext route requires an explicit
  diagnostic override and is recorded in doctor/report output.
- A `MutualTls` peer reaches the same canonical router exactly once after
  successful authentication.
- Invalid or disabled TLS configuration and every rejected TLS handshake reach
  neither body decoding, router, storage, nudge, nor post-write hook.
- Review can verify that active-runtime changes contain only transport
  selection/delegation—not Rustls, PEM, certificate fingerprint, storage trait
  use, TLS business logic, or TLS-listener logic. Composition creates the
  module once; all TLS storage ports remain private to that module.

## Required validation

- Focused active-runtime tests for plaintext selection, mTLS selection, and
  no-downgrade behavior, including the secure default after key exchange and
  the explicit diagnostic override.
- Integration tests that compare plaintext and TLS canonical request handling;
  assert rejected TLS reaches no application-side mock.
- Lifecycle tests for failed activation, start/stop, and one listener/endpoint
  owner.
- Static source/dependency check for the containment rule and frozen/fixture
  boundary.
- `just lint` and `just test`.

## Non-closure

AO.2 does not add certificate issuance/rotation or solve Windows/corporate
reachability. It does not change message delivery semantics, queues, nudge,
replay, or retry behavior.
