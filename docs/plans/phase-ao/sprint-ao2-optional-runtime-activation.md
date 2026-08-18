---
title: AO.2 — Install peer-tls below the canonical HTTP path
status: complete
recommended_agent: arch-ctm
branch: feature/pao-s2-optional-runtime-activation
worktree: ../atm-core-worktrees/feature/pao-s2-optional-runtime-activation
---

# AO.2 — Install peer-tls below the canonical HTTP path

## Scope

Compose the AO.1 adapter once from `RuntimeAssembly::peer_config_store()` in
`atm-daemon-bootstrap`, inject it into the current runtime as
`Arc<dyn atm_core::PeerIoAdapter>`, and use it for the existing direct-peer listener and
connector. `atm-http-runtime` must continue to own the one Hyper/Axum router,
request encoder/decoder, and lifecycle task; it receives an opaque stream, not
TLS configuration or storage.

`PeerIoAdapter` is the single `atm-core`-owned sealed trait. AO.2 must consume
that type directly and must not introduce an `atm-http-runtime` trait or
runtime-local alias that creates a second type identity.

```text
bootstrap -> mtls_adapter(PeerConfigStore) -> PeerIoAdapter
direct-peer TCP -> PeerIoAdapter::accept/connect -> existing HTTP handler/client
```

## Dependencies

- **must_follow:** AO.1 development pushed; merge it into this branch before
  every AO.2 development/fix round.
- **parallel_safe:** none. AO.2 owns the two active direct-peer call sites.
- **unblocks:** AO.3.

## Deliverables

1. Bootstrap composition of the opaque adapter. Bootstrap receives no Rustls
   type and performs no certificate/query/handshake work.
2. Exactly one inbound adapter call before the existing direct-peer Hyper
   connection service and one outbound adapter call before the existing shared
   HTTP client exchange.
3. A single explicit security mode selected from the persisted peer
   configuration: valid enabled exchange configuration uses mTLS; plaintext is
   allowed only by a named test/benchmark/debug override recorded in
   doctor/report metadata.
4. Fail-closed errors for unavailable configuration, DNS, handshake,
   certificate, hostname, or pin failure. No failure invokes a plaintext retry.
5. Runtime tests proving plaintext and mTLS reach the same canonical handler
   and rejected TLS reaches neither body decoding, router, durable write, nor
   post-receive hook.

## Acceptance criteria

- `atm-http-runtime` imports no Rustls/Tokio-Rustls type, `PeerConfigStore`,
  or `atm_storage::tls`, and retains one server/router/client implementation.
- The adapter is refreshed through its own configuration boundary and no
  request path queries SQLite/certificate state directly.
- Default mTLS after a valid exchange and the explicit plaintext override are
  observable and deterministic.
- The frozen daemon and interop fixture remain inactive and unreferenced by
  production delivery.

## Required validation

- Focused inbound/outbound runtime tests for plaintext, mTLS, no downgrade,
  and no handler-on-rejection.
- `cargo test -p peer-tls -p atm-http-runtime -p atm-daemon-bootstrap`.
- Boundary/manifest guards proving the precise AO.1 dependency graph and no
  concrete TLS or storage imports in the runtime.
- `just lint` and `just test`.

## Non-closure

AO.2 does not prove two-host delivery, change key exchange, broaden TLS into a
generic framework, or add corporate-network support.
