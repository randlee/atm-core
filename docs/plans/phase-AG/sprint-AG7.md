---
id: AG.7
title: Live Cross-Host Revalidation
status: planned
branch: feature/pAG-s7-live-revalidation
worktree: ../atm-core-worktrees/feature/pAG-s7-live-revalidation
target: develop
---

# Sprint AG.7 — Live Cross-Host Revalidation

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

Rerun the clean-room live host-pair matrix on the now-complete cross-host
product surface.

## Deliverables

- explicit ownership of unauthorized-host rejection row `AG-VAL-003A`
- renewed ownership of live host-pair validation rows `AG-VAL-003` through
  `AG-VAL-009`
- explicit first-choice execution order:
  - simplest LAN host pair first
  - VPN/routed host pair second
- integration findings clearly separated from AG.4 / AG.5 product-surface
  findings

## Required Validation

- real host-pair validation is rerun only after AG.4, AG.5, and AG.6 land
- simplest network path first:
  - Windows <-> Mac Studio on LAN if available
  - VPN/routed pair afterward
- the loopback lane from AG.3 remains a prerequisite diagnostic, but not a
  substitute for real host-pair evidence

## Unit-Test Plan

- n/a beyond targeted harness helpers; the sprint closes through integration and
  smoke evidence rather than new local-only unit semantics

## Integration-Test Plan

- daemon-to-daemon integration tests on controlled host pairs proving:
  - unauthorized host rejected before delivery
  - authorized host send succeeds
  - authorized host read succeeds
  - authorized host `--requires-ack` round-trip succeeds
  - degraded notification does not misclassify durable success
  - retry-visible recovery remains bounded

## Smoke-Test Plan

- LAN smoke:
  - Windows <-> Mac Studio first, if available
  - unauthorized-host rejection row `AG-VAL-003A`
  - authorized send/read/ack rows
- routed/VPN smoke:
  - rerun the same matrix once LAN is green
- copied-state remains explicitly deferred to AG.9

## Entry Gate

- AG.4 and AG.5 product work is complete
- AG.6 doctor visibility is complete

## Ownership

- execution owner: `arch-ctm`
- verification owner: `quality-mgr`

## Acceptance Criteria

- real host-pair validation runs on the intended product surface rather than env
  hacks
- LAN-first execution preference is explicit
- integration blockers such as firewall/routing/VPN are recorded as such rather
  than causing new transport-design hacks
