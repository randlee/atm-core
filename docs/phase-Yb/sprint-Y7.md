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

## Hard Dependencies

- none; this sprint establishes the first Yb implementation seam

## Governing Requirements

- `docs/phase-Yb/plan-phase-Yb.md`
- `docs/phase-Yb/removal-ledger.md`
- `docs/phase-Yb/message-path-call-stacks.md`
- `docs/phase-Yb/qa-handoff.md`
- `docs/phase-Yb/testing-and-validation.md`
- `docs/adr/ADR-013-unified-delivery-plan-and-state-machine-ownership.md`

## Exact Code And Document Targets

- `crates/atm-core/src/delivery_plan.rs`
- `crates/atm-core/src/delivery_policy.rs`
- `crates/atm-core/src/send/persistence.rs`
- `crates/atm-core/src/send/mod.rs`
- `crates/atm-core/src/ack/mod.rs`
- `docs/phase-Yb/message-path-call-stacks.md`
- `docs/phase-Yb/testing-and-validation.md`

## Required Work

1. Introduce the uniform delivery-plan shape for new-message execution in
   `crates/atm-core/src/delivery_plan.rs`:
   - `atm_core::delivery_plan::LogicalMessage`
   - `atm_core::delivery_plan::DeliveryTarget`
   - `atm_core::delivery_plan::NotificationTarget`
   - `atm_core::delivery_plan::DeliveryPlanDisposition`
   - `atm_core::delivery_plan::DeliveryPlan`
2. Introduce `atm_core::delivery_plan::ReplyDeliveryPlan` in the same module
   for ack-reply execution.
3. Ensure both harness families produce the same logical payload set:
   - success -> original message only
   - SQLite failure -> original message + `atm-system@<team>` error message
4. Remove partial Claude SQLite-failure outward delivery.
5. Add fault-injection tests for:
   - SQLite success + Claude success
   - SQLite success + Claude append degradation
   - SQLite failure + Claude path
   - SQLite failure + non-Claude path
   - companion-error failure handling
6. Prove payload equality, not just hook invocation count.
7. Route both harness families through one outer execution API:
   - `atm_core::delivery_execution::execute_delivery_plan(...)`
   - `atm_core::delivery_execution::execute_reply_delivery_plan(...)`

## Acceptance Criteria

- `crates/atm-core/src/delivery_plan.rs` defines `DeliveryPlan` and
  `ReplyDeliveryPlan`
- `rg -n "allows_claude_jsonl_append|append_compat_inbox_message"
  crates/atm-core/src/send/persistence.rs` does not show direct outward
  delivery branching in the persistence layer
- named tests prove:
  - SQLite success + Claude success
  - SQLite success + Claude append degradation
  - SQLite failure + Claude path
  - SQLite failure + non-Claude path
  - companion-error failure handling
- payload assertions prove identical logical message count, ordering, and
  content across harness families
- no metadata-only hook behavior is accepted as proof of non-Claude delivery

## Required Document Updates

- `docs/phase-Yb/message-path-call-stacks.md`
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
