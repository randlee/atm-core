---
id: Y.11
title: Post-Y.10 Boundary Gap Closure
status: complete
branch: feature/pYb-s11-y10-gap-closure
worktree: ../atm-core-worktrees/feature/pYb-s11-y10-gap-closure
target: integrate/phase-Yb
---

# Sprint Y.11 — Post-Y.10 Boundary Gap Closure

## Goal

Close the post-`Y.10` review findings by removing the last mixed-seam runtime
entrypoints and syncing the final validation docs to the actual hardened
message-path contract.

## Hard Dependencies

- `docs/plans/phase-Yb/sprint-Y10.md` must be complete first
- the `Y.10` implementation review findings are authoritative for this sprint:
  - low-level Claude append seam still receives `DeliveryHarnessPath::NonClaude`
    traffic and rejects it internally
  - repair/rebuild refresh seam still accepts a general recipient snapshot and
    silently no-ops for non-Claude requests
  - shared validation docs still name the pre-`Y.9` hook-path proof instead of
    the final outbound-boundary proof

## Governing Requirements

- `docs/plans/phase-Yb/plan-phase-Yb.md`
- `docs/plans/phase-Yb/removal-ledger.md`
- `docs/plans/phase-Yb/lintable-boundary-plan.md`
- `docs/plans/phase-Yb/qa-handoff.md`
- `docs/plans/phase-Yb/testing-and-validation.md`
- `docs/adr/ADR-013-unified-delivery-plan-and-state-machine-ownership.md`

## Exact Code And Document Targets

- `crates/atm-core/src/service_runtime.rs`
- `crates/atm-core/src/delivery_execution.rs`
- `crates/atm-core/src/send/mod.rs`
- `docs/plans/phase-Yb/removal-ledger.md`
- `docs/plans/phase-Yb/lintable-boundary-plan.md`
- `docs/plans/phase-Yb/qa-handoff.md`
- `docs/plans/phase-Yb/testing-and-validation.md`
- `docs/plans/phase-Yb/plan-phase-Yb.md`
- `docs/project-plan.md`

## Required Work

1. Delete the retained non-Claude rejection branch from
   `RetainedServiceRuntime::append_compat_inbox_message(...)` so the low-level
   Claude append seam is never selected for `DeliveryHarnessPath::NonClaude`.
2. Narrow or replace `RetainedServiceRuntime::rebuild_compat_inbox_projection(...)`
   so the repair/rebuild seam is explicit by construction and no longer accepts
   a general `DeliveryRecipientSnapshot` plus a non-Claude no-op branch.
3. Keep the low-level Claude append primitive executor-only and the rewrite
   seam repair/rebuild-only, but make those seam contracts explicit in the
   runtime trait shape and callers rather than in internal harness guards.
4. Update named validation and QA docs so they refer to outbound-boundary proof
   instead of the obsolete hook-path wording.
5. Restate final Yb closure only after the reopened seam issues are actually
   removed and the shared docs match the implemented boundary model.

## Acceptance Criteria

- `rg -n "append_compat_inbox_message is unsupported for non-Claude"
  crates/atm-core/src` returns no matches
- `rg -n "if !recipient\\.allows_claude_jsonl_append\\(\\)"
  crates/atm-core/src/service_runtime.rs` returns no mixed-seam harness gating
  in the retained append/refresh helpers
- the sprint closes ledger rows:
  - `YB-RM-029`
  - `YB-RM-030`
- low-level Claude append remains reachable only through
  `atm_core::delivery_execution::ClaudeInboxWriter`
- repair/rebuild rewrite remains reachable only through the explicit
  repair/rebuild seam and not through a generic recipient-routed runtime helper
- `docs/plans/phase-Yb/testing-and-validation.md` no longer cites the obsolete
  hook-path non-Claude proof name
- final Yb closeout docs do not claim the line is complete until these reopened
  issues are closed

## Required Document Updates

- `docs/plans/phase-Yb/removal-ledger.md`
- `docs/plans/phase-Yb/lintable-boundary-plan.md`
- `docs/plans/phase-Yb/qa-handoff.md`
- `docs/plans/phase-Yb/testing-and-validation.md`
- `docs/plans/phase-Yb/plan-phase-Yb.md`
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
