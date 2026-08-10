---
title: Phase AO Plan — Optional mTLS on the Tokio Runtime
status: proposed
branch: plan/phase-ao-tls-and-ap-outbound-connectivity
worktree: ../atm-core-worktrees/plan/phase-ao-tls-and-ap-outbound-connectivity
target: develop
---

# Phase AO — Optional mTLS on the Tokio Runtime

## Goal

Enable the already-proven key-exchange and trust-record flow on the current
Tokio/Hyper HTTP path using one small, portable `peer-tls` crate. This is
standard Tokio-Rustls stream wrapping—not a daemon rewrite, a new TLS
protocol, or a generic-framework project.

The existing key-exchange path remains authoritative. It writes the local
certificate/key reference, enabled interface, and exact trusted-peer pin
through the existing `TlsStorage` boundary. `peer-tls` consumes that completed
configuration to build Rustls client/server configuration and to wrap TCP
streams. Key exchange is not repeated in the request path.

```text
existing key exchange -> TlsStorage snapshot -> peer-tls Rustls config
                                                -> Tokio TLS stream
                                                -> existing HTTP handler
```

## Required boundary

`peer-tls` is the only new crate. It owns the portable TLS concerns:

- consumption of the existing `TlsStorage` configuration contract and TLS
  snapshot types;
- Rustls client/server configuration, certificate pinning, and client
  certificate verification;
- Tokio inbound `accept` and outbound `connect` stream wrapping; and
- typed TLS configuration/handshake errors with no key bytes in diagnostics.

ATM references `peer-tls` in only these places:

| Consumer | Responsibility | Prohibited responsibility |
| --- | --- | --- |
| `atm-storage-rusqlite` | Keep the existing `TlsStorage` implementation backed by certificate/interface/trusted-peer records. | Rustls configuration or transport policy. |
| `atm-http-runtime` | Hold the opaque `PeerTls` handle; wrap an accepted/outbound TCP stream; pass the resulting stream to the existing HTTP handler/client. | Certificate parsing, pinning, TLS storage queries, a second router, or a second request path. |
| TLS tests | Supply an in-memory `TlsStorage` and test configuration/streams. | ATM domain behavior. |

The CLI, MCP, graft, message core, roster, acknowledgement, and nudge paths
do not reference `peer-tls`. Their daemon API and domain behavior stay
unchanged.

## Runtime rule

After the existing key exchange has produced a valid enabled interface and
trusted-peer snapshot, normal peer traffic uses mTLS. Plaintext is available
only through an explicit, observable test/benchmark/debug override. TLS
configuration, DNS, handshake, hostname, or pin failure never retries as
plaintext.

The legacy `crates/atm-daemon/src/https_transport.rs` is frozen reference and
test-oracle material only. It is neither a runtime dependency nor code to copy
into the new runtime. The historical fixture crate remains fixture-only.

## Authoritative sprint sequence

| Sprint | Closure | must_follow |
| --- | --- | --- |
| [AO.1](sprint-ao1-tls-module-rehome.md) | `peer-tls` builds Rustls configurations and Tokio TLS streams from the existing `TlsStorage` contract, with focused positive/negative tests. | Accepted Tokio/Axum baseline. |
| [AO.2](sprint-ao2-optional-runtime-activation.md) | The existing SQLite storage and Tokio runtime consume `peer-tls` for inbound and outbound peer traffic with default mTLS and explicit plaintext override. | AO.1 development pushed; merge AO.1 before each AO.2 dev/fix round. |
| [AO.3](sprint-ao3-tls-proof-and-evidence.md) | Automated and two-host evidence proves canonical mTLS delivery, rejection before application dispatch, and no plaintext downgrade. | AO.2 PR merged. |

## Invariants

1. There is one existing HTTP handler/router and one existing outbound request
   shape; TLS changes only the stream below it.
2. `peer-tls` depends only on the existing `TlsStorage` contract for durable
   configuration; its own public API has no ATM message, roster, nudge, CLI,
   MCP, or daemon type.
3. The existing key-exchange/storage contract is reused unchanged unless AO.1
   proves a concrete missing TLS datum; it is not redesigned speculatively.
4. TLS failures fail before HTTP body decoding and application dispatch and
   never fall back to plaintext.
5. Private-key bytes never appear in doctor output, errors, logs, or reports.
6. No work touches, starts, tests, restores, or depends on the frozen legacy
   daemon.

## Out of scope

- A reusable daemon, CLI, MCP, or broader generic application framework.
- New certificate issuance, exchange, rotation, discovery, relay, retry, or
  application protocol work.
- Changes to ATM message, roster, acknowledgement, nudge, or graft semantics.
- Corporate-network reachability; Phase AP starts with an independent real
  host proof.

## Phase exit

AO is complete when the active Tokio runtime proves bidirectional mTLS
send/read/requires-ack/reply through the unchanged HTTP handler, all required
certificate/pin/hostname negatives are rejected before application dispatch,
plaintext works only when explicitly requested for diagnostics, and the
reports are indexed under `site/reports/`.
