# Phase S.0 — Cross-Platform Daemon Host Planning

```yaml
plan_type: sprint_plan
phase: S
sprint: "S.0"
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pS-s0-planning
branch: feature/pS-s0-planning
status: accepted
estimated_scope: M
```

## Goal

Turn the Windows daemon parity miss into a concrete cross-platform daemon host
plan with explicit boundaries, implementation sequencing, and tightened product
and crate-local documentation.

## Scope Summary

This sprint is documentation and architecture only. It does not implement
Windows daemon functionality. It produces the enforceable plan that Phase S
implementation sprints will follow.

## Governing Requirements

- `REQ-P-PLATFORM-001`
- `REQ-P-PLATFORM-002`
- `REQ-P-PRODUCT-001`
- `REQ-P-RUNTIME-002`
- `REQ-CORE-DAEMON-003`
- `REQ-CORE-TRANSPORT-001`
- `REQ-CORE-BOUNDARY-001`
- `REQ-DAEMON-PLATFORM-001`
- `REQ-DAEMON-PLATFORM-002`
- `REQ-DAEMON-TRANSPORT-008`
- `REQ-DAEMON-TEST-003`
- `REQ-DAEMON-TEST-004`

## Governing ADRs

- `docs/adr/ADR-002-host-wide-daemon-singleton.md`
- `docs/adr/ADR-003-test-fidelity-and-daemon-isolation.md`
- `docs/adr/ADR-005-host-scoped-sqlite-state-root.md`
- `docs/adr/ADR-007-supported-platform-parity.md`
- `docs/adr/ADR-008-no-flaky-test-policy-and-mechanical-enforcement.md`

## Governing ICD Sections

- `docs/atm-daemon/protocol-icd.md §3.1` Phase S supported packet surface
- `docs/atm-daemon/protocol-icd.md §5` shared ATM frame
- `docs/atm-daemon/protocol-icd.md §6.5` packet-kind to workflow mapping
- `docs/atm-daemon/protocol-icd.md §8` exchange rules
- `docs/atm-daemon/protocol-icd.md §10` timeout and failure semantics

## Governing Boundaries

- `BOUNDARY-ServerTransport-Socket`
- `BOUNDARY-RuntimeLifecycle-Daemon`
- `BOUNDARY-HostOwnership-Daemon`
- `BOUNDARY-LifecycleControlSource-Daemon`

## Prerequisites

- post-`PR #200` `origin/integrate/phase-R` review baseline at `d5e49df`

## Hard Dependencies

- do not start Windows implementation work before the target daemon host model
  is documented and internally consistent

## Non-Goals

- implementing a Windows-specific local endpoint
- implementing Windows service support
- changing `atm-core` code without a documented boundary reason

## Sub-Tasks

1. Baseline review and defect framing
   Development work:
   - review the integrated daemon host shell for Unix-only same-host
     assumptions
   - classify the miss as a product requirement gap rather than a CI-only
     issue
   Required tests:
   - none
   Required doc or boundary updates:
   - add a Phase S issue record describing the baseline defects and the
     planning baseline SHA

2. Boundary reset
   Development work:
   - define the cross-platform local IPC, lifecycle control, and host ownership
     boundaries
   - decide what remains in runtime orchestration versus platform adapters
   - define the logical same-host endpoint contract and same-user local-IPC
     access-control policy so callers do not depend on Unix socket paths,
     Windows pipe names, or platform-specific ACL details
   Required tests:
   - none
   Required doc or boundary updates:
   - update daemon architecture, requirements, and boundary inventory
   - add machine-readable boundary records for lifecycle control and host
     ownership

3. Product-document alignment
   Development work:
   - align top-level architecture, requirements, and project plan with the
     cross-platform daemon host target
   - remove active-doc wording that still treats Unix-only same-host transport
     as the target design
   Required tests:
   - none
   Required doc or boundary updates:
   - update `docs/requirements.md`
   - update `docs/architecture.md`
   - update `docs/project-plan.md`

4. Phase sequencing and crate-candidate decision
   Development work:
   - define the implementation sprint sequence for Phase S
   - name the exact current daemon files and methods each implementation sprint
     must change
   - define the same framed packet contract used by same-host local IPC and
     cross-host daemon transport
   - assign the exact Phase S.0 ATM frame constants in the ICD:
     - `magic`
     - `version`
     - `message_kind` numeric registry
     - `flags`
     - `request_id`
     - `payload_length`
   - map the current packet kinds to the exact owned payload DTO types and
     identify current protocol-layer DTOs that are not public packet kinds
   - define the transport module split so same-host IPC code can be isolated
     and reused outside ATM with only packet enums/serializers swapped
   - reject UDP for CLI-daemon request/response traffic
   - document the preferred crate candidates for local IPC, file locking, and
     lifecycle control
   - record that final crate adoption is deferred until S.1 validates the
     adapter surface
   - record that PID liveness semantics are carried forward unchanged during
     Phase S unless a later ADR reopens that design
   Required tests:
   - none
   Required doc or boundary updates:
   - add `docs/plans/phase-S/plan-phase-S.md`

5. Shared test and CI contract
   Development work:
   - document the anti-flake synchronization contract for same-host daemon
     tests
   - document the temporary Windows CI lint narrowing and the S.4 requirement
     to remove it
   Required tests:
   - none
   Required doc or boundary updates:
   - update `docs/testing-guidelines.md`
   - update `docs/cross-platform-guidelines.md`

## Split Recommendation

Do not split. This planning sprint is only successful if the product docs,
daemon docs, and Phase S sequence are all updated together.

## Acceptance Criteria

- active docs no longer describe Unix-only same-host daemon hosting as the
  target product architecture
- Phase S defines one cross-platform local IPC target for same-host transport
- Phase S defines one shared ATM frame format for same-host local IPC and
  cross-host daemon transport
- the shared ATM frame contract has one canonical ICD under
  `docs/atm-daemon/protocol-icd.md`
- the canonical ICD includes the exact current `magic`, `version`,
  `message_kind`, `flags`, `request_id`, and `payload_length` contract
- the canonical ICD includes the exact current packet-kind registry, payload
  DTO mapping, and non-packet retained-surface inventory
- Phase S defines the frame failure contract: invalid, partial, oversized, or
  timed-out frames terminate the connection instead of attempting mid-stream
  resynchronization
- host ownership and lifecycle control are explicit daemon review surfaces
- Phase S defines one stable cross-platform singleton lock-file model using
  permanent `launch.lock` / `owner.lock` paths plus held-lock owner metadata
- the Phase S sprint sequence is concrete enough to execute without reopening
  the architectural direction
- the S.1-S.5 sprint documents exist and each names the exact code targets,
  governing references, and acceptance criteria required for implementation
  review
- Phase S docs explicitly require feature parity on all supported operating
  systems rather than compile-only support
- Phase S docs enumerate the exact current daemon methods/files that S.1-S.4
  must change
- Phase S docs define shared Windows/Unix same-host functional coverage and
  explicit anti-flake rules
- Phase S docs include one consolidated allowed OS-difference inventory
- Phase S docs state the Windows hosting model decision explicitly
- Phase S docs define the temporary Windows CI lint narrowing and the S.4
  requirement to remove it

## No-Flaky-Test And Bounded-Wait Contract

Phase S same-host daemon tests must use positive synchronization primitives:
- channel-based ready handshakes
- `Barrier`, `Condvar`, or latch/predicate synchronization
- documented listener-ready or worker-ready probes with bounded deadlines
- panic-safe cleanup of shared/global test hooks
- bounded drain/join of helper threads and finalizer threads

They must not use:
- fixed sleeps
- warm-up delays
- retry loops with no observable readiness predicate
- OS-specific timing fudge added only to appease Windows CI
- unbounded `recv()`, `wait()`, or equivalent blocking operations in risky
  same-host daemon test paths
- bare `join()` when no prior bounded handshake proves completion

## Required Validation

- `just lint`

## Required Document Updates

- `docs/plans/phase-S/plan-phase-S.md`
- `docs/plans/phase-S/sprint-S0.md`
- `docs/atm-daemon/protocol-icd.md`
- `docs/plans/phase-S/sprint-S1.md`
- `docs/plans/phase-S/sprint-S2.md`
- `docs/plans/phase-S/sprint-S3.md`
- `docs/plans/phase-S/sprint-S4.md`
- `docs/plans/phase-S/sprint-S5.md`
- `docs/plans/phase-S/issues.md`
- `docs/project-plan.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`
- `docs/atm/requirements.md`
- `docs/atm/architecture.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/boundaries.md`
- `boundaries/atm-daemon/socket-server-transport.toml`
- `boundaries/atm-daemon/runtime-lifecycle-daemon.toml`
- `boundaries/atm-daemon/lifecycle-control-source.toml`
- `boundaries/atm-daemon/host-ownership-daemon.toml`
- `docs/testing-guidelines.md`
- `docs/cross-platform-guidelines.md`
- `docs/adr/ADR-007-supported-platform-parity.md`
- `docs/adr/ADR-008-no-flaky-test-policy-and-mechanical-enforcement.md`

## Risks And Watchouts

- avoid “Windows support” language that only means compile support
- avoid locking the design to Unix concepts such as socket paths or signals in
  caller-visible layers
- avoid overcommitting to one crate without preserving the ATM-owned boundary
  abstraction
