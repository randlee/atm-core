# AA.0 Daemon Architecture Restatement And State-Machine Inventory

```yaml
plan_type: sprint_plan
phase: AA
sprint: AA.0
worktree: ../atm-core-worktrees/feature/pAA-s0-daemon-architecture-restatement
branch: feature/pAA-s0-daemon-architecture-restatement
status: planned
estimated_scope: medium
```

## Goal

Freeze the intended daemon design before code deletion begins: thin router,
subsystem-owned diagnostics, direct local doctor path where possible, and a
small auditable daemon state-machine inventory.

## Scope Summary

This sprint is documentation-first. It does not remove the SQLite dependency
yet. It makes the target architecture explicit enough that later sprints can
delete code instead of arguing about ownership.

## Governing Requirements

- `REQ-P-PRODUCT-001`
- `REQ-P-DOCTOR-001`
- `REQ-CORE-BOUNDARY-001`
- `REQ-DAEMON-RUNTIME-002`

## Governing ADRs

- `docs/adr/ADR-001-sealed-trait-pattern.md`
- `docs/atm-daemon/architecture.md` ADR `ADR-ATM-DAEMON-001` as the current
  state to be superseded by Phase `AA`

## Governing Boundaries

- `boundaries/atm-rusqlite/sqlite-boundary-assembly.toml`
- `boundaries/atm-rusqlite/mail-store-sqlite.toml`
- `boundaries/atm-rusqlite/roster-store-sqlite.toml`
- `boundaries/atm-core/runtime-factory.toml`

## Prerequisites

- current daemon/SQLite leak inventory is frozen
- user-approved Phase `AA` target architecture is recorded

## Hard Dependencies

- none

## Out Of Scope

- code movement into `atm-runtime`
- trait introduction beyond documenting the ownership rule
- partial boundary rollback without a frozen leak ledger

## Deliverables

- A daemon-role restatement lands in the governing docs and says explicitly
  that `atm-daemon` owns only:
  - transport
  - lifecycle
  - request validation/routing
  - bounded dispatch/reply
  - minor runtime error handling
  and does not own concrete SQLite semantics.

- A subsystem-doctor ownership rule lands in the governing docs and says
  explicitly:
  - each subsystem diagnoses itself through a trait it owns
  - top-level doctor code aggregates subsystem reports
  - top-level doctor code may compare subsystem reports for drift
  - top-level doctor code must not reimplement backend-specific diagnosis

- `docs/phase-AA/daemon-state-machines.md` exists and freezes the target
  top-level daemon machine inventory at five or fewer machines.

- `docs/phase-AA/daemon-sqlite-leak-ledger.md` exists and classifies every
  current daemon-side SQLite leak as one of:
  - `delete`
  - `move`
  - `keep-and-rewrite`
  The minimum frozen file set is:
  - `crates/atm-daemon/src/lib.rs`
  - `crates/atm-daemon/src/composition.rs`
  - `crates/atm-daemon/src/runtime_health.rs`
  - `crates/atm-daemon/src/runtime_status_cache.rs`
  - `crates/atm-daemon/src/sqlite_observability.rs`
  - `crates/atm-daemon/src/runtime_health_test_support.rs`
  - `crates/atm-daemon/src/tests.rs`
  - `crates/atm-daemon/src/tests_advisory.rs`

- `docs/phase-AA/readiness.md` exists and records the AA sprint line plus
  phase exit criteria.

- `docs/phase-AA/issues.md` exists and is the authoritative Phase AA issues
  inventory for architectural findings that remain open across multiple
  sprints.

## Split Recommendation

Keep this sprint documentation-only. If daemon code edits begin before the
state-machine and doctor-ownership rules are accepted, later deletion sprints
will reintroduce ambiguity.

## Acceptance Criteria

- the daemon role is documented as thin and storage-agnostic
- the subsystem-doctor aggregation rule is explicit
- the daemon state-machine inventory exists and caps top-level machines at five
- the leak ledger exists and classifies every current daemon-side SQLite touch
  point as delete / move / keep-and-rewrite
- the Phase AA readiness record exists
- no implementation work is required to infer the intended daemon role or the
  leak inventory after reading this sprint doc and its listed artifacts

## Required Validation

- `git diff --check`

## Required Document Updates

- `docs/plan-phase-AA.md`
- `docs/project-plan.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`
- `docs/phase-AA/readiness.md`
- `docs/phase-AA/issues.md`
- `docs/phase-AA/daemon-state-machines.md`
- `docs/phase-AA/daemon-sqlite-leak-ledger.md`

## Risks And Watchouts

- if this sprint leaves “doctor may do either thing” ambiguity, later sprints
  will accrete more special cases instead of deleting them
