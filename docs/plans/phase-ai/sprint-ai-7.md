---
title: AI.7 canonical write path
status: complete
branch: feature/pAI-s7-canonical-write-path
worktree: ../atm-core-worktrees/feature/pAI-s7-canonical-write-path
target: integrate/phase-AI
---

# AI.7 — canonical write path (historical, partially superseded)

> **Known routing defect, owned by AI.12:** at the current integrated tip,
> `DaemonRequestDispatcher::route_write` still branches on `request.to.host`
> before persistence and directly invokes peer delivery; the retained
> `PostWriteRouter::dispatch` is a no-op. AI.12 deletes that pre-persistence
> branch and makes the ADR-035 pipeline true. This sprint's local ingress and
> shared request work remains historical evidence, not proof that host routing
> is already canonical.

## Deliverables

1. Define one `WriteRequest` carrying immutable message data, full structured
   source/destination addresses, and optional `acknowledges_message_id`.
2. Route CLI send/ack, graft, and local UDS REST to one write handler and one
   sealed storage method.
3. Make the handler persist idempotently, apply optional receiver-side ack
   mutation, then emit the post-write event exactly once.
4. Preserve chat-qualified addresses through send, reply, and acknowledgement;
   delete duplicate send/ack envelopes, handlers, persistence/nudge branches,
   and host-routing decisions. The deletion inventory is every retained
   Compose/DirectDeliver-equivalent pair, separate ack sender, sender-side ack
   mutation, and any host check outside `PostWriteRouter`; an identifier rename
   does not satisfy this deliverable.

## Contract

```rust
pub trait MessageWriter: Send + Sync {
    fn write(&self, request: WriteRequest) -> Result<MessageRecord, AtmError>;
}

pub trait PostWriteRouter: Send + Sync {
    fn dispatch(&self, request: &WriteRequest, message: &MessageRecord) -> Result<(), AtmError>;
}
```

`acknowledges_message_id` is the sole semantic difference between send and
ack. The handler owns persistence and receiver-side acknowledgement mutation.
**Historical target, not current-tip fact:** `PostWriteRouter` is the only
host-routing decision point, emitting a local nudge only for an empty host and
HTTPS for every present host. AI.12 enforces that target.

## Acceptance criteria

- The REST ack endpoint differs from send only by `acknowledges_message_id` on
  `WriteRequest`.
- **Historical target, enforced by AI.12:** one structural call graph reaches
  storage and post-write emission for all write ingress sources.
- Same-message ULID replay is idempotent and does not duplicate a nudge.
- A chat-qualified reply or ack preserves the original address and does not
  leak into a base-agent mailbox.
- **Historical target, enforced by AI.12:** exact self-address with an empty
  host is rejected once before write routing; every present host, including
  localhost and own IP, follows ordinary HTTPS routing with no special send or
  ack exception.

## Required validation

CLI, graft, and REST send/ack integration tests; chat-address reply/ack tests;
duplicate-ULID and failed-write tests; `just lint`; `just test`; canonical-write
architecture gate.

## Non-closure

AI.7 closes the in-process canonical write semantics and local ingress
convergence. It does not bind HTTPS, perform TLS, configure peers, or claim
two-host delivery proof; AI.8 through AI.10 own those outcomes.
