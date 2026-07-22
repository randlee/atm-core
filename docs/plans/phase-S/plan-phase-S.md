# Phase S Task List

## 1. Goal

Close the missed requirement that ATM daemon functionality must work on
Windows as well as Unix-like hosts.

Phase R proved the daemon/runtime split and the SQLite-backed control plane.
Phase S replaces the remaining Unix-only host-shell assumptions with explicit
cross-platform daemon host boundaries and then implements those boundaries.

Planning baseline:
- post-`PR #200` review baseline: `origin/integrate/phase-R` at `d5e49df`
- follow-on CI-only compatibility fixes after the baseline review do not change
  the portability findings this phase addresses
- planning worktree:
  `/Users/randlee/Documents/github/atm-core-worktrees/feature/pS-s0-planning`

## 2. Requirement Miss

The current integrated daemon still hard-codes Unix-only behavior in the
same-host host/runtime shell:
- same-host listener and client types are Unix-domain-socket-specific
- lifecycle control is modeled as Unix signals rather than a platform-neutral
  control source
- active-connection interruption and drain logic is tied to Unix stream
  mechanics
- same-host functional tests do not provide Windows host-runtime parity

That is not a minor CI issue. It is a product requirement miss because the
expected release behavior is full Windows feature parity.

## 3. Design Reset

Phase S hardens the daemon around four portability boundaries:

1. `LocalIpcServerTransportAdapter`
   - same-host daemon listener and stream contract
   - Unix implementation: Unix domain socket
   - Windows implementation: legacy local IPC
   - owns logical endpoint naming and same-user local-IPC access control

2. `LifecycleControlSourceAdapter`
   - graceful shutdown and bounded reload control events
   - Unix implementation may use `SIGINT` / `SIGTERM` / `SIGHUP`
   - Windows implementation may use console or service-control events

3. `HostOwnershipAdapter`
   - host-wide singleton admission
   - owner-record maintenance
   - stale-owner recovery
   - ordered release semantics
   - stable permanent lock-file paths rather than deletion-based ownership
     signaling

4. `test-socket`
   - the same dispatcher/handler contract for in-process transport testing
   - functional transport tests must use the real local-IPC boundary on both
     Unix and Windows rather than only Unix-only harnesses

Governing parity rule:
- Phase S is complete only when the same-host daemon feature set is
  production-ready on macOS, Linux, and Windows
- Windows compilation, stubbed `daemon_unavailable(...)` paths, or Unix-only
  functional test coverage do not satisfy the goal

## 3.1 Windows Hosting Model Decision

Phase S targets one Windows hosting model:
- the same user-facing background daemon model used on Unix-like hosts
- one user-scoped daemon process started by ATM commands when needed
- same-host local IPC through the owned local-IPC adapter
- bounded shutdown and reload through the owned lifecycle-control adapter

Service-control integration may exist behind the Windows lifecycle adapter, but
Phase S parity does not depend on introducing a separate SCM-only daemon model.

## 3.2 Allowed Operating-System Difference Inventory

| Area | Boundary | Unix implementation | Windows implementation | Shared contract | Key Files/Types | Responsible Sprint |
|---|---|---|---|---|---|---|
| Same-host local IPC | `BOUNDARY-ServerTransport-Socket` | Unix domain socket | legacy local IPC | same request/response framing, deadlines, and typed error surface | `crates/atm-daemon/src/local_ipc_transport.rs`, `crates/atm-daemon/src/composition.rs`, `PreparedRuntimeServer`, `LocalIpcServerTransportAdapter` | `S.1` |
| Lifecycle control | `BOUNDARY-LifecycleControlSource-Daemon` | signal-backed control source | console or service-control event source | same bounded shutdown and reload semantics | `crates/atm-daemon/src/lifecycle_control.rs`, `crates/atm-daemon/src/composition.rs`, `LifecycleControlSourceAdapter`, `RuntimeComposition` | `S.1` |
| Host ownership | `BOUNDARY-HostOwnership-Daemon` | Unix file-lock and owner-record mechanics | Windows file-lock and owner-record mechanics | same singleton admission, stable `launch.lock` / `owner.lock` paths, stale-owner recovery, and ordered release semantics | `crates/atm-daemon/src/host_ownership.rs`, `crates/atm-daemon/src/composition.rs`, `HostOwnershipAdapter`, `host_runtime_lock_path*`, `write_owner_record`, `recover_stale_owner_lock` | `S.1` |

No other production same-host daemon surface may branch on operating system
until the architecture and machine-readable boundary inventory are updated
first.

## 3.3 Shared ATM Frame Contract

Phase S standardizes one framed ATM packet for both:
- same-host local IPC
- cross-host daemon-to-daemon transport

Canonical source:
- [`docs/atm-daemon/protocol-icd.md`](./atm-daemon/protocol-icd.md)

The historical EOF-delimited JSON-on-stream behavior is a portability debt.
S.1 replaces it with the shared ATM frame helpers and keeps that framed
contract authoritative for later Windows parity work.

The ICD is the source of truth for:
- exact frame constants
- exact packet-kind numeric assignments
- exact payload DTO mapping
- exact current daemon packet surface versus retained non-packet workflows

Required semantics:
- use the ATM frame header and failure rules from the daemon protocol ICD
- keep the same frame contract across local IPC and remote daemon transport

Design rule:
- local IPC and remote TCP/TLS use the same ATM frame header and the same
  request/response packet family
- host-host traffic may add transport/session context, but must not fork a
  second daemon message system

UDP decision:
- UDP is not an accepted Phase S transport for CLI-daemon request/response
  messaging
- request/response mail/control traffic requires ordered, bounded, reliable
  stream semantics plus explicit response-based completion
- a future ADR would be required before UDP could be introduced for any ATM
  control-plane feature

## 3.4 Portable Transport Module Split

At minimum, the Phase S implementation must move same-host IPC and shared frame
helpers into dedicated transport module trees so the code is reviewable,
portable, and easy to copy or re-implement in other projects.

Required implementation direction:
- shared ATM frame definitions and encode/decode helpers stay in `atm-core`
- same-host IPC adapter internals live under a dedicated daemon transport
  module tree rather than crate-root runtime code
- the CLI local transport uses the same frame helper layer as the daemon local
  server and the remote peer transport

Planned ownership split:
- `crates/atm-core/src/protocol.rs`
  - ATM frame header schema
  - message-kind enum
  - request/response packet DTOs
  - framed read/write helpers
- `crates/atm-daemon/src/transport/local_ipc/`
  - local listener accept
  - local stream read/write plumbing
  - adapter-specific readiness, timeout, endpoint naming, and access-control
    behavior
- `crates/atm-daemon/src/transport/peer/`
  - remote TCP/TLS client/server framing reuse
  - bounded retry and acceptance semantics
- `crates/atm/src/transport/local_ipc/`
  - thin-client same-host connect/send/receive path using the shared ATM frame

Extraction rule:
- the portable framing layer must not depend on Unix socket types, Windows
  pipe types, or ATM-daemon runtime orchestration
- the portable transport code may stay inside existing crates in Phase S, but
  it must be isolated enough that a later crate extraction is mechanical rather
  than architectural

## 4. Documentation Hardening Loop

Status:
- complete on the planning branch

Completed hardening work:
- removed Unix-only same-host transport assumptions from the active target
  docs
- tightened daemon crate requirements and architecture around platform-neutral
  local IPC and runtime-control boundaries
- split host ownership and lifecycle control into review-visible boundary
  records
- aligned the top-level plan, product requirements, and daemon-local docs on
  one cross-platform target

Acceptance:
- the current documentation set describes one production-ready cross-platform
  daemon host design instead of a Unix-only host shell plus Windows compile
  stubs

## 4.1 No-Flaky-Test And Bounded-Wait Contract

The same-host daemon plan must forbid timing-only stabilization and any test
shape that can block indefinitely, and must name the positive replacements
that implementation sprints use instead:
- explicit ready handshakes over channels
- `Barrier`, `Condvar`, or latch-style predicate synchronization
- listener-ready or worker-ready state probes on documented bounded deadlines
- bounded retry only when tied to an explicit observable state transition
- panic-safe cleanup of shared/global test hooks
- bounded finalizer and helper-thread drain paths

The following are not acceptable for same-host daemon parity coverage:
- fixed sleeps
- warm-up delays
- retry loops with no explicit state predicate
- platform-specific timing fudge intended only to “make Windows pass”
- unbounded `recv()`, `wait()`, or equivalent blocking operations in flaky-risk
  test paths
- bare `join()` when the test has no prior bounded proof that the worker
  already completed
- shared/global test hooks that can remain installed after panic or timeout

Accepted production exceptions:
- `crates/atm-daemon/src/lifecycle_control.rs`
  - Windows lifecycle-control wake propagation uses a bounded `25ms` polling loop after
    `signal_hook::flag` registration because the retained cross-platform signal surface does not
    provide a blocking Windows wake primitive that matches the shared install contract.
- `crates/atm/src/composition.rs`
  - daemon auto-start waits on an external child process publishing the local IPC endpoint, so the
    client uses bounded `poll_interval` sleeps while no push-style readiness surface exists
- `crates/atm-daemon/src/runtime_health.rs`
  - retained-observability flush during shutdown remains best-effort and bounded to `2s` so daemon
    teardown cannot stall indefinitely behind sink I/O
- `crates/atm-daemon/src/test_support.rs`
  - `connect_daemon_local_ipc_until_ready` uses `sleep(5ms)` because the polling helper needs a
    fixed backoff while waiting for the listener-ready state transition; `yield_now()` does not
    provide a delay guarantee.

## 5. Planned Sprint Sequence

### S.0 Planning And Documentation Hardening

Goal:
- lock the cross-platform daemon target, seam inventory, Windows hosting model,
  anti-flake contract, and CI transition plan before implementation work begins

Required outcomes:
- product docs, daemon docs, ADRs, and machine-readable boundary records all
  describe the same target
- the implementation sprints name the exact integrated code they must change
- the temporary Windows CI mitigation and its removal condition are documented
- PID liveness semantics are explicitly carried forward unchanged during Phase S
  unless a later ADR reopens that design
- the cross-platform singleton plan uses stable permanent `launch.lock` and
  `owner.lock` files with one whole-file exclusive-lock contract rather than
  deletion-based handoff semantics

Required artifacts:
- `docs/plans/phase-S/plan-phase-S.md`
- `docs/plans/phase-S/sprint-S0.md`
- `docs/plans/phase-S/issues.md`
- `docs/atm-daemon/protocol-icd.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`
- `docs/atm/requirements.md`
- `docs/atm/architecture.md`
- `docs/adr/ADR-007-supported-platform-parity.md`
- `boundaries/atm-daemon/{socket-server-transport,runtime-lifecycle-daemon,lifecycle-control-source,host-ownership-daemon}.toml`

### S.1 Cross-Platform Host Boundary Extraction

Goal:
- refactor the daemon host shell so same-host transport, lifecycle control, and
  host ownership are explicit owned adapters rather than Unix-specific code
  paths embedded in runtime orchestration

Required outcomes:
- runtime orchestration no longer depends directly on Unix listener/stream or
  signal types
- platform cfg is isolated to owned adapter modules
- any required `atm-core` boundary trait changes are documented and landed
- shared framed transport helpers exist so same-host and remote transports no
  longer rely on EOF-delimited JSON streams

Required code targets:
- `crates/atm-core/src/protocol.rs`
  - `FramePayload`
  - `read_bounded_stream`
  - daemon frame/path helpers that currently encode Unix socket assumptions
- `crates/atm-core/src/boundary/mod.rs`
  - `AtmProtocol`
  - `ClientTransport`
  - `ServerTransport`
- `crates/atm-daemon/src/composition.rs`
  - `RuntimeComposition::start`
  - `RuntimeComposition::start_with_socket_path_for_test`
  - `validate_runtime_home_dir`
  - `compose_runtime`
- same-host endpoint validation is currently split between:
  - `crates/atm/src/composition.rs::DaemonLocalIpcEndpoint::new`
  - `crates/atm-core/src/protocol.rs::daemon_local_ipc_name_from_path`
- `crates/atm-daemon/src/lib.rs`
  - runtime crate-root ownership and adapter re-exports only
- `crates/atm-daemon/src/local_ipc_transport.rs`
  - `PreparedRuntimeServer::bind`
  - `PreparedRuntimeServer::serve_with_runtime_hooks`
  - `PreparedRuntimeServer::serve_with_deadlines_and_accept_probe`
  - `drain_active_connections_for_shutdown`
  - `handle_connection`
  - `ActiveConnectionRegistry::{register, interrupt_all, wait_for_connection_change}`
- `crates/atm-daemon/src/lifecycle_control.rs`
  - `LifecycleControlSourceAdapter::install`
- `crates/atm-daemon/src/host_ownership.rs`
  - `HostOwnershipAdapter::{acquire, acquire_at}`
  - `host_runtime_lock_path`
- `crates/atm/src/composition.rs`
  - `LocalIpcClientTransportAdapter::{try_connect, exchange}`
  - `resolve_daemon_local_ipc_endpoint`

Required refactor direction:
- remove direct `UnixListener` / `UnixStream` / signal constant dependencies
  from runtime orchestration
- replace EOF-delimited stream framing with shared ATM frame helpers
- replace broad `#[cfg(unix)]` gating on composition/runtime entrypoints with
  adapter-owned platform selection
- document every remaining allowed OS-specific seam before S.1 closes

### S.2 Windows Local IPC Implementation

Goal:
- implement the Windows same-host daemon API through the new local IPC
  transport boundary

Required outcomes:
- Windows same-host daemon listener/client path is real, not
  `daemon_unavailable`
- request/response framing, deadlines, and typed error behavior match the Unix
  path
- same-host functional tests pass on Windows through the real transport

Required code targets:
- `crates/atm-core/src/protocol.rs`
  - `daemon_socket_path`
  - `daemon_local_ipc_name`
  - `daemon_local_ipc_name_from_path`
- `crates/atm-daemon/src/local_ipc_transport.rs`
  - `PreparedRuntimeServer::bind`
  - `PreparedRuntimeServer::serve_with_deadlines_and_accept_probe`
  - `LocalIpcServerTransportAdapter::{prepare_runtime, prepare_runtime_at_socket_path}`
  - `prepare_local_ipc_endpoint`
  - `handle_connection`
- `crates/atm-daemon/src/tests.rs`
  - keep daemon-private tests transport-neutral above the local-IPC adapter
- `crates/atm/src/composition.rs`
  - `LocalIpcClientTransportAdapter`
  - `DaemonLocalIpcEndpoint`
  - `LaunchGateGuard`
- `crates/atm-daemon/tests/run_daemon_production_path.rs`
  - shared same-host production-path coverage through the real local IPC path

Required refactor direction:
- move Unix socket path handling and Windows endpoint mapping behind the
  local-IPC adapter plus shared endpoint helper
- replace Unix-only same-host runtime stubs with a real Windows transport
  implementation
- keep one shared handler/dispatcher test harness across Unix and Windows
- keep the same ATM frame header, request/response packet family, and
  request-id semantics on Unix and Windows

### S.3 Windows Runtime Control And Host Ownership

Goal:
- implement Windows lifecycle control and host-wide singleton ownership under
  the new boundaries

Required outcomes:
- Windows host ownership semantics are typed, tested, and bounded
- graceful shutdown and reload behavior are modeled through the
  lifecycle-control boundary rather than Unix signal assumptions
- teardown and stale-owner recovery semantics are documented and tested on both
  host families
- the stable `launch.lock` / `owner.lock` contract and owner-record update
  order are identical across supported operating systems

Required code targets:
- `crates/atm-daemon/src/lifecycle_control.rs`
  - `LifecycleControlSourceAdapter::install`
  - `LifecycleControlSourceAdapter::new_for_test`
- `crates/atm-daemon/src/host_ownership.rs`
  - `host_runtime_lock_path`
  - `host_runtime_lock_path_from_home`
  - `write_owner_record`
  - `recorded_owner_pid`
  - `HostOwnershipAdapter::{acquire, acquire_at}`
  - `open_lock_file`
  - `recover_stale_owner_lock`
- `crates/atm-daemon/src/composition.rs`
  - `RuntimeComposition::begin_shutdown`
  - `RuntimeComposition::finalize_shutdown`
  - `RuntimeComposition::start`

Required refactor direction:
- replace Unix-specific signal ownership with a platform-neutral lifecycle
  control source that has Unix and Windows implementations
- replace Unix-shaped host-ownership mechanics with one cross-platform
  ownership contract that preserves identical singleton and teardown rules
- implement host ownership as stable permanent lock files plus held-lock owner
  metadata rather than path deletion signaling
- prove ordered release semantics and stale-owner recovery on Unix and Windows

### S.4 Cross-Platform Hardening And Release Closeout

Goal:
- complete CI, QA, and documentation parity for the cross-platform daemon host

Required outcomes:
- same-host daemon functionality is test-covered and supported on macOS,
  Linux, and Windows
- docs, boundary inventories, and product plan match the landed host design
- no remaining production path depends on Unix-only host APIs outside owned
  adapter modules

Required closeout work:
- remove any remaining non-Unix `daemon_unavailable(...)` stubs in same-host
  runtime paths
- add review-visible coverage proving Windows same-host daemon hosting through
  shared infrastructure
- restore full Windows frame/transport linting with the same shared framing
  layer used by local IPC and remote daemon transport
- remove the temporary Windows lint guardrail by restoring full
  `cargo clippy --workspace --all-targets -- -D warnings` coverage for Windows
  in both `just lint` and GitHub CI. S.4 completed this closeout by deleting
  the `ATM_WINDOWS_CLIPPY_SCOPE=cross-platform-only` narrowing from the
  `Justfile` and `.github/workflows/ci.yml`.
- add or tighten lint/review guards that reject:
  - fixed-sleep daemon stabilization
  - new broad `#[cfg(unix)]` gating outside adapter modules
  - Unix-only same-host functionality in production paths
- reconcile docs, ADRs, and machine-readable boundaries so the production
  design names every allowed OS-specific implementation difference

### S.5 Guardrails And Bounded Queue Queries

Goal:
- tighten Phase S and top-level ATM language so the anti-flake contract is
  phase-wide rather than fixed-sleep-only
- define which anti-flake guardrails are feasible now in `just lint` versus
  deferred analyzer work
- document the mailbox-query redesign where `atm list` becomes the bounded
  metadata-search surface and `atm read` becomes the single-message detail
  surface

Required outcomes:
- top-level requirements, architecture, and test guidelines explicitly state
  that a test which might hang is invalid
- Phase S sprint docs state that same-host daemon coverage must avoid both
  timing-only stabilization and unbounded wait paths
- the repo has one review-visible inventory of feasible-now versus deferred
  mechanical anti-flake lint families
- the active docs state that default queue inspection must remain bounded even
  as SQLite-backed mailbox history grows without a practical fixed upper bound
- the active docs define the accepted `atm list` / `atm read` split and the
  shared filter contract between them
- the active docs define the legacy `atm read` flag migration, logical
  task/thread selection rule, and the ATM-authored Claude JSONL compatibility
  envelope

Required closeout work:
- add the S.5 sprint plan under `docs/plans/phase-S/sprint-S5.md`
- add an ADR for the repository-wide no-flaky-test policy and enforcement
  partition if the existing ADRs are not sufficient
- add an ADR for the bounded queue-query surface (`atm list` / single-message
  `atm read`)
- add an ADR for the ATM-authored Claude JSONL compatibility envelope and
  oversized-body projection rule
- update Phase S issue inventory with the remaining policy and lint gaps
- reconcile testing and cross-platform guidelines with the stronger no-hang
  contract
- reconcile the product and crate-local CLI docs with the new queue-inspection
  command split
- create the follow-on implementation sprint docs required to finish the phase

### S.6 Daemon Post-Mortem Runtime Remediation

Goal:
- close the remaining daemon/runtime remediation items left open after the
  S.4 parity line

Required outcomes:
- `RSH-001`, `RSH-014`, `WIN-001`, and `ATM-QA-S4-001` are assigned to one
  explicit execution sprint with concrete code targets
- shutdown ordering, lifecycle wake propagation, Windows graceful shutdown,
  and local-IPC endpoint-preparation behavior are all covered by one bounded
  remediation line

Required closeout work:
- fix `crates/atm-daemon/src/composition.rs::shutdown_background_lanes`
- fix the Unix lifecycle-control EOF wake propagation gap
- restore the Windows graceful-shutdown path and its coverage
- remove the silent non-Unix success path from
  `prepare_local_ipc_endpoint`

### S.7 Bounded Queue Query Implementation

Goal:
- implement the queue-query split defined by ADR-009

Required outcomes:
- `atm list` exists as a bounded metadata-query CLI surface
- `atm read` is a single-message logical-current selection path
- shared list/read filters stay aligned
- the durable query path is bounded by query behavior rather than full
  history materialization

Required closeout work:
- add `crates/atm/src/commands/list.rs`
- update `crates/atm/src/commands/read.rs` and output handling for
  single-message selection and match metadata
- add the bounded metadata-query service path in `atm-core`
- add the SQLite-backed bounded query implementation support in
  `atm-rusqlite`

### S.8 Claude JSONL Compatibility Envelope

Goal:
- implement the ADR-010 compatibility-envelope contract for ATM-authored
  Claude JSONL export and watcher/reconcile no-churn behavior

Required outcomes:
- `[atm].claude_jsonl_body_export_max_bytes` is implemented
- oversized ATM-authored messages export retrieval stubs instead of full
  bodies
- watcher/reconcile logic treats ATM-authored projection updates as
  idempotent

Required closeout work:
- add the config-backed ATM-authored export cap
- export `atm read --message-id <id>` stubs for oversized ATM-authored bodies
- preserve summary text while keeping full ATM-authored bodies durable in
  SQLite

### S.9 Host-Scoped Logging Defaults

Goal:
- move retained ATM logs to the host-scoped ATM state root and define the
  minimum default retained event set required for daemon operability

Required outcomes:
- ATM retained logs default to `~/.atm/logs/atm.log.jsonl`
- `ATM_LOG_DIR` redirects the exact retained log directory
- retained logs remain host-scoped and independent of `ATM_HOME`
- default retained logging includes daemon lifecycle `info!` events plus all
  `warn!` / `error!` events across ATM subsystems

Required closeout work:
- add `host_log_dir()` and `host_log_dir_from_home(...)` to `atm-core`
- move CLI observability bootstrap and daemon health/reporting to the
  host-scoped ATM log directory
- add `docs/atm-daemon/logging.md`
- reconcile product and crate-local observability docs with the new retained
  path and event baseline
- prevent self-induced churn loops in watcher/reconcile paths
- prove no reconcile event fires when a retained-log append writes to
  `~/.atm/logs/atm.log.jsonl`

### S.10 Daemon Retained Logger Bootstrap

Goal:
- make `atm-daemon` boot the live retained logger rather than reporting
  synthetic observability health from a stub

Required outcomes:
- daemon startup/shutdown lifecycle events land in `~/.atm/logs/atm.log.jsonl`
- `atm doctor` reflects live daemon retained-sink health
- daemon retained-log bootstrap fails closed when the host-scoped log path
  cannot be created or opened
- daemon retained logs rotate explicitly at bounded size/retention settings

Required closeout work:
- keep `sc-observability` imports out of the `atm-daemon` library target and
  construct the concrete adapter only in the binary entrypoint
- replace the legacy stderr-only bootstrap path with the live retained logger
- add a daemon-runtime integration test that boots the composed runtime and
  verifies retained-log output plus healthy doctor observability
- document that daemon-side query/follow remain deferred to the CLI-owned
  retained-log surface

### S.11 host_log_dir Override-First Order Fix

Goal:
- restore the documented override-first `host_log_dir()` contract for
  headless and service-style environments where `ATM_LOG_DIR` must work
  without resolving the OS home directory

Required outcomes:
- `ATM_LOG_DIR` is checked before any OS home-directory resolution
- valid absolute `ATM_LOG_DIR` overrides succeed even when `HOME` is absent
- cross-platform tests prove the override happy-path without Unix-only env
  assumptions
- ADR-011 explicitly records the override-first order and the accepted
  overlap-check scope boundary when `HOME` is not resolved

Required closeout work:
- update `crates/atm-core/src/home.rs` so `host_log_dir()` short-circuits on a
  valid `ATM_LOG_DIR` before any `home_dir()` call
- keep the Unix-only headless unset-`HOME` test, but add non-gated override
  coverage that works on Windows CI
- amend `docs/adr/ADR-011-host-scoped-retained-log-root.md` and
  `docs/plans/phase-S/sprint-S11.md` so the override-first contract and overlap
  scope are explicit
- record the detailed sprint authority in `docs/plans/phase-S/sprint-S11.md`

### S.12 Integration Gate Findings

Goal:
- close the post-S.10 integration-gate findings batch across `atm-daemon` and
  `atm-core` so `integrate/phase-S` is no longer carrying open flake,
  recovery-text, and bounded-shutdown regressions

Required outcomes:
- all 13 INTG findings from the Phase S integration gate are addressed or
  explicitly confirmed satisfied on the sprint base
- bounded shutdown, worker-warning, and recovery-text behavior align with the
  canonical INTG triage records
- the machine-readable INTG triage records are updated to closed status for
  the fixes landed at `bd9e0e8`
- `docs/plans/phase-S/sprint-S12.md` stays aligned with the canonical triage
  inventory

Required closeout work:
- harden `watch_runtime`, `reconcile_runtime`, and `runtime_health` to match
  the accepted INTG shutdown and recovery contracts
- remove the last known flake vectors from the targeted runtime tests
- keep the `config/discovery.rs` PATH_MAX guard carried forward on the branch
- update `docs/plans/phase-S/sprint-S12.md` plus the 12 resolved INTG TTL records so
  documentation and machine-readable triage state agree on closure

### S.13 IPC And Socket Shutdown Hardening

Goal:
- simplify same-host daemon transport shutdown so fatal accept/lifecycle paths
  stay bounded, typed, and ownership-safe under the existing singleton model

Required outcomes:
- local IPC shutdown uses one shared transport shutdown signal and explicit
  `Running -> Draining -> Stopped` lifecycle transitions
- accept-error, terminate, and shutdown-drain paths remain bounded and testable
- endpoint cleanup happens before singleton ownership release
- typed daemon exit mapping distinguishes configuration, transport-fatal, and
  lifecycle-wedge failures

Required closeout work:
- harden `crates/atm-daemon/src/local_ipc_transport.rs`,
  `lifecycle_control.rs`, `composition.rs`, and daemon tests around accept-loop
  failure, connection drain, and endpoint cleanup
- add regression coverage for accept-error teardown, terminate rejection, and
  panic-safe socket cleanup
- update the daemon architecture and sprint docs so the shutdown-beacon and
  endpoint-guard contracts are explicit

### S.14 Daemon Runtime Hardening

Goal:
- close the remaining daemon-runtime hardening gaps left after S.13 across
  lifecycle control, reconcile/watch shutdown, runtime status bounds, doctor
  projection detail, and retained-observability flush behavior
- carry forward the accepted S.13 direct-accept-loop contract from
  `docs/plans/phase-S/sprint-S13-ipc-plan.md` as the runtime baseline that S.14
  hardens rather than redesigns

Required outcomes:
- lifecycle wake workers are explicitly owned and joined with timeout
- reconcile/watch shutdown timeout is observable as typed failure
- bounded runtime state stays bounded in actual retained cardinality
- `atm doctor` projects daemon runtime health with the full required detail

Required closeout work:
- add the S.14 sprint authority docs under `docs/plans/phase-S/`
- land the S.14 runtime hardening fixes on the follow-on implementation branch
- keep daemon architecture and phase-plan docs aligned on the accepted
  resource-cap and eviction contracts

### S.15 SQLite Write-Worker / MessageAppendQueue Planning

Goal:
- define the next `atm-rusqlite` hardening pass around one in-process
  write-worker and a bounded message-append queue that increases throughput
  without widening current crate contracts

Required outcomes:
- the single-writer design, batching limits, queue backpressure contract, and
  shutdown semantics are documented authoritatively
- the mailbox append hot path drops the pre-write probe in favor of
  row-count-based insertion detection under explicit immutability rules
- follow-on implementation scope, test policy, and singleton assumptions are
  recorded before code changes start

Required closeout work:
- produce `docs/plans/phase-S/sprint-S15.md`,
  `docs/plans/phase-S/sprint-S15-rusqlite-plan.md`, and
  `docs/adr/ADR-ATM-RUSQLITE-002.md` as the governing S.15 planning set
- document queue capacity, batching constants, `spawn_blocking` requirements,
  and per-batch isolation semantics for the future implementation sprint
- keep S.15 sequenced after the S.13/S.14 runtime hardening line so the writer
  plan can depend on the already-accepted daemon singleton/runtime contracts

## 6. Removed Windows CI Guardrail

The temporary Windows clippy narrowing used during S.0-S.3 is retired.

Closed-out rule:
- Windows `just lint` and the Windows CI lint lane now run full workspace
  `cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings`
- no `ATM_WINDOWS_CLIPPY_SCOPE` narrowing remains in the `Justfile` or CI
- Windows workspace build and test coverage remain mandatory
- Linux and macOS continue to run full workspace clippy

S.4 enforcement now relies on:
- the restored full Windows clippy lane
- `fixed-sleep`
- `unix-gating`
- `same-host-portability`

## 7. Crate Candidates

Phase S planning assumes these crate directions unless implementation review
finds a blocking issue:
- local IPC: `interprocess::local_socket`
- cross-platform file locking / host ownership foundation: current extraction
  uses `fs2::FileExt::try_lock_exclusive` with one whole-file lock contract
- console termination control: `ctrlc`

These are preferred implementation candidates, not accepted architecture by
themselves.

Deferred crate note:
- `windows-services` remains out of scope for the S.0 accepted plan because
  the Phase S hosting model is the user-scoped same-host daemon, not an
  SCM-only Windows service product variant
- if a later sprint needs SCM-specific integration, it must introduce a new
  explicit ADR and update the lifecycle-control boundary documents first

Explicit deferral:
- final crate adoption is deferred until S.1 boundary extraction confirms the
  exact adapter surface
- if a candidate crate cannot satisfy the documented shared contract without
  leaking platform types above the adapter layer, the sprint must record a
  replacement decision before implementation proceeds

## 8. Risks And Watchouts

- do not solve this with scattered `#[cfg(windows)]` branches above the daemon
  adapter layer
- do not keep Unix domain socket assumptions in docs while implementing named
  pipes in code
- do not let Windows support stop at clean compilation; same-host daemon
  functionality must actually run
- do not leave host ownership and lifecycle control fused into one opaque blob;
  they need separate review and enforcement surfaces
- do not treat the temporary Windows clippy scope narrowing as a durable fix;
  S.4 must remove it
- do not let S.5 end the documented sprint line while remaining post-mortem or
  queue-query implementation work is still unassigned
