# ATM-Daemon Crate Requirements

> **Phase AI supersession notice:** `REQ-DAEMON-TRANSPORT-001` through `008`
> are the proposed target contract for Unix UDS/loopback-TCP HTTP, Windows
> loopback-TCP HTTP, and remote HTTPS. Any
> older text in this document that permits a custom ATM frame, replay/retry
> state, or a peer-transport runtime is historical and will be
> removed by the owning Phase AI sprint; it is not authority for new work.

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
- [`./http-api.md`](./http-api.md)

The canonical daemon observability boundary contract lives in:
- [`./observability.md`](./observability.md)

The canonical daemon/client recovery text rule set lives in:
- [`./recovery-text-rules.md`](./recovery-text-rules.md)

## 2. Ownership

`atm-daemon` owns:

- singleton daemon startup and host ownership
- same-host daemon API transport
- daemon-private orchestration around injected `atm-core` service boundaries
- live agent status cache
- daemon-side `sc-observability` emission
- daemon-side direct post-send emission routing for local and graft-backed
  recipients

`atm-daemon` does not own:

- mail business logic
- workflow/state-machine rules
- direct CLI parsing or rendering
- direct ownership of SQLite semantics beyond using the `atm-core` store
  boundary

Phase-AA target direction:
- `AA.2` moves concrete production runtime/store composition into
  `atm-runtime`
- SQLite construction, SQLite-specific observability injection, and direct
  SQLite health probing move out of this crate
- daemon-owned doctor reporting is reduced to daemon-owned runtime state rather
  than direct store readiness checks
- each subsystem owns its own backend-specific diagnosis behind a subsystem
  doctor trait
- daemon doctor code aggregates subsystem reports and daemon-owned runtime
  state only; it must not reimplement backend-specific diagnosis
- the aggregate-only doctor surface consumes `MailStoreDoctor`,
  `RosterStoreDoctor`, and `ConfigDoctor` rather than backend-shaped helpers
- `RuntimeStatusSnapshot` must not carry `sqlite_ready` / `sqlite_detail` or
  any other store-specific readiness field after `AA.3`

Current request/response packet families owned by the daemon transport line:
- send compose
- send acknowledge
- receive
- clear
- doctor
- heartbeat

Receiver-specific post-send handoff rule:
- receiver implementation details are not modeled as daemon packet families
- the accepted daemon line must not require graft session registration,
  fetch/drain inspection, bounded per-session daemon nudge queues (retired
  internal worker queue — unrelated to queue-kind nudges), or a dedicated
  advisory-stream request/response family
- daemon ownership ends at durable persistence, then emission of the steer
  nudge through the accepted capability seam; queue-kind nudges defer
  emission until harness readiness (ADR-055), and neither kind ever precedes
  persistence

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
- `REQ-DAEMON-RUNTIME-002` `atm-daemon` owns runtime orchestration around
  injected `atm-core` service boundaries only and must remain a thin wrapper
  over those boundaries. Concrete runtime/store assembly moves to
  `atm-runtime` in `AA.2`. Satisfies:
  `REQ-CORE-DAEMON-002`, `REQ-CORE-BOUNDARY-001`.
- `REQ-DAEMON-RUNTIME-003` `atm-daemon` owns graceful shutdown sequencing for
  the singleton runtime. Satisfies:
  `REQ-CORE-DAEMON-001`, `REQ-CORE-DOCTOR-002`.
- `REQ-DAEMON-RUNTIME-004` `atm-daemon` owns concrete resource-cap and
  saturation policy for runtime queues, accepts, and store handles. Satisfies:
  `REQ-CORE-QA-RUNTIME-001`.
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
  Phase AD note:
  - daemon watch/reconcile lanes are retired from the accepted runtime rather
    than preserved as the active closure of this rule
- `REQ-DAEMON-TRANSPORT-001` `atm-daemon` owns one HTTP router with local,
  peer, and test adapters:
  - Unix: HTTP over UDS and supported HTTP over loopback TCP for same-host
    access
  - Windows: HTTP over loopback TCP only for same-host access
  - HTTPS over TCP for cross-host daemon-to-daemon traffic
  - an in-process HTTP adapter; see ADR-003 §Tier 2 — for transport-boundary
    tests
  Satisfies:
  `REQ-CORE-TRANSPORT-001`, `REQ-CORE-TRANSPORT-002`.
- `REQ-DAEMON-TRANSPORT-002` `atm-daemon` owns no cross-host delivery state.
  It must not create a replay store, remote outbox, retry queue, deferred
  receipt, remote acknowledgement state, or duplicate-delivery subsystem.
  Satisfies:
  `REQ-CORE-TRANSPORT-003`, `REQ-CORE-TRANSPORT-004`.
- `REQ-DAEMON-TRANSPORT-002D` `atm-daemon` owns one bounded non-durable
  single-flight drain coordinator per canonical trusted hostname. It queries
  canonical outbound records through storage traits, opens one ordinary HTTPS
  connection, and submits existing `WriteRequest`s oldest-first through the
  normal peer endpoint. A generation signal closes the final-scan/release race.
  The coordinator contains no message ID, payload, cursor, receipt, queue, or
  per-message delivery state. It schedules only eligible backlog recovery no
  earlier than 60 seconds after failure with capped exponential backoff, never
  a ping/empty-peer monitor. `atm peer sync` uses this same coordinator.
  Satisfies: `REQ-CORE-TRANSPORT-003A`, `REQ-CORE-TRANSPORT-003B`.
- `REQ-DAEMON-TRANSPORT-002A` `atm-daemon` owns loading and enforcing durable
  cross-host HTTPS bind, certificate, and peer-trust records. Satisfies:
  `REQ-CORE-TRANSPORT-002A`.
- `REQ-DAEMON-TRANSPORT-002B` `atm-daemon` enforces mTLS plus a durable
  deny-by-default registered-hostname and certificate-fingerprint allowlist
  before routing. It resolves direct IP targets only as ADR-040 permits and
  never persists DNS aliases. Satisfies:
  `REQ-CORE-TRANSPORT-002B`.
- `REQ-DAEMON-TRANSPORT-002B1` `atm-daemon` accepts the explicit
  non-durable `--peer-wire-security plaintext-test` process mode only for
  debug/smoke diagnosis. It disables peer TLS/pin/allowlist checks without
  changing HTTP routing, canonical writes, persistence, or post-write routing;
  declared source-host data is untrusted smoke provenance. Normal startup is
  mTLS, never falls back to plaintext, and doctor/logs must expose the active
  mode. Satisfies: `REQ-CORE-TRANSPORT-002B1`.
- `REQ-DAEMON-TRANSPORT-002C` localhost and a daemon's own advertised address
  are ordinary HTTPS peer targets; no loopback-only transport branch exists.
  Satisfies:
  `REQ-CORE-TRANSPORT-002C`.
- `REQ-DAEMON-TRANSPORT-003` `atm-daemon` owns the one absolute request
  deadline budget for HTTP(S), store busy timeout, ingest batch, and doctor
  query operations. It propagates the remaining request budget to HTTPS and
  returns the ADR-041 typed outcome rather than misclassifying a live daemon
  as unavailable. Satisfies:
  `REQ-CORE-TRANSPORT-005`, `REQ-CORE-DOCTOR-002`.
- `REQ-DAEMON-TRANSPORT-004` request work launched from the daemon server path
  must remain tracked by runtime drain ownership until it finishes or is
  cancelled; detached untracked request execution is forbidden. Satisfies:
  `REQ-DAEMON-RUNTIME-003`, `REQ-P-DAEMON-DISPATCHER-001`,
  `REQ-CORE-DAEMON-001`.
- `REQ-DAEMON-TRANSPORT-005` Unix UDS HTTP, loopback-TCP HTTP, and HTTPS must call one router and one
  canonical read/write application contract rather than separate local and
  remote message systems. Satisfies:
  `REQ-CORE-TRANSPORT-001`, `REQ-CORE-TRANSPORT-002`,
  `REQ-P-CONTRACT-001`.
- `REQ-DAEMON-TRANSPORT-006` HTTP framing is owned by the HTTP implementation;
  ATM must not retain a second custom frame header, EOF-delimited request
  protocol, or resynchronization state machine. Satisfies:
  `REQ-CORE-TRANSPORT-001`, `REQ-P-RELIABILITY-001`.
- `REQ-DAEMON-TRANSPORT-007` UDP is not an accepted same-host CLI-daemon
  request/response transport for the retained product surface. Satisfies:
  `REQ-P-RELIABILITY-001`, `REQ-P-CONTRACT-001`.
- `REQ-DAEMON-TRANSPORT-008` same-host local IPC must expose owner-authenticated
  HTTP: Unix UDS plus loopback TCP, and Windows loopback TCP only. Loopback
  TCP binds only loopback and requires a daemon-created owner-readable endpoint
  record plus local capability; Unix UDS uses owner-only endpoint permissions.
  Callers above adapters must not construct endpoint paths, ports, capabilities,
  or ACL semantics directly. Alternate local transports and fallback paths are
  forbidden.
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
  and on lifecycle-control-triggered reload or rescan. The minimum daemon-owned
  config inventory includes:
  - same-host endpoint contract inputs
  - same-host timeout inputs
  - queue/cap inputs
  - retained-log / observability sink inputs
  Invalid config must produce a typed failure or bounded reload rejection
  rather than a silent degraded state. Startup-fatal or reload-fatal validation
  applies to ownership, transport, timeout-floor, and cap violations;
  warning-only handling is allowed only for optional
  observability sinks when the daemon keeps one documented degraded fallback
  path. Satisfies:
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
- `REQ-DAEMON-RPC-IDENTITY-001` `atm-daemon` must consume caller-owned command
  identity only from required request fields supplied by the client boundary.
  The daemon must not derive caller identity from daemon ambient
  `ATM_IDENTITY`, hook files, or repo-local config, and it must reject
  malformed request shapes that omit required caller identity. Satisfies:
  `REQ-P-IDENTITY-001`, `REQ-CORE-CONFIG-001`.
- `REQ-DAEMON-OBS-003` daemon observability remains bottom-of-stack:
  the shared daemon observability layer must not import daemon subsystem types
  or reconstruct subsystem meaning centrally. Subsystems emit already-shaped
  daemon event payloads through the injected daemon observability trait.
  Satisfies:
  `REQ-CORE-BOUNDARY-001`, `REQ-CORE-OBS-001`, `REQ-CORE-OBS-002`.
  Phase `AA.4` removes the daemon-private SQLite observability adapter so this
  rule applies without a daemon-local SQLite glue layer.
- `REQ-DAEMON-OBS-004` the daemon-injected observability trait must remain
  sealed and object-safe, and its event model must use typed semantic
  identifiers rather than raw strings for subsystem, message-id, and task-id
  meaning. Satisfies:
  `REQ-CORE-BOUNDARY-001`, `REQ-CORE-OBS-001`.
- `REQ-DAEMON-HEALTH-001` `atm-daemon` owns the daemon health interface
  consumed by `atm doctor`. The minimum daemon-owned field inventory is:
  - liveness
  - readiness
  - owner pid when known
  - active / idle / offline / unknown counts
  - daemon-owned degraded-runtime findings
  During `AA.0` through `AA.2`, the health contract may still carry the
  transitional SQLite readiness fields documented in the daemon architecture,
  but those fields are explicitly removed in `AA.3`. Satisfies:
  `REQ-CORE-DOCTOR-002`.
  After `AA.4`, daemon code reaches concrete SQLite-backed runtime state only
  through `atm-runtime` and `atm-core` boundaries rather than a direct
  `atm-daemon -> atm-rusqlite` dependency.
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
- `REQ-DAEMON-RUNTIME-008` is historical only.
  Phase AD retires the watcher/reconcile mailbox-compatibility line and its
  churn-loop contract from the accepted runtime. New daemon work must not
  preserve or expand that retired behavior.

## 4. Required References

The `atm-daemon` crate docs must remain aligned with:

- [`../requirements.md`](../requirements.md)
- [`../architecture.md`](../architecture.md)
- [`../project-plan.md`](../project-plan.md)
- [`../plan-phase-AA.md`](../plan-phase-AA.md)
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
- [`./http-api.md`](./http-api.md)
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
- **Historical local-frame contract:** the retired ATM frame ICD, its header
  fields, frame decoder, and platform-specific fallback mappings are not
  accepted runtime behavior. Phase AI replaces them with Unix UDS/loopback TCP
  and Windows loopback-TCP HTTP in
  `REQ-DAEMON-TRANSPORT-001`, `005`, `006`, and `008`.
- daemon-private runtime control must be partitioned into explicit ownership
  modules for these accepted runtime partitions:
  - singleton ownership
  - server runtime / connection registry / drain
  - request execution ownership
  - runtime status / reload / doctor projection
- historical watch / reconcile lanes are not part of the accepted runtime
  requirement set
- if temporary deletion scaffolding remains while `AD.4` / `AD.5` are in
  flight, it must be marked obsolete and must not be described as a required
  production partition
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
  adapter layer: Unix uses UDS HTTP plus loopback TCP; Windows uses loopback
  TCP only; caller-visible runtime code must not depend on platform socket-path
  or listener types
- platform cfg is allowed only inside owned daemon adapter modules; composition,
  dispatcher, health, and runtime-lane code must not embed transport-
  or control-source-specific OS branching
- supported operating system differences are limited to these daemon-owned
  portability boundaries:
  - local IPC transport adapter
  - lifecycle-control source adapter
  - host-ownership adapter
- unsupported-path stubs are allowed only as short-lived implementation
  scaffolding while the owning Phase S sprint is in flight; they are a direct
  release blocker once the parity line is declared complete
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
  - configured values may raise the documented defaults, but they must not
    drop below the daemon timeout floor of `250ms`; same-host request and
    daemon-health deadlines must not drop below `1s`
- runtime queues and handles must obey one documented concrete cap policy
- resource-cap matrix:
  - max concurrent accepted connections: `64`
  - max per-connection inflight requests: `32`
  - ingest queue depth: `1024`
  - SQLite handle/pool budget: min `1`, max `4`
  - live status-cache cap: `4096`
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
- ingest saturation emits `DaemonIngestQueueSaturated` and the matching health
  finding rather than silently dropping or only incrementing a counter
- until `schooks 1.0` is released, pid/activity updates may arrive through the
  interim Python hooks installed from `../agent-team-mail`
- after `schooks 1.0` is released, `schooks` becomes the controlled hook
  environment layer and reports pid/activity updates to `atm-daemon`
- if a heartbeat reports a different pid while the stored pid is still alive,
  the daemon must reject the update unless the explicit admin takeover path
  documented in `docs/team-member-state.md` is active
- accepted pid changes must update daemon memory and emit `AgentPidChanged`
- the semantic pid newtype closure for daemon-owned runtime state is assigned
  to `AA.3`, which already owns the runtime-health and doctor DTO rewrite
- crash recovery must preserve the ordering rule `SQLite commit -> export`
- daemon code must not bypass `atm-core` subsystem boundaries
- the current Phase R baseline keeps `atm-daemon` as the runtime composition
  root for production runtime wiring unless a later ADR extracts a separate
  composition crate
- daemon transport/runtime adapter implementations must remain private to the
  crate or tightly-scoped internal surfaces; public callers must not depend on
  concrete socket/runtime adapter types
- daemon boundary traits are sealed by default; opening a runtime/transport
  extension point requires explicit architecture review
- any direct post-send/advisory implementation must remain isolated from
  transport and store implementations behind its owned boundary
- daemon post-send notification logging, if retained, must append directly at
  the event site; a daemon-owned notification worker/runtime is not an accepted
  production subsystem
- daemon unavailability after one documented auto-start attempt must surface as
  explicit runtime failure rather than hidden fallback to direct SQLite or
  inbox-file access
- tests and tools are not exempt from the singleton rule; any attempt to start
  a second daemon process must fail through the same runtime ownership checks
- the HTTP receive loop must remain a thin dispatcher only:
  - decode an HTTP request
  - map it to a typed request
  - dispatch through the owning dispatcher/handler boundary
  - return typed response
- request-kind routing must stay in the dispatcher boundary, not in concrete
  local-IPC adapter code
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

## Historical Phase Yb Non-Claude Delivery Adapter

Requirement IDs:

- `REQ-ATM-DAEMON-YB-001`

Required daemon rules:

- This historical daemon adapter was unselected and had no non-test caller.
  AM.6 deletes it rather than retaining a second outbound implementation.
- The selected replacement composition owns any active backend-neutral outbound
  implementation through `RuntimeAssembly`.
