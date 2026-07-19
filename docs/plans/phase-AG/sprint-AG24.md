---
id: AG.24
title: Stop Transport From Mutating Request Shape Before Send
status: complete
execution_status: not_started  # plan doc is complete/ready-for-review; code has not landed on any feature/pAG-sN branch yet
branch: feature/pAG-s24-preserve-request-shape
worktree: ../atm-core-worktrees/feature/pAG-s24-preserve-request-shape
target: develop
---

# Sprint AG.24 — Stop Transport From Mutating Request Shape Before Send

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.24
worktree: ../atm-core-worktrees/feature/pAG-s24-preserve-request-shape
branch: feature/pAG-s24-preserve-request-shape
status: complete
estimated_scope: small
```

## Goal

Preserve one canonical send-shaped request/response model across transports and
delete the split wire families that still force compose-vs-ack distinctions
into protocol, transport, and test code.

All line citations in this sprint are pre-ladder baseline references and must
be re-resolved against the actual branch tip immediately before execution; run
the existing `rg -n ...` validation first as the anti-staleness check.

## Hard Dependencies

- AG.23 merged

## Exact Targets

- `crates/atm-core/src/protocol.rs:24-58`
- `crates/atm-core/src/protocol.rs:743-760`
- `crates/atm-core/src/transport/testing.rs:70-79`
- `crates/atm-daemon/src/runtime_health.rs:589-600`
- `crates/atm/src/composition.rs:293-316`
- `crates/atm-graft/src/lib.rs:228-244`
- source findings:
  - boundary-review item 4

## Specific Deletions Required

- `crates/atm-core/src/protocol.rs:24-37`
  - delete split wire request/response families:
    `SendRequestEnvelope`,
    `SendResponseEnvelope`
- `crates/atm-core/src/protocol.rs:743-760`
  - delete split message-kind classification for compose vs acknowledge and
    sent vs acknowledged
- `crates/atm-core/src/transport/testing.rs:70-79`
  - delete mirrored split loopback transport request/response handling
- `crates/atm-daemon/src/runtime_health.rs:589-600`
  - delete split daemon request/response wiring that still depends on the old
    request-shape split
- `crates/atm/src/composition.rs:293-316`
  - delete CLI-side split response matching for send vs ack
- `crates/atm-graft/src/lib.rs:228-244`
  - delete graft-side split response matching for send vs ack

## Logic / Branches / State That Do Not Belong

- any protocol-layer distinction between compose and ack as different
  transport request shapes
- any caller/test transport branch that exists only because the wire contract
  is split
- any response-shape distinction that encodes send vs ack as different
  transport semantics instead of one canonical send family with data fields

## Deliverables

- one canonical send-shaped request model across CLI, graft, daemon IPC, and
  cross-host transport
- one canonical send-shaped response model across those same boundaries
- protocol message-kind mapping no longer splits compose vs acknowledge
- no mirrored caller/test branching preserved solely by the old split wire
  contract

## Required Work

- collapse wire request/response DTOs to one send-shaped family
- collapse protocol message-kind mapping accordingly
- update daemon/CLI/graft/testing glue to consume the unified request/response
  shape instead of split branches

## Explicit Code Samples

```rust
pub enum RequestEnvelope {
    Send(SendRequest),
    // ...
}

pub enum ResponseEnvelope {
    Send(SendOutcome),
    // ...
}
```

## Supporting types and staged removal

- remove in AG.24 if the wire collapse can land in one patch:
  - `crates/atm-core/src/protocol.rs:27-37`
    - `SendRequestEnvelope`
    - `SendResponseEnvelope`
  - `crates/atm-core/src/protocol.rs:743-760`
    - compose/acknowledge-specific message-kind routing
  - `crates/atm-core/src/transport/testing.rs:70-79`
    - split loopback transport send/ack branch
  - `crates/atm-daemon/src/runtime_health.rs:589-600`
    - split daemon dispatch/response wiring for compose vs acknowledge
  - `crates/atm/src/composition.rs:293-316`
    - split CLI response matching
  - `crates/atm-graft/src/lib.rs:228-244`
    - split graft response matching

- if removal must be staged, deprecate in AG.24 and delete no later than the
  sprint that lands the unified wire contract:
  - `SendRequestEnvelope`
  - `SendResponseEnvelope`

- retained in AG.24:
  - canonical `SendRequest`
  - canonical send result type(s) only if they represent one send family with
    ack as data, not a second transport path

## Exact Keep / Delete Decisions

### Canonical path to keep

- retain exactly one send-shaped wire family:
  - one request DTO
  - one response DTO
  - ack represented as data on the canonical request/result path

### Protocol layer

- keep:
  - one `RequestEnvelope::Send(...)`
  - one `ResponseEnvelope::Send(...)`
- delete:
  - split `SendRequestEnvelope`
  - split `SendResponseEnvelope`
  - split compose/ack message kinds

### Caller / adapter surfaces to rewrite

- `crates/atm/src/composition.rs:293-316`
  - stop matching separate sent vs acknowledged transport variants
- `crates/atm-graft/src/lib.rs:228-244`
  - same deletion on graft caller surface
- `crates/atm-core/src/transport/testing.rs:70-79`
  - stop mirroring the old split under test

## This Sprint Does Not Close

- AG.25 owns live proof

## Acceptance Criteria

- there is one canonical send-shaped request/response wire family
- protocol/caller/test code no longer preserve compose-vs-ack transport shape
  splits
- no stale request-shape rewrite target remains in the sprint; the sprint is
  aligned to the live code path

## Hard Merge Gate

- this sprint must deliver at least `-100` net LOC across `crates/` in its
  named target files and contribute to the AG.18-AG.25 ladder-wide aggregate
  reduction; any result above `-100` net LOC fails the sprint
- every completion, validation, and QA verdict must report:
  - `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- every added line must be scrutinized for absolute necessity; lines added only
  to preserve parallel paths, socket-only semantics, or transport-local policy
  fail the sprint
- every production request-shape rewrite/adaptation path must be enumerated
  and proven deleted or unreachable except for the retained boundary; any
  surviving alternate production path is a merge blocker
- every retained boundary and wire contract must stay compatible with a future
  HTTP transport phase; any new socket-only semantic, custom state machine, or
  transport-specific message shape is a merge blocker
- quality-mgr QA must independently sweep for any new request mutation or
  transport-local request rewriting introduced by the fix

## Required Validation

- `just test`
- `just lint`
- `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- `rg -n "SendRequestEnvelope|SendResponseEnvelope|SendComposeRequest|SendAcknowledgeRequest|SendSentResponse|SendAcknowledgedResponse" crates`
