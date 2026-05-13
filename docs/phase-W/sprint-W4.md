---
id: W.4
title: Peer Replay Recovery Text
status: planned
branch: TBD
worktree: TBD
---

# Sprint W.4 — Peer Replay Recovery Text

## Goals

- close the remaining uncovered replay-persistence recovery branches in
  `peer_transport`
- ensure peer replay persistence failures remain actionable when remote
  delivery outcome is unknown
- keep replay failure reporting on the shared ATM error and doctor surfaces;
  this sprint must not add a bespoke peer-reporting path
- preserve parity between cross-daemon peer transport and the other ATM
  interfaces so remote-delivery failure classes do not drift from the shared
  ATM error model

## Acceptance Criteria

- the sprint covers the exact uncovered replay-persistence branches listed in
  the current path inventory
- each uncovered branch gains `.with_recovery(...)` coverage or an explicit
  ruling that the lower layer already preserves the exact operator guidance
- the sprint reuses the Phase V recovery-text rules and does not invent a
  parallel policy
- the sprint verifies protocol-envelope parity so cross-daemon failures preserve
  the same ATM error code and recovery intent seen by other participants
- where peer transport currently duplicates shared error-mapping/reporting
  behavior for the same failure class, the sprint should collapse those paths
  onto one shared implementation

## Implementation Notes

Primary file in scope:
- `crates/atm-daemon/src/peer_transport.rs`

Shared parity file in scope:
- `crates/atm-core/src/protocol.rs`

Targeted functions:
- `PeerClientTransport::persist_replay_request(...)`
- `PeerClientTransport::persist_outcome_unknown_request(...)`

Current branch-level insertion candidates:
- replay store missing branch
- peer endpoint missing branch
- retry-budget to expiry conversion failure branch

Current path inventory:
- `crates/atm-daemon/src/peer_transport.rs`
  - `PeerClientTransport::persist_replay_request(...)`
    - replay store missing
    - peer endpoint missing
    - retry-budget to expiry conversion failure
  - `PeerClientTransport::persist_outcome_unknown_request(...)`
    - replay metadata unsupported branch
    - durable replay persistence follow-through
  - `PeerTransportRuntime::persist_replay_request(...)`
    - runtime wrapper propagation path
- `crates/atm-core/src/protocol.rs`
  - `ProtocolErrorEnvelope::{from_error,into_atm_error}` propagation for remote
    delivery / replay persistence failures

Supporting verification paths:
- replay persistence tests in `crates/atm-daemon/src/peer_transport.rs`
- any doctor/runtime-health text that refers to remote replay durability

Critical issue classes covered directly by this sprint:
- ATM send failure where remote delivery outcome is unknown
- replay persistence failure that would otherwise leave the operator unsure
  whether retry is safe

CLI / doctor split:
- ATM CLI must keep the immediate send failure concise and tell the operator
  whether durable replay did or did not persist
- cross-daemon consumers must receive the same ATM error code and aligned
  recovery intent for the same remote-delivery / replay-persistence failure
  class
- `atm doctor` should remain able to explain replay-store configuration and
  retained pending replay state if that information is already available
- this sprint is primarily a regression-closure check on uncovered recovery
  branches, not a redesign of how send failure is reported

## Out of Scope

- broad peer transport redesign
- daemon-client startup tracing
- SQLite subsystem instrumentation
