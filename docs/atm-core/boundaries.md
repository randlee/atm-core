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

## TaskStore

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/task-store.toml](../../boundaries/atm-core/task-store.toml)


Purpose:
- Owns durable task-domain state and task/message linkage.

Notes:
- `ack` is not a top-level public method, but it still mutates task state.
- `TaskStoreDoctor` is the paired subsystem-owned diagnostics boundary for
  bounded task-store findings.

## TaskStoreDoctor

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/task-store-doctor.toml](../../boundaries/atm-core/task-store-doctor.toml)

Purpose:
- Own durable task-store diagnostics without widening the main task capability
  trait family.

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

## Phase AA Runtime Composition Adjuncts

Purpose:
- Own the storage-neutral runtime/replay contracts that concrete composition
  code and daemon runtime code share without letting those seams become
  daemon-private or SQLite-private.

Owned shared contracts:
- `DoctorFinding`
- `RuntimeBundle`
- `RemoteReplayStateRecord`
- `RemoteReplayStore`
- `RuntimeStorageFinalizer`

Notes:
- `RuntimeBundle` groups the installed storage-neutral service and doctor
  handles that callers consume after `atm-runtime` assembles the concrete
  backend.
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
  - watcher / reconcile ingest
  - `doctor` comparison
  - narrow recreated-shell preservation reads during restore
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

## NotificationSink

Canonical machine-readable boundary source:
- [../../boundaries/atm-core/notification-sink.toml](../../boundaries/atm-core/notification-sink.toml)


Purpose:
- Owns outward delivery of notifications, hooks, or plugin-facing events.

Notes:
- This replaces direct `Command::new` use in business-flow code.
- Notification fallback policy for delivery state machines belongs here as a
  sink-side effect, but event legality still belongs to the event-family state
  machine rather than to the sink adapter.
- Phase `Yb` clarifies that this boundary is notification-only:
  - hook or notifier invocation is not proof of logical message delivery
  - non-Claude outbound payload delivery must use a dedicated delivery
    boundary, not NotificationSink as a stand-in
  - impossible non-Claude append-degraded routing must fail closed before it
    reaches this sink
  - the current proof surface for non-Claude delivery lives in
    `NonClaudeOutboundDeliveryRequest`, not in `ATM_POST_SEND` metadata
- `Phase Yc` finalized the production-path ownership rule:
  - `Y.13` removed the direct
    `PostSendNotificationExecutor -> maybe_run_post_send_hook(...)` bypass
  - the surviving production notification path now executes through
    `NotificationSink::deliver(...)`

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
