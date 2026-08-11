# ATM-Daemon Boundary Inventory

> **Phase AL active composition:** `atm-daemon` is a thin Tokio process root.
> It invokes `atm-daemon-bootstrap::run_replacement_daemon`, which acquires the
> owner lock, selects the approved storage factory once, injects the sealed
> received-message hook selector, and starts `atm-http-runtime`'s Axum router.
> The runtime exposes only framework-managed UDS (where supported) and
> capability-authenticated loopback TCP. TLS, replay, and resend are not active
> daemon dependencies. `crates/atm-daemon`'s historical server source is
> reference-only until Phase AM deletes it.

The active machine-readable composition record is
[`../../boundaries/atm-daemon-bootstrap/replacement-bootstrap.toml`](../../boundaries/atm-daemon-bootstrap/replacement-bootstrap.toml).

## Historical legacy-daemon inventory

The remainder of this document records pre-AL daemon-private boundaries for
Phase AM deletion planning. They do not describe an active process path.

This historical inventory captures runtime-owned concrete adapters established
in Phase R and tightened for the Phase S cross-platform daemon host line.

Historical pre-AL design assumption (not active composition):
- `atm_daemon::RuntimeComposition` was the legacy server root. It is now
  reference-only; the active root is the replacement-bootstrap boundary above.
- `allowed_dependents: []` describes the retained legacy types while Phase AM
  prepares their deletion; it does not authorize their use by the executable.
- Direct `atm-daemon -> atm-storage-rusqlite` remains a boundary violation.
  The sole approved concrete factory selection is
  `atm-daemon-bootstrap`, whose active manifest records that exception.
- Phase AD retired the watch/reconcile runtime lanes; any retained
  watch/reconcile/notifier references in older planning material are
  historical only and must not be treated as accepted production subsystem
  contracts or live test-double seams.
- Phase `Y.3` tightened the retained runtime shape so normal compatibility
  rewrites now hang off one post-durability runtime refresh owner; `ack` and
  `clear` state transitions must not reintroduce daemon-bypassing source-inbox
  rewrite paths.
- Phase `Y.4` adds the retained delivery-policy coordinator/state-machine seam
  above that owner boundary; harness-specific compatibility-export policy must
  now stay centralized there rather than leaking back into command callers.
- This daemon-side retained runtime policy coordinator is distinct from the
  `atm-core` delivery policy module, which owns message delivery-plan
  decisions inside the reusable service library.

Historical daemon-private control-plane structs retained for AM deletion review,
even though they are not public cross-crate traits:
- `RuntimeComposition` in `atm_daemon::composition` (retired and deleted by
  AM.3)
  - formerly owned startup/shutdown sequencing and lifecycle state transitions
  - was not selected by active composition before its deletion; this historical
    entry preserves its provenance only
- `LifecycleControlSourceAdapter` / `HostOwnershipAdapter` in `atm_daemon`
  - formerly owned process-lifecycle admission and shutdown mechanics
  - the Unix signal-hook implementation is now hidden inside the extracted
    lifecycle-control adapter rather than referenced directly from runtime
    orchestration
  - are reference-only and must not be reached by active transport or
    business-logic code
- `PreparedRuntimeServer` / `ActiveConnectionRegistry` in `atm_daemon`
  - formerly owned listener accept, active connection tracking, drain
    sequencing, and force-cancel escalation
  - are reference-only; HttpRuntime owns the active framework lifecycle
- `RuntimeStatusCache` in `atm_daemon::runtime_health`
  - formerly owned live daemon-memory member state and cache-cap semantics
  - hydrates durable team/member truth only through `RosterStore`; it must not
    rediscover teams by walking `ATM_HOME/.claude/teams`
  - must remain separate from socket serving code
  - immutable snapshot publication is the accepted design for readers; cache
    mutations are serialized by the daemon-owned writer mutex
  - session, pid, heartbeat activity, and derived state are telemetry only;
    their non-authoritative consumption rule follows the canonical statement in
    the `DaemonStatusSourceAdapter` section below.
  - local `ActivityObservation` is transient request metadata. Only accepted
    heartbeat and successful environment-attested local CLI/graft ingress may
    reach this cache; HTTPS peer ingress must clear it before shared dispatch.
    Changed session/PID metadata follows the canonical non-authoritative rule
    in the `DaemonStatusSourceAdapter` section below.
  - removal of the former conflict-driven `Degraded` readiness projection is
    intentional. Its retained diagnostic metadata follows the canonical
    non-authoritative rule in the `DaemonStatusSourceAdapter` section below.
    The boundary/isolation contract is lint-verified.

## Historical R.20 partition map

The historical daemon implementation remains one crate; its review-visible
daemon-private ownership map was:
- `ownership`
  - `HostOwnershipAdapter`, lock-path helpers, stale-owner recovery
- `server_runtime`
  - `PreparedRuntimeServer`, `ActiveConnectionRegistry`, drain/cancel logic
- `request_runtime`
  - per-connection request execution and request-work accounting
- `runtime_status`
  - `RuntimeStatusCache`, roster hydration, reload assembly, and
    `atm doctor` runtime-health projection

Historical pre-AD planning names that no longer describe the accepted current
daemon-private ownership map:
- `watch_runtime`
- `reconcile_runtime`

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

## Historical RuntimeLifecycleController

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon/runtime-lifecycle-daemon.toml](../../boundaries/atm-daemon/runtime-lifecycle-daemon.toml)

Historical purpose:
- Owned the legacy daemon-private lifecycle/admission controller. AL.8 has
  replaced it with the typed HttpRuntime lifecycle.

Notes:
- This record exists so the control-plane struct is treated as an architectural
  boundary surface even though it is not a public shared trait today.
- The historical implementation was `RuntimeComposition` plus the
  crate-private `RuntimeLifecycle` state machine and related adapters.
- Active composition must enter only through
  `atm_daemon_bootstrap::run_replacement_daemon`; use of `run_daemon()` is a
  boundary violation.

## Historical local transport adapters

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon/socket-server-transport.toml](../../boundaries/atm-daemon/socket-server-transport.toml)


Purpose:
- Own Unix HTTP-over-UDS and loopback-TCP listeners, plus the Windows
  loopback-TCP listener. Every listener authenticates its ingress then calls
  `ApiRouter`.

Notes:
- Runtime composition stays in daemon-owned code, but business logic does not.
- Unix provides HTTP-over-UDS plus loopback TCP; Windows provides loopback TCP
  only. Legacy Windows local transports, custom frame headers, and frame
  decoders are retired and must not be retained as fallback.
- The adapter owns HTTP decode/response translation and same-user endpoint
  ownership only; `ApiRouter` owns route selection and application handlers.
- the adapter owns logical endpoint naming and same-user access-control
  semantics; callers above the adapter must not construct Unix socket paths,
  loopback ports/capabilities, Windows pipe names, or platform-specific ACL
  details directly
- local-IPC adapter code should live under a dedicated transport module tree
  rather than remaining mixed into crate-root runtime code
- Historical: Phase S kept `handle_connection(...)` co-located with the
  listener runtime inside `atm_daemon::local_ipc_transport`. AM.3 deleted that
  legacy local-listener family; it is not an active boundary.

## Historical: PeerClientTransportAdapter (retired)

`PeerClientTransport`, its `peer_transport` module, and the corresponding
machine-readable boundary manifest were retired during the Phase AI reset.
This section is retained only to explain older references; it describes no
current adapter, module, manifest, timeout/retry, or replay behavior. The
current daemon API contract is the HTTP/OpenAPI surface documented in
[`http-api.md`](./http-api.md).

## Phase AI post-commit admission boundary

Historical Phase AI worker model, superseded by AK.2:

- `runtime_health` and `PostWriteRouter` now retain only the ordinary local
  post-write nudge signal. Host-qualified origin admission persists immutable
  data and starts no worker, queue, retry, DNS, socket, or TLS work.
- `crates/atm-architecture/tests/boundary_enforcement.rs` and the relevant
  `boundaries/atm-daemon/*.toml` records fail closed if retired peer worker
  ownership reappears or the local nudge seam grows durable queue/receipt/retry
  state.

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

## FileWatchEventSourceAdapter

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon/file-watch-event-source.toml](../../boundaries/atm-daemon/file-watch-event-source.toml)


Historical status:
- retired by `AD.4`
- retained only as a historical boundary record while deleted code paths age
  out of planning/review references

Purpose:
- Historically owned the runtime file-watch implementation behind the
  WatchEventSource contract.

Notes:
- `AD.4` deleted `atm_daemon::watch_runtime`, its worker thread, and its
  composition wiring.
- No accepted daemon runtime path constructs or starts a watch adapter after
  `AD.4`.
- Any surviving references should be treated as historical documentation, not
  live implementation guidance.

## DaemonReconcileCoordinatorAdapter

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon/daemon-reconcile-coordinator.toml](../../boundaries/atm-daemon/daemon-reconcile-coordinator.toml)


Historical status:
- retired by `AD.4`
- retained only as a historical boundary record while deleted code paths age
  out of planning/review references

Purpose:
- Historically owned the runtime implementation of reconcile policy behind the
  ReconcileCoordinator contract.

Notes:
- `AD.4` deleted `atm_daemon::reconcile_runtime`, the reconcile worker lane,
  and its startup/shutdown composition wiring.
- The deleted lane had been the only accepted caller of daemon-local inbox
  import and watch-trigger plumbing.
- Any surviving references should be treated as historical documentation, not
  live implementation guidance.

## DaemonRequestDispatcherAdapter

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon/daemon-request-dispatcher.toml](../../boundaries/atm-daemon/daemon-request-dispatcher.toml)


Historical status:
- retired by `AI.6`
- retained only as a historical boundary record for the deleted
  `RequestDispatcher` frame boundary

Purpose:
- Historically owned the runtime dispatcher implementation behind the retired
  `RequestDispatcher` contract.

Notes:
- The live daemon dispatcher now implements `atm_core::ApiRouter` directly for
  HTTP-over-UDS requests. It is no longer governed by the retired
  `RequestDispatcher` boundary trait.
- The active `ApiRouter` dispatcher receives typed `ApiRequest` values decoded
  from HTTP-over-UDS and delegates them to the canonical application handlers.
- The dispatcher must not own graft session registration, pending nudge
  queues, fetch/drain inspection, or any client-specific receive loop.
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
- The adapter is for watcher-owned external ingest and other explicitly
  approved comparison/preservation callers only; retained runtime command
  flows must not use it as a generic roster lookup seam.
- the daemon-side adapter no longer exposes generic team-config loading; after
  `Z.8`, external Claude roster ingest is owned by the watch/reconcile lane
  rather than by any generic daemon `ConfigIngress` lookup surface

## DaemonInboxIngressAdapter

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon/daemon-inbox-ingress.toml](../../boundaries/atm-daemon/daemon-inbox-ingress.toml)


Historical status:
- retired by `DAEMON-PREAG-RESET-1`
- retained only as a historical boundary record while deleted code paths age
  out of planning/review references

Purpose:
- Historically owned the daemon runtime adapter behind the SourceIngress
  contract.

Notes:
- The deleted adapter previously owned compatibility inbox import, fingerprint,
  and diagnostic behavior at the daemon boundary.
- Any surviving references should be treated as historical documentation, not
  live implementation guidance.

## DaemonInboxExportAdapter

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon/daemon-inbox-export.toml](../../boundaries/atm-daemon/daemon-inbox-export.toml)


Historical status:
- retired by `DAEMON-PREAG-RESET-1`
- retained only as a historical boundary record while deleted code paths age
  out of planning/review references

Purpose:
- Historically owned the daemon runtime adapter behind the ProjectionExport
  contract.

Notes:
- The deleted adapter previously owned compatibility export and write-bound
  projection behavior at the daemon boundary.
- Any surviving references should be treated as historical documentation, not
  live implementation guidance.

## Policy Placement


Historical status:
- retired by `DAEMON-PREAG-RESET-1`
- retained only as a historical boundary record while deleted code paths age
  out of planning/review references

Purpose:
- Historically documented compatibility and recovery policy placement across
  the `ConfigIngress`, `SourceIngress`, and `ProjectionExport` contracts,
  including a forward-looking delivery-policy-coordinator design.

Notes:
- The `SourceIngress` and `ProjectionExport` contracts and their governing
  adapters were deleted by `DAEMON-PREAG-RESET-1`; the daemon runs as a
  local-IPC-only singleton with no compatibility export/ingress boundary to
  place policy against.
- `ConfigIngress` remains live; its retained placement rules are documented
  under `DaemonConfigIngressAdapter` above, not here.
- Any surviving references to the deleted delivery-policy-coordinator design
  should be treated as historical documentation, not live implementation
  guidance.

## DaemonNotificationSinkAdapter

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon/daemon-notification-sink.toml](../../boundaries/atm-daemon/daemon-notification-sink.toml)


Purpose:
- Historical daemon boundary record only. Phase `AD.5` retired the daemon-owned
  `NotificationSink` adapter and deleted the notification worker/runtime.

Notes:
- This record remains only to preserve historical references from earlier
  phases and ADRs.
- The accepted daemon runtime no longer starts a notification worker or owns a
  `NotificationSink` adapter for post-send behavior.
- Post-send notification logging, when retained, is a direct append at the
  event site.

## Historical DaemonNonClaudeOutboundAdapter

AM.6 verified that `DaemonNonClaudeOutbound` had no non-test caller in the
selected runtime. It and its retired boundary manifest are deleted; the active
replacement composition selects its backend-neutral outbound implementation
through `RuntimeAssembly` instead.

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
  readers; cache mutations are serialized by the daemon-owned writer mutex.
- Runtime observation is non-authoritative. Cache merge and snapshot projection
  may inspect it; routing, nudge, notification, retry, admission, delivery,
  and policy must not. The machine-readable
  `runtime_observation_non_authoritative` review gate is enforced by the
  passing `runtime-observation-boundary` lint.
- `Phase Yd` adds one daemon-private liveness DTO family owned by
  `atm_daemon::runtime_health` for final `Phase Y` closeout:
  - `NotificationWorkerLiveness`
  - `RuntimeHealthSnapshot`
  - `project_runtime_health(...)`
- these are daemon-private health projection artifacts, not public cross-crate
  boundary exports
- `runtime_health` must not reconstruct deleted notification-worker state or
  reintroduce notification-runtime liveness as a daemon-private health input
- Heartbeat and authenticated local dispatch observations converge through the
  same cache merge. Changed PID or session metadata is retained as the
  non-authoritative `runtime_observation_metadata_changed` event; it is not a
  readiness, doctor, admission, or retry policy signal.
