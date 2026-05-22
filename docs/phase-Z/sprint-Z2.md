---
id: Z.2
title: Fix And Revalidate
status: complete
branch: feature/pZ-s2-fix-and-revalidate
worktree: ../atm-core-worktrees/feature/pZ-s2-fix-and-revalidate
target: integrate/phase-Z
---

# Sprint Z.2 — Fix And Revalidate

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.2
worktree: ../atm-core-worktrees/feature/pZ-s2-fix-and-revalidate
branch: feature/pZ-s2-fix-and-revalidate
status: complete
estimated_scope: medium
```

## Goal

Close the verified smoke findings from `Z.1` and re-run the executable
validation set on the fixed branch.

## Governing Requirements

- `docs/plan-phase-Z.md`
- `docs/phase-Yd/readiness.md`
- `Z.1` smoke results are the only finding source for this sprint

## Prerequisites

- `Z.1` complete
- the validated `Z.1` smoke findings ledger is frozen

## Deliverables

- fixed branch containing only `Z.1` finding closure work
- updated smoke findings ledger with fix/defer disposition for every carried
  `Z.1` finding
- smoke revalidation result on the fixed branch

## Required Work

- fix only the findings promoted from `Z.1`
- keep the branch aligned with the approved `Phase Y` architecture and state
  machines
- rerun the frozen `Z.1` executable smoke checklist after fixes land

## Acceptance Criteria

- every `Z.1` finding is either fixed or explicitly deferred with approval
- the smoke checklist is rerun on the fixed branch
- the smoke findings ledger records final per-finding disposition and
  revalidation outcome

## Required Validation

- rerun the `Z.1` executable smoke checklist
- `cargo test --workspace`
- `git diff --check`
