---
id: AG.8
title: Transport Security Planning And Release-Language Reconciliation
status: planned
branch: feature/pAG-s8-transport-security-planning
worktree: ../atm-core-worktrees/feature/pAG-s8-transport-security-planning
target: integrate/phase-AG
---

# Sprint AG.8 — Transport Security Planning And Release-Language Reconciliation

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.8
worktree: ../atm-core-worktrees/feature/pAG-s8-transport-security-planning
branch: feature/pAG-s8-transport-security-planning
status: planned
estimated_scope: medium
```

## Goal

Lock the security direction for cross-host transport without implementing it in
this sprint, and make every Phase AG requirement/architecture/readiness
statement honest about the current line: functional cross-host code paths exist,
real AG.7 live host-pair evidence is still pending, and the shipped transport is
not yet secured.

## Deliverables

- reconciled requirements/architecture wording for cross-host transport
  security, including the distinction between the documented target contract and
  the current plain-TCP implementation line
- explicit phase-level statement of what earlier AG closure does and does not
  authorize:
  - AG.4/AG.5/AG.6/AG.7 code-path closure may support functional cross-host
    claims only after the live rows pass
  - no earlier AG sprint may imply TLS, peer-auth, or encryption closure
- ADR-030 updated to define the accepted security sequencing and the concrete
  AG.10 security direction
- AG.10 sprint scope updated so implementation, tests, and smoke rows match the
  chosen security direction
- explicit record that AG.7 real Windows/macOS or Windows/Mac-Studio host-pair
  validation is still pending and is not closed by this sprint

## Required Validation

- reconcile `AG-FIND-001` against the actual implementation line
- verify requirements, architecture, readiness, ADR-030, and AG.10 all say the
  same thing about current plain TCP vs future secured transport
- verify AG.8 does not require AG.7 live rows to be green; it requires the
  functional code paths and harnesses to exist so the security plan is grounded
  in the real transport shape
- verify no AG.1-AG.9 document implicitly grants transport-security closure
  before AG.10

## Unit-Test Plan

- review-only verification that every normative doc names the same current
  state: durable interface selection exists, durable allowlist enforcement
  exists, loopback diagnostics exist, live host-pair validation is still
  pending, and transport security remains unimplemented
- review-only verification that AG.10 names concrete trust, handshake, and
  fallback behavior rather than vague "add TLS later" language

## Integration-Test Plan

- planning-only sprint: define AG.10 integration obligations, do not implement
  or execute new integration behavior here

## Smoke-Test Plan

- planning-only sprint: define AG.10 secure smoke obligations, do not claim
  secure smoke coverage in AG.8

## Non-Closure / Out Of Scope

- no transport-security implementation
- no handshake code
- no certificate generation or trust exchange code
- no claim that AG.7 real host-pair validation is complete
- no release claim that `1.3.1` cross-host transport is encrypted or
  peer-authenticated

## Entry Gate

- the functional cross-host code paths from AG.4/AG.5/AG.6/AG.7 exist on the
  current implementation line
- AG.7 live hardware reruns may still be pending; that pending evidence must be
  recorded explicitly rather than hidden behind this sprint

## Acceptance Criteria

- the sprint text states clearly that transport security is intentionally
  sequenced after functional cross-host control-plane work
- requirements, architecture, readiness, findings, and ADR-030 agree that the
  current line is plain TCP and that any release verdict must exclude
  transport-security closure until AG.10 passes
- AG.8 states exactly what earlier AG closure does and does not authorize
- AG.8 does not claim a working secured daemon-to-daemon transport
- AG.8 does not claim AG.7 real live host-pair validation is complete
- AG.10 has concrete implementation/testing/smoke scope derived from the agreed
  security direction
