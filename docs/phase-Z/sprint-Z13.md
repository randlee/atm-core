---
id: Z.13
title: Workspace Config Boundary Cleanup And Lint Gate
status: planned
branch: feature/pZ-s13-workspace-config-boundary-cleanup-and-lint-gate
worktree: ../atm-core-worktrees/feature/pZ-s13-workspace-config-boundary-cleanup-and-lint-gate
target: integrate/phase-Z
---

# Sprint Z.13 — Workspace Config Boundary Cleanup And Lint Gate

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.13
worktree: ../atm-core-worktrees/feature/pZ-s13-workspace-config-boundary-cleanup-and-lint-gate
branch: feature/pZ-s13-workspace-config-boundary-cleanup-and-lint-gate
status: planned
estimated_scope: small
```

## Goal

Remove ambient workspace-config access paths so command and team-admin flows do
not reach `.atm.toml` helpers directly when the design requires an approved
boundary seam.

## Scope Summary

This sprint owns the currently-known ambient workspace-config reads that are
inconsistent with the architecture even though they are not Claude
`config.json` roster-truth leaks.

## Governing Requirements

- `REQ-CORE-BOUNDARY-001`
- `REQ-CORE-CONFIG-001`
- `REQ-CORE-TEAM-001`

## Governing ADRs

- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`

## Governing Boundaries

- `ConfigIngress`
- `RequestDispatcher`
- direct `.atm.toml` / `load_config(...)` reads from command/team-admin paths
  outside the approved boundary seam are forbidden and must be rejected by
  repository-local boundary lint

## Prerequisites

- `Z.12` complete

## Hard Dependencies

- `docs/atm-core/boundaries.md`
- `docs/phase-Z/readiness.md`

## Exact Targets

- `crates/atm-core/src/team_admin.rs`
- `crates/atm/src/commands/teams.rs`
- `crates/atm/src/commands/members.rs`
- `crates/atm-core/src/service_runtime.rs`
- `crates/atm-core/src/boundary/store.rs`
- `.just/fixtures/scb_workspace_known_bad.rs`
- `docs/phase-Z/readiness.md`
- `docs/project-plan.md`

## Delete / Narrow Inventory

- delete ambient `load_config(...)` access used for current-team resolution in
  `team_admin::list_teams`
- delete ambient `load_config(...)` access used for current-team resolution in
  `team_admin::list_members`
- narrow any surviving workspace-config resolution behind the approved
  boundary seam only

## Non-Goals

- no Claude `config.json` watcher/reconcile redesign
- no public runtime-factory/singleton cleanup
- no canary/dogfood execution

## Sub-Tasks

1. Remove ambient workspace-config reads.
   Development work:
   - route current-team resolution for `teams` / `members` through the approved
     `ConfigIngress` / runtime seam instead of direct `load_config(...)`
   - keep command and team-admin flows consistent with the architecture
   Required tests:
   - prove `teams` and `members` still resolve current team correctly
   - prove `team_admin.rs`, `teams.rs`, and `members.rs` no longer contain
     direct `load_config(...)` calls
   Required docs:
   - update `docs/phase-Z/readiness.md`

2. Add the mechanical boundary gate.
   Development work:
   - add repository-local lint rule `SCB-WORKSPACE-001`
   - `SCB-WORKSPACE-001` must reject direct `load_config(...)` /
     `.atm.toml` access from command/team-admin paths that should use the
     approved seam
   Required tests:
   - prove the lint fails on a known-bad fixture
   - prove `just lint boundaries` passes on the fixed branch
   Required docs:
   - update `docs/atm-core/boundaries.md`

3. Stamp closure records.
   Development work:
   - stamp `Z.13` accepted head and verdict in `docs/phase-Z/readiness.md`
   - add the `Z.13` ledger row to `docs/project-plan.md`
   Required tests:
   - `git diff --check`
   Required docs:
   - update `docs/project-plan.md`

## Split Recommendation

If the work expands into public ambient singleton/runtime-factory surface
cleanup, stop and move that scope into `Z.14` instead of widening `Z.13`.

## Acceptance Criteria

- `team_admin::list_teams` no longer performs direct ambient workspace-config
  reads
- `team_admin::list_members` no longer performs direct ambient workspace-config
  reads
- `crates/atm-core/src/team_admin.rs`, `crates/atm/src/commands/teams.rs`, and
  `crates/atm/src/commands/members.rs` contain zero direct `load_config(...)`
  matches
- `SCB-WORKSPACE-001` rejects new direct `.atm.toml` / `load_config(...)`
  command-path violations

## Non-Closure

- `Z.13` does not own public runtime-factory/singleton cleanup
- `Z.13` does not reopen Claude `config.json` ingress/export ownership

## Production-Ready Expectation

The command/team-admin current-team resolution path must be boundary-owned and
lint-enforced, not ambient.

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all --check`
- `just lint boundaries`
- `rg -n "load_config\\(" crates/atm-core/src/team_admin.rs crates/atm/src/commands/teams.rs crates/atm/src/commands/members.rs`
  - expected: zero matches
- `git diff --check`

## Required Document Updates

- `docs/atm-core/boundaries.md`
- `docs/phase-Z/readiness.md`
- `docs/project-plan.md`

## Risks And Watchouts

- do not accidentally reintroduce generic retained-command config lookup
- keep `SCB-WORKSPACE-001` focused on ambient command/team-admin access
