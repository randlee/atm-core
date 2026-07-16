---
id: AG.7
title: Live Cross-Host Revalidation
status: planned
branch: feature/pAG-s7-live-revalidation
worktree: ../atm-core-worktrees/feature/pAG-s7-live-revalidation
target: integrate/phase-AG
---

# Sprint AG.7 — Peer-Listener Harness Revalidation

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.7
worktree: ../atm-core-worktrees/feature/pAG-s7-live-revalidation
branch: feature/pAG-s7-live-revalidation
status: planned
estimated_scope: medium
```

## Goal

Keep the AG.7 branch honest about what it actually validates.

This sprint does not close live LAN/VPN host-pair execution. It owns the local
peer-listener integration harness that must stay green before the later live
host-pair rows are retried on real networks.

## Deliverables

- explicit ownership of the local harness rows that exercise:
  - unauthorized-host rejection before dispatch
  - authorized send / receiver read / `--requires-ack` reply mutation over the
    peer-listener request path
  - degraded post-send warning surfacing without downgrading durable send
    success
- explicit statement that live host-pair rows `AG-VAL-003` through `AG-VAL-009`
  remain blocked on the separately tracked ordinary-send dispatch defect
  `AG-FIND-005`
- integration findings clearly separated from AG.4 / AG.5 product-surface
  findings and from the later real-network validation lane

## Required Validation

- the local harness must stay explicitly scoped to the peer-listener request
  path and must not claim to exercise ordinary `atm send` remote dispatch
- the loopback lane from AG.3 remains a prerequisite diagnostic input, not a
  substitute for live host-pair evidence
- real host-pair validation remains deferred until the production send path is
  wired into peer transport

## Unit-Test Plan

- n/a beyond targeted harness helpers; the sprint closes through integration and
  smoke evidence rather than new local-only unit semantics

## Integration-Test Plan

- daemon-to-daemon integration tests on the local peer-listener request path
  proving:
  - unauthorized host rejected before delivery
  - authorized host send succeeds
  - authorized host read succeeds
  - authorized host `--requires-ack` round-trip succeeds
  - degraded notification does not misclassify durable success
  - retry-visible recovery remains bounded

## Smoke-Test Plan

- none on this branch beyond the retained local harness evidence; live LAN/VPN
  smoke remains deferred

## Entry Gate

- AG.4 and AG.5 product work is complete
- AG.6 doctor visibility is complete

## Ownership

- execution owner: `arch-ctm`
- verification owner: `quality-mgr`

## Acceptance Criteria

- the branch documentation and harness now make an honest claim about local
  peer-listener coverage only
- no branch artifact or test result on AG.7 claims that live host-pair routing
  is proven while `AG-FIND-005` remains open
