---
title: ADR-028 Peer Server Transport Boundary
status: accepted
date: 2026-07-15
---

# ADR-028 — Peer Server Transport Boundary

## Context

AG.1 exposed a real product defect in cross-host bring-up: released `1.3.1`
could dial outbound peers but had no production inbound daemon listener. The
fix adds a runtime-private inbound TCP listener so one daemon can accept a
bounded request/response exchange from another daemon.

That new surface needs an explicit architectural home. It is not the same as
the same-host `ServerTransport` contract, because it does not own the
host-local endpoint semantics or same-user access-control policy documented for
the local IPC adapter.

## Decision

Keep the inbound peer listener as a distinct daemon-private boundary:

- concrete type: `atm_daemon::peer_transport::PeerServerTransport`
- machine-readable record:
  `boundaries/atm-daemon/peer-server-transport.toml`
- assembly root:
  `RuntimeComposition::start_background_lanes`

Do not force it behind `atm_core::boundary::ServerTransport`.

## Rationale

- the local IPC `ServerTransport` boundary owns a different contract:
  host-scoped endpoint publication plus same-user local access control
- the peer listener owns remote daemon admission, bounded network deadlines,
  accept-loop recovery, and peer-request drain behavior
- keeping the inbound peer surface runtime-private avoids widening the shared
  trait surface before the cross-host contract is stable

## Consequences

- daemon boundary review must treat the inbound peer listener as a first-class
  runtime surface even though it is not a shared public trait
- cross-host transport work continues to reuse the shared ATM framing and
  request/response envelopes
- future extraction into a shared trait is allowed only if the remote peer
  contract becomes stable enough to justify widening `atm-core`
