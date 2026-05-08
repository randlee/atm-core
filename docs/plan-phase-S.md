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
  `/Users/randlee/Documents/github/atm-core-worktrees/phase-S-planning`

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
   - Windows implementation: named-pipe-backed local IPC

2. `LifecycleControlSourceAdapter`
   - graceful shutdown and bounded reload control events
   - Unix implementation may use `SIGINT` / `SIGTERM` / `SIGHUP`
   - Windows implementation may use console or service-control events

3. `HostOwnershipAdapter`
   - host-wide singleton admission
   - owner-record maintenance
   - stale-owner recovery
   - ordered release semantics

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

| Area | Boundary | Unix implementation | Windows implementation | Shared contract |
|---|---|---|---|---|
| Same-host local IPC | `BOUNDARY-ServerTransport-Socket` | Unix domain socket | named-pipe-backed local IPC | same request/response framing, deadlines, and typed error surface |
| Lifecycle control | `BOUNDARY-LifecycleControlSource-Daemon` | signal-backed control source | console or service-control event source | same bounded shutdown and reload semantics |
| Host ownership | `BOUNDARY-HostOwnership-Daemon` | Unix file-lock and owner-record mechanics | Windows file-lock and owner-record mechanics | same singleton admission, stale-owner recovery, and ordered release semantics |

No other production same-host daemon surface may branch on operating system
until the architecture and machine-readable boundary inventory are updated
first.

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

## 4.1 Anti-Flake Synchronization Contract

The same-host daemon plan must forbid timing-only stabilization and must name
the positive replacements that implementation sprints use instead:
- explicit ready handshakes over channels
- `Barrier`, `Condvar`, or latch-style predicate synchronization
- listener-ready or worker-ready state probes on documented bounded deadlines
- bounded retry only when tied to an explicit observable state transition

The following are not acceptable for same-host daemon parity coverage:
- fixed sleeps
- warm-up delays
- retry loops with no explicit state predicate
- platform-specific timing fudge intended only to “make Windows pass”

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

Required artifacts:
- `docs/plan-phase-S.md`
- `docs/phase-S/sprint-S0.md`
- `docs/phase-S/issues.md`
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

Required code targets:
- `crates/atm-daemon/src/composition.rs`
  - `RuntimeComposition::start`
  - `RuntimeComposition::start_with_socket_path_for_test`
  - `validate_runtime_socket_path`
  - `validate_runtime_home_dir`
  - `compose_runtime`
- `crates/atm-daemon/src/lib.rs`
  - `PreparedRuntimeServer::bind`
  - `PreparedRuntimeServer::serve_with_runtime_hooks`
  - `PreparedRuntimeServer::serve_with_deadlines_and_accept_probe`
  - `drain_active_connections_for_shutdown`
  - `handle_connection`
  - `ActiveConnectionRegistry::{register, interrupt_all, wait_for_connection_change}`
- `crates/atm-daemon/src/shutdown_signals.rs`
  - `DaemonShutdownSignals::install`

Required refactor direction:
- remove direct `UnixListener` / `UnixStream` / signal constant dependencies
  from runtime orchestration
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
- `crates/atm-daemon/src/lib.rs`
  - imports and concrete fields that currently depend on `UnixListener` and
    `UnixStream`
  - `PreparedRuntimeServer`
  - `RuntimeServerTransport::prepare_runtime`
  - `RuntimeServerTransport::prepare_runtime_at_socket_path`
  - `remove_stale_socket`
  - `handle_connection`
- `crates/atm-daemon/src/tests.rs`
  - replace Unix-only same-host transport tests with shared harness coverage

Required refactor direction:
- move Unix socket path validation and connection interruption mechanics behind
  the local-IPC adapter
- replace Unix-only same-host runtime stubs with a real Windows transport
  implementation
- keep one shared handler/dispatcher test harness across Unix and Windows

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

Required code targets:
- `crates/atm-daemon/src/shutdown_signals.rs`
  - `DaemonShutdownSignals::install`
  - `DaemonShutdownSignals::new_for_test`
- `crates/atm-daemon/src/lib.rs`
  - `host_runtime_lock_path`
  - `host_runtime_lock_path_from_home`
  - `write_owner_record`
  - `recorded_owner_pid`
  - `SingletonGuard::{acquire, acquire_at, drop}`
  - `open_singleton_lock`
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
- remove the temporary Windows lint guardrail by restoring full
  `cargo clippy --workspace --all-targets -- -D warnings` coverage for Windows
  in both `just lint` and GitHub CI once `atm-daemon` is no longer a
  stub-backed non-Unix runtime
- add or tighten lint/review guards that reject:
  - fixed-sleep daemon stabilization
  - new broad `#[cfg(unix)]` gating outside adapter modules
  - Unix-only same-host functionality in production paths
- reconcile docs, ADRs, and machine-readable boundaries so the production
  design names every allowed OS-specific implementation difference

## 6. Temporary Windows CI Guardrail

The current daemon code compiles on Windows, but `atm-daemon` is not yet
Windows-live enough for full workspace `clippy -D warnings` without triggering
dead-code and unused-item churn from the non-Unix unsupported path.

Temporary rule:
- Windows `just lint` and the Windows CI lint lane may scope clippy to the
  cross-platform workspace crates by excluding `atm-daemon`
- Windows workspace build and test coverage remain mandatory
- Linux and macOS keep full workspace clippy

Removal condition:
- S.4 must delete this temporary guardrail and re-enable full Windows
  `cargo clippy --workspace --all-targets -- -D warnings` in both local and CI
  lint paths

## 7. Crate Candidates

Phase S planning assumes these crate directions unless implementation review
finds a blocking issue:
- local IPC: `interprocess::local_socket`
- cross-platform file locking / host ownership foundation: `fs4`
- console termination control: `ctrlc`
- Windows service-control path: `windows-services`

These are preferred implementation candidates, not accepted architecture by
themselves.

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
