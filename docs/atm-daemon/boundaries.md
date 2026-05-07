# ATM-Daemon Boundary Inventory

This document captures runtime-owned concrete adapters for Phase R.

Current design assumption:
- `atm-daemon` is the production runtime composition root
- `allowed_dependents: []` means no external crate should depend on these
  daemon-private concrete adapters
- Test doubles planned; not yet landed. Until they exist,
  `allowed_test_double_paths` remains empty for the daemon-owned adapter
  records below.

Important daemon-private control-plane structs that must stay visible in review,
even though they are not public cross-crate traits:
- `RuntimeComposition` in `atm_daemon::composition`
  - owns startup/shutdown sequencing and lifecycle state transitions
  - must not be skipped in boundary or production-readiness review just because
    it is not itself a public trait boundary
- `DaemonShutdownSignals` / `SingletonGuard` in `atm_daemon`
  - own process-lifecycle admission and shutdown mechanics
  - must remain runtime-private and must not be bypassed by transport or
    business-logic code

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
- This closes the design gap between the shared ClientTransport contract and daemon-to-daemon remote delivery.

## FileWatchEventSourceAdapter

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon/file-watch-event-source.toml](../../boundaries/atm-daemon/file-watch-event-source.toml)


Purpose:
- Owns the runtime file-watch implementation behind the WatchEventSource contract.

Notes:
- This adapter captures events only; it does not own reconcile policy.

## DaemonReconcileCoordinatorAdapter

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon/daemon-reconcile-coordinator.toml](../../boundaries/atm-daemon/daemon-reconcile-coordinator.toml)


Purpose:
- Owns the runtime implementation of reconcile policy behind the ReconcileCoordinator contract.

Notes:
- This adapter should trigger ingress/service work without bypassing other boundaries.

## DaemonRequestDispatcherAdapter

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon/daemon-request-dispatcher.toml](../../boundaries/atm-daemon/daemon-request-dispatcher.toml)


Purpose:
- Owns the runtime dispatcher implementation that routes protocol requests into core services.

Notes:
- This adapter exists to keep transport loops and service logic separate.

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
- This keeps process-spawn or plugin-style delivery out of service logic.

## DaemonStatusSourceAdapter

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon/daemon-status-source.toml](../../boundaries/atm-daemon/daemon-status-source.toml)


Purpose:
- Owns the daemon runtime adapter behind the StatusSource contract.

Notes:
- Durable roster truth remains separate from runtime status sourcing.
