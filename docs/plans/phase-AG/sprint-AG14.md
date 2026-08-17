---
id: AG.14
title: Automated Integration Coverage For The Corrective Path
status: planned
branch: feature/pAG-s14-integration-coverage
worktree: ../atm-core-worktrees/feature/pAG-s14-integration-coverage
target: develop
---

# Sprint AG.14 — Automated Integration Coverage For The Corrective Path

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.14
worktree: ../atm-core-worktrees/feature/pAG-s14-integration-coverage
branch: feature/pAG-s14-integration-coverage
status: planned
estimated_scope: medium
```

## Goal

Lock the AG.11-AG.13 corrective behavior into automated integration coverage
so the release does not depend only on manual smoke.

## Deliverables

- parser/normalization integration coverage for both supported remote-target
  syntaxes
- dispatch integration coverage proving remote-target sends never fall through
  to the local mailbox path
- localhost full-function integration coverage mirroring AG.12
- self-IP full-function integration coverage mirroring AG.13
- one narrow ADR-003 Tier-3 real-socket or real-daemon-spawn test covering the
  production `CrossHostDelivery` dispatch path end to end
- automated coverage for:
  - unauthorized rejection
  - authorized send/read/ack
  - nudge/notification classification
  - retry-visible recovery

## Acceptance Criteria

- the corrective path is covered by automated integration tests, not only by
  manual smoke
- at least one AG.14 test is explicitly ADR-003 Tier-3 and exercises the live
  production `CrossHostDelivery` path end to end rather than an in-process fake
- localhost and self-IP same-host proof both have automated success and
  rejection coverage
- the integration suite fails if a remote-target send writes to the local
  mailbox path
- the integration suite is suitable for `just test` gating on the corrective
  branch

## Required Validation

- integration tests exist for all AG.11-AG.13 corrective behaviors
- the sprint doc identifies the fidelity tier for each required AG.14 test:
  - parser normalization and branch-selection coverage may be ADR-003 Tier-1
    or Tier-2
  - at least one end-to-end remote-dispatch regression must be ADR-003 Tier-3
- `just test` exercises the new integration coverage on the corrective branch

## Unit-Test Plan

- none beyond any targeted fixture helpers needed by the integration suite

## Integration-Test Plan

- ADR-003 Tier-1 or Tier-2:
  - exact CLI parsing coverage for both remote-target syntaxes
  - exact dispatch-branch coverage for local vs remote sends
- ADR-003 Tier-3:
  - one real-socket or real-daemon-spawn regression proving a remote-target
    send traverses the production `CrossHostDelivery` path and cannot fall back
    to the local mailbox path
  - localhost same-host matrix coverage
  - self-IP same-host matrix coverage
  - no-local-fallback regression coverage

## Smoke-Test Plan

- no new second-host smoke closes this sprint
- AG.15 and AG.16 consume the AG.14 integration suite as a prerequisite safety
  net

## Out Of Scope

- second-host smoke closure by manual evidence alone
- copied-state release verdict

## Entry Gate

- AG.11 dispatch routing is complete
- AG.12 and AG.13 have defined the exact same-host behavior to lock in

## Ownership

- execution owner: `arch-ctm`
- verification owner: `quality-mgr`
