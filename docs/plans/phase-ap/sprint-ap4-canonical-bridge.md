---
title: AP.4 — Canonical write and result bridge
status: planned
recommended_agent: arch-ctm
---

# AP.4 — Canonical write and result bridge

## Scope

Carry a bounded correlated canonical write over a live AP.3 session, invoke the
restricted daemon's existing canonical handler, then return its ordinary typed
result through an authenticated POST. No synthetic nudge, Telegram event,
mailbox mutation, or second router is permitted.

```text
Live session -> canonical WriteRequest + DeliveryCorrelationId
             -> existing handler -> durable write -> existing nudge
             -> authenticated POST result -> original caller outcome
```

## Dependencies

- **must_follow:** AP.3 PR merged.
- **parallel_safe:** none. AP.5 certifies this bridge.
- **unblocks:** AP.5.

## Deliverables

1. Bounded SSE event envelope containing the unchanged canonical request and
   a validated correlation identifier.
2. Restricted-side bridge to the existing handler/response contract.
3. Reachable-side correlated POST result completion and safe expiration when
   the session disappears before a result arrives.
4. Source guards forbidding direct SQLite, a second router, transport-owned
   mailbox mutation, synthetic nudge, and automatic retry.

## Acceptance criteria

- Message id, source/provenance, body, requires-ack, and typed result survive
  the bridge unchanged.
- A nudge occurs only after the normal handler has committed the write.
- Disconnect/correlation expiry is a typed direct-delivery failure, not a
  deferred/replayed delivery.

## Required validation

- End-to-end in-process tests for success, handler rejection, disconnect,
  duplicate correlation, expiry, and capacity refusal.
- Source/architecture guards for one canonical handler and no durable relay.
- `just lint` and `just test`.

## Non-closure

AP.4 does not make a physical CWin release claim or add operator controls.
