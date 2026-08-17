---
id: AG.8
title: Transport Security And Encryption Hardening
status: planned
branch: feature/pAG-s8-transport-security-planning
worktree: ../atm-core-worktrees/feature/pAG-s8-transport-security-planning
target: develop
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

Define the transport-security direction after functional cross-host operation
is real and operator-manageable, without yet claiming the secured transport is
implemented in this sprint.

## Deliverables

- exact requirements/architecture reconciliation for cross-host encryption
- implementation-plan scope for transport-security upgrades after functional
  validation is green
- explicit statement of what earlier AG closure does and does not authorize
- ADR-030 for transport-security direction once AG functional closure exists

## Required Validation

- reconcile `AG-FIND-001` against the actual implementation line
- define the late-sprint acceptance bar for encryption / peer authentication
- ensure any earlier Phase AG "functional cross-host ready" verdict explicitly
  excludes transport-security claims until this sprint closes

## Unit-Test Plan

- review-only verification that requirements, architecture, readiness, and the
  ADR all describe the same late-sequenced security posture
- review-only verification that no AG.4-AG.9 sprint silently implies secured
  transport closure before the implementation sprint lands

## Integration-Test Plan

- n/a for implementation behavior in this sprint; integration obligations are
  owned by AG.10

## Smoke-Test Plan

- n/a for implementation behavior in this sprint; smoke obligations are owned
  by AG.10

## Entry Gate

- functional cross-host operation, loopback support, interface selection,
  allowlist enforcement, doctor visibility, and AG.7 live validation must
  already exist

## Acceptance Criteria

- the sprint text states clearly that encryption/security is late-sequenced on
  purpose
- no earlier Phase AG sprint is allowed to imply TLS/security closure
- requirements, architecture, and readiness wording stay consistent about what
  remains unsecured until the implementation sprint closes
- AG.8 does not claim a working secured daemon-to-daemon transport in this
  sprint
