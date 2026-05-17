---
id: Z.1
title: Smoke Bring-Up
status: planned
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
status: planned
estimated_scope: large
```

## Goal

Bring up the daemon + SQLite mail-SSOT line on the real executables and run
the first full feature-by-feature smoke pass before broader team dogfood.

## Governing Requirements

- `docs/plan-phase-Z.md`
- completed `Phase Y` implementation line
- all delivery-policy and write-boundary behavior must already match the
  approved `Phase Y` state machines

## Prerequisites

- `Y.0` through `Y.6` complete and merged to the authoritative `Phase Z`
  baseline
- approved executable smoke checklist exists

## Required Work

- launch the daemon using the real built binaries
- verify end-to-end feature behavior across the supported operator flows
- verify corner cases and recovery behavior called out by the `Phase Y`
  architecture and state-machine docs
- record only validated smoke findings for `Z.2`

## Acceptance Criteria

- daemon bring-up is proven on the executable baseline
- every planned smoke flow has a pass/fail verdict
- only verified smoke findings roll forward to `Z.2`

## Required Validation

- executable smoke checklist for supported flows
- `cargo test --workspace`
- `git diff --check`
