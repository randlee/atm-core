---
id: Z.3
title: `atm-dev` Canary And Dogfood
status: planned
branch: feature/pZ-s3-atm-dev-canary-and-dogfood
worktree: ../atm-core-worktrees/feature/pZ-s3-atm-dev-canary-and-dogfood
target: integrate/phase-Z
---

# Sprint Z.3 — `atm-dev` Canary And Dogfood

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.3
worktree: ../atm-core-worktrees/feature/pZ-s3-atm-dev-canary-and-dogfood
branch: feature/pZ-s3-atm-dev-canary-and-dogfood
status: planned
estimated_scope: large
```

## Goal

Move from single-operator smoke to controlled `atm-dev` team use on the new
executables and verify real operator UX and recovery behavior before final
release sign-off.

## Governing Requirements

- `docs/plan-phase-Z.md`
- `Z.2` must have closed the executable smoke findings first

## Prerequisites

- `Z.2` complete
- canary participants and reporting path are defined

## Required Work

- run controlled `atm-dev` dogfood on the new executables
- capture operator-visible UX, recovery, and reliability issues
- verify real usage does not reintroduce hidden dependency on old inbox
  behavior

## Acceptance Criteria

- `atm-dev` canary usage is completed on the approved binaries
- operator-facing findings are recorded for `Z.4`

## Required Validation

- dogfood checklist and operator reports
- `git diff --check`
