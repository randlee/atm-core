---
id: Y.10
title: Boundary Enforcement And Smoke Handoff
status: planned
branch: feature/pYb-s10-boundary-enforcement-and-smoke-handoff
worktree: ../atm-core-worktrees/feature/pYb-s10-boundary-enforcement-and-smoke-handoff
target: integrate/phase-Yb
---

# Sprint Y.10 — Boundary Enforcement And Smoke Handoff

## Goal

Close the Yb implementation line with mechanical boundary enforcement and hand
the line back to executable smoke planning only after the message-path rules
are verified.

## Governing Requirements

- `docs/phase-Yb/plan-phase-Yb.md`
- `docs/phase-Yb/lintable-boundary-plan.md`
- `docs/adr/ADR-013-unified-delivery-plan-and-state-machine-ownership.md`

## Required Work

1. Land the final lint/mechanical boundary allowlists.
2. Verify the removal ledger is fully closed.
3. Confirm no policy remains outside the machines and shared executors.
4. Prepare the handoff back to smoke/dogfood planning.

## Acceptance Criteria

- only approved coordinator/executor modules can call delivery/write primitives
- removal-ledger targets are either closed or explicitly tracked as blockers
- Yb can hand control back to the smoke/dogfood line without re-opening the
  same path-consolidation issues

