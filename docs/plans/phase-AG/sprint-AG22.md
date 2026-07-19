---
id: AG.22
title: Relocate Host Matching And Endpoint Selection Out Of Transport
status: complete
branch: feature/pAG-s22-host-resolution-boundary
worktree: ../atm-core-worktrees/feature/pAG-s22-host-resolution-boundary
target: develop
---

# Sprint AG.22 — Relocate Host Matching And Endpoint Selection Out Of Transport

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.22
worktree: ../atm-core-worktrees/feature/pAG-s22-host-resolution-boundary
branch: feature/pAG-s22-host-resolution-boundary
status: complete
estimated_scope: medium
```

## Goal

Move host matching, loopback scoping, interface-port selection, and ambiguity
policy out of the transport implementation into a narrower resolution boundary.

## Hard Dependencies

- AG.21 merged

## Exact Targets

- `crates/atm-daemon/src/peer_transport/delivery.rs:107-160`
- `crates/atm-daemon/src/peer_transport/delivery.rs:349-415`
- source findings:
  - `CROSSHOST-UNIFY-5`
  - boundary-review item 8

## Deliverables

- separate host-resolution boundary or helper surface outside transport
- transport consumes a resolved endpoint, not host-policy branches
- direct tests for loopback, self-IP, one-port, and ambiguity cases

## Required Work

- move host-resolution logic out of `peer_transport/delivery.rs`
- keep ambiguity fail-closed
- make transport consume only resolved endpoints

## Explicit Code Samples

```rust
pub trait RemoteEndpointResolver {
    fn resolve(
        &self,
        remote_host: &RemoteTargetHost,
    ) -> Result<SocketAddr, AtmError>;
}
```

## This Sprint Does Not Close

- AG.23 owns deferred-receipt dispatch deletion
- AG.24 owns request-shape preservation
- AG.25 owns live proof

## Acceptance Criteria

- transport code no longer owns host-matching or interface ambiguity policy
- endpoint selection is fail-closed and covered by direct tests

### Hard Merge Gate

- net LOC in transport-owned host-selection logic trends toward reduction or
  any increase is explicitly justified and QA-approved before merge
- every completion, validation, and QA verdict must report:
  - `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- quality-mgr QA must independently sweep for any new duplicated host parsing,
  host matching, or endpoint-selection logic introduced by the fix

## Required Validation

- `just test`
- `just lint`
- `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- `rg -n "resolve_remote_port_for_host|literal_ip|targets_loopback|interface_family_preference" crates/atm-daemon`
