---
title: AI.25 DNS-backed peer authority
status: complete
branch: feature/pAI-s25-peer-authority-resolution
target: integrate/phase-AI
depends_on: AI.21-pre, AI.22, AI.23, AI.11–AI.16
---

# AI.25 — DNS-backed peer authority

## Release candidate

- First commit: set every releasable ATM assembly to `1.3.2-beta.25`; record
  matching client/daemon values from `atm doctor --json` in runtime evidence.

## Closure

One registered hostname/HTTPS-port/certificate pin authorizes its current
DNS-resolved IP addresses without durable IP aliases, and trust mutations take
effect in the one live daemon without a restart.

## Deliverables

1. Evolve the existing storage-trait-owned `atm_storage::TrustedPeer` authority
   record below; do **not** introduce a second `PeerAuthority` type or any
   peer-IP-as-authority lookup.

   Current shape on `origin/integrate/phase-AI@cb3af95188c1ba685ed93cec0512e7d38fa7f655`:

   ```rust
   pub struct TrustedPeer {
       pub host: HostName,
       pub fingerprint: CertificateFingerprint,
       pub enabled: bool,
   }
   ```

   Target shape (the new `https_port` is intentional; `enabled` is retained as
   the operator's explicit allow/revoke control and is not folded into TLS):

   ```rust
   pub struct TrustedPeer {
       pub host: HostName,
       pub fingerprint: CertificateFingerprint,
       pub enabled: bool,
       pub https_port: std::num::NonZeroU16,
   }
   ```

   Update `crates/atm-storage/src/contract.rs::PeerConfigStore`, its storage
   implementations/tests, `crates/atm-daemon/src/composition.rs`, and
   `crates/atm-daemon/src/https_transport.rs` together. The peer CLI and
   doctor DTO consume that same record; neither gets a parallel authority DTO.
2. Add bounded fresh DNS resolution: hostname targets exact-match a registered
   authority; literal IP targets match exactly one authority's current A/AAAA
   result. Zero or multiple matches fail closed with typed errors. Resolution
   occurs for every new HTTPS connection; it is never a cached delivery or
   health decision.
3. Preserve the chosen registered hostname and port for TLS authority/pin verification;
   reject reverse-DNS inference and never persist resolver output. Document
   that the peer operator maintains the hostname's forward DNS/DDNS record as
   its VPN/Wi-Fi address changes.
4. CLI trust add/replace/revoke changes durable configuration, then invokes one
   authenticated local `POST /v1/atm/runtime/reload` daemon control operation.
   That operation atomically refreshes live trust verification. No second
   daemon, listener fallback, signal-only dependency, or direct SQLite access
   outside storage traits.

## Implementation map

- `crates/atm-storage/src/contract.rs`: evolve `TrustedPeer` and
  `PeerConfigStore`; add `https_port`, retain `enabled`, and define no resolver
  cache/persistence method.
- `crates/atm-daemon/src/https_transport.rs`: resolve a hostname for each new
  peer connection, select exactly one existing `TrustedPeer`, and retain that
  record's hostname/port for TLS SNI and fingerprint verification.
- `crates/atm-daemon/src/composition.rs`: replace the live verifier atomically
  from `PeerConfigStore` after a trust mutation; do not rebuild the daemon.
- `crates/atm-core/src/doctor/report.rs` and `docs/atm-daemon/http-api.md`:
  project only hostname, configured port, and safe configuration health—never
  resolved addresses or certificate/private-key material.

## Acceptance criteria

- Registering `fastpc4.rz.local` permits both its hostname and a currently
  resolved literal IP with the same certificate pin.
- A changed DNS answer is honored on the next resolution; stale IP no longer
  authorizes.
- A forward DNS change models a VPN address change without reverse DNS,
  SQLite mutation, daemon replacement, or a second trust record.
- An IP matching zero or multiple registered names fails before TLS/route.
- A literal IP has no standalone authority record; it authorizes only when it
  resolves to exactly one registered hostname, and no resolver result is
  written to SQLite.
- A live `atm peer trust add`, `replace`, or `revoke` invokes the authenticated
  local reload operation and changes the current verifier without process
  replacement; tests prove one daemon remains.
- Two account daemons whose endpoint names resolve to one IP can be trusted
  independently on distinct configured ports; an occupied port fails closed
  rather than falling back to another listener.

## Required validation

Unit tests for exact/zero/ambiguous resolution and pin selection; integration
test for live trust refresh; structural test that transport adapters do not
import SQLite; `just lint`; `just test`.

## Non-closure

This sprint does not change write deadlines, delivery outcomes, or physical
peer evidence. Any inbound peer listener built in this sprint must decode
requests into the existing `RequestEnvelope` and dispatch them through the
daemon's single `Arc<dyn RequestDispatcher>` (`composition.rs`'s
`request_dispatcher()` accessor), per `AI.23` — it must not persist or nudge
through a second implementation.

It also does not probe peers or retain reachability state. AI.27 exposes
derived link quality and AI.28 owns the only bounded reconnect/drain
coordination.
