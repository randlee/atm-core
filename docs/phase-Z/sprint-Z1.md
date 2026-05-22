---
id: Z.1
title: Smoke Bring-Up
status: complete
branch: feature/pZ-s1-smoke-bring-up
worktree: ../atm-core-worktrees/feature/pZ-s1-smoke-bring-up
target: integrate/phase-Z
---

# Sprint Z.1 — Smoke Bring-Up

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.1
worktree: ../atm-core-worktrees/feature/pZ-s1-smoke-bring-up
branch: feature/pZ-s1-smoke-bring-up
status: complete
estimated_scope: large
```

## Goal

Bring up the daemon + SQLite mail-SSOT line on the real executables and run
the first full feature-by-feature smoke pass before broader team dogfood.

## Governing Requirements

- `docs/plan-phase-Z.md`
- `docs/phase-Yd/readiness.md`
- the merged `Phase Ye` closeout state on `develop`
- all delivery-policy and write-boundary behavior must already match the
  accepted `Phase Y` state machines on the develop-ready baseline

## Prerequisites

- `docs/phase-Yd/readiness.md` says:
  - `Phase Y` may land on `develop`
  - `Phase Z` may begin
- the current `develop` baseline already includes the accepted `Phase Ye`
  closure

## Deliverables

- frozen executable smoke checklist / matrix for the real-binary flows covered
  in `Z.1`
- real-binary bring-up evidence for the daemon baseline under test
- smoke findings ledger containing only validated `Z.1` findings promoted to
  `Z.2`

## Required Work

- launch the daemon using the real built binaries
- freeze the executable smoke checklist before the pass begins and use that
  frozen matrix for the entire sprint
- verify end-to-end feature behavior across the supported operator flows
- verify corner cases and recovery behavior called out by the `Phase Y`
  architecture and state-machine docs
- record only validated smoke findings for `Z.2`

## Acceptance Criteria

- daemon bring-up is proven on the executable baseline
- the frozen smoke checklist / matrix exists and covers every intended `Z.1`
  operator flow
- every planned smoke flow has a pass/fail verdict
- only verified smoke findings roll forward to `Z.2`
- the `Z.2` handoff names one authoritative smoke findings ledger

## Required Validation

- executable smoke checklist for supported flows
- `cargo test --workspace`
- `git diff --check`
