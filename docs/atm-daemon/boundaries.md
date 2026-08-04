# ATM-Daemon Boundary Inventory

> **Phase AI target — not yet implemented:** the daemon consumes storage traits and may
> not acquire a SQLite/replay boundary through `atm-runtime` or another
> indirection. ADR-036 is the governing crate-topology decision.

This document captures runtime-owned concrete adapters established in Phase R
and tightened for the Phase S cross-platform daemon host line.

Current design assumption:
- `atm-daemon` is the production runtime composition root
- `allowed_dependents: []` means no external crate should depend on these
  daemon-private concrete adapters
- after `AA.5`, `atm-daemon` reaches SQLite-backed stores only through
  `atm-runtime`; a direct `atm-daemon -> atm-storage-rusqlite` dependency is a
  boundary violation guarded by both the boundary TOMLs and
  `cargo test --package atm-architecture`
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
  - must remain separate from socket serving code
  - immutable snapshot publication is the accepted design for readers; no
    daemon-shared mutable cache lock is used
  - **Phase AJ planned extension — not current implementation:** session, pid,
    heartbeat activity, and derived state will be telemetry only. This boundary
    may merge/publish them, but routing, nudge, notification, retry, admission,
    and delivery code must not consume them without an explicit requirement,
    ADR, boundary record, and test.
  - **Phase AJ planned extension — not current implementation:** local
    `ActivityObservation` will be transient request metadata. Only accepted
    heartbeat and successful environment-attested local CLI/graft ingress may
    reach this cache; HTTPS peer ingress must clear it before shared dispatch.
    Changed session/PID values will be diagnostic evidence, never a liveness or
    conflict decision.
  - **Phase AJ planned extension — not current implementation:** removal of the
    former conflict-driven `Degraded` readiness projection is intentional.
    Downstream alerting must consume the retained
    `runtime_observation_metadata_changed` diagnostic event as
    non-authoritative evidence; AJ adds no replacement readiness signal or
    doctor aggregate.

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

## Local transport adapters

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
- the current integrate/phase-S branch still keeps `handle_connection(...)`
  co-located with the listener runtime inside `atm_daemon::local_ipc_transport`
  so request accounting and shutdown remain in one place during Phase S
  closeout; the follow-on partitioning sprint owns the final split

## PeerHttpAdapter

Canonical machine-readable boundary source:
- [../../boundaries/atm-daemon/peer-http-adapter.toml](../../boundaries/atm-daemon/peer-http-adapter.toml)

Purpose:
- AK.4 owns the production trusted-LAN plain-HTTP receiver in
  `atm_daemon::peer_http_listener`.
- The `PeerHttpListenerSet`/`peer_http_listener` names are an AK.4 planned
  forward-reference and are not implemented in this branch. The former
  HTTPS/TLS adapter in `atm_daemon::https_transport` is historical and
  superseded by ADR-047; it has no active boundary record.

Notes:
- When AK.4 lands, `PeerHttpListenerSet` will own plain-HTTP socket
  adaptation, HTTP translation, and peer ingress authentication only.
- This adapter cannot persist, queue, retry, route, or nudge. It has no
  storage, payload, receipt, or delivery-state capability.
- `PeerHttpAdapter` is distinct from the retired `PeerClientTransport` and
  retired `peer_transport` module below; the historical name must not be
  reused for this HTTP adapter.

## Historical: PeerClientTransportAdapter (retired)

`PeerClientTransport`, its `peer_transport` module, and the corresponding
machine-readable boundary manifest were retired during the Phase AI reset.
This section is retained only to explain older references; it describes no
current adapter, module, manifest, timeout/retry, or replay behavior. The
current daemon API contract is the HTTP/OpenAPI surface documented in
[`http-api.md`](./http-api.md).

## Phase AI post-commit admission boundary

AI.31--AI.33 tighten the split between canonical admission and peer work:

- `runtime_health` and `PostWriteRouter` may persist one canonical request,
  choose exactly one local-nudge or peer-delivery work key, and signal the
  daemon-private bounded queue. They must not perform peer-store scans, DNS,
  socket/TLS, HTTP delivery, hooks, or nudge execution before the local
  response.
- `peer_drain_coordinator` is the sole daemon-private owner of post-commit
  peer jobs. Its visible seam is crate-private and returns typed
  `PeerDeliveryOutcome`, not HTTP status integers or concrete transport types.
- the coordinator accesses canonical records only through `atm-storage`
  traits and receives an immutable runtime-view snapshot from composition; it
  must not introduce daemon-specific persistence traits or concrete SQLite
  values.
- `crates/atm-architecture/tests/boundary_enforcement.rs` and the relevant
  `boundaries/atm-daemon/*.toml` records must fail closed if the first boundary
  grows direct peer transport/store work or the second boundary grows durable
  queue/receipt/retry state.

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
- `runtime_health` must not reconstruct deleted notification-worker state or
  reintroduce notification-runtime liveness as a daemon-private health input
