---
id: AG.15
title: Other-Mac Cross-Host Smoke
status: planned
branch: feature/pAG-s15-othermac-smoke
worktree: ../atm-core-worktrees/feature/pAG-s15-othermac-smoke
target: develop
---

# Sprint AG.15 — Other-Mac Cross-Host Smoke

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.15
worktree: ../atm-core-worktrees/feature/pAG-s15-othermac-smoke
branch: feature/pAG-s15-othermac-smoke
status: planned
estimated_scope: medium
```

## Goal

Prove the corrective path survives a real second-host topology on another Mac
before introducing Windows-specific variables.

## Deliverables

- other-Mac smoke evidence:
  - `AG-VAL-021A`
  - `AG-VAL-021B`
  - `AG-VAL-021C`
  - `AG-VAL-021D`
  - `AG-VAL-021E`
  - `AG-VAL-021F`
- retained evidence for:
  - unauthorized rejection
  - authorized send
  - receiver read
  - `--requires-ack`
  - reply-state mutation
  - nudge/notification classification
  - retry-visible recovery
- first-line recovery notes for second-host firewall, routing, or operator
  errors

## Acceptance Criteria

- another-Mac smoke confirms the corrective path behaves the same way as the
  localhost and self-IP same-host proofs
- unauthorized cross-host traffic is rejected before mailbox mutation
- authorized cross-host traffic supports the full functional matrix
- failures are classified as setup, environment, product, or external blocker
  using the Phase AG finding enum only

## Required Validation

- `docs/plans/phase-AG/cross-host-smoke-checklist.md`
  - `AG-VAL-021A`
  - `AG-VAL-021B`
  - `AG-VAL-021C`
  - `AG-VAL-021D`
  - `AG-VAL-021E`
  - `AG-VAL-021F`

## Unit-Test Plan

- none; this sprint closes through retained smoke evidence

## Integration-Test Plan

- AG.14 integration coverage must already be green before this sprint starts

## Smoke-Test Plan

- use the simplest reachable Mac-to-Mac path first
- retain sender/receiver JSON and daemon logs from both Macs
- run both rejection and success rows on the same candidate line

## Out Of Scope

- Windows/macOS heterogeneous-host closure
- copied-state release verdict

## Entry Gate

- AG.13 self-IP same-host proof is complete
- AG.14 automated integration coverage is complete enough to make second-host
  failures actionable

## Ownership

- execution owner: `arch-ctm`
- host operators: `macos-operator`
- verification owner: `quality-mgr`
