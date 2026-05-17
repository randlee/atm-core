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
- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/boundaries.md`

## Exact Code And Document Targets

- `crates/atm-core/src/delivery_plan.rs`
- `crates/atm-core/src/delivery_policy.rs`
- `crates/atm-core/src/delivery_execution.rs`
- `crates/atm-core/src/send/persistence.rs`
- `crates/atm-core/src/send/mod.rs`
- `crates/atm-core/src/ack/mod.rs`
- `docs/phase-Yb/message-path-call-stacks.md`
- `docs/phase-Yb/testing-and-validation.md`
- `docs/phase-Yb/removal-ledger.md`

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
3. Introduce in `crates/atm-core/src/delivery_execution.rs`:
   - `atm_core::delivery_execution::ClaudeInboxWriter`
   - `atm_core::delivery_execution::PostSendNotificationExecutor`
4. Introduce `AckReplyStateMachine` for reply execution in the same Y.7
   implementation seam as `ReplyDeliveryPlan`.
5. Ensure both harness families produce the same logical payload set:
   - success -> original message only
   - SQLite failure -> original message + `atm-system@<team>` error message
6. Remove partial Claude SQLite-failure outward delivery.
7. Add fault-injection tests for:
   - SQLite success + Claude success
   - SQLite success + Claude append degradation
   - SQLite failure + Claude path
   - SQLite failure + non-Claude path
   - companion-error failure handling
8. Prove payload equality, not just hook invocation count.
9. Route both harness families through one outer execution API:
   - `atm_core::delivery_execution::execute_delivery_plan(...)`
   - `atm_core::delivery_execution::execute_reply_delivery_plan(...)`
10. Y.7 may leave temporary dual-path outer call scaffolding in
    `send/mod.rs` and `ack/mod.rs`, but the degraded-delivery contract itself
    must already be owned by the typed plan seam before Y.8 starts.

## Architecture Review Focus

`arch-qa` must review Y.7 as an architecture-seam sprint, not just a behavior
change sprint.

The implementation must be rejected if any of these remain true after Y.7:

1. `send/persistence.rs` still decides harness routing, append ordering,
   fallback choice, or notification strategy instead of emitting typed results
   into the `DeliveryPlan` / `ReplyDeliveryPlan` seam.
2. `send/mod.rs` or `ack/mod.rs` still branches on harness or degraded-delivery
   outcomes after the state machine has emitted the plan.
3. Claude and non-Claude paths do not expose the same outer execution
   interface, even if end behavior looks correct.
4. Non-Claude degraded delivery is still represented by notification metadata,
   hook count, or implied behavior instead of first-class logical messages in
   the plan.
5. `delivery_execution.rs` becomes a second policy layer that constructs or
   rewrites message semantics instead of executing state-machine-owned plans.
6. The Claude SQLite-failure path still allows "original delivered, companion
   error silently absent" as an untyped partial-success branch.
7. Ack reply continues to use a separate outer call graph instead of the same
   typed reply-plan seam.

`arch-qa` should prefer a slightly more explicit typed seam over a "clever"
implementation that preserves convenience helpers with hidden policy.

## Acceptance Criteria

- `crates/atm-core/src/delivery_plan.rs` defines `DeliveryPlan` and
  `ReplyDeliveryPlan`
- `crates/atm-core/src/delivery_execution.rs` defines `ClaudeInboxWriter` and
  `PostSendNotificationExecutor`
- `AckReplyStateMachine` is introduced in Y.7 and wired to `ReplyDeliveryPlan`
- the sprint closes ledger rows:
  - `YB-RM-001` through `YB-RM-005`
  - `YB-RM-008`
  - `YB-RM-026` through `YB-RM-028`
- `rg -n "allows_claude_jsonl_append|append_compat_inbox_message"
  crates/atm-core/src/send/persistence.rs` does not show direct outward
  delivery branching in the persistence layer
- `rg -n "allows_claude_jsonl_append|DeliveryHarnessPath"
  crates/atm-core/src/send/mod.rs crates/atm-core/src/ack/mod.rs` does not
  show harness-policy branching after the state-machine seam
- `rg -n "run_send_post_send_hooks|collect_ack_hook_warnings"
  crates/atm-core/src/send/persistence.rs` does not show notification logic
  standing in for message delivery in the persistence layer
- named tests prove:
  - SQLite success + Claude success
  - SQLite success + Claude append degradation
  - SQLite failure + Claude path
  - SQLite failure + non-Claude path
  - companion-error failure handling
- payload assertions prove identical logical message count, ordering, and
  content across harness families
- no metadata-only hook behavior is accepted as proof of non-Claude delivery
- `arch-qa` can point to one typed ownership seam for:
  - payload construction in state machines
  - payload execution in shared executors
  - notification execution as a side-effect-only layer
- any remaining outer scaffolding is transitional only and does not own
  degraded-delivery semantics, payload construction, or harness-specific
  branching

## Required Document Updates

- `docs/phase-Yb/message-path-call-stacks.md`
- `docs/phase-Yb/testing-and-validation.md`
- `docs/phase-Yb/removal-ledger.md`

## Required Validation

```bash
cargo fmt --all --check
python3 .just/run_lint.py all
cargo build --workspace
cargo test --workspace
git diff --check
```
