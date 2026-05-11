# Sprint S.13 — IPC/Socket Runtime Hardening Plan

**Branch**: feature/pS-s13-ipc-transport-plan  
**Base**: integrate/phase-S @ f152ae3  
**PR target**: integrate/phase-S  
**Status**: Planning

## Goal

Produce the implementation plan for the next daemon transport hardening pass.
The focus is same-host local IPC correctness: move to one receive loop per IPC
connection with an async dispatch boundary, replace the current fatal-path
control tangle with a single shared shutdown primitive, and define the endpoint
cleanup and process-exit contracts needed for supervisor-safe recovery.

Peer socket transport is included only as a separate concern inventory. It is
not converted to a persistent session model in this sprint.

This sprint also adopts the Opus recommendation that fatal local IPC transport
faults use supervisor restart rather than in-process IPC re-bind.

## Required Work

### 1. Write the design document

Add `docs/phase-S/sprint-S13-ipc-plan.md` covering:
- the single receive-loop per connection model
- async dispatch ownership and cancellation rules
- `ShutdownBeacon`
- `SocketEndpointGuard`
- typed exit-code taxonomy and supervisor contract
- runtime SLOs
- peer transport follow-up concerns
- explicit reconciliation of the Opus failure inventory, including
  accept-after-terminate behavior and removal of the event-channel wedge class
- the existing `request_runtime` tracked-work registry as the ownership anchor
  for async request dispatch
- why `LoopbackClientTransport` keeps the same dispatcher seam without needing
  direct `ShutdownBeacon` wiring

### 2. Record the sprint authority

This sprint brief must remain aligned with the design document and the accepted
Phase S architecture:
- no daemon-spawn test exceptions
- no reopening of ADR-002 singleton semantics
- no unrelated storage-layer redesign mixed into the transport plan

### 3. Record the implementation scope for the follow-on fix sprint

The follow-on implementation worktree should target:
- `crates/atm-daemon/src/local_ipc_transport.rs`
- `crates/atm-daemon/src/lifecycle_control.rs`
- `crates/atm-daemon/src/composition.rs`
- targeted transport/runtime tests

The follow-on implementation should not add:
- internal IPC re-bind/self-heal loops
- daemon-spawn test exceptions
- unrelated storage-layer concurrency work mixed into the transport sprint

`crates/atm-daemon/src/peer_transport.rs` is reference material for follow-up
concerns only in S.13.

## Acceptance Criteria

- `docs/phase-S/sprint-S13-ipc-plan.md` exists and covers all required design
  areas
- `docs/phase-S/sprint-S13.md` exists and names the implementation scope and
  acceptance criteria
- `docs/plan-phase-S.md` records S.13 in the sprint sequence
- `just lint` PASS

## References

- `docs/phase-S/sprint-S13-ipc-plan.md`
- `docs/adr/ADR-002-host-wide-daemon-singleton.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`
- `crates/atm-daemon/src/local_ipc_transport.rs`
- `crates/atm-daemon/src/lifecycle_control.rs`
- `crates/atm-daemon/src/composition.rs`
- `crates/atm-daemon/src/peer_transport.rs`
- `TASK-1219-PROD-REVIEW`
