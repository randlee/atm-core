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

## Non-Goals

- no code movement yet
- no partial boundary rollback without the full inventory

## Sub-Tasks

- Restate the daemon role in requirements and architecture docs as:
  transport, lifecycle, routing, bounded dispatch, and minor error handling
  only.
  Development work: update product and crate-local docs.
  Required tests: none beyond document validation.
  Required doc or boundary updates: requirements, architecture, daemon
  architecture, and the new Phase AA readiness record.

- Record the subsystem-doctor pattern as a hard rule:
  each subsystem diagnoses itself through a trait; top-level doctor code only
  aggregates subsystem results.
  Development work: document the trait/aggregation rule and where those traits
  live.
  Required tests: none yet.
  Required doc or boundary updates: requirements and architecture docs.

- Produce the daemon state-machine inventory with a hard target of five or
  fewer top-level machines.
  Development work: write the inventory and flag every daemon function family
  that violates it.
  Required tests: none yet.
  Required doc or boundary updates: `docs/phase-AA/*` and daemon architecture.

- Freeze the concrete leak ledger up front.
  Development work: record the exact current daemon-side SQLite leak files and
  classify them now as delete / move / keep-and-rewrite so later sprints are
  mechanical. The initial frozen file set is:
  - `crates/atm-daemon/src/lib.rs`
  - `crates/atm-daemon/src/composition.rs`
  - `crates/atm-daemon/src/runtime_health.rs`
  - `crates/atm-daemon/src/runtime_status_cache.rs`
  - `crates/atm-daemon/src/sqlite_observability.rs`
  - `crates/atm-daemon/src/runtime_health_test_support.rs`
  - `crates/atm-daemon/src/tests.rs`
  - `crates/atm-daemon/src/tests_advisory.rs`
  Required tests: none yet.
  Required doc or boundary updates:
  - `docs/phase-AA/daemon-state-machines.md`
  - `docs/phase-AA/daemon-sqlite-leak-ledger.md`

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
- `docs/phase-AA/daemon-state-machines.md`
- `docs/phase-AA/daemon-sqlite-leak-ledger.md`

## Risks And Watchouts

- if this sprint leaves “doctor may do either thing” ambiguity, later sprints
  will accrete more special cases instead of deleting them
