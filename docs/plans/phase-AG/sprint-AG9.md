---
id: AG.9
title: Copied-State Revalidation And Release Verdict
status: planned
branch: feature/pAG-s9-release-verdict
worktree: ../atm-core-worktrees/feature/pAG-s9-release-verdict
target: develop
---

# Sprint AG.9 — Copied-State Revalidation And Release Verdict

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.9
worktree: ../atm-core-worktrees/feature/pAG-s9-release-verdict
branch: feature/pAG-s9-release-verdict
status: planned
estimated_scope: medium
```

## Goal

Rerun the approved subset on disposable copied state and then close the phase
with the actual release verdict.

## Deliverables

- copied-state revalidation evidence
- exact operator repair/setup notes for realistic-state execution
- final findings ledger
- final readiness record
- explicit statement of whether cross-host communication is:
  - functionally release-usable
  - blocked
  - functionally usable but not transport-secure

## Required Validation

- copied-state rerun uses only the approved subset from the clean-room lane
- every retained failure links back to a named finding in
  `cross-host-findings-ledger.md`
- final readiness wording matches the actual AG.7 / AG.8 / AG.10 outcomes
  without implying transport-security closure when it is still open

## Integration-Test Plan

- copied-state host-pair validation repeats the approved AG.7 matrix on
  disposable realistic state only
- any copied-state-only failures are classified separately from clean-room
  product-surface failures

## Smoke-Test Plan

- final smoke package includes:
  - same-host doctor + loopback preflight
  - clean-room LAN host-pair matrix
  - clean-room routed/VPN host-pair matrix when applicable
  - secure loopback row `AG-VAL-012` when AG.10 is in scope
  - secure LAN rerun row `AG-VAL-013` when AG.10 is in scope
  - secure rejection row `AG-VAL-014` when AG.10 is in scope
  - secure routed/VPN rerun row `AG-VAL-015` when AG.10 is in scope
  - copied-state rerun of the approved subset

## Entry Gate

- AG.7 live host-pair validation is complete enough to justify copied-state
  rerun
- AG.8 planning/reconciliation work is complete
- AG.10 status is known before the final release verdict is issued:
  - if `AG.10` is `PASS`, the verdict may include transport-security closure
  - if `AG.10` is deferred, blocked, or out-of-scope, the verdict must state
    cross-host is functionally usable but not transport-secure

## Acceptance Criteria

- copied-state execution happens only after the clean-room/control-plane lane is
  credible
- every surviving failed row is linked to a named finding
- final release wording is explicit about the difference between functional
  cross-host closure and transport-security closure
- AG.10 is named explicitly as a precondition for any transport-security claim
