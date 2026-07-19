---
id: AG.22
title: Relocate Host Matching And Endpoint Selection Out Of Transport
status: complete
execution_status: not_started  # plan doc is complete/ready-for-review; code has not landed on any feature/pAG-sN branch yet
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

All line citations in this sprint are pre-ladder baseline references and must
be re-resolved against the actual branch tip immediately before execution; run
the existing `rg -n ...` validation first as the anti-staleness check.

## Hard Dependencies

- AG.21 merged

## Exact Targets

- `crates/atm-daemon/src/peer_transport.rs:93-116`
- `crates/atm-daemon/src/peer_transport.rs:161-201`
- `crates/atm-daemon/src/peer_transport.rs:288-314`
- `crates/atm-daemon/src/peer_transport.rs:734-741`
- `crates/atm-daemon/src/peer_transport.rs:898-908`
- `crates/atm-core/src/boundary/runtime.rs:8-21`
- source findings:
  - `CROSSHOST-UNIFY-5`
  - boundary-review item 8

## Scope Boundary

- this sprint owns outbound endpoint resolution only
- this sprint is the sole owner for deleting `persist_replay_request(...)` and
  replacing persisted `peer_addr` with resolver-owned target identity
- inbound host-allowlist authorization remains AG.5 scope

## Specific Deletions Required

- `crates/atm-daemon/src/peer_transport.rs:93-116`
  - delete transport-owned endpoint-configuration errors and env parsing:
    `remote_peer_endpoint_not_configured_error(...)`,
    `daemon_peer_endpoint_from_env(...)`
- `crates/atm-daemon/src/peer_transport.rs:161-201`
  - delete transport-owned endpoint storage and construction:
    `PeerClientTransport::endpoint`,
    `new_with_observability(...)` endpoint resolution,
    `new_for_test(...)` direct endpoint injection
- `crates/atm-daemon/src/peer_transport.rs:288-314`
  - delete transport-owned endpoint selection during replay persistence
- `crates/atm-daemon/src/peer_transport.rs:734-741`
  - delete transport-owned endpoint selection at send call time
- `crates/atm-daemon/src/peer_transport.rs:898-908`
  - delete transport-owned endpoint parsing:
    `parse_peer_endpoint(...)`
- `crates/atm-core/src/boundary/runtime.rs:8-21`
  - delete persisted `peer_addr: SocketAddr` ownership from replay state;
    persist a resolver-owned target identity instead

## Logic / Branches / State That Do Not Belong

- any endpoint parsing or config lookup inside `peer_transport.rs`
- any persisted replay record that stores a concrete peer socket address
- any outbound endpoint choice owned by transport instead of a resolver
- any env-driven peer endpoint control inside transport

## Deliverables

- separate host-resolution boundary or helper surface outside transport
- transport consumes a resolved endpoint, not host-policy branches
- direct tests for resolver-owned host/endpoint selection cases
- transport no longer parses or stores endpoint configuration itself

## Required Work

- move endpoint/config resolution out of `peer_transport.rs`
- make transport consume only a resolved endpoint passed from above
- replace persisted `peer_addr` replay ownership with a resolver-owned target
  identity that can be re-resolved above transport

## Explicit Code Samples

```rust
pub trait RemoteEndpointResolver {
    fn resolve(
        &self,
        remote_host: &RemoteTargetHost,
    ) -> Result<SocketAddr, AtmError>;
}
```

## Supporting types and staged removal

- remove in AG.22 if the resolver boundary can land in one patch:
  - `crates/atm-daemon/src/peer_transport.rs:161`
    - `PeerClientTransport::endpoint`
  - `crates/atm-daemon/src/peer_transport.rs:93-116`
    - `remote_peer_endpoint_not_configured_error(...)`
    - `daemon_peer_endpoint_from_env(...)`
  - `crates/atm-daemon/src/peer_transport.rs:898-908`
    - `parse_peer_endpoint(...)`
  - `crates/atm-core/src/boundary/runtime.rs:8-21`
    - `RemoteReplayStateRecord::peer_addr`
    - replace with resolver-owned target identity rather than persisted socket
      address

- if removal must be staged, deprecate in AG.22 and delete no later than the
  sprint that lands the resolver boundary:
  - `PeerClientTransport::endpoint`
  - `RemoteReplayStateRecord::peer_addr`

- retained in AG.22:
  - raw `SocketAddr` at the final transport call boundary only
  - `PeerTransportRuntime`
    - retained only as the transport consumer of already-resolved endpoints

## Exact Keep / Delete Decisions

### Canonical path to keep

- retain exactly one endpoint-selection boundary above transport:
  - parse/normalize remote target once
  - resolve it to a concrete endpoint once
  - pass the resolved endpoint into transport
  - persist replay against resolver-owned target identity, not transport-owned
    socket state

### Transport layer

- keep:
  - consuming a concrete endpoint
  - connecting to that endpoint
- delete:
  - env lookup for endpoint config
  - endpoint parsing
  - endpoint storage as transport-owned long-lived state
  - replay persistence of concrete socket address

### Test surfaces that must be rewritten with the path collapse

- `crates/atm-daemon/src/peer_transport.rs:1013-1087`
  - stop asserting transport-owned endpoint config/error behavior as the
    retained contract
- `crates/atm-daemon/src/peer_transport.rs:1130-1618`
  - keep pure transport tests, but move endpoint-selection assumptions to the
    resolver layer

## This Sprint Does Not Close

- AG.23 owns deferred-receipt dispatch deletion
- AG.24 owns request-shape preservation
- AG.25 owns live proof

## Acceptance Criteria

- transport code no longer owns endpoint parsing or endpoint configuration
- endpoint selection lives above transport and is covered by direct tests
- replay records no longer persist a transport-owned concrete peer socket
  address as their source of truth

## Hard Merge Gate

- this sprint must deliver at least `-100` net LOC across `crates/` in its
  named target files and contribute to the AG.18-AG.25 ladder-wide aggregate
  reduction; any result above `-100` net LOC fails the sprint
- every completion, validation, and QA verdict must report:
  - `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- every added line must be scrutinized for absolute necessity; lines added only
  to preserve parallel paths, socket-only semantics, or transport-local policy
  fail the sprint
- every production host-parse, host-match, and endpoint-selection path must be
  enumerated and proven to flow through the single retained boundary; any
  surviving alternate production path is a merge blocker
- every retained boundary and wire contract must stay compatible with a future
  HTTP transport phase; any new socket-only semantic, custom state machine, or
  transport-specific message shape is a merge blocker
- quality-mgr QA must independently sweep for any new duplicated host parsing,
  host matching, or endpoint-selection logic introduced by the fix

## Required Validation

- `just test`
- `just lint`
- `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- `rg -n "ATM_DAEMON_PEER_ADDR|parse_peer_endpoint|peer_addr: SocketAddr|endpoint: Option<SocketAddr>" crates/atm-daemon/src/peer_transport.rs crates/atm-core/src/boundary/runtime.rs`
