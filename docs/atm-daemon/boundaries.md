# ATM-Daemon Boundary Inventory

This document captures runtime-owned concrete adapters for Phase R.

Current design assumption:
- `atm-daemon` is the production runtime composition root
- `allowed_dependents: []` means no external crate should depend on these
  daemon-private concrete adapters
- Runtime test doubles now exist for the watch/reconcile/notifier lanes so
  boundary tests can exercise the daemon-owned runtimes without bypassing the
  declared contracts.

Important daemon-private control-plane structs that must stay visible in review,
even though they are not public cross-crate traits:
- `RuntimeComposition` in `atm_daemon::composition`
  - owns startup/shutdown sequencing and lifecycle state transitions
  - must not be skipped in boundary or production-readiness review just because
    it is not itself a public trait boundary
- `DaemonShutdownSignals` / `SingletonGuard` in `atm_daemon`
  - own process-lifecycle admission and shutdown mechanics
  - `DaemonShutdownSignals` installs OS signal hooks only on Unix; non-Unix
    builds use the no-op terminate-flag fallback in `peer_transport`
  - must remain runtime-private and must not be bypassed by transport or
    business-logic code
- `PreparedRuntimeServer` / `ActiveConnectionRegistry` in `atm_daemon`
  - own listener accept, active connection tracking, drain sequencing, and
    force-cancel escalation
  - must remain runtime-private and must not absorb dispatcher or store logic
- `RuntimeStatusCache` in `atm_daemon::runtime_health`
  - owns live daemon-memory member state and cache-cap semantics
  - must remain separate from socket serving and peer transport code

## Planned R.20 partition map

The current daemon implementation remains one crate, but the review-visible
daemon-private ownership map is:
- `ownership`
  - `SingletonGuard`, lock-path helpers, stale-owner recovery
- `server_runtime`
  - `PreparedRuntimeServer`, `ActiveConnectionRegistry`, drain/cancel logic
- `request_runtime`
  - per-connection request execution and request-work accounting
- `runtime_status`
  - `RuntimeStatusCache`, roster hydration, reload assembly
- `doctor_projection`
  - runtime-health projection for `atm doctor`
- `peer_transport`
- `watch_runtime`
- `reconcile_runtime`
- `notification_runtime`

`R.20` exists to turn that ownership map into the follow-on cleanup sprint plan
and to make those seams explicit enough for later QA and lint review.

## RuntimeLifecycleController

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon/runtime-lifecycle-daemon.toml](../../boundaries/atm-daemon/runtime-lifecycle-daemon.toml)

Purpose:
- Own the daemon-private runtime lifecycle/admission controller that coordinates
  startup, shutdown, and singleton ownership.

Notes:
- This record exists so the control-plane struct is treated as an architectural
  boundary surface even though it is not a public shared trait today.
- The active implementation is `RuntimeComposition` plus the crate-private
  `RuntimeLifecycle` state machine and the runtime-owned
  `DaemonShutdownSignals` / `SingletonGuard` helpers.
- `run_daemon()` must enter the daemon only through this lifecycle boundary;
  direct listener bootstrap is a boundary violation.

## SocketServerTransportAdapter

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon/socket-server-transport.toml](../../boundaries/atm-daemon/socket-server-transport.toml)


Purpose:
- Owns the runtime listener implementation for the ServerTransport contract.

Notes:
- Runtime composition stays in daemon-owned code, but business logic does not.

## PeerClientTransportAdapter

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon/peer-client-transport.toml](../../boundaries/atm-daemon/peer-client-transport.toml)


Purpose:
- Owns the daemon-side outbound client transport used for remote peer delivery.

Notes:
- The concrete `PeerClientTransport` implementation stays runtime-private inside
  `atm_daemon::peer_transport`.
- Runtime composition owns replay resume and exposes the transport only through
  the shared `ClientTransport` contract.

## FileWatchEventSourceAdapter

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon/file-watch-event-source.toml](../../boundaries/atm-daemon/file-watch-event-source.toml)


Purpose:
- Owns the runtime file-watch implementation behind the WatchEventSource contract.

Notes:
- The active implementation is a daemon-owned polling subscription registry in
  `atm_daemon::watch_runtime`.
- It maintains long-lived watch state behind the boundary and refreshes
  registered subscriptions on a bounded wake interval.
- The subscription registry is explicitly bounded to 256 keys per daemon
  process; callers must not assume unbounded watch-state retention.
- `WatchEventSource::poll(...)` now returns the worker-owned snapshot/error
  state instead of running direct synchronous discovery in the caller.
- Shutdown is observed between polling iterations; one in-flight synchronous
  filesystem scan may complete before the watch worker exits.
- This adapter captures events only; it does not own reconcile policy.
- Runtime lifecycle ownership stays above this boundary:
  - `start()` and `shutdown()` are composition-root responsibilities
  - callers outside `RuntimeComposition` must use `WatchEventSource::poll(...)`
    only and must not bootstrap or tear down the runtime directly

## DaemonReconcileCoordinatorAdapter

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon/daemon-reconcile-coordinator.toml](../../boundaries/atm-daemon/daemon-reconcile-coordinator.toml)


Purpose:
- Owns the runtime implementation of reconcile policy behind the ReconcileCoordinator contract.

Notes:
- The active implementation is a daemon-owned debounce/coalesce worker in
  `atm_daemon::reconcile_runtime`.
- It triggers watch polling, inbox ingress, and notifier callbacks only through
  their owned boundaries; it does not reach around into store or transport
  internals.
- Notification delivery in the reconcile path is boundary-only; tests exercise
  fake `NotificationSink` implementations rather than plugin/runtime internals.
- Runtime lifecycle ownership stays above this boundary:
  - `start()` and `shutdown()` are composition-root responsibilities
  - callers outside `RuntimeComposition` must use
    `ReconcileCoordinator::reconcile(...)` only and must not manage worker
    lifetime directly

## DaemonRequestDispatcherAdapter

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon/daemon-request-dispatcher.toml](../../boundaries/atm-daemon/daemon-request-dispatcher.toml)


Purpose:
- Owns the runtime dispatcher implementation that routes protocol requests into core services.

Notes:
- This adapter exists to keep transport loops and service logic separate.
- The active dispatcher now owns:
  - typed heartbeat request routing
  - durable pid continuity checks through the SQLite boundary assembly
  - daemon-backed doctor health projection over runtime status
- `R.20` planning treats this as an overgrown adapter surface. The follow-on
  cleanup sprint must split dispatcher shell concerns from runtime-status,
  heartbeat-continuity, and doctor-projection helpers without changing the
  external boundary contract.

## DaemonConfigIngressAdapter

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon/daemon-config-ingress.toml](../../boundaries/atm-daemon/daemon-config-ingress.toml)


Purpose:
- Owns the daemon runtime adapter behind the ConfigIngress contract.

Notes:
- This adapter owns the daemon-side `ConfigIngress` implementation at the adapter boundary.

## DaemonInboxIngressAdapter

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon/daemon-inbox-ingress.toml](../../boundaries/atm-daemon/daemon-inbox-ingress.toml)


Purpose:
- Owns the daemon runtime adapter behind the InboxIngress contract.

Notes:
- This adapter owns compatibility inbox import, fingerprint, and diagnostic behavior at the daemon boundary.

## DaemonInboxExportAdapter

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon/daemon-inbox-export.toml](../../boundaries/atm-daemon/daemon-inbox-export.toml)


Purpose:
- Owns the daemon runtime adapter behind the InboxExport contract.

Notes:
- This adapter owns compatibility export and write-bound projection behavior at the daemon boundary.

## Policy Placement

Compatibility and recovery policy placement for daemon-owned config/inbox adapters:

- `ConfigIngress` may own document loading, syntax validation, and translation into typed ATM config models.
- `ConfigIngress` must not own daemon auto-start policy, retained command fallback policy, or mailbox/task workflow mutation.
- `InboxIngress` may own compatibility-shape translation, identity fingerprint derivation, and ingress diagnostics over imported source files.
- `InboxIngress` must not own read/ack/clear business policy, workflow-state mutation policy, or mailbox lifecycle transitions beyond import normalization.
- `InboxExport` may own projection from ATM-owned source records back into compatibility mailbox shapes and write-bound export validation.
- `InboxExport` must not own read-path reconciliation, task-state updates, or notification/runtime policy.

## DaemonNotificationSinkAdapter

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon/daemon-notification-sink.toml](../../boundaries/atm-daemon/daemon-notification-sink.toml)


Purpose:
- Owns the daemon runtime adapter behind the NotificationSink contract.

Notes:
- The active implementation is a daemon-owned queued worker in
  `atm_daemon::notification_runtime`.
- It returns typed unavailable/backpressure failures at the boundary and
  persists delivered events through the runtime-owned notifier path instead of
  degrading to tracing-only behavior.
- The queue is intentionally bounded at `64` events; overflow fails closed with
  typed backpressure instead of silently buffering unbounded plugin traffic.
- Runtime lifecycle ownership stays above this boundary:
  - `start()` and `shutdown()` are composition-root responsibilities
  - callers outside `RuntimeComposition` must use
    `NotificationSink::deliver(...)` only and must not open plugin/agent
    delivery paths directly

## DaemonStatusSourceAdapter

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon/daemon-status-source.toml](../../boundaries/atm-daemon/daemon-status-source.toml)


Purpose:
- Owns the daemon runtime adapter behind the StatusSource contract.

Notes:
- Durable roster truth remains separate from runtime status sourcing.
- The active implementation is a daemon-memory status cache shared with the
  dispatcher and projected into `atm doctor`.
- The cache-cap rule must bound actual retained entries, not only member-state
  labels.
