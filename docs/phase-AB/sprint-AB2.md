---
id: AB.2
title: One-Way Cross-Host Delivery
status: complete
branch: feature/pAB-s2-one-way-cross-host-delivery
worktree: ../atm-core-worktrees/feature/pAB-s2-one-way-cross-host-delivery
target: integrate/phase-AB
---

# Sprint AB.2 — One-Way Cross-Host Delivery

```yaml
plan_type: sprint_plan
phase: AB
sprint: AB.2
worktree: ../atm-core-worktrees/feature/pAB-s2-one-way-cross-host-delivery
branch: feature/pAB-s2-one-way-cross-host-delivery
status: complete
estimated_scope: medium
```

## Goal

Prove durable one-way ATM delivery in both directions on the disposable
cross-host lane:

- Windows -> macOS
- macOS -> Windows

## Purpose

This sprint owns the first cross-host durable-delivery proof and records the
transport/bootstrap evidence that later ack and degraded-behavior rows depend
on.

## Governing Plan

- `docs/plan-phase-AB.md`

## Execution Branch

- `feature/pAB-s2-one-way-cross-host-delivery`

## Execution Worktree

- `../atm-core-worktrees/feature/pAB-s2-one-way-cross-host-delivery`

## Deliverables

- durable one-way send coverage for both host directions on the clean-room lane
- retained-log evidence for sender and receiver hosts during cross-host send
- documented bootstrap/peer-transport inputs needed for successful one-way
  delivery

## Acceptance Criteria

- Windows -> macOS durable send succeeds on disposable state
- macOS -> Windows durable send succeeds on disposable state
- retained logs on both hosts capture the transport/bootstrap evidence for the
  successful rows
- no row depends on live host state or copied-state shortcuts

