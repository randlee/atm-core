---
id: Z.4
title: Final Fixes And Release Sign-Off
status: planned
branch: feature/pZ-s4-final-fixes-and-release-sign-off
worktree: ../atm-core-worktrees/feature/pZ-s4-final-fixes-and-release-sign-off
target: integrate/phase-Z
---

# Sprint Z.4 — Final Fixes And Release Sign-Off

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.4
worktree: ../atm-core-worktrees/feature/pZ-s4-final-fixes-and-release-sign-off
branch: feature/pZ-s4-final-fixes-and-release-sign-off
status: planned
estimated_scope: medium
```

## Goal

Close the findings from the `atm-dev` canary, rerun the final validation set,
and produce the final release-readiness verdict.

## Governing Requirements

- `docs/plan-phase-Z.md`
- `Z.3` canary findings are the only input finding source for this sprint

## Prerequisites

- `Z.3` complete
- the canary finding list is frozen

## Required Work

- fix only the findings promoted from `Z.3`
- rerun the final executable validation and release checklist
- produce the final release-ready / not-ready decision with evidence

## Acceptance Criteria

- every `Z.3` finding is either fixed or explicitly dispositioned
- the final validation set is rerun on the closeout branch
- the release-readiness verdict is documented

## Required Validation

- final executable validation checklist
- `cargo test --workspace`
- `git diff --check`
