---
title: AI.23 end-to-end peer write deadline
status: proposed
branch: feature/pAI-s23-peer-write-deadline
target: integrate/phase-AI
depends_on: AI.11–AI.16
---

# AI.23 — end-to-end peer write deadline

## Closure

One absolute `RequestDeadline` governs local HTTP admission through peer HTTPS
completion or cancellation; no route creates a longer independent deadline.

## Deliverables

1. Pass the remaining `RequestDeadline` budget from local HTTP adapter through
   router, dispatcher, post-write router, and HTTPS transport.
2. Delete the fresh default `HttpsRequestDeadline` created below dispatcher
   scope; connect/TLS/request operations consume only the remaining budget.
3. Keep every accepted request in runtime drain accounting and cancel it on
   expiration or local connection close. Detached peer delivery is forbidden.
4. Return the typed error below when dispatch began but peer acceptance cannot
   be established before the shared deadline; repeat uses the same ULID.

   ```rust
   pub enum AtmErrorCode {
       // ...
       RemoteDeliveryUnconfirmed,
   }

   // Stable wire/API spelling: "REMOTE_DELIVERY_UNCONFIRMED".
   // The error reports local persistence separately from remote acceptance.
   ```

   AI.24 emits the retained terminal event for this same error; it does not
   create a second delivery outcome or a second error contract.

## Acceptance criteria

- A 3s local request cannot run a 5s-per-leg peer attempt.
- Timeout/disconnect never reports `DAEMON_UNAVAILABLE` while local daemon
  health is intact.
- A cancellation race cannot claim remote success; receiver duplicate-ULID
  semantics make a caller retry safe.
- Runtime shutdown and request cancellation leave no detached tracked work.

## Required validation

Deterministic delayed-peer tests for budget propagation, disconnect,
cancellation, and retry with the original ULID; runtime tracked-work test;
`just lint`; `just test`.

## Non-closure

This sprint does not alter DNS authority or observability vocabulary.
