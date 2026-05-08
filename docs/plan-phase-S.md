# Phase S Task List

## 1. Goal

Close the missed requirement that ATM daemon functionality must work on
Windows as well as Unix-like hosts.

Phase R proved the daemon/runtime split and the SQLite-backed control plane.
Phase S replaces the remaining Unix-only host-shell assumptions with explicit
cross-platform daemon host boundaries and then implements those boundaries.

Planning baseline:
- `integrate/phase-R` at `6a072c1`
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

## 5. Planned Sprint Sequence

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

### S.4 Cross-Platform Hardening And Release Closeout

Goal:
- complete CI, QA, and documentation parity for the cross-platform daemon host

Required outcomes:
- same-host daemon functionality is test-covered and supported on macOS,
  Linux, and Windows
- docs, boundary inventories, and product plan match the landed host design
- no remaining production path depends on Unix-only host APIs outside owned
  adapter modules

## 6. Crate Candidates

Phase S planning assumes these crate directions unless implementation review
finds a blocking issue:
- local IPC: `interprocess::local_socket`
- cross-platform file locking / host ownership foundation: `fs4`
- console termination control: `ctrlc`
- Windows service-control path: `windows-services`

These are implementation candidates, not a waiver for architecture review.
They still must land behind ATM-owned boundaries.

## 7. Risks And Watchouts

- do not solve this with scattered `#[cfg(windows)]` branches above the daemon
  adapter layer
- do not keep Unix domain socket assumptions in docs while implementing named
  pipes in code
- do not let Windows support stop at clean compilation; same-host daemon
  functionality must actually run
- do not leave host ownership and lifecycle control fused into one opaque blob;
  they need separate review and enforcement surfaces
