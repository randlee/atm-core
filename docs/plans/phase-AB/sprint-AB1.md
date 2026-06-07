---
id: AB.1
title: Cross-Host Harness And Clean-Room Baseline
status: complete
branch: feature/pAB-s1-cross-host-harness-and-clean-room-baseline
worktree: ../atm-core-worktrees/feature/pAB-s1-cross-host-harness-and-clean-room-baseline
target: integrate/phase-AB
---

# Sprint AB.1 — Cross-Host Harness And Clean-Room Baseline

```yaml
plan_type: sprint_plan
phase: AB
sprint: AB.1
worktree: ../atm-core-worktrees/feature/pAB-s1-cross-host-harness-and-clean-room-baseline
branch: feature/pAB-s1-cross-host-harness-and-clean-room-baseline
status: complete
estimated_scope: medium
```

## Goal

Freeze the authoritative Windows/macOS cross-host smoke checklist and prove
that both hosts can execute the same-host release-binary commands under
disposable clean-room state before any cross-host messaging is attempted.

## Purpose

This sprint establishes the cross-host execution harness and the clean-room
baseline for every later `Phase AB` smoke row.

## Governing Plan

- `docs/plans/phase-AB/plan-phase-AB.md`

## Execution Branch

- `feature/pAB-s1-cross-host-harness-and-clean-room-baseline`

## Execution Worktree

- `../atm-core-worktrees/feature/pAB-s1-cross-host-harness-and-clean-room-baseline`

## Deliverables

- frozen `docs/plans/phase-AB/cross-host-smoke-checklist.md`
- documented disposable `ATM_HOME`, `ATM_CONFIG_HOME`, and `ATM_LOG_DIR` rules
  for Windows and macOS hosts
- explicit host-pair setup guidance for clean-room bring-up
- proof that release-binary `doctor`, `list`, `clear`, `send`, and
  `read --all --json` can be exercised on both hosts individually under
  disposable state before cross-host send rows begin

## Acceptance Criteria

- the smoke checklist is frozen before `AB.2` begins
- both participating hosts run disposable ATM/Claude roots without touching
  live state
- Windows and macOS each prove release-binary same-host command health under
  clean-room state
- any required firewall or local-network prompt handling is captured in the
  checklist and operator notes

