---
id: Z.18
title: Smoke Skill Scaffold And Fast/Normal Runner
status: planned
branch: feature/pZ-s18-smoke-skill-and-fast-normal-runner
worktree: ../atm-core-worktrees/feature/pZ-s18-smoke-skill-and-fast-normal-runner
target: integrate/phase-Z
---

# Sprint Z.18 — Smoke Skill Scaffold And Fast/Normal Runner

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.18
worktree: ../atm-core-worktrees/feature/pZ-s18-smoke-skill-and-fast-normal-runner
branch: feature/pZ-s18-smoke-skill-and-fast-normal-runner
status: planned
estimated_scope: medium
```

## Goal

- create the smoke-test skill scaffold
- land the report contract and `.smoke-reports` output path
- land `just smoke-fast` and default `just smoke`

## Scope Summary

This sprint creates the first reusable smoke runner and report format. It does
not yet automate every frozen smoke row.

## Governing Requirements

- `REQ-CORE-ATM-JSON-001`
- `REQ-CORE-CLI-001`

## Governing ADRs

- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`

## Governing Boundaries

- `RosterStore`
- `RuntimeFactory`

## Prerequisites

- `Z.17` complete

## Hard Dependencies

- `docs/phase-Z/smoke-skill-plan.md`
- `docs/phase-Z/smoke-checklist.md`
- `docs/phase-Z/readiness.md`

## Exact Targets

- `.claude/skills/smoke-test/SKILL.md`
- `.claude/skills/smoke-test/references/level-matrix.md`
- `.claude/skills/smoke-test/references/report-schema.md`
- `scripts/smoke/run.py`
- `scripts/smoke/report.py`
- `Justfile`

## Deliverables

Every listed deliverable is expected to land at a production-ready level for
the scope this sprint claims. If that cannot be done cleanly in one sprint, the
sprint must be split before implementation begins. No deliverable may be
silently dropped or partially deferred.

- smoke-test skill scaffold under `.claude/skills/smoke-test/`
- `.smoke-reports/<timestamp>.json` report writing
- human-readable stdout summary
- `just smoke-fast`
- `just smoke` defaulting to `normal`

## Required Work

- define the smoke-level entrypoint contract in the skill
- implement `fast` and `normal` runner dispatch
- write the required JSON schema fields
- print a human-readable summary suitable for ATM handoff
- return exit `0` on all-pass and exit `1` on any fail

## Explicit Code Samples

If the sprint introduces or changes important traits, features, enums, protocol
types, boundary contracts, or execution seams, this section must include
explicit code samples or signatures showing the intended end state.

```text
just smoke-fast
just smoke
```

```json
{
  "level": "normal",
  "timestamp": "2026-05-24T12:34:56Z",
  "binary_sha": "0123456789abcdef",
  "duration_secs": 123,
  "rows": [],
  "summary": {
    "pass": 0,
    "fail": 0,
    "skip": 0
  }
}
```

## This Sprint Does Not Close

- full frozen smoke-checklist automation
- canary-checklist integration
- binary baseline readiness stamping

## Acceptance Criteria

- the smoke-test skill exists and documents `fast` and `normal`
- `just smoke-fast` exists and runs the `fast` level
- `just smoke` exists and defaults to the `normal` level
- each run writes `.smoke-reports/<timestamp>.json`
- each run prints a human-readable summary
- exit `0` means all-pass, exit `1` means any fail

## Required Validation

- `cargo test --workspace`
- `python3 .just/run_lint.py all`
- `git diff --check`

## Split Recommendation

If the runner starts absorbing complete-level copied-state fixtures or
canary/release wiring, stop and push that into `Z.19` or `Z.20`.

## Production-Ready Expectation

Every listed `Z.18` deliverable is expected to land at a production-ready
level for the fast/normal smoke entrypoint contract this sprint claims.

## Required Document Updates

- `docs/phase-Z/smoke-skill-plan.md`
- `docs/phase-Z/readiness.md`
- `docs/project-plan.md`

## Risks And Watchouts

- do not let the default `just smoke` become too thin to be useful
- keep the report format stable enough that `Z.19` only extends row coverage,
  not rewrites the schema
