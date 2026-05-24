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

## Hard Dependencies

- `docs/plan-phase-Z.md`
- `docs/phase-Yd/readiness.md`
- `Z.2` must have closed the executable smoke findings first
- `Z.5` through `Z.10` must have closed the roster/config/restore follow-on
  line first
- `Z.11` through `Z.15` must have closed the boundary/follow-up hardening line
  first

## Prerequisites

- `Z.2` complete
- `Z.5` through `Z.10` complete
- `Z.11` through `Z.15` complete
- canary participants and reporting path are defined at sprint start and frozen
  before operator use begins
- the canary participant list is approved by `team-lead` before the first
  operator-use session

## Exact Targets

- `docs/phase-Z/canary-dogfood-checklist.md`
- `docs/phase-Z/canary-findings-ledger.md`
- the approved canary binary baseline under test on `integrate/phase-Z`

## Deliverables

- `docs/phase-Z/canary-dogfood-checklist.md`, containing the frozen
  `atm-dev` canary participant list for at least two `atm-dev` participants,
  the approved binary baseline under evaluation, and the operator reporting
  path used during the sprint
- `docs/phase-Z/canary-findings-ledger.md` promoted to `Z.4`

## Required Work

- run controlled `atm-dev` dogfood on the new executables
- capture operator-visible UX, recovery, and reliability issues
- verify real usage does not reintroduce dependency on the deprecated
  pre-Phase-Y file-migration inbox behavior documented in
  `docs/archive/file-migration-plan.md`
- record only validated canary findings in
  `docs/phase-Z/canary-findings-ledger.md` for `Z.4`

## Acceptance Criteria

- `atm-dev` canary usage is completed on the approved binaries
- `docs/phase-Z/canary-dogfood-checklist.md` records the approved participant
  list and reporting path as frozen at sprint start, with evidence that
  predates the first operator report
- operator-facing findings are recorded for `Z.4`
- every canary-checklist row in `docs/phase-Z/canary-dogfood-checklist.md`
  records one authoritative verdict before `Z.3` closes; any row left without
  a final verdict is blocking for this sprint
- the deprecated pre-Phase-Y file-migration inbox behavior from
  `docs/archive/file-migration-plan.md` is not required by any canary flow on
  the approved binaries
- `docs/phase-Z/canary-dogfood-checklist.md` and
  `docs/phase-Z/canary-findings-ledger.md` are frozen for `Z.4`

## Non-Closure

- `Z.3` does not fix canary findings
- `Z.3` does not produce the final release verdict

## Production-Ready Expectation

Every listed `Z.3` deliverable is expected to land at a production-ready level
for the canary scope this sprint claims: the checklist and findings ledger
must be usable directly by `Z.4` without scope reconstruction or manual QA
interpretation.

## Required Validation

- `cargo build --release` or equivalent release build that refreshes the
  canary executable baseline under test
- `docs/phase-Z/canary-dogfood-checklist.md`
- `docs/phase-Z/canary-findings-ledger.md`
- `cargo test --workspace`
- `git diff --check`
