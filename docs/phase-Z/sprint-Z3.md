---
id: Z.3
title: `atm-dev` Canary And Dogfood
status: complete
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
status: complete
estimated_scope: large
```

## Goal

Move from single-operator smoke to controlled `atm-dev` team use on the new
executables and verify real operator UX and recovery behavior before final
release sign-off.

## Governing Requirements

- `docs/plan-phase-Z.md`
- `docs/phase-Yd/readiness.md`
- `Z.2` must have closed the executable smoke findings first

## Prerequisites

- `Z.2` complete
- canary participants and reporting path are defined at sprint start and frozen
  before operator use begins

## Deliverables

- frozen `atm-dev` canary participant list recorded in
  `docs/phase-Z/canary-dogfood-checklist.md`
- `docs/phase-Z/canary-dogfood-checklist.md` for the approved binaries
- operator reporting path recorded in `docs/phase-Z/canary-dogfood-checklist.md`
- `docs/phase-Z/canary-findings-ledger.md` promoted to `Z.4`

## Required Work

- run controlled `atm-dev` dogfood on the new executables
- capture operator-visible UX, recovery, and reliability issues
- verify real usage does not reintroduce hidden dependency on old inbox
  behavior
- record only validated canary findings in
  `docs/phase-Z/canary-findings-ledger.md` for `Z.4`

## Acceptance Criteria

- `atm-dev` canary usage is completed on the approved binaries
- operator-facing findings are recorded for `Z.4`
- `docs/phase-Z/canary-dogfood-checklist.md` and
  `docs/phase-Z/canary-findings-ledger.md` are frozen for `Z.4`

## Non-Closure

- `Z.3` does not fix canary findings
- `Z.3` does not produce the final release verdict

## Required Validation

- `docs/phase-Z/canary-dogfood-checklist.md`
- `docs/phase-Z/canary-findings-ledger.md`
- `cargo test --workspace`
- `git diff --check`
