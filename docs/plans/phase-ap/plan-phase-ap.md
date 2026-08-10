---
title: Phase AP Plan — Outbound-Only Corporate Network Peer Connectivity
status: proposed
branch: plan/phase-ao-tls-and-ap-outbound-connectivity
worktree: ../atm-core-worktrees/plan/phase-ao-tls-and-ap-outbound-connectivity
target: develop
---

# Phase AP — Outbound-Only Corporate Network Peer Connectivity

## Goal

Support a daemon that cannot accept unsolicited inbound peer TCP because of a
corporate firewall, NAT, VPN, DNS, or proxy policy. The proposed product path
is an authenticated outbound Server-Sent Events (SSE) session from the
restricted daemon to a reachable peer, plus an ordinary authenticated HTTP
POST for its response leg.

This is a separate transport feature. It does not fabricate the missing CWin
direct-cross-host proof, alter Phase AL's direct-peer contract, or introduce a
durable relay.

## Non-negotiable entry gate

[AP.1](sprint-ap1-real-network-feasibility.md) is first. No product transport
code begins before it proves the proposed outbound mTLS SSE + POST exchange on
the actual CWin, M4, and M5 environment, or records the exact infrastructure
block. A localhost simulation, SSH tunnel, port forward, raw-IP substitute,
or external relay cannot pass this gate.

AP.2–AP.5 begin only after AP.1 passes and AO.3 has merged its mTLS proof and
boundary guards into the active Tokio/Axum line. The frozen legacy daemon is
never an implementation, test, or fallback target.

## Why SSE + POST

SSE is ordinary HTTP/1.1 response streaming and is less likely than a
WebSocket upgrade to be rejected or rewritten by restrictive corporate
proxies. The restricted host initiates the only persistent connection. Its
response leg is an ordinary authenticated POST, so the feature introduces no
new bidirectional socket protocol or durable store-and-forward behavior.

## Invariants

1. The session registry is bounded, in-memory, and online-only; it has no
   outbox, retry worker, replay, receipt store, or offline delivery claim.
2. A reachable peer derives remote identity from the mutually authenticated
   TLS session, never a header, raw IP, or bearer secret.
3. A streamed write invokes the existing canonical write handler and only
   emits a nudge after normal persistence.
4. Missing, expired, overloaded, disconnected, or unauthorized sessions fail
   the originating call with a typed direct-delivery error.
5. Runtime ownership and all new boundary/manifest edges are explicit and
   machine-enforced before implementation begins.

## Authoritative sprint sequence

| Sprint | Closure | must_follow |
| --- | --- | --- |
| [AP.1](sprint-ap1-real-network-feasibility.md) | Real CWin↔M4/M5 outbound mTLS SSE + POST feasibility evidence, or a documented infrastructure block. | None; executes first. |
| [AP.2](sprint-ap2-session-contract.md) | Reviewed typed session/correlation/error contract and architecture guards. | AP.1 PASS and AO.3 merged. |
| [AP.3](sprint-ap3-authenticated-session.md) | Authenticated live SSE session and bounded registry lifecycle. | AP.2 PR merged. |
| [AP.4](sprint-ap4-canonical-bridge.md) | Canonical write/response bridge using the existing handler. | AP.3 PR merged. |
| [AP.5](sprint-ap5-operations-proof.md) | Doctor, controls, negative cases, reports, and physical CWin proof. | AP.4 PR merged. |

## Out of scope

- Changing CWin network policy without its operator's authorization.
- SSH reverse tunnels, VPN alteration, raw-IP identity, public exposure, or a
  third-party relay as the product implementation.
- WebSocket or long-poll product support in AP.
- A durable outbox, store-and-forward relay, replay/retry scheduler, offline
  mailbox replication, or delivery receipt subsystem.
