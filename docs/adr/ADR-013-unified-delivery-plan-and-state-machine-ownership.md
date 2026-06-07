# ADR-013: Unified Delivery Plan And State-Machine-Owned Path Decisions

## Status

Accepted

Accepted note: Implementation is complete through `Phase Yc Y.13`
(`feature/pYc-s13-notification-boundary-and-readiness-gate`).

Closeout note:
- `Y.12` closed the recovered Claude logical-message-set contract
- `Y.13` closed the production `NotificationSink` boundary bypass and narrowed
  retained-runtime assembly to one approved delivery-boundary constructor
- `Y.15` re-proved the production `NotificationSink` boundary invariant on the
  final accepted `Phase Y` candidate line at `ad61b3dd`, preserving
  `NotificationSink::deliver(...)` as the only approved notification side
  effect on the live send/ack path

## Context

The accepted `integrate/phase-Y` implementation still leaves message-path
policy in the wrong places:

- persistence helpers branch on harness-specific outward delivery behavior
- outer send/ack flows translate persistence outcomes into notification and
  transition behavior
- non-Claude degraded delivery is represented by metadata-only hook activity
  rather than a first-class outbound payload boundary
- low-level Claude append helpers still perform harness gating internally

This violates the intended architectural rule that state machines own policy
and outer layers execute a shared plan.

## Decision

ATM will adopt a uniform delivery-plan contract and prohibit message-path
policy outside the state machines.

### 1. Identical logical payload contract across harness families

- success:
  - `message[1] = original message`
- SQLite failure:
  - `message[1] = original message`
  - `message[2] = atm-system@<team>` error message
- Claude and non-Claude paths may differ only in delivery target and transport
  executor
- message count, ordering, and content must be identical

### 2. State machines own delivery decisions

- harness selection happens at the coordinator plus event-family state-machine
  layer
- persistence helpers may not branch on harness or emit outward messages
- outer send/ack callers may not branch on harness or degraded-delivery cases

### 3. Shared execution interface

The state machine boundary emits a uniform plan shape.

Exact module and type ownership:

- `crates/atm-core/src/delivery_plan.rs`
  - `atm_core::delivery_plan::LogicalMessage`
  - `atm_core::delivery_plan::DeliveryTarget`
  - `atm_core::delivery_plan::NotificationTarget`
  - `atm_core::delivery_plan::DeliveryPlanDisposition`
  - `atm_core::delivery_plan::DeliveryPlan`
- `crates/atm-core/src/delivery_execution.rs`
  - `atm_core::delivery_execution::execute_delivery_plan(...)`
  - `atm_core::delivery_execution::execute_reply_delivery_plan(...)`

Required plan contents:

- logical messages
- delivery targets
- notification targets
- typed degradation or failure disposition

Shared executors consume that plan:

- Claude inbox writer
- non-Claude outbound delivery writer
- post-send notification executor

Required boundary ownership:

- Claude delivery uses `atm_core::boundary::InboxExport`
- non-Claude delivery uses `atm_core::boundary::NonClaudeOutbound`
- notification side effects use `atm_core::boundary::NotificationSink`

### 4. Notification is not delivery

- post-send-hook execution remains a notification boundary only
- post-send-hook invocation count is not evidence of message delivery
- non-Claude delivery must use a real payload boundary

### 5. Boundary enforcement

Only approved coordinator/executor modules may call low-level delivery/write
primitives.

The documented lintable boundary plan must forbid direct calls from:

- `send/mod.rs`
- `send/persistence.rs`
- `ack/mod.rs`
- other generic outer orchestration modules

## Consequences

### Positive

- message semantics become auditable and identical across harness families
- degraded-delivery behavior can be tested by payload, not by inference
- low-level append/rewrite helpers become proper executors, not hidden policy
  surfaces

### Negative

- Yb must remove or redesign several current convenience helpers
- the non-Claude outbound boundary must become explicit before smoke work can
  resume

## Required Follow-On

- [`docs/plans/phase-Yb/removal-ledger.md`](../plans/phase-Yb/removal-ledger.md)
- [`docs/plans/phase-Yb/message-path-call-stacks.md`](../plans/phase-Yb/message-path-call-stacks.md)
- [`docs/plans/phase-Yb/lintable-boundary-plan.md`](../plans/phase-Yb/lintable-boundary-plan.md)
- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/boundaries.md`

Historical note:
- later implementation sequencing ran through Phase Yb/Yc sprint artifacts, but
  those sprint and phase-plan docs record execution order only and are not part
  of this ADR's rationale or durable follow-on contract
