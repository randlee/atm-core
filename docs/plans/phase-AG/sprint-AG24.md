---
id: AG.24
title: Stop Transport From Mutating Request Shape Before Send
status: complete
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

Preserve the canonical request model across transports and move any wire-only
field suppression into one serializer/adapter boundary rather than mutating
`SendRequest` in transport code.

## Hard Dependencies

- AG.23 merged

## Exact Targets

- `crates/atm-daemon/src/peer_transport/delivery.rs:317-325`
- source findings:
  - boundary-review item 4

## Deliverables

- no transport-layer mutation of `SendRequest.remote_host`
- one explicit wire-adapter decision for how canonical request fields are
  encoded on the cross-host wire

## Required Work

- delete `request.remote_host = None`
- if the remote host must not be serialized as-is, handle that in one protocol
  serializer/wire-adapter layer

## Explicit Code Samples

```rust
let wire_request = WireSendRequest::from(&canonical_request);
peer_transport.send(endpoint, wire_request)?;
```

## This Sprint Does Not Close

- AG.25 owns live proof

## Acceptance Criteria

- transport no longer mutates the canonical send request before send
- any field omission or normalization is explicit in one wire-adapter layer

### Hard Merge Gate

- net LOC in request-shape adaptation logic trends toward reduction or any
  increase is explicitly justified and QA-approved before merge
- every completion, validation, and QA verdict must report:
  - `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- quality-mgr QA must independently sweep for any new request mutation or
  transport-local request rewriting introduced by the fix

## Required Validation

- `just test`
- `just lint`
- `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- `rg -n "remote_host = None|mut .*remote_host|WireSendRequest::from" crates`
