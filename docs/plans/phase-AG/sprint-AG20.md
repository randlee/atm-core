---
id: AG.20
title: Move Deferred Replay Policy Out Of Transport
status: complete
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

- `crates/atm-daemon/src/peer_transport/delivery.rs:14-32`
- `crates/atm-daemon/src/peer_transport/delivery.rs:89-105`
- `crates/atm-daemon/src/peer_transport/delivery.rs:163-186`
- `crates/atm-daemon/src/peer_transport/delivery.rs:188-197`
- `crates/atm-daemon/src/peer_transport/delivery.rs:223-289`
- source findings:
  - `CROSSHOST-UNIFY-2`
  - boundary-review item 3

## Deliverables

- transport module no longer owns retry/deferred policy
- transport module no longer owns replay persistence
- one higher-level outcome gate owns deferred / unknown / terminal result
  shaping

## Required Work

- delete transport-local semantic outcome enums where possible
- replace transport result shaping with a narrower result contract
- move retry persistence to the higher shared outbound delivery policy layer

## Explicit Code Samples

```rust
pub enum PeerSendResult {
    Delivered(ResponseEnvelope),
    TransportError(AtmError),
}
```

## This Sprint Does Not Close

- AG.21 owns duplicate daemon dispatch/inbound persistence routing
- AG.22 owns host-resolution relocation
- AG.23 owns deferred-receipt dispatch deletion
- AG.24 owns request-shape preservation
- AG.25 owns live proof

## Acceptance Criteria

- `peer_transport/delivery.rs` no longer decides deferred-vs-immediate policy
- replay persistence is not initiated inside the transport implementation
- transport returns transport-level facts only

### Hard Merge Gate

- net LOC in transport-owned policy/replay code trends toward reduction or any
  increase is explicitly justified and QA-approved before merge
- every completion, validation, and QA verdict must report:
  - `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- quality-mgr QA must independently sweep for any new transport-owned policy
  or replay/state branch introduced by the fix

## Required Validation

- `just test`
- `just lint`
- `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- `rg -n "RemoteDeliveryDecision|persist_remote_retry|deferred_listener_unavailable|classify_error" crates/atm-daemon/src/peer_transport`
