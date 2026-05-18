# Phase Yb Plan

## Goal

Plan the message-path consolidation follow-on needed after `integrate/phase-Y`
so the delivery contract, state-machine ownership, removal targets, and
boundary enforcement are fixed before any further implementation begins.

This is a planning-only phase on a worktree off `develop`. It does not start
implementation.

Historical follow-on note:

- `Yb` closed the planned path-consolidation line, but a later
  `integrate/phase-Y` production-readiness review reopened two final blockers:
  - Claude recovered degraded delivery still allowed partial logical-message-set
    success
  - production notification execution still bypassed `NotificationSink`
- those blockers now live in the dedicated `Phase Yc` follow-on plan:
  [../phase-Yc/plan-phase-Yc.md](../phase-Yc/plan-phase-Yc.md)

## Baseline

- planning branch: `message-path-consolidation-plan-Yb`
- planning worktree:
  `../atm-core-worktrees/message-path-consolidation-plan-Yb`
- branch base: `develop` at `292b8e38`
- implementation baseline under review:
  `integrate/phase-Y` at `b8785617`
- future integration branch for approved implementation work:
  `integrate/phase-Yb`
- `integrate/phase-Yb` must be created from `integrate/phase-Y` at
  `b8785617`, not from `develop`
- the `integrate/phase-Y` baseline at `b8785617` includes
  `crates/atm-core/src/send/persistence.rs` and the other message-path files
  targeted by the Yb removal ledger
- all sprint grep-based acceptance criteria assume that `integrate/phase-Y`
  baseline is present in `integrate/phase-Yb`

## Planning Direction

### 1. Planning is happening first, on a planning-only worktree off develop.

- This is not an implementation sprint.
- The purpose is to lock down architecture, removal targets, state-machine
  contracts, and sprint sequencing before code work starts.

### 2. The output of that worktree is a multi-sprint implementation plan for later execution.

- Plan the next implementation line as `Y.7 / Y.8 / ...`
- Do not start implementation from this planning worktree.
- The implementation sprints begin only after the plan is reviewed and
  approved.

### 3. The fixes we discussed need to be encoded as explicit implementation instructions in that plan.

## What The Plan Must Cover

### A. Delivery Contract

- `RosterHarness::ClaudeCode` and non-Claude harness paths must produce the same
  logical payload set.
- Success:
  - `message[1] = original message`
- SQLite failure:
  - `message[1] = original message`
  - `message[2] = atm-system@<team>` error message
- The only difference between
  `DeliveryHarnessPath::ClaudeCode` and `DeliveryHarnessPath::NonClaude` is
  where the messages are delivered.
- Message count, ordering, and content must be identical across harness
  families.

Planning artifacts:

- [removal-ledger.md](./removal-ledger.md)
- [message-path-call-stacks.md](./message-path-call-stacks.md)
- [ADR-013-unified-delivery-plan-and-state-machine-ownership.md](../adr/ADR-013-unified-delivery-plan-and-state-machine-ownership.md)

### B. State-Machine Boundary

- `DeliveryHarnessPath::ClaudeCode` and `DeliveryHarnessPath::NonClaude`
  state machines must expose the same interface.
- Outside the state machines, callers must not branch on harness.
- The state machine decides the plan.
- Shared executors then run that plan through the same outer call pattern.
- No policy/decision logic is allowed outside the state machines.

Required planning decision:

- the shared interface is a uniform `DeliveryPlan` / `ReplyDeliveryPlan`
  shaped output that carries:
  - logical messages
  - delivery targets
  - notification targets
  - degradation / failure disposition
- the authoritative Rust ownership is:
  - `crates/atm-core/src/delivery_plan.rs`
    - `atm_core::delivery_plan::LogicalMessage`
    - `atm_core::delivery_plan::DeliveryTarget`
    - `atm_core::delivery_plan::NotificationTarget`
    - `atm_core::delivery_plan::DeliveryPlanDisposition`
    - `atm_core::delivery_plan::DeliveryPlan`
    - `atm_core::delivery_plan::ReplyDeliveryPlan`
  - `crates/atm-core/src/delivery_execution.rs`
    - `atm_core::delivery_execution::execute_delivery_plan(...)`
    - `atm_core::delivery_execution::execute_reply_delivery_plan(...)`
- `DeliveryPlan` is the only approved new-message output shape from the
  coordinator/machine seam
- `ReplyDeliveryPlan` is the only approved ack-reply output shape from the
  coordinator/machine seam

### C. Central Decision Layer

- There should be a central delivery-policy layer for event routing.
- It must not become a god object.
- Use separate state machines per event family.
- At minimum:
  - `NewMessageStateMachine`
  - `ThreadUpdateStateMachine`
- Likely also:
  - `AckReplyStateMachine`
  - `InboxRepairStateMachine`
  - `RestoreInboxRebuildStateMachine`

Required planning decision:

- the coordinator dispatches by:
  - event family
  - canonical roster `harness` via `RosterHarness`
- but the machines own legality, payload construction, and failure contracts
- explicit introduction ownership:
  - `AckReplyStateMachine` lands in `Y.7`
  - `InboxRepairStateMachine` and `RestoreInboxRebuildStateMachine` remain
    Phase `Y` artifacts unless a later Yb sprint needs them reopened

### D. Shared Execution Model

- State machine output should be a uniform delivery plan.
- That plan should drive shared execution modules, not harness-specific outer
  call graphs.
- Example executors:
  - Claude inbox writer
  - non-Claude outbound delivery writer
  - post-send notification executor

Required planning decision:

- persistence helpers may return persisted tokens and typed failures only
- they may not directly emit Claude/non-Claude outward delivery
- post-send-hook execution remains notification-only and must not stand in for
  message delivery semantics
- the shared executors and their owning seams are:
  - `DeliveryHarnessPath::ClaudeCode` payload delivery:
    - `atm_core::boundary::InboxExport`
    - executor module:
      `atm_core::delivery_execution::execute_delivery_plan(...)`
    - executor type introduced in `Y.7`:
      `atm_core::delivery_execution::ClaudeInboxWriter`
  - `DeliveryHarnessPath::NonClaude` payload delivery:
    - `atm_core::boundary::NonClaudeOutbound`
    - daemon adapter:
      `atm_daemon::non_claude_outbound_runtime::DaemonNonClaudeOutbound`
    - executor type introduced in `Y.9`:
      `atm_core::delivery_execution::NonClaudeOutboundDeliveryWriter`
  - notification side effects:
    - `atm_core::boundary::NotificationSink`
    - daemon adapter:
      `atm_daemon::notification_runtime::DaemonNotificationSink`
    - executor type introduced in `Y.7`:
      `atm_core::delivery_execution::PostSendNotificationExecutor`

### E. Boundary Tightening

- Remove direct/historical logic where command or persistence code makes
  harness-specific delivery decisions.
- No `"if DeliveryHarnessPath::ClaudeCode do X, else do Y"` outside the state
  machines.
- No `"two hook invocations imply two delivered messages"` behavior.
- Non-Claude needs a real outbound payload boundary, not metadata-only
  stand-ins.

Required planning artifacts:

- [removal-ledger.md](./removal-ledger.md)
- [lintable-boundary-plan.md](./lintable-boundary-plan.md)
- [qa-handoff.md](./qa-handoff.md)
- [testing-and-validation.md](./testing-and-validation.md)

### F. Locking / Concurrency

- Minimize locks.
- Do not introduce a broad lock hierarchy.
- Preferred model:
  - short-lived roster snapshot
  - SQLite as durable mutation boundary
  - compatibility / notification side effects afterward
- Event timing around add/remove races is not a reason to widen locking.

Required planning decision:

- Yb implementation must not normalize:
  - roster lock -> SQLite -> mailbox lock ordering
  - mailbox/workflow lock as message-truth correctness
- Yb implementation should converge on:
  - roster snapshot
  - state-machine decision
  - SQLite durability
  - shared executors

### G. Planning Artifacts Required

- Exact removal ledger:
  - file
  - line
  - function/method
  - keep / delete / move
  - replacement path
- Full call-stack tracing for current message delivery paths.
- Any newly discovered path during planning is a planning miss and must be
  recorded immediately.

Required planning artifacts:

- [removal-ledger.md](./removal-ledger.md)
- [message-path-call-stacks.md](./message-path-call-stacks.md)
- [lintable-boundary-plan.md](./lintable-boundary-plan.md)
- [ADR-013-unified-delivery-plan-and-state-machine-ownership.md](../adr/ADR-013-unified-delivery-plan-and-state-machine-ownership.md)
- [hardening-audit.md](./hardening-audit.md)

## Current Structural Issues

The accepted `integrate/phase-Y` implementation is close but still leaves the
message-path contract underspecified or implemented in the wrong layers:

1. non-Claude SQLite-failure handling does not prove real original+error
   payload delivery; it proves metadata-only hook activity instead
2. Claude SQLite-failure handling can partially append the original message and
   then fail before the companion error is delivered
3. state-machine ownership is incomplete because outer layers still branch on
   persistence disposition and compatibility append semantics
4. notification hooks still act as an implied delivery mechanism for non-Claude
   degraded paths
5. repair/reexport write helpers are still close enough to runtime write
   helpers that the final keep/delete/move contract must be restated before
   more implementation begins

## Recommended Sprint Shape

- `Y.7`: planning-approved degraded-delivery contract hardening
- `Y.8`: policy cleanup and impossible-path removal
- `Y.9`: `DeliveryHarnessPath::NonClaude` outbound boundary formalization
- `Y.10`: boundary enforcement, validation closure, and smoke-handoff
  preparation
- `Y.11`: post-`Y.10` boundary gap closure for the two remaining mixed-seam
  runtime entrypoints and the shared validation-doc drift
- Later sprints as needed for migration, validation, and smoke/dogfood

## Sprint Sequence

### Y.7 Degraded Delivery Contract Hardening

Purpose:

- make the logical message contract exact for both harness families, including
  `RosterHarness::ClaudeCode`
- eliminate partial Claude SQLite-failure delivery
- force proof of payload equivalence for success and SQLite-failure branches

Authoritative sprint doc:

- [sprint-Y7.md](./sprint-Y7.md)

Hard dependency:

- none; this is the first implementation sprint on `integrate/phase-Yb`

### Y.8 Policy Cleanup And Impossible-Path Removal

Purpose:

- delete harness-policy leakage outside the machines
- remove impossible transition surfaces
- fail closed when unsupported routing or fallback requests are attempted
- this sprint does not delete non-Claude fallback surfaces; those defer to
  `Y.9` so the new non-Claude boundary and old non-Claude routing removals land
  atomically

Authoritative sprint doc:

- [sprint-Y8.md](./sprint-Y8.md)

Hard dependency:

- [sprint-Y7.md](./sprint-Y7.md) must close first because Y.8 deletes or moves
  paths that Y.7 replaces with the uniform delivery-plan seam

### Y.9 Non-Claude Outbound Boundary Formalization

Purpose:

- introduce a dedicated `DeliveryHarnessPath::NonClaude` outbound payload
  boundary
- stop treating metadata-only post-send-hook execution as message delivery
- make Claude and non-Claude paths use the same outer executor contract
- delete the retained non-Claude fallback surfaces only after the new boundary
  exists

Authoritative sprint doc:

- [sprint-Y9.md](./sprint-Y9.md)

Hard dependency:

- [sprint-Y8.md](./sprint-Y8.md) must close first because Y.9 formalizes the
  retained non-Claude boundary only after the outer policy leakage is removed

### Y.10 Boundary Enforcement And Smoke Handoff

Purpose:

- land lintable / documented boundary enforcement
- close the Yb implementation line with explicit validation evidence
- hand the line back to smoke/dogfood planning only after Yb closes

Authoritative sprint doc:

- [sprint-Y10.md](./sprint-Y10.md)

Hard dependency:

- [sprint-Y9.md](./sprint-Y9.md) must close first because Y.10 enforces the
  final caller allowlists only after the non-Claude boundary exists

### Y.11 Post-Y.10 Boundary Gap Closure

Purpose:

- remove the last two mixed-seam runtime entrypoints discovered by the
  sprint-plan-to-implementation review
- make the repair/rebuild rewrite seam explicit by construction rather than by
  internal harness guards
- sync the shared validation docs to the actual outbound-boundary proof model
- re-close the Yb implementation line only after those review findings are
  resolved

Authoritative sprint doc:

- [sprint-Y11.md](./sprint-Y11.md)

Hard dependency:

- [sprint-Y10.md](./sprint-Y10.md) must close first because Y.11 is a focused
  follow-up on the remaining Y.10/Y.9 seam mismatches rather than a new
  independent implementation line

## Planning Outputs

- [plan-phase-Yb.md](./plan-phase-Yb.md)
- [sprint-Y7.md](./sprint-Y7.md)
- [sprint-Y8.md](./sprint-Y8.md)
- [sprint-Y9.md](./sprint-Y9.md)
- [sprint-Y10.md](./sprint-Y10.md)
- [sprint-Y11.md](./sprint-Y11.md)
- [removal-ledger.md](./removal-ledger.md)
- [message-path-call-stacks.md](./message-path-call-stacks.md)
- [lintable-boundary-plan.md](./lintable-boundary-plan.md)
- [hardening-audit.md](./hardening-audit.md)
- [qa-handoff.md](./qa-handoff.md)
- [testing-and-validation.md](./testing-and-validation.md)
- [ADR-013-unified-delivery-plan-and-state-machine-ownership.md](../adr/ADR-013-unified-delivery-plan-and-state-machine-ownership.md)

## Phase Rules

- this planning branch must not implement Rust changes
- every Yb sprint must cite this plan as authoritative scope
- every Yb sprint must cite the removal ledger and call-stack audit where
  relevant
- any newly discovered runtime path that violates the Yb contract is a
  blocking planning miss until documented
- smoke/dogfood work must not resume until the Yb implementation line closes,
  including any post-review closure sprint required to remove reopened seam
  issues
