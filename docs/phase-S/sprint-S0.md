# Phase S.0 — Cross-Platform Daemon Host Planning

```yaml
plan_type: sprint_plan
phase: S
sprint: "S.0"
worktree: /Users/randlee/Documents/github/atm-core-worktrees/phase-S-planning
branch: phase-S-planning
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

- `REQ-P-PRODUCT-001`
- `REQ-P-RUNTIME-002`
- `REQ-CORE-DAEMON-004`
- `REQ-CORE-TRANSPORT-001`
- `REQ-CORE-BOUNDARY-001`

## Governing ADRs

- `docs/adr/ADR-002-host-wide-daemon-singleton.md`
- `docs/adr/ADR-003-test-fidelity-and-daemon-isolation.md`
- `docs/adr/ADR-005-host-scoped-sqlite-state-root.md`
- `docs/adr/ADR-007-supported-platform-parity.md`

## Governing Boundaries

- `BOUNDARY-ServerTransport-Socket`
- `BOUNDARY-RuntimeLifecycle-Daemon`
- `BOUNDARY-HostOwnership-Daemon`
- `BOUNDARY-LifecycleControlSource-Daemon`

## Prerequisites

- `integrate/phase-R` baseline at `6a072c1`

## Hard Dependencies

- do not start Windows implementation work before the target daemon host model
  is documented and internally consistent

## Non-Goals

- implementing named pipes
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
   - document the preferred crate candidates for local IPC, file locking, and
     lifecycle control
   Required tests:
   - none
   Required doc or boundary updates:
   - add `docs/plan-phase-S.md`

## Split Recommendation

Do not split. This planning sprint is only successful if the product docs,
daemon docs, and Phase S sequence are all updated together.

## Acceptance Criteria

- active docs no longer describe Unix-only same-host daemon hosting as the
  target product architecture
- Phase S defines one cross-platform local IPC target for same-host transport
- host ownership and lifecycle control are explicit daemon review surfaces
- the Phase S sprint sequence is concrete enough to execute without reopening
  the architectural direction
- Phase S docs explicitly require feature parity on all supported operating
  systems rather than compile-only support
- Phase S docs enumerate the exact current daemon methods/files that S.1-S.4
  must change
- Phase S docs define shared Windows/Unix same-host functional coverage and
  explicit anti-flake rules

## Required Validation

- `just lint`

## Required Document Updates

- `docs/plan-phase-S.md`
- `docs/phase-S/sprint-S0.md`
- `docs/phase-S/sprint-S1.md`
- `docs/phase-S/sprint-S2.md`
- `docs/phase-S/sprint-S3.md`
- `docs/phase-S/sprint-S4.md`
- `docs/phase-S/issues.md`
- `docs/project-plan.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/boundaries.md`
- `docs/testing-guidelines.md`
- `docs/cross-platform-guidelines.md`
- `docs/adr/ADR-007-supported-platform-parity.md`

## Risks And Watchouts

- avoid “Windows support” language that only means compile support
- avoid locking the design to Unix concepts such as socket paths or signals in
  caller-visible layers
- avoid overcommitting to one crate without preserving the ATM-owned boundary
  abstraction
