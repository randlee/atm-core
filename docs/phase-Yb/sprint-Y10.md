---
id: Y.10
title: Boundary Enforcement And Smoke Handoff
status: complete
branch: feature/pYb-s10-boundary-enforcement-and-smoke-handoff
worktree: ../atm-core-worktrees/feature/pYb-s10-boundary-enforcement-and-smoke-handoff
target: integrate/phase-Yb
---

# Sprint Y.10 — Boundary Enforcement And Smoke Handoff

## Goal

Close the Yb implementation line with mechanical boundary enforcement and hand
the line back to executable smoke planning only after the message-path rules
are verified.

## Hard Dependencies

- `docs/phase-Yb/sprint-Y9.md` must be complete first

## Governing Requirements

- `docs/phase-Yb/plan-phase-Yb.md`
- `docs/phase-Yb/lintable-boundary-plan.md`
- `docs/phase-Yb/qa-handoff.md`
- `docs/phase-Yb/testing-and-validation.md`
- `docs/adr/ADR-013-unified-delivery-plan-and-state-machine-ownership.md`

## Exact Code And Document Targets

- `boundaries/atm-core/inbox-export.toml`
- `boundaries/atm-daemon/daemon-inbox-export.toml`
- `docs/phase-Yb/removal-ledger.md`
- `docs/phase-Yb/lintable-boundary-plan.md`
- `docs/phase-Yb/qa-handoff.md`
- `docs/phase-Yb/testing-and-validation.md`
- `docs/project-plan.md`

## Required Work

1. Land the final lint/mechanical boundary allowlists.
2. Verify the removal ledger is fully closed.
3. Confirm no policy remains outside the machines and shared executors.
4. Prepare the handoff back to smoke/dogfood planning.
5. Produce the final Yb closure note for smoke-planning handoff.

## Acceptance Criteria

- only approved coordinator/executor modules can call delivery/write primitives
- removal-ledger targets are either closed or explicitly tracked as blockers
- the sprint closes ledger rows:
  - `YB-RM-017`
  - `YB-RM-018`
  - `YB-RM-020` through `YB-RM-025`
- `python3 .just/run_lint.py all` enforces the final boundary allowlists
- final smoke handoff docs name no open Yb path-consolidation blocker that is
  not explicitly tracked
- Yb can hand control back to the smoke/dogfood line without re-opening the
  same path-consolidation issues

## Required Document Updates

- `docs/phase-Yb/removal-ledger.md`
- `docs/phase-Yb/lintable-boundary-plan.md`
- `docs/phase-Yb/qa-handoff.md`
- `docs/phase-Yb/testing-and-validation.md`
- `docs/project-plan.md`

## Required Validation

```bash
cargo fmt --all --check
python3 .just/run_lint.py all
cargo build --workspace
cargo test --workspace
git diff --check
```

## Validation Record

- branch closeout validated with:
  - `cargo fmt --all --check`
  - `python3 .just/run_lint.py all`
  - `cargo build --workspace`
  - `cargo test --workspace`
  - `git diff --check`
- post-sprint review reopened `YB-RM-029` and `YB-RM-030`; those are tracked
  explicitly in `Y.11` rather than being silently ignored at `Y.10` closeout
