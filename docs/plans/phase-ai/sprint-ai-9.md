---
title: AI.9 HTTPS peer transport
status: complete
branch: feature/pAI-s9-https-peer-transport
worktree: ../atm-core-worktrees/feature/pAI-s9-https-peer-transport
target: integrate/phase-AI
---

# AI.9 — HTTPS peer transport

## Deliverables

1. Bind enabled HTTPS listeners and establish outbound HTTPS connections using
   AI.8 certificate/trust records.
2. Enforce mTLS and exact peer identity/fingerprint before passing an inbound
   request to the shared REST router.
3. Make the post-write router the sole destination-host decision point:
   empty host -> local nudge; every present host -> HTTPS message endpoint.
4. Preserve source/destination chat IDs unmodified across HTTPS; delete any
   remote replay, retry, receipt, remote-ack, host-specific persistence, or
   inbound-special-handler code encountered during wiring. The deletion
   inventory is every `peer_transport` queue/store/receipt type, any remote
   acknowledgement state, and any HTTPS-specific handler that does not call
   `ApiRouter`.
5. Enforce the concrete `5s` connect, TLS-handshake, and request deadlines;
   reject the shared `1_048_576` byte request body limit before decode; and on shutdown stop HTTPS
   accepts then drain or cancel tracked connections within the documented
   daemon shutdown deadline.

## Contract

```rust
pub trait PeerHttpTransport: Send + Sync {
    fn deliver(
        &self,
        request: WriteRequest,
        peer: &TrustedPeer,
        deadline: PeerRequestDeadline,
    ) -> Result<MessageRecord, AtmError>;
}

pub struct AuthenticatedPeer(/* private; built only after mTLS + exact trust */);

pub struct PeerRequestDeadline {
    pub connect: Duration,
    pub handshake: Duration,
    pub request: Duration,
}
```

`PeerHttpTransport` owns mTLS HTTP I/O only. Inbound HTTPS calls `ApiRouter`;
outbound delivery is selected by `DaemonRequestDispatcher::dispatch` through
`route_write` in `crates/atm-daemon/src/runtime_health.rs`. Neither owns storage,
ack state, nudge state, receipt synthesis, or retry state.
The HTTPS adapter creates `AuthenticatedPeer` only after the exact configured
host and fingerprint checks succeed, then passes it as the peer form of
`AuthenticatedIngress`; no unauthenticated caller can invoke the peer router
entry.

## Acceptance criteria

- HTTPS inbound and UDS inbound call the same router and write handler.
- Untrusted/incorrect-fingerprint peers are rejected before routing.
- Unavailable peer returns a normal transport error and adds no transport
  state; a repeated immutable message is the only retry mechanism.
- Every present host, including `localhost` and own-IP, uses the HTTPS adapter
  without a special loopback or current-host path.
- The receiving daemon checks only its local recipient roster in the shared
  write handler; peer transport performs no roster lookup.
- A remote message and remote acknowledgement preserve the chat-qualified
  source/destination addresses visible to the receiving agent.
- HTTPS bind failure, oversized body, each timeout leg, and shutdown draining
  are covered by listener lifecycle tests; no partial listener remains live.

## Non-closure

AI.9 closes implementation and in-process integration only. AI.10 owns the
two-Mac and Windows-host release evidence.

## Required validation

mTLS allow/reject integration tests; two-daemon in-process adapter test;
chat-qualified HTTPS send/ack tests; own-IP HTTPS proof; `just lint`; `just
test`; transport-only boundary gate.
