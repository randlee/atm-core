---
title: AO.1 — Portable peer-tls stream component
status: planned
recommended_agent: arch-ctm
---

# AO.1 — Portable peer-tls stream component

## Scope

Create one `peer-tls` crate that consumes the existing `TlsStorage` key
exchange/trust contract and supplies Rustls configuration plus Tokio TLS stream
wrapping. It does not create or change key exchange, a daemon, CLI, MCP,
HTTP router, or ATM domain behavior.

The public surface must be application-neutral. Equivalent signatures are:

```rust
pub struct PeerTls { /* private Rustls client/server configs */ }

impl PeerTls {
    pub async fn refresh_from(storage: &dyn TlsStorage) -> Result<Self, TlsError>;
    pub async fn accept(&self, tcp: TcpStream) -> Result<TlsStream<TcpStream>, TlsError>;
    pub async fn connect(
        &self, peer: &PeerId, tcp: TcpStream,
    ) -> Result<TlsStream<TcpStream>, TlsError>;
}
```

`TlsStorage`, `PeerId`, and `TlsSnapshot` above are the already-extracted
storage contract and types, not a second contract for AO.1 to design. Exact
type names may differ, but `peer-tls` must introduce no public item mentioning
ATM messages, rosters, nudge, CLI, MCP, or daemon types. `TlsSnapshot` contains
only the configuration required to construct Rustls state; private key bytes
remain private to the component and are never serializable/debug printable.

## Dependencies

- **must_follow:** accepted Tokio/Axum runtime baseline. Parent development
  must be pushed before AO.1 begins; AO.1 never uses the frozen daemon as a
  build or test target.
- **parallel_safe:** none. AO.2 consumes this crate's public API.
- **unblocks:** AO.2.

## Deliverables

1. `peer-tls`, dependent on the existing portable `TlsStorage` contract, with
   Rustls client/server construction, exact hostname/pin/client-certificate
   validation, and Tokio `accept`/`connect` wrappers.
2. A compatibility test proving the existing key-exchange storage records
   provide every datum required by `TlsStorage`; do not redesign the exchange
   flow without a reproduced missing datum.
3. Typed, non-secret errors for missing/disabled configuration, invalid key or
   certificate, missing/wrong client certificate, hostname mismatch, pin
   mismatch, and handshake/deadline failure.
4. Focused tests for valid mutual TLS, all delivered negative cases, and a
   Rustls stream carrying arbitrary HTTP bytes without interpreting them.
5. Source/dependency guards proving `peer-tls` has no ATM domain dependency
   and neither depends on nor executes the frozen daemon or fixture crate.

## Acceptance criteria

- Given a valid snapshot from the existing exchange, two Tokio peers complete
  mutual TLS and exchange opaque bytes successfully.
- Every invalid/disabled/mismatched snapshot or certificate fails before any
  application handler can receive bytes.
- The crate is reusable by another Tokio project through `TlsStorage`; no
  ATM-specific type appears in its public API.
- The work does not modify ATM key-exchange business rules or the active HTTP
  handler.

## Required validation

- `cargo test -p peer-tls`.
- A focused in-memory `TlsStorage` integration matrix for the positive and
  negative cases above.
- Dependency/source guard for portable public API and frozen-daemon exclusion.
- `just lint` and `just test`.

## Non-closure

AO.1 does not attach TLS to an ATM listener or outbound client. It does not
claim any ATM cross-host transport proof.
