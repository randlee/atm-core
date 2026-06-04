---
id: AB.5
title: Copied-State Revalidation And Readiness Closeout
status: complete
branch: feature/pAB-s5-copied-state-revalidation-and-readiness-closeout
worktree: ../atm-core-worktrees/feature/pAB-s5-copied-state-revalidation-and-readiness-closeout
target: integrate/phase-AB
---

# Sprint AB.5 — Copied-State Revalidation And Readiness Closeout

```yaml
plan_type: sprint_plan
phase: AB
sprint: AB.5
worktree: ../atm-core-worktrees/feature/pAB-s5-copied-state-revalidation-and-readiness-closeout
branch: feature/pAB-s5-copied-state-revalidation-and-readiness-closeout
status: complete
estimated_scope: medium
```

## Goal

Rerun the approved cross-host subset on disposable copied state from both hosts
and record the final readiness verdict for `Phase AB`.

## Purpose

This sprint is the closeout gate: it confirms that copied-state revalidation is
allowed only after the clean-room lane passes and that the phase readiness
decision is documented explicitly.

## Governing Plan

- `docs/plan-phase-AB.md`

## Execution Branch

- `feature/pAB-s5-copied-state-revalidation-and-readiness-closeout`

## Execution Worktree

- `../atm-core-worktrees/feature/pAB-s5-copied-state-revalidation-and-readiness-closeout`

## Deliverables

- copied-state rerun of the approved smoke subset for both hosts
- captured operator setup/repair guidance discovered during copied-state
  revalidation
- final `docs/phase-AB/readiness.md` verdict update
- findings-ledger updates for any revalidated or newly discovered issues

## Acceptance Criteria

- `AB.5` does not begin until `AB.2` through `AB.4` are complete on the
  accepted `integrate/phase-AB` line
- copied-state revalidation uses disposable copies and does not write to live
  host state
- the phase verdict remains not-ready/fail until both the clean-room lane and
  copied-state lane pass
- readiness closeout records any remaining operator guidance explicitly

