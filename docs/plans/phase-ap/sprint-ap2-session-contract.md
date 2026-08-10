---
title: AP.2 — Outbound session contract and guards
status: planned
recommended_agent: arch-ctm
branch: plan/phase-ao-tls-and-ap-outbound-connectivity
worktree: ../atm-core-worktrees/plan/phase-ao-tls-and-ap-outbound-connectivity
---

# AP.2 — Outbound session contract and guards

## Scope

Define the small, sealed, online-only session capability before wiring SSE.
The capability is internal to the Tokio runtime and is not a plugin surface,
relay, queue, or durable delivery abstraction.

```rust
pub struct OutboundSessionId(Ulid);
pub struct DeliveryCorrelationId(Ulid);

pub enum OutboundSessionState {
    Connecting,
    Authenticated { peer: HostName },
    Live { peer: HostName },
    Closed,
}
```

The actual API must use validated newtypes and an unforgeable live-state
transition; a raw string session/correlation id or caller-supplied host is not
permitted.

## Dependencies

- **must_follow:** AP.1 PASS and AO.3 merged.
- **parallel_safe:** none. AP.3 consumes this contract.
- **unblocks:** AP.3.

## Deliverables

1. Validated `OutboundSessionId` and `DeliveryCorrelationId`, typed session
   state, and an object-safe sealed internal registry port with a finite list
   of authorized adapters recorded under the ADR-001 boundary process.
2. Typed errors and recovery text for session unavailable, authentication
   rejected, capacity refused, correlation expired, peer result rejected, and
   deadline/cancellation.
3. Explicit per-session body/frame limits, heartbeat interval, in-flight
   correlation bound, and disconnect cleanup semantics.
4. Boundary TOML, manifest, and source guards that forbid durable state,
   SQLite access, retry/replay, alternate message routes, and unauthenticated
   registration.

## Acceptance criteria

- No send can select a session that is not authenticated and live.
- Capacity exhaustion fails immediately; it does not create a queue or retry.
- The registry has no persistence API or storage dependency.
- A fake in-memory implementation and contract tests cover every lifecycle
  transition and error without a live SSE socket.

## Required validation

- Unit/contract tests for newtypes, transitions, bounds, and errors.
- Architecture/manifest guard tests for the finite authorized adapter set.
- `just lint` and `just test`.

## Non-closure

AP.2 does not bind a listener, issue an SSE request, or bridge a message.
