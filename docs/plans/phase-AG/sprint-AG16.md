---
id: AG.16
title: Windows/macOS Cross-Host Smoke
status: planned
branch: TBD
worktree: TBD
target: develop
---

# Sprint AG.16 — Windows/macOS Cross-Host Smoke

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.16
worktree: TBD
branch: TBD
status: planned
estimated_scope: medium
```

## Goal

Prove the corrective path survives the real heterogeneous-host topology.

## Deliverables

- Windows/macOS smoke evidence:
  - `AG-VAL-022A`
  - `AG-VAL-022B`
  - `AG-VAL-022C`
  - `AG-VAL-022D`
  - `AG-VAL-022E`
  - `AG-VAL-022F`
- retained evidence for:
  - unauthorized rejection
  - authorized send
  - receiver read
  - `--requires-ack`
  - reply-state mutation
  - nudge/notification classification
  - retry-visible recovery
- Windows-specific recovery/runbook deltas for firewall, routing, daemon
  bring-up, and operator setup

## Acceptance Criteria

- Windows/macOS smoke confirms the corrective path survives the heterogeneous
  host pair
- unauthorized cross-host traffic is rejected before mailbox mutation
- authorized cross-host traffic supports the full functional matrix
- Windows-only environmental failures are recorded separately from product
  defects

## Required Validation

- `docs/plans/phase-AG/cross-host-smoke-checklist.md`
  - `AG-VAL-022A`
  - `AG-VAL-022B`
  - `AG-VAL-022C`
  - `AG-VAL-022D`
  - `AG-VAL-022E`
  - `AG-VAL-022F`

## Unit-Test Plan

- none; this sprint closes through retained smoke evidence

## Integration-Test Plan

- AG.14 integration coverage must already be green before this sprint starts

## Smoke-Test Plan

- retain sender/receiver JSON and daemon logs from both hosts
- run both rejection and success rows on the same candidate line
- exercise the same remote-target syntax and feature matrix already proven on
  localhost, self-IP same-host proof, and the other-Mac host pair

## Out Of Scope

- copied-state release verdict

## Entry Gate

- AG.15 other-Mac smoke is complete

## Ownership

- execution owner: `arch-ctm`
- host operators: `windows-operator`, `macos-operator`
- verification owner: `quality-mgr`
