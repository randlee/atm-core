---
id: AG.17
title: Corrective Copied-State Revalidation And Release Verdict
status: planned
branch: TBD
worktree: TBD
target: develop
---

# Sprint AG.17 — Corrective Copied-State Revalidation And Release Verdict

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.17
worktree: TBD
branch: TBD
status: planned
estimated_scope: medium
```

## Goal

Rerun the approved corrective subset on disposable copied state after AG.11
through AG.16 are green, then record the final release verdict.

## Deliverables

- copied-state rerun of the approved corrective subset:
  - mandatory same-host rows:
    - `AG-VAL-017`
    - `AG-VAL-020`
  - mandatory second-host success rows:
    - `AG-VAL-022B`
    - `AG-VAL-022C`
    - `AG-VAL-022D`
  - fallback second-host success rows when the Windows/macOS lane is blocked by
    an `EXTERNAL-BLOCKER` unrelated to product behavior:
    - `AG-VAL-021B`
    - `AG-VAL-021C`
    - `AG-VAL-021D`
- final findings-ledger reconciliation after AG.11 through AG.16
- final readiness verdict after the corrective line
- explicit statement of whether cross-host is:
  - functionally release-usable
  - blocked
  - functionally usable but not transport-secure

## Acceptance Criteria

- copied-state rerun happens only after the same-host and second-host
  corrective ladder is credible
- every surviving failed row links to a named finding
- final verdict states whether the release is functionally usable and whether
  transport security remains open
- no release statement implies encryption closure unless AG.10 is actually
  complete and passing

## Required Validation

- copied-state rerun uses only the approved subset named in this sprint doc
- readiness wording matches the actual AG.10 and AG.11-AG.16 outcomes

## Unit-Test Plan

- none; this sprint closes through retained validation evidence and readiness
  reconciliation

## Integration-Test Plan

- AG.14 remains the automated backstop; this sprint consumes its results

## Smoke-Test Plan

- copied-state rerun of the approved localhost/public-interface/second-host
  subset only, exactly as named under Deliverables
- no live-state writes

## Out Of Scope

- any new functional scope beyond the approved AG.11-AG.16 corrective subset

## Entry Gate

- AG.16 Windows/macOS smoke is complete enough to justify copied-state rerun
- AG.10 security status is known before the final verdict is issued
- if AG.16 is blocked only by an `EXTERNAL-BLOCKER` unrelated to product
  behavior, the fallback AG.15 Mac-to-Mac copied-state subset may be used, but
  the final verdict must say the heterogeneous-host copied-state lane did not
  close

## Ownership

- execution owner: `arch-ctm`
- verification owner: `quality-mgr`
