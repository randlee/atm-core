---
id: Z.20
title: Normal Smoke Systemic Execution
status: complete
branch: feature/pZ-s20-normal-smoke-systemic-execution
worktree: ../atm-core-worktrees/feature/pZ-s20-normal-smoke-systemic-execution
target: integrate/phase-Z
---

# Sprint Z.20 — Normal Smoke Systemic Execution

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.20
worktree: ../atm-core-worktrees/feature/pZ-s20-normal-smoke-systemic-execution
branch: feature/pZ-s20-normal-smoke-systemic-execution
status: complete
estimated_scope: medium
```

## Goal

- implement the default `just smoke` run
- exercise most important feature/system behavior beyond the fast happy path
- root-cause every deviation from expected behavior
- keep normal operational logging quiet while still making smoke diagnostics
  sufficient

## Scope Summary

This sprint owns the default operator smoke lane. It includes everything in
the fast lane, then broadens the run across the most important retained,
admin, and operator surfaces. It must produce root-cause notes for every
deviation and may fix minor local blockers directly when doing so keeps the
lane predictable.

## Governing Requirements

- `REQ-P-SMOKE-001`
- `REQ-P-SMOKE-002`
- `REQ-P-SMOKE-003`

## Governing ADRs

- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`

## Governing Boundaries

- `RosterStore`
- `RuntimeFactory`

## Prerequisites

- `Z.19` complete

## Hard Dependencies

- `docs/phase-Z/smoke-skill-plan.md`
- `docs/phase-Z/smoke-checklist.md`
- `docs/phase-Z/readiness.md`

## Exact Targets

- `.claude/skills/smoke-test/SKILL.md`
- `scripts/smoke/run.py`
- `scripts/smoke/analyze_logs.py`
- `scripts/smoke/render_report.py`
- `templates/smoke-report/smoke.md.j2`
- `Justfile`
- `reports/smoke/smoke.md`

## Deliverables

Every listed deliverable is expected to land at a production-ready level for
the scope this sprint claims. If that cannot be done cleanly in one sprint,
the sprint must be split before implementation begins. No deliverable may be
silently dropped or partially deferred.

- default `just smoke` execution
- latest snapshot report at `reports/smoke/smoke.md`
- timestamped normal smoke markdown and JSON artifacts
- root-cause notes for every non-pass row
- deterministic normal-lane retained/admin/operator coverage
- minor in-sprint fixes required to make the normal lane predictable

## Required Work

- include everything from the fast lane
- extend coverage across broader retained/admin/operator behavior
- include team/member inspection and mailbox/log surfaces that matter in
  routine use
- include important validation/error-path checks
- keep normal production logging quiet at routine verbosity and avoid logging
  every send/read/ack event at ordinary operator levels
- use explicit smoke/debug mode for deeper evidence when the lane deviates
- add missing log messages at the appropriate level when that is the only
  local blocker
- fix minor localized requirement or architecture violations when they are the
  only local blocker to predictable normal execution
- promote larger rework findings into `docs/phase-Z/smoke-findings-review.md`

## Explicit Code Samples

```text
just smoke
```

## This Sprint Does Not Close

- the full `thorough` CLI surface
- every copied-state or degraded/recovery lane from the frozen checklist
- major rework findings discovered during the normal lane

## Acceptance Criteria

- `just smoke` exists and defaults to the normal level
- the normal run explicitly reports `Z1-001`, `Z1-002`, `Z1-003`, `Z1-004`,
  `Z1-005`, and `Z1-007`
- the normal run retains the fast-lane log-analysis gate:
  `FAST-LOG-001` and `FAST-LOG-002` must still pass at the normal level
- the normal lane includes everything required by the fast lane
- the normal lane covers the broader retained/admin/operator surfaces claimed
  in the sprint
- every deviation includes observed behavior, expected behavior, likely root
  cause, and artifact pointer
- the normal report is rendered to the tracked-latest and timestamped
  artifacts
- any remaining large issue is captured in
  `docs/phase-Z/smoke-findings-review.md`

## Required Validation

- `cargo test --workspace`
- `python3 .just/run_lint.py all`
- `git diff --check`

## Split Recommendation

If the sprint starts absorbing full CLI common-error-path coverage or every
copied-state/degraded lane from the thorough contract, stop and keep that work
in `Z.21`.

## Production-Ready Expectation

Every listed `Z.20` deliverable is expected to land at a production-ready
level for deterministic normal smoke execution.

## Required Document Updates

- `docs/phase-Z/smoke-skill-plan.md`
- `docs/phase-Z/readiness.md`
- `docs/plan-phase-Z.md`
- `docs/project-plan.md`
- `docs/phase-Z/smoke-findings-review.md`, when needed

## Risks And Watchouts

- do not let the default `just smoke` become too thin to be useful
- do not clutter routine production logs to compensate for missing smoke-mode
  diagnostics
- do not leave deviations at "FAIL" without a root-cause hypothesis
