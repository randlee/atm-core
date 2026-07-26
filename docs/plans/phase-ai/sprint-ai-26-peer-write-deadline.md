---
title: AI.26 end-to-end peer write deadline
status: complete
branch: feature/pAI-s26-peer-write-deadline
target: integrate/phase-AI
depends_on: AI.21-pre, AI.23, AI.11–AI.16
---

# AI.26 — end-to-end peer write deadline

## Release candidate

- First commit: set every releasable ATM assembly to `1.3.2-beta-26`; record
  matching client/daemon values from `atm doctor --json` in runtime evidence.

## Closure

One absolute `RequestDeadline` governs local HTTP admission through peer HTTPS
completion or cancellation; no route creates a longer independent deadline.

## Governing decision

`docs/adr/ADR-041-end-to-end-peer-write-outcome.md` owns the
`RemoteDeliveryUnconfirmed` mapping: after local persistence, deadline,
disconnect, or response-write failure before peer HTTP acceptance maps to
`REMOTE_DELIVERY_UNCONFIRMED`; only an unavailable local daemon maps to
`ATM_DAEMON_UNAVAILABLE`. This sprint implements that already-defined mapping;
AI.27 consumes it for events and doctor output.

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

   AI.27 emits the retained terminal event for this same error; it does not
   create a second delivery outcome or a second error contract.

## Implementation map

- `crates/atm-core/src/api.rs`: retain `RequestDeadline` as the sole public
  deadline value; add no peer-specific deadline type to core.
- `crates/atm-daemon/src/local_ipc_transport/request_worker.rs` and
  `local_tcp_transport.rs`: create the deadline once at HTTP admission and
  pass it unchanged to `ApiRouter`.
- `crates/atm-daemon/src/runtime_health.rs`: pass only remaining budget through
  `PostWriteRouter`; delete its fresh `HttpsRequestDeadline::default()` call.
- `crates/atm-daemon/src/https_transport.rs`: consume `RequestDeadline` for
  DNS/connect/TLS/request/response, and map expiry/disconnect after local
  persistence to the one `RemoteDeliveryUnconfirmed` error.

## Acceptance criteria

- A 3s local request cannot run a 5s-per-leg peer attempt.
- Timeout/disconnect never reports `DAEMON_UNAVAILABLE` while local daemon
  health is intact.
- A cancellation race cannot claim remote success; receiver duplicate-ULID
  semantics make a caller retry safe.
- Runtime shutdown and request cancellation leave no detached tracked work.

The synchronous foreground attempt is one ordinary `WriteRequest` on the
shared peer HTTP endpoint. If its deadline expires after local persistence,
the request ends with `REMOTE_DELIVERY_UNCONFIRMED`; AI.28 may later start a
new bounded recovery drain from the persisted immutable record. It is not a
detached continuation of this request.

## Required validation

Deterministic delayed-peer tests for budget propagation, disconnect,
cancellation, and retry with the original ULID; runtime tracked-work test;
`just lint`; `just test`.

## Non-closure

This sprint does not alter DNS authority or observability vocabulary. The
deadline budget must propagate through the same `Arc<dyn RequestDispatcher>`
call path AI.23 requires for inbound peer requests — this sprint does not
introduce a second dispatch/write path to carry the deadline.
