---
id: AG.20
title: Move Deferred Replay Policy Out Of Transport
status: complete
execution_status: not_started  # plan doc is complete/ready-for-review; code has not landed on any feature/pAG-sN branch yet
branch: feature/pAG-s20-move-deferred-policy-out-of-transport
worktree: ../atm-core-worktrees/feature/pAG-s20-move-deferred-policy-out-of-transport
target: develop
---

# Sprint AG.20 — Move Deferred Replay Policy Out Of Transport

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.20
worktree: ../atm-core-worktrees/feature/pAG-s20-move-deferred-policy-out-of-transport
branch: feature/pAG-s20-move-deferred-policy-out-of-transport
status: complete
estimated_scope: medium
```

## Goal

Reduce the cross-host transport layer to transport only by removing deferred,
retry, replay, and semantic-outcome policy from `peer_transport/delivery.rs`.

## Hard Dependencies

- AG.19 merged

## Exact Targets

- `crates/atm-daemon/src/peer_transport.rs:133-145`
- `crates/atm-daemon/src/peer_transport.rs:216-338`
- `crates/atm-daemon/src/peer_transport.rs:341-620`
- `crates/atm-daemon/src/peer_transport.rs:625-838`
- `crates/atm-core/src/send/delivery_persistence.rs:1-38`
- `crates/atm-core/src/delivery_plan.rs:1-115`
- source findings:
  - `CROSSHOST-UNIFY-2`
  - boundary-review item 3

## Specific Deletions Required

- `crates/atm-daemon/src/peer_transport.rs:133-145`
  - delete transport-owned semantic failure taxonomy:
    `AttemptFailureKind`, `AttemptFailure`, `DeliveryRetryState`
- `crates/atm-daemon/src/peer_transport.rs:24-82`
  - delete transport-owned retry-budget policy from `PeerTransportConfig`
- `crates/atm-daemon/src/peer_transport.rs:216-338`
  - delete transport-owned retry-loop entry and replay-policy branching around
    the retained helper calls; AG.22 owns `persist_replay_request(...)` /
    persisted endpoint identity and AG.23 owns
    `persist_outcome_unknown_request(...)` / `resume_pending_replay(...)`
- `crates/atm-daemon/src/peer_transport.rs:341-620`
  - delete transport-owned retry loop and delivery-outcome shaping:
    `send_to_endpoint(...)`,
    `handle_send_failure(...)`,
    `handle_retryable_failure(...)`,
    `handle_terminal_failure(...)`,
    `DeliveryLoopDecision`
- `crates/atm-daemon/src/peer_transport.rs:625-838`
  - delete transport-owned retryability/outcome classifiers:
    `peer_read_deadline_error(...)`,
    `peer_write_deadline_error(...)`,
    `peer_flush_error(...)`,
    `peer_closed_before_response_error(...)`,
    `peer_response_decode_error(...)`,
    `peer_response_id_mismatch_error(...)`,
    `wait_for_retry_backoff(...)`,
    `classify_io_error(...)`
- `crates/atm-core/src/delivery_plan.rs:8-115`
  - delete reply/send plan reconstruction that exists only because transport
    returns persistence-shaped policy state instead of transport facts

## Logic / Branches / State That Do Not Belong

- any transport code that decides `Delivered/Deferred/OutcomeUnknown/Terminal`
- any replay budget, expiry, or terminality policy inside `peer_transport/*`
- any transport-owned receipt finalization or sender-inbox mutation policy
- any transport-owned backoff timer / retry sleep loop
- any transport-owned persistence enqueue / replay resume policy state machine
- any transport-owned mapping from raw socket or protocol facts to operator
  policy outcome
- any transport-owned retry/replay config field

## Deliverables

- transport module no longer owns retry/deferred policy
- transport module no longer owns replay persistence
- one higher-level outcome gate owns deferred / unknown / terminal result
  shaping
- explicit shared outcome-policy boundary:
  `shared_outcome_policy(attempt: TransportAttemptResult) -> OutboundDeliveryDisposition`
- peer transport reduced to:
  connect -> write request frame -> read response frame -> decode response
  frame -> return transport facts

## Required Work

- delete transport-local semantic outcome enums where possible
- replace transport result shaping with a narrower result contract
- move retry persistence policy to the higher shared outbound delivery policy
  layer without claiming AG.22/AG.23-owned helper deletion in this sprint
- remove transport-local retry/backoff loop ownership
- remove replay-store ownership from the peer transport client surface

## Explicit Code Samples

```rust
pub enum TransportAttemptResult {
    Delivered(ResponseEnvelope),
    TransportError(AtmError),
}

pub enum OutboundDeliveryDisposition {
    Delivered(ResponseEnvelope),
    Deferred(AtmError),
    OutcomeUnknown(AtmError),
    RejectedTerminal(AtmError),
}
```

## Supporting types and staged removal

- remove in AG.20 if the boundary can be collapsed in one patch:
  - `crates/atm-daemon/src/peer_transport.rs:24-82`
    - `PeerTransportConfig::remote_retry_budget`
    - move retry-budget ownership to the shared outbound policy owner
  - `crates/atm-daemon/src/peer_transport.rs:133-145`
    - `AttemptFailureKind`
    - `AttemptFailure`
    - `DeliveryRetryState`
  - `crates/atm-daemon/src/peer_transport.rs:620-623`
    - `DeliveryLoopDecision`
  - `crates/atm-daemon/src/peer_transport.rs:216-338`
    - transport-owned replay policy methods on `PeerClientTransport`
  - `crates/atm-daemon/src/peer_transport.rs:161-163`
    - `PeerClientTransport::replay_store`
    - transport should not own durable replay storage
  - `crates/atm-core/src/send/delivery_persistence.rs:4-38`
    - `DeliveryPersistenceDisposition`
    - `DeliveryPersistenceResult`
    - only if a shared higher-level delivery policy result replaces
      persistence-shaped transport feedback in the same patch
  - `crates/atm-core/src/delivery_plan.rs:8-115`
    - `DeliveryPlanDisposition`
    - `DeliveryPlanKind`
    - `LogicalMessage`
    - `DeliveryTarget`
    - `DeliveryPlan`
    - only where they exist solely to reconstruct a second policy layer from
      transport/persistence results

- if removal must be staged, deprecate in AG.20 and delete no later than the
  sprint that lands the shared outbound policy layer:
  - `DeliveryPersistenceDisposition`
  - `DeliveryPersistenceResult`
  - `DeliveryPlan*` types that remain only as compatibility shims

- retained in AG.20:
  - raw frame encode/decode helpers
  - peer socket connect/read/write deadline helpers only if they return
    transport facts and no semantic delivery policy
  - `PeerTransportRuntime`
    - retained only as the narrow transport facade after policy/replay removal

## Exact Keep / Delete Decisions

### Canonical path to keep

- retain exactly one transport responsibility:
  - resolve peer endpoint
  - connect socket
  - write canonical request frame
  - read canonical response frame
  - decode canonical response frame
  - return transport facts upward

### Transport layer

- keep:
  - request frame encode/decode
  - socket connect/read/write operations
  - bounded I/O deadlines as transport mechanics only
- delete:
  - retry-budget config ownership
  - retry loop ownership
  - replay persistence ownership
  - retryability classification as a semantic policy decision
  - outcome shaping beyond raw transport facts

### Policy / replay layer

- keep:
  - one shared outbound policy gate above transport
  - one shared replay persistence owner above transport
- delete:
  - any replay enqueue or resume entrypoint on `PeerClientTransport`
  - any transport-local durable replay TTL / expiry policy

### Test surfaces that must be rewritten with the path collapse

- `crates/atm-daemon/src/peer_transport.rs:987-1618`
  - split tests so transport tests assert only transport facts
  - move retry/deferred/replay assertions to the shared outbound policy layer

## This Sprint Does Not Close

- AG.21 owns duplicate daemon dispatch/inbound persistence routing
- AG.22 owns host-resolution relocation, `persist_replay_request(...)`, and
  persisted endpoint identity (`peer_addr` replacement)
- AG.23 owns deferred-receipt dispatch deletion,
  `persist_outcome_unknown_request(...)`, and `resume_pending_replay(...)`
- AG.24 owns request-shape preservation
- AG.25 owns live proof

## Acceptance Criteria

- `peer_transport.rs` no longer decides deferred-vs-immediate policy
- replay persistence is not initiated inside the transport implementation
- transport returns transport-level facts only
- transport tests no longer assert policy/replay outcomes directly

## Hard Merge Gate

- this sprint must deliver at least `-100` net LOC across `crates/` in its
  named target files and contribute to the AG.18-AG.25 ladder-wide aggregate
  reduction; any result above `-100` net LOC fails the sprint
- every completion, validation, and QA verdict must report:
  - `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- every added line must be scrutinized for absolute necessity; lines added only
  to preserve parallel paths, socket-only semantics, or transport-local policy
  fail the sprint
- every production policy/replay branch outside the intended boundary must be
  enumerated and proven deleted or unreachable; any surviving alternate
  production path is a merge blocker
- every retained boundary and wire contract must stay compatible with a future
  HTTP transport phase; any new socket-only semantic, custom state machine, or
  transport-specific message shape is a merge blocker
- quality-mgr QA must independently sweep for any new transport-owned policy
  or replay/state branch introduced by the fix

## Required Validation

- `just test`
- `just lint`
- `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- `rg -n "AttemptFailureKind|DeliveryRetryState|handle_retryable_failure|handle_terminal_failure|DeliveryLoopDecision|wait_for_retry_backoff|classify_io_error" crates/atm-daemon/src/peer_transport.rs`
