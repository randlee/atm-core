---
id: AB.3
title: Cross-Host Ack Round-Trip
status: complete
branch: feature/pAB-s3-cross-host-ack-round-trip
worktree: ../atm-core-worktrees/feature/pAB-s3-cross-host-ack-round-trip
target: integrate/phase-AB
---

# Sprint AB.3 — Cross-Host Ack Round-Trip

```yaml
plan_type: sprint_plan
phase: AB
sprint: AB.3
worktree: ../atm-core-worktrees/feature/pAB-s3-cross-host-ack-round-trip
branch: feature/pAB-s3-cross-host-ack-round-trip
status: complete
estimated_scope: medium
```

## Goal

Prove `--requires-ack` send plus receiver-side reply-state mutation across the
Windows/macOS host pair on the disposable lane.

## Purpose

This sprint closes the cross-host send/read/ack loop by validating receiver
visibility and sender reply visibility after the one-way delivery baseline is
already healthy.

## Governing Plan

- `docs/plans/phase-AB/plan-phase-AB.md`

## Execution Branch

- `feature/pAB-s3-cross-host-ack-round-trip`

## Execution Worktree

- `../atm-core-worktrees/feature/pAB-s3-cross-host-ack-round-trip`

## Deliverables

- cross-host `--requires-ack` send coverage
- receiver-side `read` proof for the delivered message
- sender-side reply visibility proof after the receiver acknowledges
- documented cross-host logical-message mutation evidence for the ack round-trip

## Acceptance Criteria

- receiver reads the cross-host delivered message successfully
- receiver can issue the required ack successfully
- original sender observes the reply/ack mutation on the disposable lane
- ack-round-trip rows remain attributable to disposable state only

