# ATM-Daemon Crate Requirements

## 1. Purpose

This document defines the `atm-daemon` crate requirements.

The `atm-daemon` crate owns the runtime wrapper around the current ATM system.
Product behavior remains defined in [`../requirements.md`](../requirements.md).
`atm-daemon` must satisfy those product requirements without re-owning
`atm-core` business logic.

This crate remains part of the current workspace.

The crate-local machine-readable boundary inventory lives in:
- [`./boundaries.md`](./boundaries.md)

The canonical daemon transport wire contract lives in:
- [`./protocol-icd.md`](./protocol-icd.md)

The canonical daemon observability boundary contract lives in:
- [`./observability.md`](./observability.md)

The canonical daemon/client recovery text rule set lives in:
- [`./recovery-text-rules.md`](./recovery-text-rules.md)

## 2. Ownership

`atm-daemon` owns:

- singleton daemon startup and host ownership
- same-host daemon API transport
- cross-host daemon-to-daemon transport
- runtime composition of `atm-core` service boundaries
- runtime composition of the current concrete adapter set used in production
- live agent status cache
- runtime watch/reconcile loop if enabled
- daemon-side `sc-observability` emission
- daemon-side compatibility projection behavior that must keep ATM-authored
  JSONL re-export idempotent under watcher/reconcile observation

`atm-daemon` does not own:

- mail business logic
- workflow/state-machine rules
- direct CLI parsing or rendering
- direct ownership of SQLite semantics beyond using the `atm-core` store
  boundary

Current request/response packet families owned by the daemon transport line:
- send compose
- send acknowledge
- receive
- clear
- doctor
- heartbeat
- advisory register
- advisory unregister
- advisory fetch
- advisory drain
- advisory stream
  - production requirement: one live advisory stream per active embedded
    client session
  - the live advisory stream is the production nudge-delivery path whenever the
    selected same-host transport supports streaming

Current retained ATM surfaces not modeled as daemon request/response packets:
- `atm log`
- `atm teams`
- `atm members`

## 3. Requirement Namespace

The `atm-daemon` crate uses the `REQ-DAEMON-*` namespace.

Initial allocation:

- `REQ-DAEMON-RUNTIME-*`
- `REQ-DAEMON-TRANSPORT-*`
- `REQ-DAEMON-STATUS-*`
- `REQ-DAEMON-CONFIG-*`
- `REQ-DAEMON-TEST-*`
- `REQ-DAEMON-OBS-*`
- `REQ-DAEMON-HEALTH-*`
- `REQ-DAEMON-SIGNAL-*`
- `REQ-DAEMON-PLATFORM-*`

Initial crate requirement IDs:

- `REQ-DAEMON-RUNTIME-001` `atm-daemon` owns singleton runtime enforcement and
  must make it impossible for more than one `atm-daemon` process to exist
  anywhere on the host for the supported runtime model. Singleton is daemon
  requirement `#1` and is not subordinate to convenience test or tooling
  flows. Satisfies the runtime ownership aspects of:
  `REQ-CORE-DAEMON-001`, `REQ-CORE-QA-RUNTIME-001`.
- `REQ-DAEMON-RUNTIME-002` `atm-daemon` owns runtime composition only and must
  remain a thin wrapper over `atm-core` service boundaries. Satisfies:
  `REQ-CORE-DAEMON-002`, `REQ-CORE-BOUNDARY-001`.
- `REQ-DAEMON-RUNTIME-003` `atm-daemon` owns graceful shutdown sequencing for
  the singleton runtime. Satisfies:
  `REQ-CORE-DAEMON-001`, `REQ-CORE-DOCTOR-002`.
- `REQ-DAEMON-RUNTIME-004` `atm-daemon` owns concrete resource-cap and
  saturation policy for runtime queues, accepts, and store handles. Satisfies:
  `REQ-CORE-QA-RUNTIME-001`.
- `REQ-DAEMON-RUNTIME-005` `atm-daemon` owns crash-recovery and replay policy
  around daemon-managed delivery/export work. Satisfies:
  `REQ-CORE-TRANSPORT-004`, `REQ-CORE-LOCK-RETIRE-001`.
  The replay store is a fail-closed startup dependency because the bounded
  replay-resume sweep must run before the daemon can enter serving state.
- `REQ-DAEMON-RUNTIME-006` `atm-daemon` daemon-private control-plane code must
  be partitioned into explicit ownership modules rather than one mixed
  crate-root implementation surface. Satisfies:
  `REQ-CORE-BOUNDARY-002`, `REQ-P-DAEMON-PARTITION-001`.
- `REQ-DAEMON-RUNTIME-007` singleton cleanup must remain ownership-safe and
  must not create a relock/unlink race for the host-wide ownership path.
  Satisfies:
  `REQ-P-RUNTIME-002`, `REQ-P-DAEMON-LIFECYCLE-001`, `REQ-CORE-DAEMON-001`.
- `REQ-DAEMON-RUNTIME-009` daemon background worker lanes that own active
  coordination state must use single-owner bounded command channels or
  equivalent actor ownership rather than daemon-shared queue/debounce mutable
  locks. The accepted ownership rule is bounded command-channel handoff into
  one actor-owned request lane. Satisfies:
  `REQ-CORE-BOUNDARY-002`, `REQ-DAEMON-RUNTIME-004`.
  `NotificationRuntime` closes this rule in `Y.20` by using one bounded
  `sync_channel` handoff, immutable runtime-status publication, and a
  worker-owned persistence/drain lane.
  `ReconcileRuntime` closes the contract/fanout half of this rule in `Y.21`
  by freezing the command-in / reply-out actor contract and shared
  `JoinHandleOwner` lifecycle helper ahead of the final `Y.22` production
  cutover.
  `ReconcileRuntime` closes the final production rule in `Y.22` by removing
  the daemon-shared reconcile mutex/condvar path and moving notification
  fingerprint ownership into `ReconcileWorkerState`.
- `REQ-DAEMON-TRANSPORT-001` `atm-daemon` owns one protocol with two
  production transport implementations plus one test transport:
  - one cross-platform local IPC contract for same-host
    - Unix implementation: Unix domain socket
    - Windows implementation: named-pipe-backed local IPC
  - TCP/TLS for cross-host daemon-to-daemon traffic
  - `test-socket` — implemented as `LoopbackClientTransport`; see ADR-003
    §Tier 2 — for in-process transport-boundary tests
  Satisfies:
  `REQ-CORE-TRANSPORT-001`, `REQ-CORE-TRANSPORT-002`.
- `REQ-DAEMON-TRANSPORT-002` `atm-daemon` owns bounded transient retry for
  remote delivery and must not create a durable long-lived remote outbox.
  Satisfies:
  `REQ-CORE-TRANSPORT-003`, `REQ-CORE-TRANSPORT-004`.
- `REQ-DAEMON-TRANSPORT-003` `atm-daemon` owns the concrete timeout budget
  policy for transport, store busy timeout, ingest batch, retry, and doctor
  query operations. Satisfies:
  `REQ-CORE-TRANSPORT-003`, `REQ-CORE-DOCTOR-002`.
- `REQ-DAEMON-TRANSPORT-004` request work launched from the daemon server path
  must remain tracked by runtime drain ownership until it finishes or is
  cancelled; detached untracked request execution is forbidden. Satisfies:
  `REQ-DAEMON-RUNTIME-003`, `REQ-P-DAEMON-DISPATCHER-001`,
  `REQ-CORE-DAEMON-001`.
- `REQ-DAEMON-TRANSPORT-005` same-host local IPC and cross-host daemon
  transport must use one shared ATM frame header and one typed
  request/response packet family rather than separate local and remote daemon
  message systems, as defined by `docs/atm-daemon/protocol-icd.md`. Satisfies:
  `REQ-CORE-TRANSPORT-001`, `REQ-CORE-TRANSPORT-002`,
  `REQ-P-CONTRACT-001`.
- `REQ-DAEMON-TRANSPORT-006` the daemon request/response transport must use
  explicit frame-length delimiting; EOF-delimited request framing and
  mid-stream resynchronization after partial-frame failure are forbidden, as
  defined by `docs/atm-daemon/protocol-icd.md`. Satisfies:
  `REQ-CORE-TRANSPORT-001`, `REQ-P-RELIABILITY-001`.
- `REQ-DAEMON-TRANSPORT-007` UDP is not an accepted same-host CLI-daemon
  request/response transport for the retained product surface. Satisfies:
  `REQ-P-RELIABILITY-001`, `REQ-P-CONTRACT-001`.
- `REQ-DAEMON-TRANSPORT-008` same-host local IPC must expose one logical ATM
  endpoint contract and one same-user access-control policy across Unix and
  Windows. Callers above the local-IPC adapter must not construct Unix socket
  paths, Windows pipe names, or platform-specific ACL semantics directly.
  Adapter-internal mapping from the logical endpoint contract to the concrete
  Windows named-pipe name is allowed, but the mapping must be deterministic
  and shared by the daemon and CLI.
  Satisfies:
  `REQ-P-CONTRACT-001`, `REQ-P-PLATFORM-002`.
- `REQ-DAEMON-STATUS-001` `atm-daemon` owns the live agent-status cache and
  must keep it separate from SQLite roster/mail truth. Satisfies:
  `REQ-CORE-RUNTIME-002`.
- `REQ-DAEMON-STATUS-002` bounded daemon status-cache policy must bound actual
  retained entries rather than only downgrading entry state labels. Satisfies:
  `REQ-DAEMON-RUNTIME-004`.
- `REQ-DAEMON-STATUS-003` `atm-daemon` runtime-health and `atm doctor`
  projection remain owned by the runtime-status partition and must not bypass
  daemon-owned status truth. Satisfies:
  `REQ-CORE-DOCTOR-002`.
- `REQ-DAEMON-STATUS-004` read-mostly daemon status projection must publish
  immutable coherent snapshots for readers; doctor/status consumers must not
  depend on one daemon-shared mutable cache lock for ordinary reads. Satisfies:
  `REQ-CORE-DOCTOR-002`, `REQ-DAEMON-STATUS-001`.
- `REQ-DAEMON-CONFIG-001` `atm-daemon` owns daemon config validation at startup
  and on lifecycle-control-triggered reload or rescan. Invalid config must
  produce a typed failure or bounded reload rejection rather than a silent
  degraded state. Satisfies:
  `REQ-CORE-CONFIG-001`, `REQ-CORE-CONFIG-003`, `REQ-DAEMON-SIGNAL-001`.
- `REQ-DAEMON-TEST-001` `atm-daemon` must not define the core test strategy.
  Core correctness must remain testable without daemon process spawning.
  Satisfies:
  `REQ-CORE-TEST-RUNTIME-001`.
- `REQ-DAEMON-TEST-002` `atm-daemon` must not introduce or bless a test-only
  daemon launch path for ordinary ATM correctness tests. Any real daemon
  process coverage is limited to a narrow daemon-runtime suite for singleton,
  startup, shutdown, and recovery requirements. Satisfies:
  `REQ-CORE-TEST-RUNTIME-001`, `REQ-P-TEST-001`.
- `REQ-DAEMON-OBS-001` `atm-daemon` owns daemon/runtime/transport structured
  event emission through `sc-observability`. Satisfies:
  `REQ-CORE-OBS-002`.
- `REQ-DAEMON-OBS-002` `atm-daemon` owns the daemon-side retained logging
  baseline and must preserve daemon lifecycle `info!` events plus every
  daemon/runtime/transport `warn!` / `error!` event at the default retained
  logger level and host-scoped retained path. Satisfies:
  `REQ-P-OBS-002`, `REQ-P-OBS-003`, `REQ-CORE-OBS-002`.
  In S.10, daemon-side historical log `query()` / `follow()` remain deferred;
  operators use the CLI-owned retained-log surface until a later sprint
  extracts daemon-side query/follow support explicitly.
  If S.9 introduces any public retained-logging trait or sink boundary, that
  trait must be sealed by default per the product architecture trait-extension
  policy.
- `REQ-DAEMON-OBS-003` daemon observability remains bottom-of-stack:
  the shared daemon observability layer must not import daemon subsystem types
  or reconstruct subsystem meaning centrally. Subsystems emit already-shaped
  daemon event payloads through the injected daemon observability trait.
  Satisfies:
  `REQ-CORE-BOUNDARY-001`, `REQ-CORE-OBS-001`, `REQ-CORE-OBS-002`.
- `REQ-DAEMON-OBS-004` the daemon-injected observability trait must remain
  sealed and object-safe, and its event model must use typed semantic
  identifiers rather than raw strings for subsystem, message-id, and task-id
  meaning. Satisfies:
  `REQ-CORE-BOUNDARY-001`, `REQ-CORE-OBS-001`.
- `REQ-DAEMON-HEALTH-001` `atm-daemon` owns the daemon health interface
  consumed by `atm doctor`. Satisfies:
  `REQ-CORE-DOCTOR-002`.
- `REQ-DAEMON-SIGNAL-001` `atm-daemon` owns runtime-control installation and
  handling for daemon lifecycle transitions. Unix may satisfy this through
  signals; Windows may satisfy it through console or service-control events.
  The accepted Phase S Windows console mapping is terminate events -> graceful
  shutdown and `SIGBREAK` / `CTRL_BREAK_EVENT` -> bounded reload / rescan.
  Satisfies:
  `REQ-CORE-DAEMON-001`, `REQ-CORE-DOCTOR-002`.
- `REQ-DAEMON-PLATFORM-001` `atm-daemon` must deliver full same-host daemon
  functionality on every supported operating system rather than clean
  compilation plus unsupported-path stubs. Satisfies:
  `REQ-P-PLATFORM-001`, `REQ-P-PLATFORM-002`.
- `REQ-DAEMON-PLATFORM-002` OS-specific implementation differences are allowed
  only inside the documented daemon portability boundaries for local IPC,
  lifecycle control, and host ownership. Satisfies:
  `REQ-P-PLATFORM-002`, `REQ-CORE-BOUNDARY-001`.
- `REQ-DAEMON-TEST-003` `atm-daemon` same-host functional tests must use one
  shared transport/dispatcher test harness on Unix and Windows, with
  platform-specific test code limited to the owned portability adapters.
  The S.4 release audit verifies this through the real local-IPC round-trip
  smoke test in `crates/atm-daemon/src/tests.rs`, the daemon host-ownership
  tests, the Windows lifecycle tests, and the CLI same-host client tests.
  Satisfies:
  `REQ-P-PLATFORM-002`, `REQ-CORE-TEST-RUNTIME-001`.
- `REQ-DAEMON-TEST-004` `atm-daemon` must not use fixed sleeps, timing-only
  stabilization, or unbounded wait paths in same-host functional tests;
  readiness, shutdown, retry, and helper-thread drain behavior must be proven
  through explicit synchronization or bounded runtime contracts. Satisfies:
  `REQ-P-TEST-001`, `REQ-P-PLATFORM-002`.
- `REQ-DAEMON-RUNTIME-008` watcher/reconcile handling of ATM-authored
  compatibility projection updates must remain idempotent for the same
  logical message and must not create self-induced churn loops. Satisfies:
  `REQ-CORE-COMPAT-001`, `REQ-P-RELIABILITY-001`, `ADR-010`.
  The daemon boundary contract proves this through import/re-export coverage
  that preserves the same identity fingerprint when an ATM-authored message is
  re-observed through a retrieval-stub projection.

## 4. Required References

The `atm-daemon` crate docs must remain aligned with:

- [`../requirements.md`](../requirements.md)
- [`../architecture.md`](../architecture.md)
- [`../project-plan.md`](../project-plan.md)
- [`../plan-phase-R.md`](../plan-phase-R.md)
- [`../plan-phase-S.md`](../plan-phase-S.md)
- [`../plan-phase-U.md`](../plan-phase-U.md)
- [`../testing-guidelines.md`](../testing-guidelines.md)
- [`../team-member-state.md`](../team-member-state.md)
- [`../documentation-guidelines.md`](../documentation-guidelines.md)
- [`../atm-core/requirements.md`](../atm-core/requirements.md)
- [`../atm-core/architecture.md`](../atm-core/architecture.md)
- [`./boundaries.md`](./boundaries.md)
- [`./observability.md`](./observability.md)
- [`./protocol-icd.md`](./protocol-icd.md)
- [`./logging.md`](./logging.md)
- [`./recovery-text-rules.md`](./recovery-text-rules.md)

## 5. Phase R Runtime Requirements

Requirement IDs:
- `REQ-DAEMON-RUNTIME-001`
- `REQ-DAEMON-RUNTIME-002`
- `REQ-DAEMON-RUNTIME-003`
- `REQ-DAEMON-RUNTIME-004`
- `REQ-DAEMON-RUNTIME-005`
- `REQ-DAEMON-RUNTIME-006`
- `REQ-DAEMON-RUNTIME-007`
- `REQ-DAEMON-RUNTIME-008`
- `REQ-DAEMON-RUNTIME-009`
- `REQ-DAEMON-TRANSPORT-001`
- `REQ-DAEMON-TRANSPORT-002`
- `REQ-DAEMON-TRANSPORT-003`
- `REQ-DAEMON-TRANSPORT-004`
- `REQ-DAEMON-TRANSPORT-005`
- `REQ-DAEMON-TRANSPORT-006`
- `REQ-DAEMON-TRANSPORT-007`
- `REQ-DAEMON-TRANSPORT-008`
- `REQ-DAEMON-STATUS-001`
- `REQ-DAEMON-STATUS-002`
- `REQ-DAEMON-STATUS-003`
- `REQ-DAEMON-STATUS-004`
- `REQ-DAEMON-CONFIG-001`
- `REQ-DAEMON-TEST-001`
- `REQ-DAEMON-TEST-002`
- `REQ-DAEMON-TEST-003`
- `REQ-DAEMON-TEST-004`
- `REQ-DAEMON-OBS-001`
- `REQ-DAEMON-OBS-002`
- `REQ-DAEMON-OBS-003`
- `REQ-DAEMON-OBS-004`
- `REQ-DAEMON-HEALTH-001`
- `REQ-DAEMON-SIGNAL-001`
- `REQ-DAEMON-PLATFORM-001`
- `REQ-DAEMON-PLATFORM-002`

Required runtime rules:
- exactly one daemon process may be active on a host at a time
- singleton enforcement is host-wide rather than socket-path-local; changing
  `ATM_HOME`, socket path, or test working directory must not create a legal
  second daemon
- the host-wide ownership mechanism uses stable permanent lock-file paths under
  `~/.atm/daemon/` rather than lock-file creation/deletion as the ownership
  signal:
  - `launch.lock`
  - `owner.lock`
- the cross-platform locking foundation is one whole-file exclusive-lock
  contract on those paths; the current S.1 extraction preserves that contract
  through the existing `fs2` crate
- lock acquisition for `launch.lock` and `owner.lock` must use
  `FileExt::try_lock_exclusive`; blocking `lock_exclusive` is not the
  serving-admission contract
- failed acquisition must surface one typed `already_owned` admission outcome
  rather than a blocking wait loop
- owner-visible metadata is the lock-file contents while the exclusive lock is
  held, not the mere existence of the lock file path
- owner-record contents use the documented `pid[:token]` format
- supported singleton deployment assumes `~/.atm/daemon/` is on a local
  filesystem with working host-local advisory lock semantics; NFS or other
  network-mounted roots are not supported singleton configurations
- daemon startup is blocked by at least two runtime guard layers:
  - a pre-spawn launch gate that serializes daemon creation attempts
  - a daemon-side startup gate that refuses serving state when ownership is
    already held
- launch-to-owner handoff must be:
  1. launcher holds `launch.lock` through fork/exec
  2. daemon acquires `owner.lock` before publishing a local endpoint or
     entering serving state
  3. launcher releases `launch.lock` only after daemon serving confirmation
- daemon startup must fail deterministically if a live daemon already owns the
  runtime
- daemon startup must not publish a serving socket or accept requests before
  singleton ownership is confirmed
- stale ownership cleanup must never allow two live daemons
- stale ownership cleanup must preserve the same singleton guarantee as normal
  startup; cleanup is recovery, not an alternate launch path
- if `try_lock_exclusive` succeeds on `owner.lock`, the acquiring process owns
  the authority to inspect and replace stale owner-record contents under that
  held lock
- if `try_lock_exclusive` does not succeed on `owner.lock`, the caller must
  treat ownership as live and must not attempt sidecar deletion or path-based
  recovery
- singleton cleanup must not depend on deleting a lock-file path to express
  ownership transfer
- graceful shutdown must stop accepts, drain or cancel inflight work within one
  bounded deadline, checkpoint WAL, and release singleton ownership
- the same-host and remote daemon transport families must share one ATM frame
  contract defined by `docs/atm-daemon/protocol-icd.md`
- the governing ICD owns the exact `magic`, `version`, `flags`,
  `request_id`, `payload_length`, `message_kind`, and public packet-payload
  mapping contract
- same-host local IPC must expose one logical endpoint contract and same-user
  access-control policy across Unix and Windows instead of leaking socket-path
  or named-pipe details above the adapter line
- `message_kind` must be available before payload decode so transport handlers
  can switch on packet type before touching payload JSON
- explicit frame-length delimiting is required; connection shutdown/EOF is not
  the request boundary contract
- invalid header, partial frame, timeout, oversize payload, or decode failure
  must fail the connection rather than triggering best-effort mid-stream
  resynchronization
- daemon-private runtime control must be partitioned into explicit ownership
  modules for exactly these eight partitions:
  - singleton ownership
  - server runtime / connection registry / drain
  - request execution ownership
  - runtime status / reload / doctor projection
  - peer transport
  - watch runtime
  - reconcile runtime
  - notification runtime
- background-lane startup rollback and shutdown must attempt every lane needed
  for cleanup and must not leave partial runtime ownership after the first lane
  failure
- signal handlers must be installed before listeners are opened
- the host runtime-control source must be installed before listeners are opened
- daemon config must validate once at startup before listeners are opened
- lifecycle-control-triggered config or roster rescan must either apply a
  fully valid configuration or fail with a typed reload error while retaining
  the prior serving configuration
- same-host daemon functionality must remain feature-complete on every
  supported operating system; compile-only support or typed unsupported-path
  stubs are not a releasable end state
- the same-host transport boundary must remain platform-neutral above the
  adapter layer:
  - Unix may use Unix domain sockets
  - Windows may use named-pipe-backed local IPC
  - caller-visible runtime code above the transport adapter must not depend on
    Unix-only stream or listener types
- platform cfg is allowed only inside owned daemon adapter modules; composition,
  dispatcher, health, replay, and runtime-lane code must not embed transport-
  or control-source-specific OS branching
- supported operating system differences are limited to these daemon-owned
  portability boundaries:
  - local IPC transport adapter
  - lifecycle-control source adapter
  - host-ownership adapter
- unsupported-path stubs are allowed only as short-lived implementation
  scaffolding while the owning Phase S sprint is in flight; they are a direct
  release blocker once the parity line is declared complete
- remote delivery must be daemon-to-daemon only
- the same transport protocol must be exercisable through an in-process
  `test-socket` without changing handler/business logic
- same-host functional tests must use shared infrastructure on Unix and Windows
  so one handler/dispatcher contract is proven through both platform
  implementations
- fixed sleeps, warmup polling, timing-only daemon stabilization, and
  unbounded wait paths are prohibited in same-host functional tests; tests
  must use explicit synchronization or bounded runtime contracts
- transport/store/health operations must obey one documented timeout budget
  - authoritative timeout budget references:
    [`../architecture.md §21.6.4`](../architecture.md) and
    [`architecture.md §3.4`](./architecture.md)
- runtime queues and handles must obey one documented concrete cap policy
- resource-cap matrix:
  - max concurrent accepted connections: `64`
  - max per-connection inflight requests: `32`
  - ingest queue depth: `1024`
  - bounded remote retry queue depth: `256`
  - SQLite handle/pool budget: min `1`, max `4`
  - live status-cache cap: `4096`
  - reconcile notification fingerprint registry cap:
    `MAX_RECONCILE_FINGERPRINT_KEYS = 1024`, evict-oldest-and-log
  - watch subscription cap: `256`
  - notification work queue depth: `64`
- request work launched from the server path must remain tracked by runtime
  shutdown accounting until it completes or is cancelled
- the current Phase R transport remains single-request-per-connection, so the
  per-connection in-flight count is structurally `1` today; the documented
  `32` cap is the retained protocol resource ceiling for any later framed
  multiplexing extension and must not be contradicted by daemon partition docs
- daemon memory is the live truth for agent status
- daemon memory must also retain `last_active_at` for each known active agent
- daemon memory must retain the current agent `pid` as a first-class liveness
  field, but `pid` is transient runtime state rather than durable roster truth
- `pid` is a semantic newtype candidate and must not remain an unvalidated raw
  integer at the daemon boundary once the runtime API hardening slice lands
- SQLite must not own live `last_active_at` or the current process `pid`
- the daemon-managed member fields (`pid`, `last_active_at`, `state`) must
  update only through one documented heartbeat socket handler shared by ATM CLI
  and hook/runtime producers; see `docs/team-member-state.md`
- status-cache saturation behavior must keep the retained live-member map
  actually bounded in cardinality; demotion to `unknown` alone is not
  sufficient
- watch runtime must reject subscriptions beyond the bounded cap rather than
  retaining unbounded watcher state
- notification runtime must reject or degrade delivery beyond the bounded queue
  cap rather than silently buffering unbounded work
- notification runtime producer paths must publish only lifecycle/degraded
  checks plus bounded command-channel submission; callers must not mutate queue
  state or persistence sequencing directly
- until `schooks 1.0` is released, pid/activity updates may arrive through the
  interim Python hooks installed from `../agent-team-mail`
- after `schooks 1.0` is released, `schooks` becomes the controlled hook
  environment layer and reports pid/activity updates to `atm-daemon`
- if a heartbeat reports a different pid while the stored pid is still alive,
  the daemon must reject the update unless the explicit admin takeover path
  documented in `docs/team-member-state.md` is active
- accepted pid changes must update daemon memory and emit `AgentPidChanged`
- crash recovery must preserve the ordering rule `SQLite commit -> export`
  and any retry/re-export state needed after daemon crash must be durable rather
  than RAM-only
- daemon code must not bypass `atm-core` subsystem boundaries
- the current Phase R baseline keeps `atm-daemon` as the runtime composition
  root for production runtime wiring unless a later ADR extracts a separate
  composition crate
- remote daemon-to-daemon client behavior uses the same shared `AtmProtocol`
  and `ClientTransport` / `ServerTransport` contract family as local runtime
  transport
- daemon transport/runtime adapter implementations must remain private to the
  crate or tightly-scoped internal surfaces; public callers must not depend on
  concrete socket/runtime adapter types
- daemon boundary traits are sealed by default; opening a runtime/transport
  extension point requires explicit architecture review
- watcher/reconcile runtime code must remain isolated from transport, store,
  and notifier implementations behind its own owned boundary
- daemon unavailability after one documented auto-start attempt must surface as
  explicit runtime failure rather than hidden fallback to direct SQLite or
  inbox-file access
- tests and tools are not exempt from the singleton rule; any attempt to start
  a second daemon process must fail through the same runtime ownership checks
- the socket receive loop must remain a thin dispatcher only:
  - read framed request
  - parse qualified request type
  - dispatch through the owning dispatcher/handler boundary
  - return typed response
- request-kind routing must stay in the dispatcher boundary, not in concrete
  local-IPC or TCP/TLS adapter code
- handler implementations for request families must be injectable behind that
  dispatcher
- the dispatcher boundary itself must remain thin and must not absorb request
  family business logic
- the socket receive loop must not perform SQL, watcher, notifier, or
  workflow/state-transition logic inline
- the daemon must not own client-specific runtime behavior for any plugin crate
  (e.g., `atm-graft`); client-specific receive loops, injection paths, and
  host-integration logic belong in the client plugin crate, not in `atm-daemon`;
  the daemon's responsibility is request serving, post-commit notification, and
  generic runtime composition only (REQ-CORE-DAEMON-003)
- Phase U.8 keeps the shared daemon packet family limited to the existing CLI-
  shaped unary envelopes; `atm-graft` must consume that shared family rather
  than reopening graft-private request/response packets in the daemon crate
- any violation of these daemon boundary rules is a direct QA failure
- daemon tests must not become the normal mechanism for validating core ATM
  correctness
- daemon runtime failures must remain typed across transport/runtime boundaries
  rather than collapsing into panic/unwrap control flow
- daemon runtime and transport paths must emit structured observability events
- daemon must expose one explicit health/status query interface for `atm doctor`
- no `atm-daemon` crate API, helper, or test support path may bless daemon
  spawning as a routine correctness strategy

## Phase Yb Non-Claude Delivery Adapter

Requirement IDs:

- `REQ-ATM-DAEMON-YB-001`

Required daemon rules:

- `atm-daemon` must implement
  `atm_daemon::non_claude_outbound_runtime::DaemonNonClaudeOutbound`
- the daemon adapter must preserve the same logical `message[]` payload set
  used by the Claude path
- the daemon adapter must not degrade non-Claude message delivery into
  notification-only metadata
- only the approved delivery executor seam may call the adapter directly
