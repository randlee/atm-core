# ATM-Core Boundary Inventory

> **Phase AI supersession notice:** storage contracts remain backend-neutral.
> ADR-036 retires `RemoteReplayStore`, `RuntimeStorageFinalizer`, and any
> daemon-specific persistence trait; retained boundary records must not be used
> to recreate them.
>
> **Phase AI transport supersession:** `AtmProtocol`, `ClientTransport`,
> `ServerTransport`, and `RequestDispatcher` describe the retired custom-frame
> line until AI.6. The accepted target is ADR-033's HTTP router over UDS/HTTPS,
> with transport-neutral requests and one canonical write handler; these legacy
> records must not be extended or used to add a parallel route.

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

## AtmProtocol (historical through AI.5)

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/atm-protocol.toml](../../boundaries/atm-core/atm-protocol.toml)

Historical status:
- retired by AI.6 in favor of ADR-033's HTTP/OpenAPI request contract
- retained protocol DTOs in `atm_core::protocol` are data contracts, not a
  live `AtmProtocol` trait boundary

Purpose:
- Historically owned the custom framed request/response contract. AI.6 retires
  it in favor of ADR-033's HTTP/OpenAPI request contract.

Notes:
- The canonical request/response family now includes:
  - send-shaped `Send` envelopes for compose and acknowledge
  - typed `Heartbeat` request/response envelopes for daemon runtime-state
    ownership
  - `HeartbeatActivity` / `TeamMemberHeartbeat{Request,Response}` as the
    canonical daemon-owned member-liveness DTO family added in `R.15`
  - `RuntimeStatusSnapshot` as the daemon-health/status DTO consumed by
    `atm doctor`; after `AA.3` it carries daemon-owned runtime state only and
    no store-specific readiness fields
- `atm-runtime-test-support` is an allowed workspace-local dependent for the
  retained-runtime test harness seam; it is not a production consumer
  boundary.

## ClientTransport (historical through AI.5)

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/client-transport.toml](../../boundaries/atm-core/client-transport.toml)

Historical status:
- retired by AI.6 in favor of one HTTP/UDS `DaemonApiClient` adapter
- retained daemon launch/bootstrap helpers live below `atm-daemon-client`, not
  this atm-core boundary

Purpose:
- Historically owned the outbound custom-frame client path. AI.6 replaces it
  with one HTTP/UDS `DaemonApiClient` adapter used by CLI, graft, Python, and
  in-process contract tests.

Notes:
- The public workflow surface above this boundary should stay centered on send
  and receive.
- Thin clients must use this shared boundary and must not take a dependency on
  `atm-daemon` internals.
- `atm-graft` now lands as one such thin client crate and is expected to stay
  on this boundary plus the shared ATM envelopes rather than on any daemon-
  private request family.
- Receiver-private lifecycle, buffering, or wakeup state must not be promoted
  into shared transport methods or shared request/response DTOs on this
  boundary.

## WatchEventSource

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/watch-event-source.toml](../../boundaries/atm-core/watch-event-source.toml)

Historical status:
- retired from the accepted runtime by `ADR-019`
- any surviving references are deletion-planning or historical boundary records
  only
Purpose:
- historically owned filesystem watch event capture and delivery to the runtime
  reconcile layer.

Notes:
- on the earlier compatibility line, this kept raw watch APIs out of store,
  transport, and service logic.

## ReconcileCoordinator

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/reconcile-coordinator.toml](../../boundaries/atm-core/reconcile-coordinator.toml)

Historical status:
- retired from the accepted runtime by `ADR-019`
- any surviving references are deletion-planning or historical boundary records
  only
Purpose:
- historically owned watch-driven reconcile policy and ingress triggering above
  raw watch events.

Notes:
- on the earlier compatibility line, this closed the missing watch/reconcile
  boundary gap in the initial Phase R set.

## ServerTransport (historical through AI.5)

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/server-transport.toml](../../boundaries/atm-core/server-transport.toml)

Historical status:
- retired by AI.6 in favor of ADR-033 HTTP/OpenAPI routing
- runtime adapters translate HTTP and call `ApiRouter`; this is not a live
  atm-core trait boundary

Purpose:
- Historically owned inbound custom-frame serving. AI.6 replaces it with an
  HTTP adapter that only translates HTTP and calls `ApiRouter`.

Notes:
- Listener/runtime code should remain thin and dispatch through RequestDispatcher.

## RequestDispatcher (historical through AI.5)

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/request-dispatcher.toml](../../boundaries/atm-core/request-dispatcher.toml)

Historical status:
- retired by AI.6 in favor of `ApiRouter` and the canonical typed handler path
- retained references are historical boundary inventory only

Purpose:
- Historically routed custom-frame protocol requests. AI.6 replaces it with
  `ApiRouter`, which selects a shared typed handler and does not own storage,
  host routing, or nudges.

Notes:
- Transport-specific listeners should not embed request-family logic.

## MailStore

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/mail-store.toml](../../boundaries/atm-core/mail-store.toml)

Transitional status:
- active legacy compile bridge while retained production callers still route
  through `atm-runtime::legacy_storage_adapters`
- the authoritative long-term message contract lives in
  `crates/atm-storage::MessageStore`

Purpose:
- Bridges legacy `atm-core::boundary::MailStore` callers onto the
  storage-owned durable message lifecycle contract.

Notes:
- This stays the canonical durable truth behind send and receive workflows.
- Retained command/runtime code now resolves mailbox durability only through the
  installed store-backed runtime factory.
- If that runtime factory is unavailable, ATM must fail closed with shared ATM
  errors instead of selecting a second mailbox backend.
- Compatibility inbox files remain ingress/export surfaces, not a parallel
  durable mailbox implementation behind retained command logic.
- After `Y.3`, retained `send` reaches compatibility rewrite only through the
  post-commit runtime refresh owner; retained `ack` and `clear` no longer own
  source-inbox compatibility rewrites.
- `MailStoreDoctor` is the paired subsystem-owned diagnostics boundary for
  path resolution, openability, schema/bootstrap/migration readiness, and
  bounded store findings.

## MailStoreDoctor

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/mail-store-doctor.toml](../../boundaries/atm-core/mail-store-doctor.toml)

Purpose:
- Own durable mail-store diagnostics without moving backend-specific diagnosis
  into daemon or CLI code.

## RosterStore

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/roster-store.toml](../../boundaries/atm-core/roster-store.toml)

Transitional status:
- active legacy compile bridge while retained production callers still route
  through `atm-runtime::legacy_storage_adapters`
- the authoritative long-term roster contract lives in
  `crates/atm-storage::RosterStore`

Purpose:
- Bridges legacy `atm-core::boundary::RosterStore` callers onto the
  storage-owned durable roster state contract.

Notes:
- Runtime status remains outside durable roster ownership.
- Durable roster truth is the canonical team/member model used for daemon
  runtime hydration; `config.json` documents are ingress inputs and daemon-owned
  live `pid` state stays outside this boundary.
- `RosterStoreDoctor` is the paired subsystem-owned diagnostics boundary for
  bounded roster-store findings.

## RosterStoreDoctor

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/roster-store-doctor.toml](../../boundaries/atm-core/roster-store-doctor.toml)

Purpose:
- Own durable roster-store diagnostics without moving backend-specific
  diagnosis into daemon or CLI code.

## NudgeTemplateOverrideStore

Canonical machine-readable boundary source:
- [../../boundaries/atm-storage/nudge-template-override-store.toml](../../boundaries/atm-storage/nudge-template-override-store.toml)

Purpose:
- expose the stable `atm-core::boundary` compatibility facade for the
  storage-owned team-scoped built-in nudge template override contract

Notes:
- This boundary exists specifically so built-in nudge override lookup resolves
  upstream of `PostSendHookEmitter`.
- canonical ownership now lives in `atm-storage`; `atm-core` re-exports the
  moved trait and storage-neutral row/kind types so retained compile-bridge
  consumers do not break during cutover
- `atm-core` retains only
  `built_in_nudge_template_kind_from_post_send_event(...)` because it depends
  on the core-owned `PostSendHookEvent`
- the first concrete implementation remains `atm-storage-rusqlite`
- [../adr/ADR-024-nudge-template-override-storage-ownership-relocation.md](../adr/ADR-024-nudge-template-override-storage-ownership-relocation.md)
  supersedes ADR-021's older `atm-core` ownership assumption
- `atm` remains the owner of the six built-in product template bodies and the
  bounded placeholder substitution/rendering policy.
- Accepted row semantics are explicit:
  - no row => product default
  - override row => stored non-empty template body
  - disabled row => no built-in nudge emission
  - clear/reset => row deletion
- the durable ack classifier used by mailbox metadata and retained list/read
  projections now lives beside this contract in `atm-storage`, not in
  `atm-core`

## Phase AA Runtime Composition Adjuncts

Purpose:
- Own storage-neutral runtime assembly contracts without letting concrete
  backend knowledge become daemon-private or SQLite-private. Remote replay is
  not an accepted shared contract.

Owned shared contracts:
- `DoctorFinding`
- `RuntimeDoctorPorts`

Notes:
- `RuntimeDoctorPorts` groups the installed storage-neutral doctor handles
  that callers consume after `atm-runtime` assembles the concrete backend.
- `RuntimeAssembly` now carries the generic `StorageBackends<M, R>` seam plus
  the legacy compile-bridge `MailStore` / `RosterStore` handles used during
  AC.4 consumer cutover.
- `AA.4` relies on these adjunct contracts to remove the direct
  `atm-daemon -> atm-rusqlite` dependency while keeping shutdown finalization
  storage-neutral at the daemon boundary. Any retained lifecycle operation is
  storage-owned; it is not a `RuntimeStorageFinalizer` boundary. ADR-038's
  canonical-record query is a narrow storage trait method, not a
  `RemoteReplayStore` replacement.

## ConfigIngress

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/config-ingress.toml](../../boundaries/atm-core/config-ingress.toml)


Purpose:
- Owns loading and validating persisted ATM/team configuration into typed models.

Notes:
- This is one of the main explicit corrections to earlier boundary leakage.
- `atm-daemon-client` may consume this boundary only for canonical ATM-owned
  caller/environment/config and daemon-endpoint resolution used by shared
  same-host bootstrap.
- canonical ATM roster truth does not live here; normal retained runtime
  membership checks must use `RosterStore` / `ProjectionRoster` instead
- the `Z.6` send warning path is allowed to mention the underlying
  `config.json` mismatch in returned warning text, but it must obtain member
  truth from `ProjectionRoster` rather than from `ConfigIngress`
- approved surviving callers after the `Phase Z` follow-on line are:
  - historical watcher / reconcile ingest on the pre-`ADR-019` line
  - `doctor` comparison
  - narrow recreated-shell preservation reads during restore
- watcher/reconcile has since been retired from the accepted runtime; only the
  explicit comparison/preservation callers remain accepted
- before `Z.8`, one temporary startup-only bridge was allowed outside the
  trait surface:
  - `atm_core::boundary_support::hydrate_roster_from_team_config_once_at_startup_if_empty(...)`
  - it could seed canonical ATM roster state only when the roster was empty at
    daemon startup
  - it was explicitly allowlisted in `Z.7` and deleted in `Z.8`
- generic retained command/runtime `load_team_config(...)` lookup behavior is a
  boundary violation and should be mechanically gated by repository-local lint
  / later `sc-lint` extraction
- active follow-on lint families now gate:
  - `SCB-RETAINED-001`: direct command-entry or team-admin
    `service_runtime_store::default_runtime()` reachability in `atm teams`,
    `atm members`, or `atm teams add-member`
  - `SCB-WORKSPACE-001`: direct command/team-admin ambient `.atm.toml` /
    `load_config(...)` reads outside the approved seam
- later follow-on lint families should also gate:
  - `SCB-SINGLETON-001`: public ambient singleton/runtime-factory exposure
    that bypasses approved wrappers; accepted branches must expose retained
    runtime installation only through the bounded hidden hooks and approved
    crate wrappers landed in `Z.14`
- those rule families must distinguish:
  - pre-existing survivors explicitly recorded in TOML allowlists with owner
    and sunset-sprint metadata
  - new violations, which fail lint immediately

## ConfigDoctor

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/config-doctor.toml](../../boundaries/atm-core/config-doctor.toml)

Purpose:
- Own config-specific diagnosis so daemon/CLI callers aggregate typed config
  findings instead of embedding backend-specific config investigation logic.

## InboxIngress / SourceIngress

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/inbox-ingress.toml](../../boundaries/atm-core/inbox-ingress.toml)

Historical status:
- retired by AI.1 with the deletion of daemon peer/compatibility adapters
- retained only as a historical boundary record; no accepted runtime path
  constructs or calls this contract

Purpose:
- Historically owned import from compatibility/shared inbox surfaces into
  ATM-owned state.

Notes:
- It is not a live store, daemon, or transport extension point.
- Any future compatibility import requires a new approved contract; this
  retired record must not be reactivated by adding an implementation.

## InboxExport / ProjectionExport

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/inbox-export.toml](../../boundaries/atm-core/inbox-export.toml)

Historical status:
- retired by AI.1 with the deletion of daemon peer/compatibility adapters
- retained only as a historical boundary record; no accepted runtime path
  constructs or calls this contract

Purpose:
- Historically owned projection of ATM-owned state back to compatibility/shared
  inbox surfaces.

Notes:
- It is not a live store, daemon, or transport extension point.
- Any future compatibility projection requires a new approved contract; this
  retired record must not be reactivated by adding an implementation.

## PostSendHookEmitter

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/post-send-hook-emitter.toml](../../boundaries/atm-core/post-send-hook-emitter.toml)


Purpose:
- Owns the one accepted post-commit recipient-emission seam for post-send
  behavior after durable message persistence succeeds.

Notes:
- Phase `AD` established this boundary as the replacement for
  `DeliveryPlan`/`NotificationSink` post-send routing on the accepted send/ack
  path.
- `send` / `ack` remain responsible for:
  - persistence success
  - deciding whether the recipient exposes post-send capability
  - logging emission failure
  - constructing sender-visible warnings on emission failure
- accepted send/ack finalization emits post-send directly from persisted
  logical messages on the accepted runtime with no retained compatibility
  delivery-plan executor on this path
- the emitter is responsible only for attempting recipient-side emission and
  returning typed success/failure.
- the accepted `AD.25` through `AD.30` follow-up line keeps that attempt-only
  ownership explicit:
  - caller-owned send/ack code resolves matching external hooks, built-in
    fallback eligibility, and the concrete built-in recipient target before
    invoking this boundary
  - this boundary does not reopen config lookup, team override lookup, or
    recipient-capability policy selection
- local tmux-backed emission may live in `atm-core`; the graft-backed emitter
  is the explicitly allowlisted out-of-owner implementation
  `atm_daemon::post_send_emitter::DaemonPostSendHookEmitter`.
- this boundary must not become a logical-message-delivery, persistence, or
  generic notification-planning seam.
- AD18/ARCH-004 scope ruling, governed by
  `docs/adr/ADR-020-rule001-observability-adapter-exception.md`, is accepted
  on the `AD.25` through `AD.30` follow-up line:
  - `crates/atm-daemon/src/daemon_runtime_observability.rs` is a sanctioned
    library-internal adapter module for direct
    `sc_observability_types::{ActionName, OutcomeLabel}` imports in the dual
    `lib.rs` + `main.rs` `atm-daemon` crate
  - `crates/atm-daemon/src/main.rs` may still import those construction types
    directly as the binary entrypoint
  - `daemon_runtime_observability.rs` must expose a concrete achievable
    crate-visible mechanism, such as `pub(crate)` aliases or constructor
    helpers, so `runtime_sqlite_observer.rs` and `test_observability.rs` stop
    importing `sc_observability_types` directly
  - every other file under `crates/atm-daemon/src/` must route those aliases
    through the sanctioned adapter module; relocating the import to any other
    daemon source file is a boundary violation
  - CI enforcement must live in `.just/lint_boundaries.py` with one explicit
    allowlist entry for the sanctioned adapter module rather than only a manual
    review-time grep

## GraftPostSendPort

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/graft-post-send-port.toml](../../boundaries/atm-core/graft-post-send-port.toml)


Purpose:
- Owns the one accepted graft-backed advisory handoff for post-send events
  after `atm-core` has already decided that recipient-side graft emission is
  required.

Notes:
- This stays narrower than `PostSendHookEmitter`:
  - `atm-core` still decides whether graft-backed post-send applies
  - `atm-core` still logs failures and constructs sender-visible warnings
  - the port only attempts the graft-side advisory delivery
- the accepted out-of-owner implementation is
  `atm_daemon::runtime_health::DaemonGraftPostSendPort`.
- this boundary must not expand into generic notification routing, mailbox
  compatibility append, tmux delivery, or local process spawning.

## NotificationSink

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/notification-sink.toml](../../boundaries/atm-core/notification-sink.toml)


Purpose:
- Historical boundary record only. Phase `AD.5` retired `NotificationSink`
  from the accepted post-send/send/ack path.

Notes:
- The retired boundary remains documented only so historical plan/ADR
  references still resolve.
- Post-send ownership now stays at the send/ack event site:
  - durable message persistence succeeds first
  - recipient-specific post-send emission happens directly through the accepted
    post-send emitter seam
  - retained notification logging, when enabled, appends directly to the
    notification log with no `NotificationSink` substitution
- Non-Claude outbound payload delivery still uses the dedicated
  `NonClaudeOutbound` boundary rather than any notification surface.

## ClaudeCompatibilityMailboxWriter

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/claude-compatibility-mailbox-writer.toml](../../boundaries/atm-core/claude-compatibility-mailbox-writer.toml)

Historical status:
- retired from the accepted runtime by `AD.3`
- any surviving references are historical boundary records only

Purpose:
- Historical boundary record only. Phase `AD.3` retired the
  `ClaudeCompatibilityMailboxWriter` executor seam from the accepted send/ack
  runtime.

Notes:
- The retired boundary remains documented only so historical plan/ADR
  references still resolve.
- The deleted seam previously owned:
  - `execute_claude_delivery(...)`
  - direct `append_claude_inbox_message(...)` / recovered message-set append
    execution
- Accepted send/ack delivery now routes through the retained
  `NonClaudeOutbound` seam only.
- Repair-only inbox rebuild/export support remains outside the live send/ack
  executor and must not be treated as a surviving delivery boundary.

## NonClaudeOutbound

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/non-claude-outbound.toml](../../boundaries/atm-core/non-claude-outbound.toml)


Purpose:
- Owns first-class non-Claude logical message delivery after the
  state-machine/coordinator seam has produced a typed delivery plan.

Notes:
- This boundary must receive the same `LogicalMessage` payload set that the
  Claude path receives; only transport target differs.
- `NotificationSink` must not be used as a substitute for this boundary.
- only approved delivery executors may call this boundary directly.
- the active `atm-core` handoff seam is:
  - `atm_core::delivery_execution::NonClaudeOutboundDeliveryWriter`
  - `atm_core::service_runtime::RetainedServiceRuntime::deliver_non_claude_payloads(...)`

## RuntimeFactory

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/runtime-factory.toml](../../boundaries/atm-core/runtime-factory.toml)

Planned status:
- planned retained-runtime construction seam for CLI and test/smoke entrypoints
- ambient singleton lookup remains outside this boundary and is governed by the
  singleton lint line

Purpose:
- Define the eventual `atm-core` contract for constructing runtime dependency
  assemblies without leaking concrete storage or daemon process ownership into
  callers.

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
