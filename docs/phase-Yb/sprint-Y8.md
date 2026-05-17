---
id: Y.8
title: Policy Cleanup And Impossible-Path Removal
status: planned
branch: feature/pYb-s8-policy-cleanup-and-impossible-path-removal
worktree: ../atm-core-worktrees/feature/pYb-s8-policy-cleanup-and-impossible-path-removal
target: integrate/phase-Yb
---

# Sprint Y.8 — Policy Cleanup And Impossible-Path Removal

## Goal

Delete harness-policy leakage outside the machines and remove impossible
transition surfaces from the runtime.

## Hard Dependencies

- `docs/phase-Yb/sprint-Y7.md` must be complete first

## Governing Requirements

- `docs/phase-Yb/plan-phase-Yb.md`
- `docs/phase-Yb/removal-ledger.md`
- `docs/phase-Yb/message-path-call-stacks.md`
- `docs/phase-Yb/lintable-boundary-plan.md`
- `docs/phase-Yb/qa-handoff.md`
- `docs/phase-Yb/testing-and-validation.md`
- `docs/adr/ADR-013-unified-delivery-plan-and-state-machine-ownership.md`

## Exact Code And Document Targets

- `crates/atm-core/src/delivery_policy.rs`
- `crates/atm-core/src/send/mod.rs`
- `crates/atm-core/src/send/persistence.rs`
- `crates/atm-core/src/ack/mod.rs`
- `crates/atm-core/src/service_runtime.rs`
- `docs/phase-Yb/removal-ledger.md`
- `docs/phase-Yb/lintable-boundary-plan.md`

## Required Work

1. Delete or move every removal target marked for `Y.8`.
2. Remove fail-open or misleading Claude-specific or generic outer routing
   helpers only.
3. Move transition emission and degradation translation into the
   coordinator/machine execution layer.
4. Introduce fail-closed behavior for unsupported routing or deferred-machine
   requests.
5. Land documented lintable boundaries for illegal direct callers.
6. Limit Y.8 to Claude-specific harness branching cleanup and generic outer
   policy deletion; non-Claude fallback-surface deletion defers to Y.9.
7. Remove or move every removal-ledger target assigned to Y.8 before closing
   the sprint.

## Acceptance Criteria

- `rg -n "DeliveryHarnessPath|allows_claude_jsonl_append"
  crates/atm-core/src/send/mod.rs crates/atm-core/src/send/persistence.rs
  crates/atm-core/src/ack/mod.rs` returns no harness-policy branches outside
  approved machine/executor modules
- the sprint closes ledger rows:
  - `YB-RM-006`
  - `YB-RM-007`
  - `YB-RM-009`
  - `YB-RM-010`
  - `YB-RM-011`
- impossible transition surfaces are deleted, not merely ignored
- unsupported requests fail closed with typed errors and named tests
- `docs/phase-Yb/removal-ledger.md` marks all Y.8 targets closed, moved, or
  blocked explicitly
- lint/boundary documentation identifies the only approved callers of
  low-level delivery primitives and the enforcement point for each rule

## Required Document Updates

- `docs/phase-Yb/removal-ledger.md`
- `docs/phase-Yb/message-path-call-stacks.md`
- `docs/phase-Yb/lintable-boundary-plan.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/boundaries.md`

## Required Validation

```bash
cargo fmt --all --check
python3 .just/run_lint.py all
cargo build --workspace
cargo test --workspace
git diff --check
```
