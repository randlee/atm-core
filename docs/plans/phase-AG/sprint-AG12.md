---
id: AG.12
title: Localhost Full-Function Remote-Target Loopback
status: planned
branch: docs/cross-host-remote-target-contract
worktree: ../atm-core-worktrees/docs/cross-host-remote-target-contract
target: develop
---

# Sprint AG.12 — Localhost Full-Function Remote-Target Loopback

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.12
worktree: ../atm-core-worktrees/docs/cross-host-remote-target-contract
branch: docs/cross-host-remote-target-contract
status: planned
estimated_scope: medium
```

## Goal

Prove 100% of the remote-target functionality on localhost with real ATM
payloads before involving a public interface or a second host.

## Deliverables

- localhost unauthorized rejection evidence (`AG-VAL-016`)
- localhost full-function success evidence (`AG-VAL-017`)
- localhost transport-security disposition (`AG-VAL-018`)
- retained proof that remote-target localhost sends traverse the peer-transport
  path instead of the local mailbox path
- exact localhost runbook additions for the corrective path

## Acceptance Criteria

- localhost remote-target sends use the same cross-host dispatch branch as a
  second-host send
- unauthorized localhost traffic is rejected before mailbox mutation
- authorized localhost traffic supports the full functional matrix:
  - durable send
  - receiver read
  - `--requires-ack`
  - reply-state mutation
  - nudge/notification classification
  - retry-visible recovery
- the sprint records whether localhost proof is secured or unsecured and links
  the result to `AG-FIND-001` when TLS is still open

## Required Validation

- `docs/plans/phase-AG/cross-host-smoke-checklist.md`
  - `AG-VAL-016`
  - `AG-VAL-017`
  - `AG-VAL-018`

## Unit-Test Plan

- targeted helper tests only when needed for localhost-specific fixture setup
- no new semantic unit coverage beyond what AG.11 already required

## Integration-Test Plan

- localhost remote-target send/read/ack integration coverage
- localhost unauthorized rejection before mailbox mutation
- localhost nudge/notification behavior remains classified as degradation, not as
  durable-send failure
- localhost retry-visible recovery remains bounded and observable

## Smoke-Test Plan

- one daemon bound for localhost-only use
- one sender identity and one receiver identity using real ATM payloads
- both supported remote-target syntaxes are exercised:
  - `<agent>@<team>.localhost`
  - `<agent>@<team> --host localhost`

## Out Of Scope

- non-loopback public-interface proof
- second-host smoke
- copied-state release verdict

## Entry Gate

- AG.11 routing work is complete enough that localhost remote-target sends no
  longer fall back to the local mailbox path

## Ownership

- execution owner: `arch-ctm`
- verification owner: `quality-mgr`
