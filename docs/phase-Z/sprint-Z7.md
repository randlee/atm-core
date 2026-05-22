---
id: Z.7
title: Config Ingress Boundary Narrowing And Static Gates
status: planned
branch: feature/pZ-s7-config-ingress-boundary-narrowing-and-static-gates
worktree: ../atm-core-worktrees/feature/pZ-s7-config-ingress-boundary-narrowing-and-static-gates
target: integrate/phase-Z
---

# Sprint Z.7 — Config Ingress Boundary Narrowing And Static Gates

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.7
worktree: ../atm-core-worktrees/feature/pZ-s7-config-ingress-boundary-narrowing-and-static-gates
branch: feature/pZ-s7-config-ingress-boundary-narrowing-and-static-gates
status: planned
estimated_scope: medium
```

## Goal

Narrow `ConfigIngress` so it cannot remain a generic runtime roster lookup
boundary, and define the repository-local lint / `sc-lint`-candidate gates that
mechanically guard the approved caller set.

## Scope Summary

This sprint owns the boundary/helper contract cleanup and static gate
definition. It does not yet implement watcher/reconcile ingest behavior.

## Governing Requirements

- `REQ-CORE-CLAUDE-ROSTER-001`
- `REQ-CORE-CLAUDE-ROSTER-002`
- `REQ-P-LINT-POSTMORTEM-001`

## Governing ADRs

- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`

## Governing Boundaries

- `ConfigIngress`
- `DaemonConfigIngressAdapter`

## Prerequisites

- `Z.6` complete

## Hard Dependencies

- `docs/phase-Z/config-json-violation-inventory.md`
- `docs/phase-Z/readiness.md`

## Exact Targets

- `crates/atm-core/src/boundary/store.rs`
- `crates/atm-core/src/boundary_support.rs`
- `crates/atm-core/src/direct_boundaries.rs`
- `crates/atm-daemon/src/boundary_adapters.rs`
- `crates/atm-daemon/src/direct_boundaries.rs`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/boundaries.md`
- `docs/phase-Z/config-json-violation-inventory.md`
- `docs/phase-Z/readiness.md`

## Delete / Narrow Inventory

- delete generic retained-command/runtime `load_team_config(...)` boundary
  behavior from:
  - `crates/atm-core/src/boundary_support.rs`
  - `crates/atm-core/src/direct_boundaries.rs`
  - `crates/atm-daemon/src/boundary_adapters.rs`
  - `crates/atm-daemon/src/direct_boundaries.rs`
- narrow `ConfigIngress` in `crates/atm-core/src/boundary/store.rs` to the
  approved caller set
- define `SCB-CONFIG-001`, `SCB-CONFIG-002`, and `SCB-CONFIG-003`

## Non-Goals

- no watcher/reconcile roster import logic yet
- no team-admin cutover
- no restore automation

## Sub-Tasks

1. Narrow the `ConfigIngress` contract.
   Development work:
   - remove generic roster-lookup semantics from:
     - `crates/atm-core/src/boundary/store.rs`
     - `crates/atm-core/src/boundary_support.rs`
     - `crates/atm-core/src/direct_boundaries.rs`
     - `crates/atm-daemon/src/boundary_adapters.rs`
     - `crates/atm-daemon/src/direct_boundaries.rs`
   - leave only the approved ingress/comparison/preservation behavior
   Required tests:
   - prove generic retained command/runtime lookup no longer compiles or no
     longer exists on the boundary surface
   Required docs:
   - update `docs/atm-core/boundaries.md`
   - update `docs/atm-daemon/boundaries.md`

2. Define static gates for future regressions.
   Development work:
   - define repository-local lint / `sc-lint`-candidate rules:
     - `SCB-CONFIG-001`
     - `SCB-CONFIG-002`
     - `SCB-CONFIG-003`
   - define the explicit allowlist from
     `docs/phase-Z/config-json-violation-inventory.md`
   Required tests:
   - add a machine-runnable boundary-lint fixture that contains one known-bad
     direct `config.json` roster lookup and proves the rule family rejects it
   - wire that fixture into `just lint boundaries` so the rule family produces
     a verifiable reject signal rather than prose-only documentation
   Required docs:
   - update `docs/requirements.md`
   - update `docs/architecture.md`

3. Sync the planning records.
   Development work:
   - stamp `Z.7` accepted head and verdict in `docs/phase-Z/readiness.md`
   Required tests:
   - `git diff --check`
   Required docs:
   - update `docs/phase-Z/claude-roster-sync-and-restore.md`

## Split Recommendation

If the work starts implementing watcher/reconcile import behavior, daemon-write
suppression, or external roster change ingestion, stop and move that scope into
`Z.8`.

## Acceptance Criteria

- `ConfigIngress` is no longer documented or shaped as a generic runtime roster
  lookup boundary
- the helper/adapter chain no longer exposes generic retained command/runtime
  `load_team_config(...)` behavior
- repo-local lint / `sc-lint`-candidate rule definitions exist for the
  `config.json` boundary violation family and produce a verifiable reject
  signal on a known-bad fixture

## Required Validation

- `cargo test --workspace`
- `git diff --check`
- `just lint boundaries`
  - expected: the `SCB-CONFIG-001` / `002` / `003` fixture path is exercised and
    known-bad direct `config.json` boundary violations are rejected
- `rg -n "load_team_config\\(" crates/atm-core/src/boundary_support.rs crates/atm-core/src/direct_boundaries.rs crates/atm-daemon/src/boundary_adapters.rs crates/atm-daemon/src/direct_boundaries.rs`
  - expected: surviving matches are explicitly ingress/comparison/preservation
    only
- `docs/phase-Z/readiness.md`

## Required Document Updates

- `docs/phase-Z/claude-roster-sync-and-restore.md`
- `docs/phase-Z/config-json-violation-inventory.md`
- `docs/phase-Z/readiness.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/boundaries.md`

## Risks And Watchouts

- this sprint is not allowed to hide the boundary leak behind new naming while
  keeping the same generic behavior
- the allowlist must stay explicit and narrow
- if the static gates cannot be stated crisply here, later implementation will
  drift back toward ad hoc exceptions
