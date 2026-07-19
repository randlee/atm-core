---
id: AG.21
title: Collapse Duplicate Dispatch Routing And Inbound Persistence Paths
status: complete
execution_status: not_started  # plan doc is complete/ready-for-review; code has not landed on any feature/pAG-sN branch yet
branch: feature/pAG-s21-collapse-dispatch-routing
worktree: ../atm-core-worktrees/feature/pAG-s21-collapse-dispatch-routing
target: develop
---

# Sprint AG.21 — Collapse Duplicate Dispatch Routing And Inbound Persistence Paths

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.21
worktree: ../atm-core-worktrees/feature/pAG-s21-collapse-dispatch-routing
branch: feature/pAG-s21-collapse-dispatch-routing
status: complete
estimated_scope: medium
```

## Goal

Reduce the daemon dispatch layer to one routing decision and one inbound
persistence path for send-shaped requests.

All line citations in this sprint are pre-ladder baseline references and must
be re-resolved against the actual branch tip immediately before execution; run
the existing `rg -n ...` validation first as the anti-staleness check.

## Hard Dependencies

- AG.20 merged

## Exact Targets

- `crates/atm-daemon/src/runtime_health.rs:584-603`
- `crates/atm-core/src/transport/testing.rs:69-80`
- `crates/atm-daemon/src/tests_post_send_graft_warning.rs:123-193`
- `crates/atm-daemon/src/tests/runtime_root.rs:76-92`
- source findings:
  - `CROSSHOST-UNIFY-3`
  - `CROSSHOST-UNIFY-4`
  - boundary-review item 1

## Specific Deletions Required

- `crates/atm-daemon/src/runtime_health.rs:589-603`
  - delete dispatcher-owned compose-vs-ack execution branching
- `crates/atm-core/src/transport/testing.rs:74-78`
  - delete mirrored split test transport branch that preserves separate
    dispatcher semantics under test
- `crates/atm-daemon/src/tests_post_send_graft_warning.rs:123-193`
  - delete test surfaces that encode split send/ack response families as the
    canonical daemon contract instead of one send-shaped path
- `crates/atm-daemon/src/tests/runtime_root.rs:76-92`
  - delete runtime-root assertions that still require the split send response
    family to prove dispatch correctness

## Logic / Branches / State That Do Not Belong

- any second semantic dispatcher branch after `RequestEnvelope::Send(...)`
- any dispatcher-local distinction between compose and ack transport handling
- any mirrored test transport branch that keeps the split dispatch contract
  alive after production collapse
- any duplicate response-shaping branch outside the single retained
  send-shaped entry point

## Deliverables

- one daemon send dispatch decision point
- one inbound send persistence path for all send-shaped requests
- elimination of duplicate compose-vs-ack branching above the shared outbound
  delivery layer
- retain the explicit canonical outbound symbol introduced in AG.18:
  `execute_outbound_send(dispatcher, request, post_send_emitter)`

## Required Work

- collapse dispatcher send handling to one send request family
- ensure inbound peer requests still land on the same send persistence path
  after the dispatcher split is removed
- collapse mirrored test transport glue so tests prove the single retained
  dispatch contract

## Explicit Code Samples

```rust
fn dispatch_send(
    &self,
    request: SendRequest,
) -> Result<ResponseEnvelope, AtmError>;
```

## Supporting types and staged removal

- remove in AG.21 if the dispatcher collapse can land in one patch:
  - `crates/atm-daemon/src/runtime_health.rs:589-603`
    - split `SendRequestEnvelope::{Compose,Acknowledge}` handling branches
    - split `SendResponseEnvelope::{Sent,Acknowledged}` return shaping
  - `crates/atm-core/src/transport/testing.rs:69-80`
    - split test transport handling for compose vs acknowledge
  - `crates/atm-daemon/src/tests_post_send_graft_warning.rs:123-193`
    - split response-shape assertions that preserve the old dispatcher
      contract

- if removal must be staged, deprecate in AG.21 and delete no later than the
  sprint that physically removes the split wire families:
  - `SendRequestEnvelope`
  - `SendResponseEnvelope`
  - only if AG.18 has not yet physically removed them from the protocol layer

- retained in AG.21:
  - `DaemonRequestDispatcher`
    - retained only as the single send-shaped daemon entry point
  - `RequestEnvelope::Receive`
    - retained as the read path; it is not part of the send/ack dispatch split

## Exact Keep / Delete Decisions

### Canonical path to keep

- retain exactly one daemon send path:
  - dispatcher receives one send-shaped request family
  - one outbound send executor owns routing/local-vs-remote
  - one response contract returns delivery result

### Dispatcher layer

- keep:
  - one `RequestEnvelope::Send(...)` match arm
  - one shared outbound send call
- delete:
  - separate compose branch in `runtime_health.rs`
  - separate ack branch in `runtime_health.rs`
  - any response wrapping that depends on send vs ack being different transport
    semantics

### Test surfaces that must be rewritten with the path collapse

- `crates/atm-core/src/transport/testing.rs:69-80`
  - stop mirroring the split dispatcher
- `crates/atm-daemon/src/tests_post_send_graft_warning.rs:123-193`
  - stop asserting split sent/acknowledged response families as the retained
    contract
- `crates/atm-daemon/src/tests/runtime_root.rs:76-92`
  - assert unified send-shaped dispatch behavior instead of split response
    variants

## This Sprint Does Not Close

- AG.22 owns host-resolution relocation
- AG.23 owns deferred-receipt dispatch deletion
- AG.24 owns request-shape preservation
- AG.25 owns live proof

## Acceptance Criteria

- daemon dispatch has one send-shaped handler entry
- local-vs-remote routing happens in one shared outbound delivery function
- inbound peer-delivered send-shaped requests persist through the same handler
  family as local send
- tests no longer preserve a split daemon send contract

## Hard Merge Gate

- this sprint must deliver at least `-100` net LOC across `crates/` in its
  named target files and contribute to the AG.18-AG.25 ladder-wide aggregate
  reduction; any result above `-100` net LOC fails the sprint
- every completion, validation, and QA verdict must report:
  - `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- every added line must be scrutinized for absolute necessity; lines added only
  to preserve parallel paths, socket-only semantics, or transport-local policy
  fail the sprint
- every production dispatch/persistence caller path must be enumerated and
  proven to reach the single retained route only; any surviving alternate
  production path is a merge blocker
- every retained boundary and wire contract must stay compatible with a future
  HTTP transport phase; any new socket-only semantic, custom state machine, or
  transport-specific message shape is a merge blocker
- quality-mgr QA must independently sweep for any new duplicate routing branch
  or inbound persistence handler introduced by the fix

## Required Validation

- `just test`
- `just lint`
- `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- `rg -n "SendRequestEnvelope::Compose|SendRequestEnvelope::Acknowledge|SendResponseEnvelope::Sent|SendResponseEnvelope::Acknowledged" crates/atm-daemon/src/runtime_health.rs crates/atm-core/src/transport/testing.rs crates/atm-daemon/src/tests_post_send_graft_warning.rs crates/atm-daemon/src/tests/runtime_root.rs`
