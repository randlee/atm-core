---
title: AI.9 HTTPS peer transport
status: proposed
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

## Contract

```rust
pub trait PeerHttpTransport: Send + Sync {
    fn deliver(&self, request: WriteRequest, peer: &TrustedPeer) -> Result<MessageRecord, AtmError>;
}
```

`PeerHttpTransport` owns mTLS HTTP I/O only. Inbound HTTPS calls `ApiRouter`;
outbound delivery is selected only by `PostWriteRouter`. Neither owns storage,
ack state, nudge state, receipt synthesis, or retry state.

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

## Non-closure

AI.9 closes implementation and in-process integration only. AI.10 owns the
two-Mac and Windows-host release evidence.

## Required validation

mTLS allow/reject integration tests; two-daemon in-process adapter test;
chat-qualified HTTPS send/ack tests; own-IP HTTPS proof; `just lint`; `just
test`; transport-only boundary gate.
