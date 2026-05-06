# ATM-Daemon Crate Requirements

## 1. Purpose

This document defines the `atm-daemon` crate requirements.

The `atm-daemon` crate owns the runtime wrapper around the current ATM system.
Product behavior remains defined in [`../requirements.md`](../requirements.md).
`atm-daemon` must satisfy those product requirements without re-owning
`atm-core` business logic.

This crate is introduced by the Phase Q implementation line. It is not present
in the pre-Phase-Q workspace yet.

The crate-local machine-readable boundary inventory lives in:
- [`./boundaries.md`](./boundaries.md)

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

`atm-daemon` does not own:

- mail business logic
- workflow/state-machine rules
- direct CLI parsing or rendering
- direct ownership of SQLite semantics beyond using the `atm-core` store
  boundary

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
- `REQ-DAEMON-TRANSPORT-001` `atm-daemon` owns one protocol with two
  production transport implementations plus one test transport:
  - Unix domain socket for same-host
  - TCP/TLS for cross-host daemon-to-daemon traffic
  - `test-socket` for in-process transport-boundary tests
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
- `REQ-DAEMON-STATUS-001` `atm-daemon` owns the live agent-status cache and
  must keep it separate from SQLite roster/mail truth. Satisfies:
  `REQ-CORE-RUNTIME-002`.
- `REQ-DAEMON-CONFIG-001` `atm-daemon` owns daemon config validation at startup
  and on `SIGHUP` rescan. Invalid config must produce a typed failure or
  bounded reload rejection rather than a silent degraded state. Satisfies:
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
- `REQ-DAEMON-HEALTH-001` `atm-daemon` owns the daemon health interface
  consumed by `atm doctor`. Satisfies:
  `REQ-CORE-DOCTOR-002`.
- `REQ-DAEMON-SIGNAL-001` `atm-daemon` owns signal installation and handling
  for daemon lifecycle transitions. Satisfies:
  `REQ-CORE-DAEMON-001`, `REQ-CORE-DOCTOR-002`.

## 4. Required References

The `atm-daemon` crate docs must remain aligned with:

- [`../requirements.md`](../requirements.md)
- [`../architecture.md`](../architecture.md)
- [`../project-plan.md`](../project-plan.md)
- [`../plan-phase-Q.md`](../plan-phase-Q.md)
- [`../plan-phase-R.md`](../plan-phase-R.md)
- [`../testing-guidelines.md`](../testing-guidelines.md)
- [`../team-member-state.md`](../team-member-state.md)
- [`../documentation-guidelines.md`](../documentation-guidelines.md)
- [`../atm-core/requirements.md`](../atm-core/requirements.md)
- [`../atm-core/architecture.md`](../atm-core/architecture.md)
- [`./boundaries.md`](./boundaries.md)

## 5. Phase R Runtime Requirements

Requirement IDs:
- `REQ-DAEMON-RUNTIME-001`
- `REQ-DAEMON-RUNTIME-002`
- `REQ-DAEMON-RUNTIME-003`
- `REQ-DAEMON-RUNTIME-004`
- `REQ-DAEMON-RUNTIME-005`
- `REQ-DAEMON-TRANSPORT-001`
- `REQ-DAEMON-TRANSPORT-002`
- `REQ-DAEMON-TRANSPORT-003`
- `REQ-DAEMON-STATUS-001`
- `REQ-DAEMON-CONFIG-001`
- `REQ-DAEMON-TEST-001`
- `REQ-DAEMON-TEST-002`
- `REQ-DAEMON-OBS-001`
- `REQ-DAEMON-HEALTH-001`
- `REQ-DAEMON-SIGNAL-001`

Required runtime rules:
- exactly one daemon process may be active on a host at a time
- singleton enforcement is host-wide rather than socket-path-local; changing
  `ATM_HOME`, socket path, or test working directory must not create a legal
  second daemon
- daemon startup is blocked by at least two runtime guard layers:
  - a pre-spawn launch gate that serializes daemon creation attempts
  - a daemon-side startup gate that refuses serving state when ownership is
    already held
- daemon startup must fail deterministically if a live daemon already owns the
  runtime
- daemon startup must not publish a serving socket or accept requests before
  singleton ownership is confirmed
- stale ownership cleanup must never allow two live daemons
- stale ownership cleanup must preserve the same singleton guarantee as normal
  startup; cleanup is recovery, not an alternate launch path
- graceful shutdown must stop accepts, drain or cancel inflight work within one
  bounded deadline, checkpoint WAL, and release singleton ownership
- signal handlers must be installed before listeners are opened
- daemon config must validate once at startup before listeners are opened
- `SIGHUP`-driven config or roster rescan must either apply a fully valid
  configuration or fail with a typed reload error while retaining the prior
  serving configuration
- remote delivery must be daemon-to-daemon only
- the same transport protocol must be exercisable through an in-process
  `test-socket` without changing handler/business logic
- transport/store/health operations must obey one documented timeout budget
  - authoritative timeout budget references:
    [`../architecture.md §21.6.4`](../architecture.md) and
    [`architecture.md §3.4`](./architecture.md)
- runtime queues and handles must obey one documented concrete cap policy
- daemon memory is the live truth for agent status
- daemon memory must also retain `last_active_at` for each known active agent
- daemon memory must retain the current agent `pid` as a first-class liveness
  field, cached from SQLite; `pid` is durable roster truth rather than
  advisory metadata
- `pid` is a semantic newtype candidate and must not remain an unvalidated raw
  integer at the daemon boundary once the runtime API hardening slice lands
- SQLite must not own live `last_active_at`; it owns durable roster state and
  the current per-member `pid`
- the daemon-managed member fields (`pid`, `last_active_at`, `state`) must
  update only through one documented heartbeat socket handler shared by ATM CLI
  and hook/runtime producers; see `docs/team-member-state.md`
- until `schooks 1.0` is released, pid/activity updates may arrive through the
  interim Python hooks installed from `../agent-team-mail`
- after `schooks 1.0` is released, `schooks` becomes the controlled hook
  environment layer and reports pid/activity updates to `atm-daemon`
- if a heartbeat reports a different pid while the stored pid is still alive,
  the daemon must reject the update unless the explicit admin takeover path
  documented in `docs/team-member-state.md` is active
- accepted pid changes must update SQLite and emit `AgentPidChanged`
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
  Unix-domain or TCP/TLS adapter code
- handler implementations for request families must be injectable behind that
  dispatcher
- the dispatcher boundary itself must remain thin and must not absorb request
  family business logic
- the socket receive loop must not perform SQL, watcher, notifier, or
  workflow/state-transition logic inline
- any violation of these daemon boundary rules is a direct QA failure
- daemon tests must not become the normal mechanism for validating core ATM
  correctness
- daemon runtime failures must remain typed across transport/runtime boundaries
  rather than collapsing into panic/unwrap control flow
- daemon runtime and transport paths must emit structured observability events
- daemon must expose one explicit health/status query interface for `atm doctor`
- no `atm-daemon` crate API, helper, or test support path may bless daemon
  spawning as a routine correctness strategy
