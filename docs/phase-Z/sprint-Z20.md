---
id: Z.20
title: Canary Smoke Integration And Binary Baseline Tracking
status: planned
branch: feature/pZ-s20-canary-smoke-integration-and-binary-baseline-tracking
worktree: ../atm-core-worktrees/feature/pZ-s20-canary-smoke-integration-and-binary-baseline-tracking
target: integrate/phase-Z
---

# Sprint Z.20 — Canary Smoke Integration And Binary Baseline Tracking

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.20
worktree: ../atm-core-worktrees/feature/pZ-s20-canary-smoke-integration-and-binary-baseline-tracking
branch: feature/pZ-s20-canary-smoke-integration-and-binary-baseline-tracking
status: planned
estimated_scope: medium
```

## Goal

- connect smoke reports to canary/release evidence
- define binary-baseline tracking on the accepted executable under test
- document how automated smoke augments, but does not replace, manual canary runs

## Scope Summary

This sprint closes the planning and wiring gap between smoke automation and the
later `Z.3` / `Z.4` canary and release closeout line.

## Governing Requirements

- `REQ-CORE-ATM-JSON-001`
- `REQ-CORE-CLI-001`

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
- `docs/phase-Z/canary-dogfood-checklist.md`
- `docs/phase-Z/readiness.md`

## Exact Targets

- `.claude/skills/smoke-test/SKILL.md`
- `.claude/skills/smoke-test/references/phase-z-row-map.md`
- `.claude/skills/smoke-test/references/level-matrix.md`
- `scripts/smoke/report.py`
- `docs/phase-Z/readiness.md`

## Deliverables

Every listed deliverable is expected to land at a production-ready level for
the scope this sprint claims. If that cannot be done cleanly in one sprint, the
sprint must be split before implementation begins. No deliverable may be
silently dropped or partially deferred.

- canary-checklist integration guidance
- binary baseline tracking rules tied to smoke reports
- finalized operator guidance for using smoke reports in Phase Z evidence

## Required Work

- define how smoke reports map into canary entry evidence
- define how `binary_sha` is tied back to accepted baseline records
- document what remains manual in `Z.3` and `Z.4`
- keep smoke automation additive to the checklist artifacts, not authoritative
  over them

## Explicit Code Samples

If the sprint introduces or changes important traits, features, enums, protocol
types, boundary contracts, or execution seams, this section must include
explicit code samples or signatures showing the intended end state.

```text
binary_sha -> readiness candidate / smoke report / canary baseline note
```

## This Sprint Does Not Close

- final canary execution
- final release sign-off
- optional CI smoke promotion if cross-platform stabilization still needs a
  dedicated sprint

## Acceptance Criteria

- the smoke skill explains how automated smoke evidence augments
  `docs/phase-Z/canary-dogfood-checklist.md`
- binary baseline tracking is explicit and tied to the smoke report schema
- readiness/baseline document expectations are defined for the smoke line

## Required Validation

- `cargo test --workspace`
- `python3 .just/run_lint.py all`
- `git diff --check`

## Split Recommendation

If CI promotion or platform-specific fixture drift becomes large enough to
obscure the smoke/canary evidence contract, create optional `Z.21` and move
that stabilization there.

## Production-Ready Expectation

Every listed `Z.20` deliverable is expected to land at a production-ready
level for smoke/canary integration and binary-baseline tracking.

## Required Document Updates

- `docs/phase-Z/smoke-skill-plan.md`
- `docs/phase-Z/readiness.md`
- `docs/project-plan.md`

## Risks And Watchouts

- do not let smoke automation claim to replace human canary usage
- keep binary-baseline tracking deterministic and simple enough to audit later
