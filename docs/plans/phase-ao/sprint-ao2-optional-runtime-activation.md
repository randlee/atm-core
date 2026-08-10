---
title: AO.2 — Attach peer-tls to the existing Tokio runtime
status: planned
recommended_agent: arch-ctm
---

# AO.2 — Attach peer-tls to the existing Tokio runtime

## Scope

Integrate AO.1 in exactly two existing runtime paths: accepted peer TCP
connections and outbound direct-peer TCP connections. The resulting decrypted
Tokio stream enters the existing HTTP server/client path unchanged.

`atm-storage-rusqlite` implements the already-defined `TlsStorage` contract
from its existing certificate/interface/trusted-peer records. The runtime
creates/refreshes one opaque `PeerTls` handle at composition time. It performs
no direct certificate lookup, parsing, pinning, or Rustls configuration.

```text
TCP accept/connect -> PeerTls::accept/connect -> existing Hyper HTTP/1 path
```

## Dependencies

- **must_follow:** AO.1 development must be pushed. Merge AO.1 into this
  branch before every AO.2 development/fix round.
- **parallel_safe:** none. This sprint owns both active peer-TCP call sites.
- **unblocks:** AO.3.

## Deliverables

1. An `atm-storage-rusqlite` `TlsStorage` implementation that reads the
   existing key-exchange outputs without changing their schema or business
   rules.
2. One opaque `PeerTls` composition/lifecycle handle in `atm-http-runtime`.
3. One inbound TCP-to-TLS wrap before the existing HTTP connection handler and
   one outbound TCP-to-TLS wrap before the existing peer HTTP client.
4. Explicit transport selection: a peer with a valid enabled exchange snapshot
   uses mTLS by default; plaintext is available only through a clearly named,
   observable test/benchmark/debug override.
5. Fail-closed behavior: unavailable configuration, DNS, handshake,
   certificate, hostname, or pin failure returns a typed error and never
   retries via plaintext.
6. Runtime tests proving the existing handler sees an identical request after
   plaintext and mTLS wrapping, while rejected TLS reaches no handler.

## Acceptance criteria

- The active runtime has no new HTTP router, message/storage business logic,
  CLI behavior, MCP behavior, or domain-specific TLS path.
- An exchanged peer selects mTLS by default; a non-exchanged peer does not
  silently claim mTLS readiness.
- The explicit plaintext override is visible in doctor/report metadata and is
  impossible to select as a fallback after TLS failure.
- Rejected TLS cannot reach body decoding, router, storage write, or
  post-receive nudge.
- The frozen daemon and fixture crate remain inactive and unreferenced at
  runtime.

## Required validation

- Focused runtime tests for inbound and outbound plaintext/mTLS selection,
  no-downgrade, and no-handler-on-rejection.
- `cargo test -p atm-http-runtime -p atm-storage-rusqlite`.
- `just lint` and `just test`.
- Source/dependency guard confirming the runtime only owns selection and
  `PeerTls` calls, not TLS configuration logic.

## Non-closure

AO.2 does not change key exchange, add key rotation, broaden TLS into a
framework project, or prove physical cross-host delivery. AO.3 owns proof.
