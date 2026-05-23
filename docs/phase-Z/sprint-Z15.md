---
id: Z.15
title: Deferred Hardening Follow-Up Consolidation
status: planned
branch: feature/pZ-s15-deferred-hardening-follow-up-consolidation
worktree: ../atm-core-worktrees/feature/pZ-s15-deferred-hardening-follow-up-consolidation
target: integrate/phase-Z
---

# Sprint Z.15 — Deferred Hardening Follow-Up Consolidation

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.15
worktree: ../atm-core-worktrees/feature/pZ-s15-deferred-hardening-follow-up-consolidation
branch: feature/pZ-s15-deferred-hardening-follow-up-consolidation
status: planned
estimated_scope: medium
```

## Goal

Close the deferred hardening and type-safety findings that do not belong to the
explicit `Z.11` through `Z.14` command-path and boundary-cleanup scopes.

## Scope Summary

This sprint owns the remaining deferred follow-up line after the narrower
`Z.11` through `Z.14` sprints are defined:

- daemon/runtime shutdown and lifecycle hardening that remains outside the
  first-send, retained-runtime, workspace-config, and ambient-singleton scopes
- Rust best-practices and typed-surface follow-up that was intentionally
  deferred out of `Z.6` through `Z.10`
- final cleanup of any deferred `Phase Z` hardening item that does not already
  have a tighter named sprint home

## Governing Requirements

- `REQ-CORE-BOUNDARY-001`
- `REQ-CORE-BOUNDARY-002`
- `REQ-CORE-DAEMON-001`
- `REQ-CORE-TEAM-001`

## Governing ADRs

- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`

## Governing Boundaries

- `RequestDispatcher`
- `RosterStore`
- `ConfigIngress`

## Prerequisites

- `Z.14` complete

## Hard Dependencies

- `docs/phase-Z/readiness.md`
- `docs/phase-Z/claude-roster-sync-and-restore.md`
- `docs/phase-Z/config-json-violation-inventory.md`
- `docs/atm-core/boundaries.md`

## Exact Targets

- `docs/phase-Z/readiness.md`
- `docs/project-plan.md`
- concrete source files named by the deferred findings accepted into `Z.15`

## Delete / Narrow Inventory

- delete any remaining deferred `Phase Z` hardening item that lacks a tighter
  named sprint home after `Z.11` through `Z.14`
- narrow the `Z.15` scope to only the deferred findings assigned here; do not
  re-open already-closed `Z.11` through `Z.14` deliverables

## Non-Goals

- no widening of `Z.15` into new canary or release execution work
- no reopening of `Z.11` through `Z.14` closure rules unless a deferred finding
  was assigned here explicitly

## Sub-Tasks

1. Close the remaining deferred hardening list.
   Development work:
   - resolve every deferred finding assigned to `Z.15` in
     `docs/phase-Z/readiness.md`
   - keep the accepted home list in `readiness.md` the single planning
     source-of-truth for the remaining hardening line
   Required tests:
   - prove every deferred finding row assigned to `Z.15` records a final
     disposition or closure note
   Required docs:
   - update `docs/phase-Z/readiness.md`

2. Stamp closure records.
   Development work:
   - stamp `Z.15` accepted head and verdict in `docs/phase-Z/readiness.md`
   - add the `Z.15` ledger row to `docs/project-plan.md`
   Required tests:
   - `git diff --check`
   Required docs:
   - update `docs/project-plan.md`

## Split Recommendation

If any `Z.15` finding turns into a materially new product capability or a
fresh rollout path instead of a deferred hardening cleanup, stop and open a new
post-`Z.15` sprint instead of widening this closure line.

## Acceptance Criteria

- every deferred finding that does not belong to `Z.11`, `Z.12`, `Z.13`, or
  `Z.14` has `Z.15` as its explicit home in `docs/phase-Z/readiness.md`
- `docs/phase-Z/readiness.md` remains the single deferred-findings
  source-of-truth for the `Z.11` through `Z.15` line
- `docs/project-plan.md` includes the `Z.15` sprint ledger row

## Non-Closure

- `Z.15` does not begin `Z.3` canary execution
- `Z.15` does not replace the narrow closure rules already defined for
  `Z.11` through `Z.14`

## Production-Ready Expectation

No deferred hardening finding should remain homeless or ambiguously assigned
before `Phase Z` can proceed into canary execution.

## Required Validation

- `git diff --check`

## Required Document Updates

- `docs/phase-Z/readiness.md`
- `docs/project-plan.md`

## Risks And Watchouts

- do not use `Z.15` as a catch-all for unrelated new work
- keep the deferred-finding assignments explicit and auditable
