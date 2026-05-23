---
id: Z.12
title: Retained Runtime Path Elimination And Boundary Lint Gate
status: planned
branch: feature/pZ-s12-retained-runtime-path-elimination-and-boundary-lint-gate
worktree: ../atm-core-worktrees/feature/pZ-s12-retained-runtime-path-elimination-and-boundary-lint-gate
target: integrate/phase-Z
---

# Sprint Z.12 — Retained Runtime Path Elimination And Boundary Lint Gate

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.12
worktree: ../atm-core-worktrees/feature/pZ-s12-retained-runtime-path-elimination-and-boundary-lint-gate
branch: feature/pZ-s12-retained-runtime-path-elimination-and-boundary-lint-gate
status: planned
estimated_scope: small
```

## Goal

Eliminate the incorrect retained-runtime acquisition path behind `Z2-F001` and
make that misuse mechanically impossible to reintroduce without tripping
repository-local boundary lint.

## Scope Summary

This sprint owns the `atm teams`, `atm members`, and `atm teams add-member`
runtime-install-path bug and the lint gate that prevents direct
`service_runtime_store::default_runtime()` access from command-entry paths.

## Governing Requirements

- `REQ-CORE-BOUNDARY-001`
- `REQ-CORE-TEAM-001`

## Governing ADRs

- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`

## Governing Boundaries

- `RosterStore`
- `RequestDispatcher`
- direct `service_runtime_store::default_runtime()` use from
  command-entry and team-admin paths is forbidden and must be rejected by
  repository-local boundary lint

## Prerequisites

- `Z.11` complete

## Hard Dependencies

- `docs/phase-Z/readiness.md`
- `docs/atm-core/boundaries.md`

## Exact Targets

- `crates/atm/src/commands/teams.rs`
- `crates/atm/src/commands/members.rs`
- `crates/atm-core/src/team_admin.rs`
- `.just/fixtures/scb_runtime_known_bad.rs`
- `docs/phase-Z/smoke-findings-ledger.md`
- `docs/phase-Z/readiness.md`
- `docs/project-plan.md`

## Delete / Narrow Inventory

- delete the command-entry path that reaches
  `service_runtime_store::default_runtime()` before the approved runtime
  install/composition path runs
- narrow CLI team/member command entrypoints so they can only obtain retained
  runtime access through the approved `RosterStore` trait seam
- narrow `atm teams add-member` so it no longer fails on the same ambient
  retained-runtime acquisition path

## Non-Goals

- no workspace `.atm.toml` boundary cleanup
- no public runtime-factory/singleton cleanup
- no first-send roster/bootstrap redesign

## Sub-Tasks

1. Eliminate the incorrect retained-runtime path.
   Development work:
   - route `atm teams`, `atm members`, and `atm teams add-member` through the
     approved `RosterStore` trait seam already used by the
     `_with_roster_store(...)` helpers
   - remove the command-entry misuse that leaves the default runtime factory
     uninstalled
   Required tests:
   - prove `atm teams --json` succeeds on the clean-room baseline
   - prove `atm members --json` succeeds on the clean-room baseline
   - prove `atm teams add-member z1-team z1-operator --json` succeeds once a
     valid clean-room `z1-team` shell exists
   Required docs:
   - update `docs/phase-Z/readiness.md`
   - update `docs/phase-Z/smoke-findings-ledger.md`

2. Add the mechanical boundary gate.
   Development work:
   - add repository-local lint rule `SCB-RETAINED-001`
   - `SCB-RETAINED-001` must reject direct
     `service_runtime_store::default_runtime()` use from command-entry and
     team-admin paths
   Required tests:
   - prove the lint fails on a known-bad command-entry or team-admin call-site
     fixture
   - prove `just lint boundaries` passes on the fixed branch
   Required docs:
   - update `docs/atm-core/boundaries.md`

3. Stamp closure records.
   Development work:
   - stamp `Z.12` accepted head and verdict in `docs/phase-Z/readiness.md`
   - add the `Z.12` ledger row to `docs/project-plan.md`
   Required tests:
   - `git diff --check`
   Required docs:
   - update `docs/project-plan.md`

## Split Recommendation

If the work expands into ambient workspace-config reads or public
singleton/runtime-factory surface cleanup, stop and move that scope into
`Z.13` or `Z.14` instead of widening `Z.12`.

## Acceptance Criteria

- `atm teams --json` no longer fails with the default-runtime-factory install
  error
- `atm members --json` no longer fails with that same retained-runtime install
  error
- `atm teams add-member` no longer fails with that same retained-runtime
  install error once the target team shell exists
- direct `service_runtime_store::default_runtime()` use from command-entry
  and team-admin paths is rejected by `SCB-RETAINED-001`
- `atm teams`, `atm members`, and `atm teams add-member` obtain roster truth
  only through the approved `RosterStore` trait seam

## Non-Closure

- `Z.12` does not own ambient workspace config reads
- `Z.12` does not own public runtime-factory/singleton cleanup
- `Z.12` does not begin canary/dogfood execution

## Production-Ready Expectation

Every listed `Z.12` deliverable is expected to land at a production-ready
level for this exact command-wiring and boundary-enforcement scope.

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all --check`
- `just lint boundaries`
- `git diff --check`

## Required Document Updates

- `docs/phase-Z/smoke-findings-ledger.md`
- `docs/atm-core/boundaries.md`
- `docs/phase-Z/readiness.md`
- `docs/project-plan.md`

## Risks And Watchouts

- do not let this sprint absorb workspace config resolution cleanup
- keep `SCB-RETAINED-001` narrow enough to target forbidden command-entry misuse
