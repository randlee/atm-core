---
id: AG.3
title: Degraded Path And Retry-Visible Recovery
status: planned
branch: feature/cross-host-communication
worktree: ../atm-core-worktrees/feature/cross-host-communication
target: develop
---

# Sprint AG.3 — Degraded Path And Retry-Visible Recovery

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.3
worktree: ../atm-core-worktrees/feature/cross-host-communication
branch: feature/cross-host-communication
status: planned
estimated_scope: medium
```

## Goal

Validate the non-happy-path cross-host rows without misclassifying durable
delivery outcomes.

## Deliverables

- checklist rows `AG-VAL-008` and `AG-VAL-009`
- degraded-notification row after durable send
- retry-visible interruption/recovery row
- failure-classification evidence for both cases

## Acceptance Criteria

- notification degradation is not treated as durable-delivery failure after
  persistence succeeded
- interruption/recovery evidence is explicit and bounded
