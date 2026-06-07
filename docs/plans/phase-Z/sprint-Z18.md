---
id: Z.18
title: Smoke Skill Scaffold And Report Infrastructure
status: complete
branch: feature/pZ-s18-smoke-skill-and-report-infrastructure
worktree: ../atm-core-worktrees/feature/pZ-s18-smoke-skill-and-report-infrastructure
target: integrate/phase-Z
---

# Sprint Z.18 — Smoke Skill Scaffold And Report Infrastructure

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.18
worktree: ../atm-core-worktrees/feature/pZ-s18-smoke-skill-and-report-infrastructure
branch: feature/pZ-s18-smoke-skill-and-report-infrastructure
status: complete
estimated_scope: medium
```

## Goal

- create the smoke-test skill scaffold
- land the smoke report contract and `reports/smoke/` output layout
- land the shared smoke runner plumbing and template/rendering contract

## Scope Summary

This sprint creates the smoke infrastructure. It does not claim that `fast`,
`normal`, or `thorough` are fully implemented yet. It closes only when the
templates, artifact layout, shared runner, and ignore rules are
production-ready enough for the execution sprints to build on without
redefining the contract.

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

- `Z.17` complete

## Hard Dependencies

- `docs/plans/phase-Z/smoke-skill-plan.md`
- `docs/plans/phase-Z/smoke-checklist.md`
- `docs/plans/phase-Z/readiness.md`

## Exact Targets

- `.claude/skills/smoke-test/SKILL.md`
- `.claude/skills/smoke-test/references/level-matrix.md`
- `.claude/skills/smoke-test/references/report-schema.md`
- `.claude/skills/smoke-test/references/phase-z-row-map.md`
- `scripts/smoke/run.py`
- `scripts/smoke/render_report.py`
- `scripts/smoke/analyze_logs.py`
- `scripts/smoke/fixtures.py`
- `templates/smoke-report/smoke-fast.md.j2`
- `templates/smoke-report/smoke.md.j2`
- `templates/smoke-report/smoke-thorough.md.j2`
- `reports/smoke/`
- `.gitignore`

## Deliverables

Every listed deliverable is expected to land at a production-ready level for
the scope this sprint claims. If that cannot be done cleanly in one sprint,
the sprint must be split before implementation begins. No deliverable may be
silently dropped or partially deferred.

- smoke-test skill scaffold under `.claude/skills/smoke-test/`
- canonical smoke JSON payload contract
- J2 smoke report templates
- tracked-latest smoke artifact policy under `reports/smoke/`
- gitignored timestamped smoke artifact policy under `reports/smoke/`
- shared smoke runner plumbing for later `just smoke*` entrypoints
- shared fixture/setup helper support in `scripts/smoke/fixtures.py`

## Required Work

- define the smoke skill and supporting reference documents
- implement shared smoke runner plumbing for `fast`, `normal`, and `thorough`
- implement report rendering from J2 templates
- implement tracked-latest versus timestamped artifact writing
- implement shared timestamp propagation for smoke artifacts
- implement shared smoke fixture/setup helpers for later execution sprints
- add ignore rules for timestamped artifacts only

## Explicit Code Samples

```text
scripts/smoke/run.py <level>
```

```json
{
  "level": "normal",
  "timestamp": "2026-05-24T12:34:56Z",
  "binary_sha": "0123456789abcdef",
  "duration_secs": 123,
  "status": "scaffold-only",
  "rows": [],
  "summary": {
    "pass": 0,
    "fail": 0,
    "skip": 0
  }
}
```

## This Sprint Does Not Close

- proving the `fast` smoke happy path
- proving the `normal` smoke systemic lane
- proving the `thorough` smoke CLI coverage lane
- fixing smoke findings beyond what is required to land the infrastructure

## Acceptance Criteria

- the smoke-test skill exists and documents `fast`, `normal`, and `thorough`
- the smoke runner emits the canonical JSON payload shape
- J2 smoke report templates exist for `fast`, `normal`, and `thorough`
- tracked-latest and timestamped artifact rules are documented and enforced by
  path layout and `.gitignore`
- later execution sprints can consume the shared smoke runner plumbing without
  redefining the schema or artifact layout

## Required Validation

- `cargo test --workspace`
- `python3 .just/run_lint.py all`
- `git diff --check`

## Split Recommendation

If the sprint starts absorbing public `just smoke*` behavior, real happy-path
execution, log-analysis semantics, or root-cause behavior from `Z.19` through
`Z.21`, stop and keep that work in the later execution sprints.

## Production-Ready Expectation

Every listed `Z.18` deliverable is expected to land at a production-ready
level for the infrastructure contract this sprint claims.

## Required Document Updates

- `docs/requirements.md`
- `docs/architecture.md`
- `docs/testing-guidelines.md`
- `docs/plans/phase-Z/smoke-skill-plan.md`
- `docs/plans/phase-Z/readiness.md`
- `docs/plans/phase-Z/plan-phase-Z.md`
- `docs/project-plan.md`
- `.gitignore`

## Risks And Watchouts

- do not let infrastructure work quietly redefine smoke semantics later
- keep the smoke artifact layout deterministic from the first commit
- do not let infrastructure-only closure quietly claim the smoke levels are
  already implemented
