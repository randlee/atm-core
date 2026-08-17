---
id: AG.13
title: Self-IP Full-Function Same-Host Proof
status: planned
branch: feature/pAG-s13-selfip-proof
worktree: ../atm-core-worktrees/feature/pAG-s13-selfip-proof
target: develop
---

# Sprint AG.13 — Self-IP Full-Function Same-Host Proof

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.13
worktree: ../atm-core-worktrees/feature/pAG-s13-selfip-proof
branch: feature/pAG-s13-selfip-proof
status: planned
estimated_scope: medium
```

## Goal

Rerun the full remote-target functionality on one host through its own
advertised or bound IP address.

## Deliverables

- self-IP same-host unauthorized rejection evidence (`AG-VAL-019`)
- self-IP same-host full-function success evidence (`AG-VAL-020`)
- retained proof that bind/advertise configuration and allowlist enforcement
  both survive ordinary same-host IP addressing
- exact self-IP same-host setup instructions for the corrective path

## Acceptance Criteria

- self-IP same-host remote-target sends use the same cross-host dispatch branch
  proven in AG.12
- unauthorized self-IP same-host traffic is rejected before mailbox mutation
- authorized self-IP same-host traffic supports the full functional matrix:
  - durable send
  - receiver read
  - `--requires-ack`
  - reply-state mutation
  - nudge/notification classification
  - retry-visible recovery
- unhealthy self-IP transport returns immediate deferred status and later
  writes final delivery/failure receipt into sender inbox
- no result in this sprint is allowed to rely on the local mailbox path as a
  hidden fallback

## Required Validation

- `docs/plans/phase-AG/cross-host-smoke-checklist.md`
  - `AG-VAL-019`
  - `AG-VAL-020`

## Unit-Test Plan

- targeted helper tests only when needed for self-IP fixture setup

## Integration-Test Plan

- self-IP same-host integration coverage for send/read/ack
- self-IP same-host unauthorized rejection before mailbox mutation
- self-IP same-host nudge/notification classification
- self-IP same-host retry-visible recovery

## Smoke-Test Plan

- same host, one advertised or bound IP address
- authorized and unauthorized rows both retained
- both supported remote-target syntaxes are exercised against the same
  IP address

## Out Of Scope

- second-host smoke
- copied-state release verdict

## Entry Gate

- AG.12 localhost remote-target closure is complete

## Ownership

- execution owner: `arch-ctm`
- verification owner: `quality-mgr`
