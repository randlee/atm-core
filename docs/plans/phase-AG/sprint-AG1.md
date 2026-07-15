---
id: AG.1
title: Cross-Host Setup Contract And Channel Bring-Up
status: planned
branch: feature/cross-host-communication
worktree: ../atm-core-worktrees/feature/cross-host-communication
target: develop
---

# Sprint AG.1 — Cross-Host Setup Contract And Channel Bring-Up

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.1
worktree: ../atm-core-worktrees/feature/cross-host-communication
branch: feature/cross-host-communication
status: planned
estimated_scope: medium
```

## Goal

Document an operational Windows/macOS clean-room setup contract and use it to
attempt the first live cross-host daemon-to-daemon channel.

## Deliverables

- `cross-host-setup-runbook.md`
- frozen clean-room env contract for both hosts
- checklist rows `AG-VAL-001` and `AG-VAL-002`
- transport-security requirement disposition row `AG-VAL-011`
- exact first-live-channel validation order
- exact evidence contract for setup and bring-up failures
- one AG.1-only first-live-channel viability attempt that can open a finding
  but does not formally close `AG-VAL-003` or later rows

## Required Validation

- `docs/plans/phase-AG/cross-host-smoke-checklist.md`
  - `AG-VAL-001`
  - `AG-VAL-002`
  - `AG-VAL-011`
  - AG.1 viability may exercise `AG-VAL-003` or `AG-VAL-005`, but does not
    formally close them

## Ownership

- execution owner: `arch-ctm`
- host operators: `windows-operator`, `macos-operator`
- verification owner: `quality-mgr`

## Acceptance Criteria

- the runbook is concrete enough that both hosts can execute without guessing
- the first live channel attempt has a defined pass/fail evidence contract
- setup ambiguity is classified as a finding instead of being hand-waved away
