# ATM-Core Boundary Inventory

This document is the initial Phase R boundary inventory for `atm-core`.

Contract-owner records intentionally use:
- `implementation.type: null`
- `implementation.module: null`
- `implementation.visibility: trait_only`
- `implementation.constructor: none`

That means `atm-core` owns the public contract and semantics, but not a default
concrete adapter in this crate.

Test doubles planned; not yet landed. Until they exist, `allowed_test_double_paths`
remains empty for the `atm-core` contract-owner records below.

## AtmProtocol

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/atm-protocol.toml](../../boundaries/atm-core/atm-protocol.toml)


Purpose:
- Owns the shared ATM request/response contract used by local CLI, daemon, and
  thin extension clients.

Notes:
- This is the first boundary updated explicitly for `atm-graft`.
- The canonical request/response family now includes:
  - send-shaped `Send` envelopes for compose and acknowledge
  - typed `Heartbeat` request/response envelopes for daemon runtime-state
    ownership
  - `HeartbeatActivity` / `TeamMemberHeartbeat{Request,Response}` as the
    canonical daemon-owned member-liveness DTO family added in `R.15`
  - `RuntimeStatusSnapshot` as the daemon-health/status DTO consumed by
    `atm doctor`

## ClientTransport

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/client-transport.toml](../../boundaries/atm-core/client-transport.toml)


Purpose:
- Owns the outbound ATM request path from thin clients into a server/runtime.

Notes:
- The public workflow surface above this boundary should stay centered on send
  and receive.
- Thin graft-facing clients should layer typed workflow/session DTOs above this
  transport rather than publishing daemon-shaped transport details directly.
- Shared same-host daemon bootstrap helpers used by transport consumers now
  live in `atm-daemon-client`; this boundary still owns the request/response
  transport contract itself.

## AtmGraftClient / GraftSessionPort

Purpose:
- Own the thin embedded ATM client surface that `atm-graft` consumes without a
  Rust dependency on `atm-daemon`.

Notes:
- These are intentionally open traits, not sealed boundary traits.
- `AtmGraftClient` owns the unary `send` / `read` / `ack` daemon request
  surface for embedded consumers.
- `GraftSessionPort` owns graft-session registration plus nudge fetch/drain
  session contracts.
- The graft-facing public API must stay small and typed rather than exposing
  raw `RequestEnvelope` / `ResponseEnvelope` values.

## WatchEventSource

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/watch-event-source.toml](../../boundaries/atm-core/watch-event-source.toml)


Purpose:
- Owns filesystem watch event capture and delivery to the runtime reconcile layer.

Notes:
- This keeps raw watch APIs out of store, transport, and service logic.

## ReconcileCoordinator

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/reconcile-coordinator.toml](../../boundaries/atm-core/reconcile-coordinator.toml)


Purpose:
- Owns watch-driven reconcile policy and ingress triggering above raw watch events.

Notes:
- This closes the missing watch/reconcile boundary gap in the initial Phase R set.

## ServerTransport

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/server-transport.toml](../../boundaries/atm-core/server-transport.toml)


Purpose:
- Owns inbound ATM request serving and response framing for runtime hosts.

Notes:
- Listener/runtime code should remain thin and dispatch through RequestDispatcher.

## RequestDispatcher

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/request-dispatcher.toml](../../boundaries/atm-core/request-dispatcher.toml)


Purpose:
- Owns routing of typed protocol requests to the correct service handlers.

Notes:
- Transport-specific listeners should not embed request-family logic.

## BOUNDARY-MailStore-Sqlite

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/mail-store.toml](../../boundaries/atm-core/mail-store.toml)


Purpose:
- Owns durable message lifecycle and mailbox-facing state.

Notes:
- This stays the canonical durable truth behind send and receive workflows.

## BOUNDARY-TaskStore-Sqlite

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/task-store.toml](../../boundaries/atm-core/task-store.toml)


Purpose:
- Owns durable task-domain state and task/message linkage.

Notes:
- `ack` is not a top-level public method, but it still mutates task state.

## BOUNDARY-RosterStore-Sqlite

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/roster-store.toml](../../boundaries/atm-core/roster-store.toml)


Purpose:
- Owns durable roster state and routing-relevant member metadata.

Notes:
- Runtime status remains outside durable roster ownership.

## ConfigIngress

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/config-ingress.toml](../../boundaries/atm-core/config-ingress.toml)


Purpose:
- Owns loading and validating persisted ATM/team configuration into typed models.

Notes:
- This is one of the main explicit corrections to the Phase Q leakage.

## InboxIngress

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/inbox-ingress.toml](../../boundaries/atm-core/inbox-ingress.toml)


Purpose:
- Owns import from compatibility/shared inbox surfaces into ATM-owned state.

Notes:
- The import path stays separate from durable store ownership.

## InboxExport

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/inbox-export.toml](../../boundaries/atm-core/inbox-export.toml)


Purpose:
- Owns projection of ATM-owned state back to compatibility/shared inbox surfaces.

Notes:
- This is the write-facing sibling of InboxIngress, not a general store boundary.

## NotificationSink

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/notification-sink.toml](../../boundaries/atm-core/notification-sink.toml)


Purpose:
- Owns outward delivery of notifications, hooks, or plugin-facing events.

Notes:
- This replaces direct `Command::new` use in business-flow code.

## StatusSource

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/status-source.toml](../../boundaries/atm-core/status-source.toml)


Purpose:
- Owns inbound runtime status/activity updates before they become ATM-visible state.

Notes:
- This stays distinct from `RosterStore` to avoid durable/live-state collapse.
- The live snapshot contract now carries liveness, readiness, singleton-owner,
  SQLite-ready, degraded-ingest, and member-count fields rather than a generic
  placeholder string.
