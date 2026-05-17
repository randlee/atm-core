---
id: Y.7
title: Degraded Delivery Contract Hardening
status: planned
branch: feature/pYb-s7-degraded-delivery-contract-hardening
worktree: ../atm-core-worktrees/feature/pYb-s7-degraded-delivery-contract-hardening
target: integrate/phase-Yb
---

# Sprint Y.7 — Degraded Delivery Contract Hardening

## Goal

Make the degraded-delivery contract exact and symmetric across Claude and
non-Claude harnesses.

## Governing Requirements

- `docs/phase-Yb/plan-phase-Yb.md`
- `docs/phase-Yb/removal-ledger.md`
- `docs/phase-Yb/message-path-call-stacks.md`
- `docs/adr/ADR-013-unified-delivery-plan-and-state-machine-ownership.md`

## Required Work

1. Introduce the uniform delivery-plan shape for new-message execution.
2. Ensure both harness families produce the same logical payload set:
   - success -> original message only
   - SQLite failure -> original message + `atm-system@<team>` error message
3. Remove partial Claude SQLite-failure outward delivery.
4. Add fault-injection tests for:
   - SQLite success + Claude success
   - SQLite success + Claude append degradation
   - SQLite failure + Claude path
   - SQLite failure + non-Claude path
   - companion-error failure handling
5. Prove payload equality, not just hook invocation count.

## Acceptance Criteria

- Claude and non-Claude paths emit identical logical payload sets
- the only harness difference is delivery target selection
- Claude SQLite-failure recovery is atomic at the plan level
- no metadata-only hook behavior is accepted as proof of non-Claude delivery

## Required Document Updates

- `docs/phase-Yb/message-path-call-stacks.md`
- `docs/project-plan.md`

