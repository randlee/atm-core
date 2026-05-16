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
- The canonical request/response family now includes:
  - send-shaped `Send` envelopes for compose and acknowledge
  - typed `Heartbeat` request/response envelopes for daemon runtime-state
    ownership
  - typed advisory-session envelopes for:
    - register
    - unregister
    - fetch
    - drain
    - live advisory stream
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
- Thin clients must use this shared boundary and must not take a dependency on
  `atm-daemon` internals.
- `atm-graft` now lands as one such thin client crate and is expected to stay
  on this boundary plus the shared ATM envelopes rather than on any daemon-
  private request family.
- Long-lived advisory registration, fetch/drain inspection, and live advisory
  stream traffic are part of this shared boundary family rather than a
  plugin-private daemon API.

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

## MailStore

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/mail-store.toml](../../boundaries/atm-core/mail-store.toml)


Purpose:
- Owns durable message lifecycle and mailbox-facing state.

Notes:
- This stays the canonical durable truth behind send and receive workflows.
- Retained command/runtime code now resolves mailbox durability only through the
  installed store-backed runtime factory.
- If that runtime factory is unavailable, ATM must fail closed with shared ATM
  errors instead of selecting a second mailbox backend.
- Compatibility inbox files remain ingress/export surfaces, not a parallel
  durable mailbox implementation behind retained command logic.

## TaskStore

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/task-store.toml](../../boundaries/atm-core/task-store.toml)


Purpose:
- Owns durable task-domain state and task/message linkage.

Notes:
- `ack` is not a top-level public method, but it still mutates task state.

## RosterStore

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/roster-store.toml](../../boundaries/atm-core/roster-store.toml)


Purpose:
- Owns durable roster state and routing-relevant member metadata.

Notes:
- Runtime status remains outside durable roster ownership.
- Durable roster truth is the canonical member model only; `config.json`
  documents are ingress inputs and daemon-owned live `pid` state stays outside
  this boundary.

## ConfigIngress

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/config-ingress.toml](../../boundaries/atm-core/config-ingress.toml)


Purpose:
- Owns loading and validating persisted ATM/team configuration into typed models.

Notes:
- This is one of the main explicit corrections to earlier boundary leakage.

## InboxIngress

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/inbox-ingress.toml](../../boundaries/atm-core/inbox-ingress.toml)


Purpose:
- Owns import from compatibility/shared inbox surfaces into ATM-owned state.

Notes:
- The import path stays separate from durable store ownership.
- The hidden `atm_core::boundary_support` helper module is the only retained
  implementation seam that may still touch compatibility inbox source files for
  this boundary family.

## InboxExport

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/inbox-export.toml](../../boundaries/atm-core/inbox-export.toml)


Purpose:
- Owns projection of ATM-owned state back to compatibility/shared inbox surfaces.

Notes:
- This is the write-facing sibling of InboxIngress, not a general store boundary.
- Retained command/runtime code must reach compatibility inbox export only
  through the daemon-owned ingress/export seam; it must not treat export-file
  reads as a second source of mailbox truth.

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
