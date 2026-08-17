---
id: AG.2
title: Core Cross-Host Interface Validation
status: reclassified
branch: feature/cross-host-communication
worktree: ../atm-core-worktrees/feature/cross-host-communication
target: develop
---

# Sprint AG.2 — Core Cross-Host Interface Validation

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.2
worktree: ../atm-core-worktrees/feature/cross-host-communication
branch: feature/cross-host-communication
status: reclassified
estimated_scope: medium
```

## Goal

Original intent:

- validate the main cross-host interface set on clean-room state:

- durable send in both directions
- receiver-side read in both directions
- `--requires-ack` ack round-trip

Actual outcome:

- live attempts were valuable, but they did not close the sprint
- they exposed a missing product control plane:
  - no durable CLI-managed interface/bind surface
  - no durable CLI-managed inbound host allowlist
  - no SQLite-owned configuration for either
  - no `atm doctor` projection for either
- the original AG.2 closure target is therefore reclassified as provisional and
  moved behind the later control-plane sprints plus the renewed live validation
  sprint

## Deliverables

- initial validation attempts for rows `AG-VAL-003` through `AG-VAL-007`
- evidence-backed findings showing why those rows cannot be treated as
  production-meaningful closure on the original product surface
- explicit handoff into the later AG control-plane sprints that unblock real
  closure

## Required Validation

- `docs/plans/phase-AG/cross-host-smoke-checklist.md`
  - `AG-VAL-003`
  - `AG-VAL-004`
  - `AG-VAL-005`
  - `AG-VAL-006`
  - `AG-VAL-007`

## Entry Gate

- `AG.1` must already have:
  - recorded `AG-VAL-001` and `AG-VAL-002`
  - resolved the first live-channel viability attempt to either a working
    channel or a named blocking finding

## Ownership

- execution owner: `arch-ctm`
- host operators: `windows-operator`, `macos-operator`
- verification owner: `quality-mgr`

## Acceptance Criteria

- the sprint record preserves the failed assumption that existing product
  controls were sufficient
- the missing control-plane surface is promoted into explicit later AG sprint
  scope instead of being treated as ad hoc implementation fallout
