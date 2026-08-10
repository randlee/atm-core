---
title: Phase AO Plan — Optional mTLS Peer Adapter
status: proposed
branch: plan/phase-ao-tls-and-ap-outbound-connectivity
worktree: ../atm-core-worktrees/plan/phase-ao-tls-and-ap-outbound-connectivity
target: develop
---

# Phase AO — Optional mTLS Peer Adapter

## Goal

Restore optional mTLS to the canonical Tokio/Hyper direct-peer path with one
production adapter crate, `peer-tls`. This is standard Tokio-Rustls stream
wrapping, not a daemon rewrite, new key-exchange protocol, or generic
framework project.

The already-landed TLS boundary is authoritative:

```text
PeerConfigStore (sealed durable configuration)
  + atm_storage::tls (identity, pinning, Rustls verification)
  -> peer-tls (Tokio mTLS adapter)
  -> existing atm-http-runtime HTTP handler/client
```

`peer-tls` consumes those existing values; it does not recreate certificate
parsing, fingerprint matching, trust policy, or key exchange. The old
`atm-peer-tls-interop` remains a one-shot curl fixture and is not renamed,
promoted, or used by production delivery.

## Required ownership and boundary transition

`peer-tls` owns all TLS-specific work: reading the sealed `PeerConfigStore`,
building/refreshing client and server configuration from `atm_storage::tls`,
Tokio-Rustls handshakes, hostname/pin/client-certificate validation, and typed
TLS errors. It contains no mailbox, nudge, retry, replay, CLI, MCP, or graft
logic.

`atm-http-runtime` remains the single HTTP router, encoder/decoder, and
lifecycle owner. It sees only an opaque, sealed `PeerIoAdapter` and calls its
`accept`/`connect` operations; it must not import Rustls/Tokio-Rustls,
`PeerConfigStore`, or `atm_storage::tls`, and it must not parse certificates
or make TLS policy decisions. This is the intended equivalent of “if mTLS,
call the adapter”; it is not a second HTTP path.

The implementation must make the following boundary changes in the same
reviewed AO.1/AO.2 series; no code may rely on the current lint pattern gap.

| Boundary record | Explicit change |
| --- | --- |
| `boundaries/atm-storage/peer-config-store.toml` | Add `peer-tls` as the sole new allowed dependent. It may read configuration only. |
| `boundaries/atm-storage/tls.toml` | Add `peer-tls` as the sole new allowed dependent; it consumes existing helpers rather than duplicating them. |
| `boundaries/atm-http-runtime/http-runtime.toml` | Preserve the ban on concrete TLS/storage imports; explicitly allow a core-defined opaque `PeerIoAdapter` slot and add a guard that rejects Rustls/Tokio-Rustls/`PeerConfigStore`/`atm_storage::tls` there. |
| `boundaries/atm-daemon-bootstrap/*` | Add `peer-tls` as an allowed dependency only for composition of the opaque adapter from `RuntimeAssembly::peer_config_store()`. Bootstrap owns no handshake, certificate, or transport policy. |
| `boundaries/peer-tls/peer-tls.toml` | New active production boundary: permit only `atm-core`, `atm-storage`, `atm-http-runtime`, Tokio/Hyper/Rustls dependencies; forbid legacy daemon and `atm-peer-tls-interop` dependency edges. |
| architecture tests | Add expected allowed edges and explicit forbidden-edge/source checks for all rows above, including the sole authorized `PeerIoAdapter` implementation. |

The adapter port is sealed under ADR-001 and has one authorized implementer:
`peer-tls`. Its shape is equivalent to:

```rust
pub trait PeerIoAdapter: atm_core::boundary::sealed::Sealed + Send + Sync {
    async fn accept(&self, tcp: TcpStream, deadline: RequestDeadline)
        -> Result<BoxedPeerIo, AtmError>;
    async fn connect(&self, peer: &HostName, deadline: RequestDeadline)
        -> Result<BoxedPeerIo, AtmError>;
}

pub fn mtls_adapter(
    store: Arc<dyn PeerConfigStore + Send + Sync>,
) -> Result<Arc<dyn PeerIoAdapter>, AtmError>;
```

Exact module names may vary. `PeerIoAdapter` is transport-only; it may expose
neither a message DTO nor a generic extension point. `peer-tls` is deliberately
ATM-integrated through the existing sealed storage types for this phase. A
future app-agnostic TLS-storage extraction requires a second real consumer and
is explicitly out of scope.

## Runtime rule

After key exchange has produced a valid enabled interface and trusted-peer
record, peer traffic uses mTLS. Plaintext is available only through an
explicit, observable test/benchmark/debug override. TLS configuration, DNS,
handshake, hostname, or pin failure never retries as plaintext.

The frozen `crates/atm-daemon` tree is reference-only and must not be edited,
started, tested, restored, or made a dependency.

## Authoritative sprint sequence

| Sprint | Closure | must_follow |
| --- | --- | --- |
| [AO.1](sprint-ao1-tls-module-rehome.md) | `peer-tls`, its explicit boundary records, and focused TLS configuration/stream tests exist without activating ATM traffic. | Accepted Tokio/Axum baseline. |
| [AO.2](sprint-ao2-optional-runtime-activation.md) | The runtime uses the opaque adapter for inbound and outbound peer traffic while retaining one HTTP handler/client path. | AO.1 development pushed; merge AO.1 before every AO.2 dev/fix round. |
| [AO.3](sprint-ao3-tls-proof-and-evidence.md) | Automated and two-host evidence proves canonical mTLS delivery, rejection before application dispatch, and no plaintext downgrade. | AO.2 PR merged. |

## Invariants

1. TLS changes only the stream below the existing HTTP handler and encoder;
   there is one canonical request and result path.
2. `peer-tls` is the only production owner of Rustls/Tokio-Rustls and of
   `PeerConfigStore` reads for transport configuration.
3. `atm_storage::tls` remains the one canonical owner of identity loading,
   fingerprint normalization, pinning, and verification helpers.
4. TLS failures occur before HTTP body decoding and application dispatch and
   never fall back to plaintext.
5. Private key bytes never appear in doctor output, errors, logs, or reports.
6. The fixture and frozen daemon have no production dependency edge.

## Out of scope

- Generic TLS framework, reusable daemon/CLI/MCP framework, certificate
  issuance/rotation/discovery, or a new key-exchange protocol.
- ATM message, roster, acknowledgement, nudge, graft, retry, replay, or
  mailbox behavior.
- Corporate-network reachability; Phase AP starts with a separate real-host
  feasibility proof.

## Phase exit

AO is complete when the active Tokio runtime proves bidirectional mTLS
send/read/requires-ack/reply through the unchanged HTTP handler, all required
certificate/pin/hostname negatives fail before application dispatch, plaintext
works only under its explicit override, and indexed reports under
`site/reports/` retain the evidence.
