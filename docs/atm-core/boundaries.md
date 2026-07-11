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
  - `HeartbeatActivity` / `TeamMemberHeartbeat{Request,Response}` as the
    canonical daemon-owned member-liveness DTO family added in `R.15`
  - `RuntimeStatusSnapshot` as the daemon-health/status DTO consumed by
    `atm doctor`; after `AA.3` it carries daemon-owned runtime state only and
    no store-specific readiness fields
- `atm-runtime-test-support` is an allowed workspace-local dependent for the
  retained-runtime test harness seam; it is not a production consumer
  boundary.

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


Purpose:
- Owns durable roster state and routing-relevant member metadata.

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
- Own the storage-neutral runtime/replay contracts that concrete composition
  code and daemon runtime code share without letting those seams become
  daemon-private or SQLite-private.

Owned shared contracts:
- `DoctorFinding`
- `RuntimeDoctorPorts`
- `RemoteReplayStateRecord`
- `RemoteReplayStore`
- `RuntimeStorageFinalizer`

Notes:
- `RuntimeDoctorPorts` groups the installed storage-neutral doctor handles
  that callers consume after `atm-runtime` assembles the concrete backend.
- `RuntimeAssembly` now carries the generic `StorageBackends<M, R>` seam plus
  the legacy compile-bridge `MailStore` / `RosterStore` handles used during
  AC.4 consumer cutover.
- `RemoteReplayStore` keeps bounded replay persistence behind an
  `atm-core`-owned contract even though the first implementation is SQLite.
- `RuntimeStorageFinalizer` keeps shutdown-time storage finalization, such as
  bounded WAL checkpoint work, outside daemon-private adapter knowledge.
- `AA.4` relies on these adjunct contracts to remove the direct
  `atm-daemon -> atm-rusqlite` dependency while keeping replay persistence and
  shutdown finalization storage-neutral at the daemon boundary.

## ConfigIngress

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/config-ingress.toml](../../boundaries/atm-core/config-ingress.toml)


Purpose:
- Owns loading and validating persisted ATM/team configuration into typed models.

Notes:
- This is one of the main explicit corrections to earlier boundary leakage.
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

## SourceIngress

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/inbox-ingress.toml](../../boundaries/atm-core/inbox-ingress.toml)


Purpose:
- Owns import from compatibility/shared inbox surfaces into ATM-owned state.

Notes:
- The import path stays separate from durable store ownership.
- The hidden `atm_core::boundary_support` helper module is the only retained
  implementation seam that may still touch compatibility inbox source files for
  this boundary family.

## ProjectionExport

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/inbox-export.toml](../../boundaries/atm-core/inbox-export.toml)


Purpose:
- Owns projection of ATM-owned state back to compatibility/shared inbox surfaces.

Notes:
- This is the write-facing sibling of SourceIngress, not a general store boundary.
- Retained command/runtime code must reach compatibility inbox export only
  through the daemon-owned ingress/export seam; it must not treat export-file
  reads as a second source of mailbox truth.
- Harness-specific export policy belongs in one central delivery-policy
  coordinator and event-family state machines above this boundary, not in
  scattered command callers.
- `Y.4` lands that retained-command coordinator seam in
  `crates/atm-core/src/delivery_policy.rs`; retained `send` and `ack` now
  resolve roster snapshots through `RosterStore` before choosing whether
  compatibility export is allowed for the recipient harness.
- `Y.5` removes mutable compatibility fields from the shared inbox export path
  via two helper functions in
  `crates/atm-core/src/schema/inbox_message.rs`:
  - `strip_removed_compatibility_fields` — removes: `source_team`,
    `pendingAckAt`, `acknowledgedAt`, `acknowledgesMessageId`, `expiresAt`
  - `strip_metadata_atm_namespace` — removes the `atm` key from the
    `metadata` object
- See [docs/plans/phase-Y/inbox-field-inventory.md](../phase-Y/inbox-field-inventory.md)
  for the full field inventory.
- Phase `Yb` adds a stricter rule:
  - only approved delivery executors may call the write-facing export/append
    primitives behind this boundary
  - send/ack/persistence modules must not call them directly
  - delivery-target construction and transition translation must stay in the
    shared plan/execution seam:
    - `crates/atm-core/src/delivery_plan.rs`
    - `crates/atm-core/src/delivery_execution.rs`
  - see:
    - [../phase-Yb/lintable-boundary-plan.md](../phase-Yb/lintable-boundary-plan.md)
    - [../adr/ADR-013-unified-delivery-plan-and-state-machine-ownership.md](../adr/ADR-013-unified-delivery-plan-and-state-machine-ownership.md)
- `Phase Yc` adds one final recovered-Claude seam requirement:
  - `Y.12` introduces one explicit recovered logical-message-set export seam
    for `DeliveryPlanDisposition::SqliteFailedRecovered` through
    `ProjectionExport::append_message_set(...)`
  - the recovered Claude path must not loop one message at a time through the
    normal append helper while degrading to warnings after partial success
  - persisted single-message Claude append remains on the existing append-only
    path and is not reopened onto this boundary

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
