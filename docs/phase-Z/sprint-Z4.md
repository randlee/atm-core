---
id: Z.4
title: Final Fixes And Release Sign-Off
status: complete
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
status: complete
estimated_scope: medium
```

## Goal

Close the findings from the `atm-dev` canary, rerun the final validation set,
and produce the final release-readiness verdict.

## Governing Requirements

- `docs/plan-phase-Z.md`
- `docs/phase-Yd/readiness.md`
- `Z.3` canary findings are the only input finding source for this sprint

## Prerequisites

- `Z.3` complete
- the canary findings ledger is frozen

## Deliverables

- fixed closeout branch containing only `Z.3` finding closure work
- `docs/phase-Z/release-checklist.md` populated with the final executable
  validation and release checklist result
- `docs/phase-Z/readiness.md` populated with the final release-ready /
  not-ready verdict and evidence

## Required Work

- fix only the findings promoted from `Z.3`
- rerun `docs/phase-Z/release-checklist.md`
- produce the final release-ready / not-ready decision with evidence in
  `docs/phase-Z/readiness.md`

## Acceptance Criteria

- every `Z.3` finding is either fixed or explicitly dispositioned
- the final validation set is rerun on the closeout branch
- `docs/phase-Z/readiness.md` records the release-readiness verdict
- `docs/phase-Z/release-checklist.md` and `docs/phase-Z/readiness.md`
  identify one authoritative closeout result for the `integrate/phase-Z`
  candidate

## Non-Closure

- `Z.4` does not reopen smoke or canary scope outside the frozen ledgers

## Required Validation

- `docs/phase-Z/release-checklist.md`
- `docs/phase-Z/readiness.md`
- `cargo test --workspace`
- `git diff --check`
