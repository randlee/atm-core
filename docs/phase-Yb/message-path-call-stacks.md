# Phase Yb Message-Path Call Stacks

Baseline:

- planning branch: `message-path-consolidation-plan-Yb`
- implementation baseline under review: `integrate/phase-Y` at `b8785617`

This document records the current `Y.8` implementation seam after degraded
delivery contract hardening and outer-policy cleanup landed on:

- `feature/pYb-s7-degraded-delivery-contract-hardening`
- `feature/pYb-s8-policy-cleanup-and-impossible-path-removal`

## 1. Current Y.7 Stack: New Message Success, `DeliveryHarnessPath::ClaudeCode`

Current stack:

1. [crates/atm-core/src/send/mod.rs](/Users/randlee/Documents/github/atm-core-worktrees/feature/pYb-s7-degraded-delivery-contract-hardening/crates/atm-core/src/send/mod.rs:190)
   - `send_mail_with_runtime_impl(...)`
2. [crates/atm-core/src/send/mod.rs](/Users/randlee/Documents/github/atm-core-worktrees/feature/pYb-s7-degraded-delivery-contract-hardening/crates/atm-core/src/send/mod.rs:365)
   - `prepare_send_context(...)`
3. [crates/atm-core/src/send/mod.rs](/Users/randlee/Documents/github/atm-core-worktrees/feature/pYb-s7-degraded-delivery-contract-hardening/crates/atm-core/src/send/mod.rs:511)
   - `persist_send_message(...)`
4. [crates/atm-core/src/send/persistence.rs](/Users/randlee/Documents/github/atm-core-worktrees/feature/pYb-s7-degraded-delivery-contract-hardening/crates/atm-core/src/send/persistence.rs:17)
   - `persist_message_and_seed_workflow(...)`
5. `runtime.commit_workflow_state(...)`
6. [crates/atm-core/src/send/mod.rs](/Users/randlee/Documents/github/atm-core-worktrees/feature/pYb-s7-degraded-delivery-contract-hardening/crates/atm-core/src/send/mod.rs:323)
   - `build_send_delivery_plan(...)`
7. [crates/atm-core/src/delivery_execution.rs](/Users/randlee/Documents/github/atm-core-worktrees/feature/pYb-s7-degraded-delivery-contract-hardening/crates/atm-core/src/delivery_execution.rs:97)
   - `execute_delivery_plan(...)`
8. [crates/atm-core/src/delivery_execution.rs](/Users/randlee/Documents/github/atm-core-worktrees/feature/pYb-s7-degraded-delivery-contract-hardening/crates/atm-core/src/delivery_execution.rs:188)
   - `execute_claude_delivery(...)`
9. `runtime.append_compat_inbox_message(...)`
10. [crates/atm-core/src/delivery_execution.rs](/Users/randlee/Documents/github/atm-core-worktrees/feature/pYb-s7-degraded-delivery-contract-hardening/crates/atm-core/src/delivery_execution.rs:175)
    - shared notification execution

Current ownership:

- payload construction, delivery-target construction, execution, and
  transition emission now all happen inside the typed plan/execution seam
- outer send code supplies only command-local context and telemetry emission

## 2. Current Y.7 Stack: New Message Success, `DeliveryHarnessPath::NonClaude`

Current stack:

1. `send_mail_with_runtime_impl(...)`
2. `prepare_send_context(...)`
3. `persist_send_message(...)`
4. `persist_message_and_seed_workflow(...)`
5. `runtime.commit_workflow_state(...)`
6. `build_send_delivery_plan(...)`
7. `execute_delivery_plan(...)`
8. no Claude append is selected because the plan target is
   `DeliveryTarget::NonClaude`
9. shared notification execution

Remaining issue:

- the outer call graph is now shared, but real non-Claude outbound transport
  still defers to `Y.9`

## 3. Current Y.7 Stack: SQLite Failure, `DeliveryHarnessPath::ClaudeCode`

Current stack:

1. `persist_send_message(...)`
2. `persist_message_and_seed_workflow(...)`
3. `runtime.commit_workflow_state(...)` returns mailbox-write error
4. [crates/atm-core/src/send/persistence.rs](/Users/randlee/Documents/github/atm-core-worktrees/feature/pYb-s7-degraded-delivery-contract-hardening/crates/atm-core/src/send/persistence.rs:57)
   - `recover_after_sqlite_failure(...)`
5. `recover_after_sqlite_failure(...)` constructs original + companion typed
   payloads
6. `build_send_delivery_plan(...)`
7. `execute_delivery_plan(...)`
8. `execute_claude_delivery(...)`
9. original notification + companion notification via shared notification
   executor

Current ownership:

- the degraded payload contract is explicit and symmetric
- partial Claude append is an execution-level warning surface
- transition ownership now lives with the shared execution seam

## 4. Current Y.7 Stack: SQLite Failure, `DeliveryHarnessPath::NonClaude`

Current stack:

1. `persist_send_message(...)`
2. `persist_message_and_seed_workflow(...)`
3. `runtime.commit_workflow_state(...)` returns mailbox-write error
4. `recover_after_sqlite_failure(...)`
5. persistence returns original + companion typed payloads in
   `DeliveryPersistenceResult`
6. `build_send_delivery_plan(...)`
7. `execute_delivery_plan(...)`
8. no Claude append is selected
9. original notification + companion notification via shared notification
   executor

Remaining issue:

- `Y.7` proves identical logical payload sets at the typed-plan seam
- real non-Claude outward payload delivery remains a `Y.9` boundary task

## 5. Current Y.7 Stack: Append Degraded After SQLite Success

Current stack:

1. `persist_message_and_seed_workflow(...)`
2. SQLite workflow commit succeeds
3. `build_send_delivery_plan(...)`
4. `execute_delivery_plan(...)`
5. `DeliveryExecutionResult::AppendDegraded`
6. shared notification execution
7. `delivery_execution::emit_delivery_plan_transitions(...)`

Current ownership:

- execution degradation is executor-owned
- impossible non-Claude append-degraded transition requests now fail closed in
  `delivery_execution.rs`

## 6. Current Y.7 Stack: Ack Reply Delivery

Current stack:

1. [crates/atm-core/src/ack/mod.rs](/Users/randlee/Documents/github/atm-core-worktrees/feature/pYb-s7-degraded-delivery-contract-hardening/crates/atm-core/src/ack/mod.rs:368)
   - `persist_ack_reply(...)`
2. ack state persisted in SQLite
3. [crates/atm-core/src/ack/mod.rs](/Users/randlee/Documents/github/atm-core-worktrees/feature/pYb-s7-degraded-delivery-contract-hardening/crates/atm-core/src/ack/mod.rs:417)
   - `persist_message_and_seed_workflow(...)`
4. shared persistence result seam returns typed payloads
5. [crates/atm-core/src/ack/mod.rs](/Users/randlee/Documents/github/atm-core-worktrees/feature/pYb-s7-degraded-delivery-contract-hardening/crates/atm-core/src/ack/mod.rs:514)
   - `AckReplyStateMachine::from_persistence(...)`
6. [crates/atm-core/src/ack/mod.rs](/Users/randlee/Documents/github/atm-core-worktrees/feature/pYb-s7-degraded-delivery-contract-hardening/crates/atm-core/src/ack/mod.rs:545)
   - `AckReplyStateMachine::into_reply_delivery_plan(...)`
7. [crates/atm-core/src/delivery_execution.rs](/Users/randlee/Documents/github/atm-core-worktrees/feature/pYb-s7-degraded-delivery-contract-hardening/crates/atm-core/src/delivery_execution.rs:119)
   - `execute_reply_delivery_plan(...)`
8. shared notification execution

Current ownership:

- ack reply shares the typed reply-plan seam
- shared execution owns transition translation and fail-closed checks

## 7. Y.7 Closure Summary

`Y.7` replaced persistence-owned degraded outward delivery with:

- typed payload construction in `DeliveryPersistenceResult`
- `DeliveryPlan` / `ReplyDeliveryPlan`
- shared execution in `delivery_execution.rs`
- explicit `crates/atm-core/src/ack/mod.rs::AckReplyStateMachine` ownership
  for reply-plan construction

Note:

- `crates/atm-core/src/ack/mod.rs::AckReplyStateMachine` is the typed
  reply-plan constructor seam
- `crates/atm-core/src/delivery_policy.rs::AckReplyStateMachine` remains the
  documented transition inventory

`Y.8` still does not finish:

- non-Claude real outbound transport
- low-level repair/rebuild boundary cleanup

Those remaining items are intentionally deferred to `Y.9` and `Y.10`.

## 8. Required End-State

After Yb:

1. caller constructs event-family request
2. coordinator resolves canonical harness snapshot
3. event-family machine returns a uniform delivery plan
4. shared executors run:
   - Claude inbox writer only for Claude delivery targets
   - non-Claude outbound writer only for non-Claude delivery targets
   - post-send notification executor for the same logical plan
5. transition emission occurs from the same machine/executor result surface

Approved executor ownership:

- `atm_core::delivery_execution::execute_delivery_plan(...)`
- `atm_core::delivery_execution::execute_reply_delivery_plan(...)`
- `atm_core::delivery_execution::emit_delivery_plan_transitions(...)`
- `atm_core::delivery_execution::emit_reply_delivery_plan_transitions(...)`
- `atm_core::delivery_execution::ClaudeInboxWriter`
  - introduced in `Y.7`
- `atm_core::delivery_execution::PostSendNotificationExecutor`
  - introduced in `Y.7`
- `atm_core::delivery_execution::NonClaudeOutboundDeliveryWriter`
  - introduced in `Y.9`
- `atm_core::boundary::InboxExport` handles Claude delivery only
- `atm_core::boundary::NonClaudeOutbound` handles non-Claude delivery only
- `atm_core::boundary::NotificationSink` handles notification side effects only

Outside the state machines and shared executors, there should be no harness
branching and no payload-shape branching.
