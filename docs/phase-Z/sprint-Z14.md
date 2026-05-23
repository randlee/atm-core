---
id: Z.14
title: Ambient Singleton Surface Removal And Lint Gate
status: planned
branch: feature/pZ-s14-ambient-singleton-surface-removal-and-lint-gate
worktree: ../atm-core-worktrees/feature/pZ-s14-ambient-singleton-surface-removal-and-lint-gate
target: integrate/phase-Z
---

# Sprint Z.14 — Ambient Singleton Surface Removal And Lint Gate

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.14
worktree: ../atm-core-worktrees/feature/pZ-s14-ambient-singleton-surface-removal-and-lint-gate
branch: feature/pZ-s14-ambient-singleton-surface-removal-and-lint-gate
status: planned
estimated_scope: small
```

## Goal

Remove public ambient runtime-factory / singleton exposure and make that class
of leak mechanically impossible to reintroduce.

## Scope Summary

This sprint owns the broad root-surface leak:

- `atm_core::install_default_runtime_factory`

and any equivalent new ambient singleton installation surface that bypasses
approved wrappers.

Approved surviving wrappers for this sprint are limited to:

- `atm_daemon_bootstrap::install_sqlite_retained_runtime_factory()`
- `atm_runtime_test_support::install_sqlite_retained_runtime_factory()`

## Governing Requirements

- `REQ-CORE-BOUNDARY-001`
- `REQ-CORE-TEAM-001`

## Governing ADRs

- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`

## Governing Boundaries

- `RequestDispatcher`
- public crate-root singleton/runtime-factory installation surfaces are
  forbidden unless explicitly approved and wrapped for one bounded use-case

## Prerequisites

- `Z.13` complete

## Hard Dependencies

- `docs/atm-core/boundaries.md`
- `docs/phase-Z/readiness.md`

## Exact Targets

- `crates/atm-core/src/lib.rs`
- `crates/atm-core/src/service_runtime_store.rs`
- `crates/atm-daemon-bootstrap/src/lib.rs`
- `crates/atm-runtime-test-support/src/lib.rs`
- `crates/atm-daemon/src/tests.rs`
- `crates/atm-rusqlite/src/lib.rs`
- `.just/fixtures/scb_singleton_known_bad.rs`
- `docs/phase-Z/readiness.md`
- `docs/project-plan.md`

## Delete / Narrow Inventory

- delete the public crate-root `install_default_runtime_factory` exposure
- narrow all surviving callers to approved wrappers only
- forbid new public ambient runtime-factory installation surfaces

## Boundary Contract Sample

```rust
// Forbidden ambient root surface:
atm_core::install_default_runtime_factory(...);

// Approved bounded wrappers:
atm_daemon_bootstrap::install_sqlite_retained_runtime_factory();
atm_runtime_test_support::install_sqlite_retained_runtime_factory();
```

## Non-Goals

- no general daemon/test bootstrap redesign beyond the approved wrapper split
- no canary/dogfood execution

## Sub-Tasks

1. Remove the public ambient singleton surface.
   Development work:
   - remove `atm_core::install_default_runtime_factory` from the broad public
     crate-root surface
   - move all surviving callers to approved wrappers only
   Required tests:
   - prove downstream callers no longer use the crate-root install surface
   Required docs:
   - update `docs/phase-Z/readiness.md`

2. Add the mechanical boundary gate.
   Development work:
   - add repository-local lint rule `SCB-SINGLETON-001`
   - `SCB-SINGLETON-001` must reject public ambient runtime-factory /
     singleton installation surfaces that bypass approved wrappers
   Required tests:
   - prove the lint fails on a known-bad fixture
   - prove `just lint boundaries` passes on the fixed branch
   Required docs:
   - update `docs/atm-core/boundaries.md`

3. Stamp closure records.
   Development work:
   - stamp `Z.14` accepted head and verdict in `docs/phase-Z/readiness.md`
   - add the `Z.14` ledger row to `docs/project-plan.md`
   Required tests:
   - `git diff --check`
   Required docs:
   - update `docs/project-plan.md`

## Split Recommendation

If the work requires a broader daemon/test bootstrap redesign rather than a
bounded wrapper split and public-surface deletion, stop and open a new sprint
instead of widening `Z.14`.

## Acceptance Criteria

- the public crate-root `install_default_runtime_factory` leak is gone
- production and test callers install retained runtime through approved
  wrappers only
- `SCB-SINGLETON-001` rejects new ambient singleton/runtime-factory exposure

## Non-Closure

- `Z.14` does not own first-send setup behavior
- `Z.14` does not begin canary/dogfood execution

## Production-Ready Expectation

No new ambient singleton/runtime-factory surface should be able to leak back
onto the public crate-root API without tripping lint.

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all --check`
- `just lint boundaries`
- `git diff --check`
- `rg -n 'atm_core::install_default_runtime_factory' crates/`
  - expected: zero matches

## Required Document Updates

- `docs/atm-core/boundaries.md`
- `docs/phase-Z/readiness.md`
- `docs/project-plan.md`

## Risks And Watchouts

- do not replace one ambient public surface with another broad hidden export
- approved wrappers must stay explicit and purpose-bounded
