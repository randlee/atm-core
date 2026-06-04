---
id: AB.4
title: Degraded Notification And Retry-Visible Recovery
status: complete
branch: feature/pAB-s4-degraded-notification-and-retry-visible-recovery
worktree: ../atm-core-worktrees/feature/pAB-s4-degraded-notification-and-retry-visible-recovery
target: integrate/phase-AB
---

# Sprint AB.4 — Degraded Notification And Retry-Visible Recovery

```yaml
plan_type: sprint_plan
phase: AB
sprint: AB.4
worktree: ../atm-core-worktrees/feature/pAB-s4-degraded-notification-and-retry-visible-recovery
branch: feature/pAB-s4-degraded-notification-and-retry-visible-recovery
status: complete
estimated_scope: medium
```

## Goal

Prove that durable cross-host delivery still succeeds when notification paths
degrade and that interruption/restart behavior remains retry-visible in the
recorded evidence.

## Purpose

This sprint owns the non-happy-path rows that must remain visible without being
misclassified as durable-delivery failures.

## Governing Plan

- `docs/plan-phase-AB.md`

## Execution Branch

- `feature/pAB-s4-degraded-notification-and-retry-visible-recovery`

## Execution Worktree

- `../atm-core-worktrees/feature/pAB-s4-degraded-notification-and-retry-visible-recovery`

## Deliverables

- one degraded-notification row after a durable cross-host send succeeds
- one retry-visible interruption/recovery row
- retained-log evidence from both hosts for degraded/retry-visible behavior
- operator guidance for expected warning/error interpretation during these rows

## Acceptance Criteria

- degraded notification is observable after persistence succeeds and is not
  treated as a durable-delivery failure
- retry-visible interruption/recovery evidence is captured on both hosts
- the rows document concrete recovery steps instead of relying on implicit
  operator knowledge

