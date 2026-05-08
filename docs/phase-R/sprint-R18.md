# Phase R.18 — Production Hardening And Closeout

```yaml
plan_type: sprint_plan
phase: R
sprint: "R.18"
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pR-s18-runtime-ops
branch: feature/pR-s18-runtime-ops
status: accepted
estimated_scope: L
```

## Acceptance Record

- `status: accepted`
- `accepted_on: 2026-05-07`
- `completion_note: bounded SIGHUP reload now preserves the last-known-good runtime view, graceful shutdown performs singleton-held background-lane shutdown plus SQLite WAL checkpoint / observability flush finalization, and the final daemon runtime closeout validations pass on this branch apart from the known host-side Windows sqlite cross-toolchain limitation.`

## Goal

Finish the remaining production-hardening, type-safety, portability, and final document/boundary reconciliation work required to call Phase R complete.

## Scope Summary

This sprint is the explicit closeout gate after the major runtime lanes land. It absorbs the remaining importants/minors and any boundary/doc hardening that should not be spread ad hoc across earlier implementation sprints.

## Governing Requirements

- `REQ-P-RUNTIME-002`
  - defined in [docs/requirements.md](../requirements.md) under Product
    requirement `REQ-P-RUNTIME-002`
- `REQ-CORE-BOUNDARY-002`
- `REQ-CORE-TEST-RUNTIME-001`
- portability/test-fidelity requirements in `docs/requirements.md`

## Governing ADRs

- `docs/adr/ADR-002-host-wide-daemon-singleton.md`
- `docs/adr/ADR-003-test-fidelity-and-daemon-isolation.md`
- `docs/adr/ADR-005-host-scoped-sqlite-state-root.md`

## Governing Boundaries

- `BOUNDARY-AtmProtocol`
- `BOUNDARY-ClientTransport`
- `BOUNDARY-MailStore-Sqlite`
- `BOUNDARY-TaskStore-Sqlite`
- `BOUNDARY-RosterStore-Sqlite`

## Prerequisites

- `R.13` through `R.17` are complete

## Hard Dependencies

- this is the phase closeout gate; do not merge it as a partial hardening pass

## Non-Goals

- new major runtime subsystems
- new product-surface features outside the already-documented Phase R continuation scope

## Sub-Tasks

1. Runtime hardening residuals
   Development work:
   - implement validated serving-config reload on `SIGHUP`
   - add default daemon logging, request IDs, and remaining runtime-facing observability polish
   - eliminate remaining placeholder runtime-boundary delegation that should now be runtime-owned
   Required tests:
   - config reload success/failure tests preserving last-known-good serving config
   - request-id propagation tests across client/daemon logging
   Required doc or boundary updates:
   - update daemon architecture and health/observability docs to the final landed design

2. Type-safety and store residuals
   Development work:
   - replace raw-string address/home APIs with newtypes where planned
   - either wire or delete orphaned typestate markers
   - finish any remaining typed SQLite/store error mapping not already absorbed by `R.14`
   Required tests:
   - construction/validation tests for the newtypes
   - compile-level or unit tests proving orphaned markers are gone or wired
   Required doc or boundary updates:
   - update atm-core boundary/architecture docs when public/store DTOs or helper APIs change

3. Test portability and flake cleanup
   Development work:
   - remove remaining `std::env::set_var()` test violations
   - eliminate wall-clock and child-reap races in retained tests
   - close any remaining daemon/test harness portability issues
   Required tests:
   - targeted retained-test fixes
   - full workspace validation on the final branch
   Required doc or boundary updates:
   - update testing or portability notes only if the final enforcement model changes

4. Final doc and boundary reconciliation
   Development work:
   - verify every important runtime struct/facade that remains architecturally meaningful is reflected in the boundary inventories or explicitly documented as crate-private implementation detail
   - update requirements, architecture, project plan, and sprint docs to the final landed state
   Required tests:
   - lint/doc-consistency pass only; no behavior-specific test beyond the final workspace run
   Required doc or boundary updates:
   - all Phase R planning docs touched in this continuation line

## Split Recommendation

Only split if earlier sprints finish with a small residual list that can be carved cleanly into:
- `R.18.1` runtime/config/observability hardening
- `R.18.2` type-safety + portability + doc closeout

## Acceptance Criteria

- config reload preserves last-known-good serving config and returns typed failure on invalid reload input
- daemon logs have a non-silent default and request IDs are traceable across the client/daemon path
- raw-string boundary APIs and dead typestate markers are either replaced or removed as planned
- remaining `std::env::set_var()` test violations and known flake points are closed
- requirements, architecture, project plan, and boundary inventories all match the final landed Phase R implementation without known unresolved contradictions

## Required Validation

- `cargo test --workspace`
- `cargo check --workspace --target x86_64-pc-windows-msvc`
- `just lint`

## Required Document Updates

- `docs/requirements.md`
- `docs/architecture.md`
- `docs/project-plan.md`
- `docs/atm-core/architecture.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/boundaries.md`
- `docs/atm-rusqlite/boundaries.md`
- `docs/phase-R/issues.md`

## Risks And Watchouts

- do not let the final sprint become a grab bag; every residual item should trace back to an issue, requirement, or boundary gap
- boundary inventories should capture meaningful facades and traits, not every helper type
- the phase is not done if the docs still describe placeholders as finished subsystems
