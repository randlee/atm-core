---
id: AG.2
title: Core Cross-Host Interface Validation
status: planned
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
status: planned
estimated_scope: medium
```

## Goal

Validate the main cross-host interface set on clean-room state:

- durable send in both directions
- receiver-side read in both directions
- `--requires-ack` ack round-trip

## Deliverables

- validation rows `AG-VAL-003` through `AG-VAL-007`
- evidence-backed findings for every failed interface row

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
- verification owner: `quality-mgr`

## Acceptance Criteria

- every core interface row ends in `PASS` or a named finding
- no speculative code work begins without one failing row and artifacts
- no AG.2 execution begins while AG.1's first live-channel outcome is still
  unresolved
