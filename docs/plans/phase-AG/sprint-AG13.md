---
id: AG.13
title: Public-Interface Full-Function Loopback
status: planned
branch: docs/cross-host-remote-target-contract
worktree: ../atm-core-worktrees/docs/cross-host-remote-target-contract
target: develop
---

# Sprint AG.13 — Public-Interface Full-Function Loopback

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.13
worktree: ../atm-core-worktrees/docs/cross-host-remote-target-contract
branch: docs/cross-host-remote-target-contract
status: planned
estimated_scope: medium
```

## Goal

Rerun the full remote-target functionality on one host through its
non-loopback advertised address.

## Deliverables

- public-interface unauthorized rejection evidence (`AG-VAL-019`)
- public-interface full-function success evidence (`AG-VAL-020`)
- retained proof that bind/advertise configuration and allowlist enforcement
  both survive off-loopback addressing
- exact public-interface loopback setup instructions for the corrective path

## Acceptance Criteria

- public-interface remote-target sends use the same cross-host dispatch branch
  proven in AG.12
- unauthorized public-interface traffic is rejected before mailbox mutation
- authorized public-interface traffic supports the full functional matrix:
  - durable send
  - receiver read
  - `--requires-ack`
  - reply-state mutation
  - nudge/notification classification
  - retry-visible recovery
- no result in this sprint is allowed to rely on the local mailbox path as a
  hidden fallback

## Required Validation

- `docs/plans/phase-AG/cross-host-smoke-checklist.md`
  - `AG-VAL-019`
  - `AG-VAL-020`

## Unit-Test Plan

- targeted helper tests only when needed for public-interface fixture setup

## Integration-Test Plan

- public-interface loopback integration coverage for send/read/ack
- public-interface unauthorized rejection before mailbox mutation
- public-interface nudge/notification classification
- public-interface retry-visible recovery

## Smoke-Test Plan

- same host, non-loopback advertised address
- authorized and unauthorized rows both retained
- both supported remote-target syntaxes are exercised against the same
  advertised address

## Out Of Scope

- second-host smoke
- copied-state release verdict

## Entry Gate

- AG.12 localhost remote-target closure is complete

## Ownership

- execution owner: `arch-ctm`
- verification owner: `quality-mgr`
