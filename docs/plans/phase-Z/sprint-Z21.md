---
id: Z.21
title: Thorough Smoke CLI Coverage And Reporting
status: complete
branch: feature/pZ-s21-thorough-smoke-cli-coverage-and-reporting
worktree: ../atm-core-worktrees/feature/pZ-s21-thorough-smoke-cli-coverage-and-reporting
target: integrate/phase-Z
---

# Sprint Z.21 — Thorough Smoke CLI Coverage And Reporting

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.21
worktree: ../atm-core-worktrees/feature/pZ-s21-thorough-smoke-cli-coverage-and-reporting
branch: feature/pZ-s21-thorough-smoke-cli-coverage-and-reporting
status: complete
estimated_scope: medium
```

## Goal

- implement `just smoke thorough`
- cover every CLI interface on happy path plus common error paths
- cover the same-host `atm-graft` advisory and unary ICD path
- produce row-by-row evidence for the frozen smoke checklist
- root-cause every discrepancy from expected behavior

## Scope Summary

This sprint owns the broad CLI and checklist lane. It includes everything from
the normal lane, then extends coverage to every CLI interface on happy path
plus common error paths, using copied-state fixtures where the checklist
requires them. Small local blockers may be fixed in-sprint; larger rework must
be promoted into the findings review artifact.

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

- `Z.20` complete

## Hard Dependencies

- `docs/plans/phase-Z/smoke-skill-plan.md`
- `docs/plans/phase-Z/smoke-checklist.md`

## Exact Targets

- `.claude/skills/smoke-test/SKILL.md`
- `.claude/skills/smoke-test/references/phase-z-row-map.md`
- `scripts/smoke/run.py`
- `scripts/smoke/fixtures.py`
- `scripts/smoke/analyze_logs.py`
- `scripts/smoke/render_report.py`
- `templates/smoke-report/smoke-thorough.md.j2`
- `Justfile`
- `reports/smoke/smoke-thorough.md`

## Deliverables

Every listed deliverable is expected to land at a production-ready level for
the scope this sprint claims. If that cannot be done cleanly in one sprint,
the sprint must be split before implementation begins. No deliverable may be
silently dropped or partially deferred.

- `just smoke thorough`
- row-by-row mapping to every frozen smoke-checklist ID
- thorough-level copied-state fixture coverage where required by the checklist
- happy-path plus common error-path coverage for every CLI interface
- one real same-host `atm-graft` advisory plus unary ICD lane
- explicit PASS / FAIL / SKIP row output
- root-cause notes for every deviation
- rendered `reports/smoke/smoke-thorough.md` plus timestamped artifacts
- minor in-sprint fixes required to make the thorough lane predictable

## Required Work

- include everything from the normal lane
- add the `thorough` level entrypoint
- map every frozen smoke-checklist row to one report row
- cover every public CLI interface on happy path plus common error paths:
  - `atm send`
  - `atm read`
  - `atm ack`
  - `atm list`
  - `atm clear`
  - `atm log`
  - `atm doctor`
  - `atm teams`
  - `atm members`
  - `atm help`
- add one real same-host `atm-graft` host lane that:
  - activates a session against the same daemon used by the CLI lane
  - proves advisory registration and nudge delivery
  - proves unary `read`, `ack`, and `send` over the shared daemon contract
- carry copied-state lane setup where required by the checklist
- keep skipped/manual-only situations explicit in the report output
- add missing log messages at the appropriate level when that is the only
  local blocker
- fix minor localized requirement or architecture violations when they are the
  only local blocker to predictable thorough execution
- promote larger rework findings into `docs/plans/phase-Z/smoke-findings-review.md`

## Explicit Code Samples

```text
just smoke thorough
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

- canary/dogfood execution
- final release sign-off
- major rework findings discovered during the thorough lane

## Acceptance Criteria

- `just smoke thorough` exists
- the thorough run explicitly reports `Z1-001`, `Z1-002`, `Z1-003`, `Z1-004`,
  `Z1-005`, `Z1-006`, `Z1-007`, `Z1-008`, and `Z1-009`
- the thorough run also reports `GRAFT-001` for the same-host `atm-graft`
  advisory plus unary ICD lane
- the thorough run retains the fast-lane log-analysis gate:
  `FAST-LOG-001` and `FAST-LOG-002` must still pass at the thorough level
- every row in `docs/plans/phase-Z/smoke-checklist.md` maps to one report row
- copied-state fixture coverage is part of the thorough lane where the
  checklist requires it
- every CLI interface listed in Required Work is covered on happy path plus
  common error paths: `atm send`, `atm read`, `atm ack`, `atm list`,
  `atm clear`, `atm log`, `atm doctor`, `atm teams`, `atm members`, and
  `atm help`
- one real same-host `atm-graft` host proves advisory registration, nudge
  delivery, unary `read`, unary `ack`, and unary `send` on the same daemon
  contract used by the retained CLI lane
- failures and skips are explicit in both JSON and stdout summary
- every deviation includes observed behavior, expected behavior, likely root
  cause, and artifact pointer
- any remaining large issue is captured in
  `docs/plans/phase-Z/smoke-findings-review.md`

## Required Validation

- `cargo test --workspace`
- `python3 .just/run_lint.py all`
- `git diff --check`

## Split Recommendation

If canary-entry or release-sign-off semantics start changing in this sprint,
stop and keep that work out of the smoke execution line.

## Production-Ready Expectation

Every listed `Z.21` deliverable is expected to land at a production-ready
level for deterministic thorough smoke execution and row-level reporting.

## Required Document Updates

- `docs/plans/phase-Z/smoke-skill-plan.md`
- `docs/plans/phase-Z/readiness.md`
- `docs/plans/phase-Z/plan-phase-Z.md`
- `docs/project-plan.md`
- `docs/plans/phase-Z/smoke-findings-review.md`, when needed

## Risks And Watchouts

- do not silently collapse multiple checklist rows into one report row
- keep the copied-state lane disposable and non-destructive
- keep the common-error-path inventory explicit so the sprint does not drift
  into vague pseudo-exhaustive coverage
