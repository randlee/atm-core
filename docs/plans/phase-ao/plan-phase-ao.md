---
title: Phase AO Plan — Optional mTLS Module Activation
status: proposed
branch: plan/phase-ao-tls-and-ap-outbound-connectivity
worktree: ../atm-core-worktrees/plan/phase-ao-tls-and-ap-outbound-connectivity
target: develop
---

# Phase AO — Optional mTLS Module Activation

## Goal

Activate the already implemented, archived mTLS peer transport as an optional,
self-contained module on ATM's canonical Tokio/Axum peer path. This is **not**
a six-part reimplementation of TLS.

The active runtime owns only explicit transport selection and delegation:

```rust
match peer_transport_mode {
    PeerTransportMode::PlainDirect => plain.process(request).await,
    PeerTransportMode::MutualTls => tls.process(request).await,
}
```

All TLS-side behavior belongs inside the TLS module: database/control-plane
lookups through the storage traits, certificate/key loading, Rustls
configuration and handshake, certificate pinning, client-certificate
verification, TLS HTTP framing, authenticated request processing, and TLS
listener lifecycle. The canonical runtime must not copy any of that behavior
or acquire TLS state beyond an opaque module handle.

## Existing asset and boundary

`crates/atm-daemon/src/https_transport.rs` is frozen reference material. It
already implements the substantive TLS behavior: PEM/key loading and
fingerprint matching, Rustls client/server setup, mTLS allow-list validation,
pinning, request/response framing, trusted-peer refresh, and shutdown.

AO re-homes that behavior into a production-contained TLS crate (planned name:
`atm-peer-tls`) without changing the frozen legacy daemon. The module may call
the existing sealed storage traits; it is not required to be storage-free or
business-logic-free. The
fixture-only `atm-peer-tls-interop` crate remains quarantined and may not gain
a production dependency edge.

The persisted control-plane data already exists: `HttpsInterface`,
`LocalCertificate`, and `TrustedPeer`. AO consumes those records; it does not
invent a second certificate store, peer registry, application protocol,
router, delivery state, queue, retry loop, or nudge path.

## Entry gate

Implementation begins only after the accepted Tokio/Axum runtime is the sole
active daemon path and the implementation branch is based on that accepted
line. No AO work may patch, start, test, or restore `crates/atm-daemon`.

## Invariants

1. **One application path.** TLS and plaintext encode the same canonical HTTP
   request and reach the same router, storage write, post-receive hook, and
   reply semantics.
2. **TLS module containment.** The active HTTP runtime contains only explicit
   selection/delegation. It contains no Rustls setup, PEM parsing, fingerprint
   comparison, TLS handshake, TLS-specific listener implementation, TLS
   storage lookup, or TLS-side business logic.
3. **Fail closed.** A `MutualTls` peer requires an enabled interface, local
   certificate reference, enabled exact trusted peer, hostname/SNI check, and
   certificate pin. Failure never falls back to plaintext.
4. **Authentication before dispatch.** A missing, disabled, unknown, or
   mismatched certificate cannot reach body decoding, router, storage, nudge,
   or application handling.
5. **No legacy/fixture execution.** `atm-daemon` is reference-only and
   `atm-peer-tls-interop` is fixture-only; neither is an active runtime
   dependency.
6. **Secrets remain opaque.** Durable state and reports contain only public
   fingerprints and opaque key references. Key bytes never appear in status,
   logs, errors, or report artifacts.
7. **Secure-by-default after exchange.** Once key exchange/provisioning has
   successfully established the local certificate, enabled interface, and exact
   trusted peer record, the normal peer route is mTLS. Plaintext is available
   only through an explicit, observable test/benchmark/debug override; it is
   never an automatic compatibility fallback.
8. **Enforced containment.** CI source and dependency guards must fail if
   certificate, Rustls, TLS storage, or TLS business logic enters the active
   Tokio/Axum daemon path. That path may only select and call the module.

## Authoritative sprint plans

| Sprint | Authoritative document | Closure | Dependency |
| --- | --- | --- | --- |
| AO.1 | [AO.1 TLS module re-home](sprint-ao1-tls-module-rehome.md) | Existing mTLS implementation is production-contained in one isolated crate, with its security behavior and tests preserved. | Accepted Tokio/Axum baseline |
| AO.2 | [AO.2 optional runtime activation](sprint-ao2-optional-runtime-activation.md) | Canonical runtime selects either plaintext or the opaque TLS module for both ingress and direct peer delivery. | AO.1 implementation pushed; merge forward before every AO.2 dev/fix round |
| AO.3 | [AO.3 proof and release evidence](sprint-ao3-tls-proof-and-evidence.md) | Automated and physical evidence proves the enabled module uses the canonical handler and has no downgrade or unauthorized-dispatch path. | AO.2 PR merged |

The sprint documents are authoritative for their deliverables, acceptance
criteria, validation, dependencies, and explicit non-closure.

## Out of scope

- A new TLS protocol, crypto design, certificate issuance service, key escrow,
  key rotation service, peer discovery, or public-internet exposure.
- Corporate-firewall/NAT reachability; Phase AP owns proving outbound
  connectivity first.
- Any modification to the frozen legacy daemon.
- A second router, message flow, mailbox/outbox, retry system, replay cache,
  receipt store, or nudge representation.

## Phase exit

AO is complete only when AO.3's report proves bidirectional canonical
send/read/requires-ack/reply over mTLS, all required negative certificate and
plaintext cases fail before application dispatch, and the active runtime's
TLS-related code is limited to selecting and calling the self-contained
module.
