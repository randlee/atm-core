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

Make the degraded-delivery contract exact and symmetric across
`RosterHarness::ClaudeCode` and non-Claude harnesses.

## Hard Dependencies

- `integrate/phase-Yb` must be branched from `integrate/phase-Y` at
  `b8785617`, not from `develop`
- `crates/atm-core/src/send/persistence.rs` and the other message-path files
  cited by the Yb removal ledger exist only on that `integrate/phase-Y`
  baseline
- this sprint establishes the first Yb implementation seam on top of that
  imported baseline

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
5. Ensure both `DeliveryHarnessPath::ClaudeCode` and
   `DeliveryHarnessPath::NonClaude` produce the same logical payload set:
   - success -> original message only
   - SQLite failure -> original message + `atm-system@<team>` error message
6. Remove partial Claude SQLite-failure outward delivery.
7. Add fault-injection tests for:
   - SQLite success + `DeliveryHarnessPath::ClaudeCode` success
   - SQLite success + `DeliveryHarnessPath::ClaudeCode` append degradation
   - SQLite failure + `DeliveryHarnessPath::ClaudeCode` path
   - SQLite failure + `DeliveryHarnessPath::NonClaude` path
   - companion-error failure handling
8. Prove payload equality, not just hook invocation count.
9. Route both harness families through one outer execution API:
   - `atm_core::delivery_execution::execute_delivery_plan(...)`
   - `atm_core::delivery_execution::execute_reply_delivery_plan(...)`
10. Y.7 may leave temporary dual-path outer call scaffolding in
    `send/mod.rs` and `ack/mod.rs`, but the degraded-delivery contract itself
    must already be owned by the typed plan seam before Y.8 starts.

## Early Baseline Inconsistencies To Remove

The `integrate/phase-Y @ b8785617` baseline already uses harness enums for
routing, but these specific code surfaces still encode ambiguous or misplaced
policy and must be addressed at the start of Y.7:

1. [delivery_policy.rs](</Users/randlee/Documents/github/atm-core-worktrees/integrate/phase-Y/crates/atm-core/src/delivery_policy.rs:61>)
   - `DeliveryRecipientSnapshot::fallback_claude`
   - inconsistency: the name and behavior silently default an unresolved route
     to `DeliveryHarnessPath::ClaudeCode`
2. [send/persistence.rs](</Users/randlee/Documents/github/atm-core-worktrees/integrate/phase-Y/crates/atm-core/src/send/persistence.rs:85>)
   - `recipient.allows_claude_jsonl_append()` branch inside
     `recover_after_sqlite_failure(...)`
   - inconsistency: persistence still decides a harness-specific outward path
3. [service_runtime.rs](</Users/randlee/Documents/github/atm-core-worktrees/integrate/phase-Y/crates/atm-core/src/service_runtime.rs:181>)
   - early return in `refresh_compat_inbox_projection(...)`
   - inconsistency: the Claude writer is still asked to inspect a non-Claude
     request and no-op
4. [service_runtime.rs](</Users/randlee/Documents/github/atm-core-worktrees/integrate/phase-Y/crates/atm-core/src/service_runtime.rs:202>)
   - early return in `append_compat_inbox_message(...)`
   - inconsistency: the low-level Claude append seam still receives
     `DeliveryHarnessPath::NonClaude` traffic instead of never being selected

No model-based routing inconsistency was found in the `integrate/phase-Y`
runtime path review. The ambiguity is currently in harness fallback naming and
misplaced outer-path/low-level branching, not in `model` field usage.

## Architecture Review Focus

`arch-qa` must review Y.7 as an architecture-seam sprint, not just a behavior
change sprint.

The implementation must be rejected if any of these remain true after Y.7:

1. `send/persistence.rs` still decides harness routing, append ordering,
   fallback choice, or notification strategy instead of emitting typed results
   into the `DeliveryPlan` / `ReplyDeliveryPlan` seam.
2. `send/mod.rs` or `ack/mod.rs` still branches on harness or degraded-delivery
   outcomes after the state machine has emitted the plan.
   Condition 2 applies to branching on machine-emitted plan output
   post-seam. Temporary scaffolding that routes requests into the machines
   pre-seam, as permitted by Required Work item 10, is not by itself a
   Condition 2 violation.
3. `DeliveryHarnessPath::ClaudeCode` and `DeliveryHarnessPath::NonClaude`
   paths do not expose the same shared dispatch API shape, even if end behavior
   looks correct.
   The `NonClaudeOutboundDeliveryWriter` transport executor is a Y.9
   deliverable. Its absence at Y.7 is not a rejection condition. Condition 3
   applies to the `execute_delivery_plan(...)` /
   `execute_reply_delivery_plan(...)` seam plus the typed
   `DeliveryPlan` / `ReplyDeliveryPlan` contract.
4. `DeliveryHarnessPath::NonClaude` degraded delivery is still represented by
   notification metadata, hook count, or implied behavior instead of
   first-class logical messages in the plan.
   Real transport proof for `DeliveryHarnessPath::NonClaude` defers to Y.9
   when `NonClaudeOutboundDeliveryWriter` lands. Y.7 must still prove that the
   typed plan carries those logical messages explicitly.
5. `delivery_execution.rs` becomes a second policy layer that constructs or
   rewrites message semantics instead of executing state-machine-owned plans.
6. The `DeliveryHarnessPath::ClaudeCode` SQLite-failure path still allows
   "original delivered, companion error silently absent" as an untyped
   partial-success branch.
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
- `rg -n "allows_claude_jsonl_append"
  crates/atm-core/src/send/persistence.rs` does not show direct outward
  delivery branching in the persistence layer
- `rg -n "DeliveryHarnessPath"
  crates/atm-core/src/send/mod.rs crates/atm-core/src/ack/mod.rs` is limited
  to transitional scaffolding allowed by Required Work item 10 and does not
  re-derive harness semantics from machine-emitted plan output
- `rg -n "collect_ack_hook_warnings"
  crates/atm-core/src/ack/mod.rs` does not show notification logic standing in
  for message delivery in the reply path
- a passing `rg` result is a necessary precondition only; `arch-qa` must also
  verify positive evidence that the state machine emits a `DeliveryPlan`, that
  `delivery_execution.rs` executes it without re-deriving harness semantics,
  and that no structurally equivalent inline branch replaces the removed
  symbols
- named tests prove:
  - SQLite success + `DeliveryHarnessPath::ClaudeCode` success
  - SQLite success + `DeliveryHarnessPath::ClaudeCode` append degradation
  - SQLite failure + `DeliveryHarnessPath::ClaudeCode` path
  - SQLite failure + `DeliveryHarnessPath::NonClaude` path
  - companion-error failure handling
- payload assertions prove identical logical message count, ordering, and
  content across `DeliveryHarnessPath::ClaudeCode` and
  `DeliveryHarnessPath::NonClaude`
- a test-double executor is acceptable for the
  `DeliveryHarnessPath::NonClaude` Y.7 proof; real transport-level payload
  proof defers to Y.9 when `NonClaudeOutboundDeliveryWriter` lands
- no metadata-only hook behavior is accepted as proof of
  `DeliveryHarnessPath::NonClaude` delivery
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
