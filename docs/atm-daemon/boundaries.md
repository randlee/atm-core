# ATM-Daemon Boundary Inventory

This document captures runtime-owned concrete adapters established in Phase R
and tightened for the Phase S cross-platform daemon host line.

Current design assumption:
- `atm-daemon` is the production runtime composition root
- `allowed_dependents: []` means no external crate should depend on these
  daemon-private concrete adapters
- Runtime test doubles now exist for the watch/reconcile/notifier lanes so
  boundary tests can exercise the daemon-owned runtimes without bypassing the
  declared contracts.
- Phase `Y.3` tightened the retained runtime shape so normal compatibility
  rewrites now hang off one post-durability runtime refresh owner; `ack` and
  `clear` state transitions must not reintroduce daemon-bypassing source-inbox
  rewrite paths.
- Phase `Y.4` adds the retained delivery-policy coordinator/state-machine seam
  above that owner boundary; harness-specific compatibility-export policy must
  now stay centralized there rather than leaking back into command callers.

Important daemon-private control-plane structs that must stay visible in review,
even though they are not public cross-crate traits:
- `RuntimeComposition` in `atm_daemon::composition`
  - owns startup/shutdown sequencing and lifecycle state transitions
  - must not be skipped in boundary or production-readiness review just because
    it is not itself a public trait boundary
- `LifecycleControlSourceAdapter` / `HostOwnershipAdapter` in `atm_daemon`
  - own process-lifecycle admission and shutdown mechanics
  - the Unix signal-hook implementation is now hidden inside the extracted
    lifecycle-control adapter rather than referenced directly from runtime
    orchestration
  - must remain runtime-private and must not be bypassed by transport or
    business-logic code
- `PreparedRuntimeServer` / `ActiveConnectionRegistry` in `atm_daemon`
  - own listener accept, active connection tracking, drain sequencing, and
    force-cancel escalation
  - must remain runtime-private and must not absorb dispatcher or store logic
- `RuntimeStatusCache` in `atm_daemon::runtime_health`
  - owns live daemon-memory member state and cache-cap semantics
  - hydrates durable team/member truth only through `RosterStore`; it must not
    rediscover teams by walking `ATM_HOME/.claude/teams`
  - must remain separate from socket serving and peer transport code
  - immutable snapshot publication is the accepted design for readers; no
    daemon-shared mutable cache lock is used

## Planned R.20 partition map

The current daemon implementation remains one crate, but the review-visible
daemon-private ownership map is:
- `ownership`
  - `HostOwnershipAdapter`, lock-path helpers, stale-owner recovery
- `server_runtime`
  - `PreparedRuntimeServer`, `ActiveConnectionRegistry`, drain/cancel logic
- `request_runtime`
  - per-connection request execution and request-work accounting
- `runtime_status`
  - `RuntimeStatusCache`, roster hydration, reload assembly, and
    `atm doctor` runtime-health projection
- `peer_transport`
- `watch_runtime`
- `reconcile_runtime`
- `notification_runtime`

`R.20` exists to turn that ownership map into the follow-on cleanup sprint plan
and to make those seams explicit enough for later QA and lint review.

Observability note:
- daemon-owned `sc-observability` sinks are cross-cutting runtime support, not
  a separate daemon-private partition
- the shared daemon observability layer is bottom-of-stack and must not import
  daemon subsystem types
- daemon subsystems may depend on the injected daemon observability trait only
- central daemon observability must not reconstruct subsystem semantics after
  the fact
- the authoritative design contract is
  [`./observability.md`](./observability.md)

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
  `RuntimeLifecycle` state machine plus the runtime-owned
  `LifecycleControlSourceAdapter` and `HostOwnershipAdapter`.
- `run_daemon()` must enter the daemon only through this lifecycle boundary;
  direct listener bootstrap is a boundary violation.

## LocalIpcServerTransportAdapter

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon/socket-server-transport.toml](../../boundaries/atm-daemon/socket-server-transport.toml)


Purpose:
- Owns the same-host runtime listener implementation for the ServerTransport
  contract.

Notes:
- Runtime composition stays in daemon-owned code, but business logic does not.
- The historical machine-readable boundary id remains
  `BOUNDARY-ServerTransport-Socket` for continuity, but the target boundary
  surface is one cross-platform local IPC contract:
  - Unix implementation: Unix domain socket
  - Windows implementation: named-pipe-backed local IPC
- release closeout requires both Unix and Windows implementations to exist
  behind this boundary; non-Unix unsupported-path stubs are an intermediate
  implementation state only
- the local IPC adapter must use the same ATM frame header and request/response
  packet family as the remote peer transport
- the adapter must not treat EOF or half-close as the stable request boundary;
  framed read/write helpers own packet delimiting
- the adapter owns logical endpoint naming and same-user access-control
  semantics; callers above the adapter must not construct Unix socket paths,
  Windows pipe names, or platform-specific ACL details directly
- local-IPC adapter code should live under a dedicated transport module tree
  rather than remaining mixed into crate-root runtime code
- the current integrate/phase-S branch still keeps `handle_connection(...)`
  co-located with the listener runtime inside `atm_daemon::local_ipc_transport`
  so request accounting and shutdown remain in one place during Phase S
  closeout; the follow-on partitioning sprint owns the final split

## LifecycleControlSourceAdapter

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon/lifecycle-control-source.toml](../../boundaries/atm-daemon/lifecycle-control-source.toml)

Purpose:
- Owns the host lifecycle-control source that translates OS-specific shutdown
  and reload events into daemon-private typed control signals.

Notes:
- Unix may implement this boundary through process signals.
- Windows may implement this boundary through console or service-control
  events.
- Callers above this boundary must not branch on `SIG*` constants or Windows
  control-event types directly.
- if one supported operating system lacks a production implementation for this
  boundary, Phase S is not complete

## HostOwnershipAdapter

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon/host-ownership-daemon.toml](../../boundaries/atm-daemon/host-ownership-daemon.toml)

Purpose:
- Owns host-wide singleton admission, stale-owner recovery, and lock metadata
  handling behind one daemon-private portability boundary.

Notes:
- This boundary exists so file locking, owner-record maintenance, and teardown
  rules can be reviewed and linted separately from the transport boundary.
- The target implementation is cross-platform even when individual OS locking
  calls differ.
- Phase S ownership uses stable permanent lock files under `~/.atm/daemon/`
  rather than lock-file path deletion as the ownership signal.
- The preferred implementation foundation is one whole-file exclusive-lock
  contract on:
  - `launch.lock`
  - `owner.lock`
- owner-visible metadata is the documented `pid[:token]` record stored in the
  held lock file contents.
- supported deployment assumes `~/.atm/daemon/` is on a local filesystem with
  working host-local advisory lock semantics; NFS or other network-mounted
  roots are an accepted limitation and are not a supported singleton
  deployment configuration
- singleton, stale-owner recovery, and release ordering semantics must be the
  same on every supported operating system even when the adapter internals
  differ

## Phase S Boundary Guardrails

Phase S adds these review rules for the three daemon portability boundaries:

- only the owned adapter modules may contain operating-system-specific
  `cfg(...)` branching for same-host daemon hosting
- composition, dispatcher, replay, health, watch/reconcile, notifier, and
  request-family code must stay platform-neutral
- shared same-host functional tests must prove the same handler/dispatcher
  contract on Unix and Windows
- `just lint` now includes `same-host-portability`, which rejects broad
  Unix-only same-host gating above the adapter line and non-Unix
  `daemon_unavailable(...)` stubs in production adapter code
- a boundary with only one supported-operating-system implementation is
  incomplete and must not be documented as production-ready
- module-level platform test gates such as `#[cfg(all(test, unix))]` are not
  allowed in daemon-owned test modules; use `#[cfg(unix)]` on individual test
  functions when one assertion is OS-specific

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
- Runtime composition also owns peer-transport config resolution through the
  daemon-side `ConfigIngress` adapter; `PeerClientTransport` must not call the
  workspace config loader directly or silently fall back to defaults after a
  config-load failure.
- The peer transport must reuse the shared ATM frame header and packet DTOs
  used by the same-host local IPC boundary; host-host traffic is not a second
  daemon message system.

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
- accepted `Phase Ye` design is one worker-owned actor lane with bounded
  command input plus per-request reply routing; pending/completed/debounce
  state must not remain daemon-shared mutex state at closure.
- `Y.21` closes the command/reply contract, reply fanout, and shared
  `JoinHandleOwner` lifecycle helper; `Y.22` closes deletion of the remaining
  production shared-state runtime path and moves notification fingerprint
  ownership fully inside `ReconcileWorkerState`.
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
- Phase `Yb` tightens this adapter further:
  - runtime compatibility export and repair/rebuild export must be reviewed as
    separate caller classes
  - only approved delivery executors may invoke normal runtime export
  - repair/rebuild re-export must remain outside the normal send/ack path
  - see:
    - [../phase-Yb/plan-phase-Yb.md](../phase-Yb/plan-phase-Yb.md)
    - [../phase-Yb/lintable-boundary-plan.md](../phase-Yb/lintable-boundary-plan.md)
- `Phase Yc` adds one final recovered-Claude seam requirement:
  - `Y.12` must document the daemon-side adapter behavior for the recovered
    logical-message-set seam through
    `InboxExport::append_message_set(...)` rather than treating
    `DaemonInboxExportAdapter` as append-only by implication
  - the daemon adapter must expose the recovered Claude message-set export as
    one owned `InboxExport` operation, not as repeated single-message appends

## Policy Placement

Compatibility and recovery policy placement for daemon-owned config/inbox adapters:

- `ConfigIngress` may own document loading, syntax validation, and translation into typed ATM config models.
- `ConfigIngress` must not own daemon auto-start policy, retained command fallback policy, or mailbox/task workflow mutation.
- `InboxIngress` may own compatibility-shape translation, identity fingerprint derivation, and ingress diagnostics over imported source files.
- `InboxIngress` must not own read/ack/clear business policy, workflow-state mutation policy, or mailbox lifecycle transitions beyond import normalization.
- `InboxExport` may own projection from ATM-owned source records back into compatibility mailbox shapes and write-bound export validation.
- `InboxExport` must not own read-path reconciliation, task-state updates, or notification/runtime policy.
- one delivery-policy coordinator above `InboxExport` must decide:
  - whether a given event may use compatibility export
  - which harness path applies
  - which event-family state machine owns the transition
- Phase `Yb` adds:
  - the coordinator and state machines must emit one uniform delivery plan
  - the daemon must not branch on harness outside that plan-to-executor seam
  - notification fallback remains a side effect after the plan, not a second
    delivery policy surface
  - plan-to-target translation and transition emission must remain in the
    shared `atm_core` plan/execution seam rather than reappearing in daemon
    adapters

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
- `Y.20` replaces the caller-visible shared queue/lifecycle lock with one
  bounded `sync_channel` producer handoff, immutable runtime-status
  publication, and worker-owned drain/persistence state.
- Runtime lifecycle ownership stays above this boundary:
  - `start()` and `shutdown()` are composition-root responsibilities
  - callers outside `RuntimeComposition` must use
    `NotificationSink::deliver(...)` only and must not open plugin/agent
    delivery paths directly
- `Phase Yc` closed the last daemon/runtime consistency gap for this boundary:
  - `Y.13` ensures the retained runtime factory installs the daemon-owned
    `NotificationSink` adapter on the live send/ack executor path rather than
    allowing direct helper-owned notification execution to survive
  - shutdown remains bounded by the current `3s`
    `NotificationRuntime::shutdown()` deadline; if exceeded, the adapter emits
    a structured warning, returns `AtmErrorCode::DaemonUnavailable`
    (`ATM_DAEMON_UNAVAILABLE`), detaches the join helper, and any still-pending
    queued notification events are treated as dropped

## DaemonNonClaudeOutboundAdapter

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon/daemon-non-claude-outbound.toml](../../boundaries/atm-daemon/daemon-non-claude-outbound.toml)


Purpose:
- Owns the daemon runtime adapter behind the `NonClaudeOutbound` boundary.

Notes:
- This adapter must deliver the same logical `message[]` payload set that the
  Claude path receives.
- It must not downgrade message delivery into notification-only metadata.
- Its callers are limited to the approved delivery executor seam.
- the current daemon-owned adapter is
  `atm_daemon::non_claude_outbound_runtime::DaemonNonClaudeOutbound`
- the current runtime-owned sink is `~/.atm/non_claude_outbound.jsonl`, which
  records the typed non-Claude outbound payload requests for the daemon-owned
  delivery lane

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
- immutable snapshot publication through `ArcSwap` is the accepted design for
  readers; no daemon-shared mutable cache lock is used.
- `Phase Yd` adds one daemon-private liveness DTO family owned by
  `atm_daemon::runtime_health` for final `Phase Y` closeout:
  - `NotificationWorkerLiveness`
  - `RuntimeHealthSnapshot`
  - `project_runtime_health(...)`
- these are daemon-private health projection artifacts, not public cross-crate
  boundary exports
- `runtime_health` may project the owner-provided
  `NotificationRuntime::worker_liveness()` signal directly, but it must not
  inspect queue internals or retry state to reconstruct liveness
