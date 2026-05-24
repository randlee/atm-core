---
id: Z.19
title: Complete Smoke Checklist Automation And Reporting
status: planned
branch: feature/pZ-s19-complete-smoke-checklist-automation-and-reporting
worktree: ../atm-core-worktrees/feature/pZ-s19-complete-smoke-checklist-automation-and-reporting
target: integrate/phase-Z
---

# Sprint Z.19 — Complete Smoke Checklist Automation And Reporting

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.19
worktree: ../atm-core-worktrees/feature/pZ-s19-complete-smoke-checklist-automation-and-reporting
branch: feature/pZ-s19-complete-smoke-checklist-automation-and-reporting
status: planned
estimated_scope: medium
```

## Goal

- automate the full frozen smoke checklist
- add `just smoke-complete`
- make row-by-row smoke results explicit and machine-readable

## Scope Summary

This sprint closes the gap between a useful default smoke runner and the full
row-by-row evidence line that `Phase Z` needs for release gating.

## Governing Requirements

- `REQ-CORE-ATM-JSON-001`
- `REQ-CORE-CLI-001`

## Governing ADRs

- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`

## Governing Boundaries

- `RosterStore`
- `RuntimeFactory`

## Prerequisites

- `Z.18` complete

## Hard Dependencies

- `docs/phase-Z/smoke-skill-plan.md`
- `docs/phase-Z/smoke-checklist.md`
- `docs/phase-Z/smoke-findings-ledger.md`

## Exact Targets

- `.claude/skills/smoke-test/SKILL.md`
- `.claude/skills/smoke-test/references/phase-z-row-map.md`
- `scripts/smoke/run.py`
- `scripts/smoke/fixtures.py`
- `Justfile`

## Deliverables

Every listed deliverable is expected to land at a production-ready level for
the scope this sprint claims. If that cannot be done cleanly in one sprint, the
sprint must be split before implementation begins. No deliverable may be
silently dropped or partially deferred.

- `just smoke-complete`
- row-by-row mapping to every smoke-checklist ID
- complete-level copied-state fixture coverage
- explicit PASS / FAIL / SKIP row output

## Required Work

- add the `complete` level entrypoint
- map every frozen smoke-checklist row to a report row
- carry copied-state lane setup where required by the checklist
- keep skipped/manual-only situations explicit in the report output

## Explicit Code Samples

If the sprint introduces or changes important traits, features, enums, protocol
types, boundary contracts, or execution seams, this section must include
explicit code samples or signatures showing the intended end state.

```text
just smoke-complete
```

```json
{
  "id": "Z1-008",
  "flow": "Bring up the current real-state durable baseline without touching live host data",
  "verdict": "PASS",
  "notes": "copied-state daemon-backed lane succeeded"
}
```

## This Sprint Does Not Close

- canary/dogfood checklist integration
- final binary-baseline readiness wiring

## Acceptance Criteria

- `just smoke-complete` exists
- every row in `docs/phase-Z/smoke-checklist.md` maps to one report row
- copied-state fixture coverage is part of the complete-level plan where the
  checklist requires it
- failures and skips are explicit in both JSON and stdout summary

## Required Validation

- `cargo test --workspace`
- `python3 .just/run_lint.py all`
- `git diff --check`

## Split Recommendation

If canary/report-readiness wiring starts changing `docs/phase-Z/readiness.md`
semantics beyond smoke-row output, stop and push that into `Z.20`.

## Production-Ready Expectation

Every listed `Z.19` deliverable is expected to land at a production-ready
level for complete smoke execution and row-level reporting.

## Required Document Updates

- `docs/phase-Z/smoke-skill-plan.md`
- `docs/phase-Z/readiness.md`
- `docs/project-plan.md`

## Risks And Watchouts

- do not silently collapse multiple checklist rows into one report row
- keep the copied-state lane disposable and non-destructive
