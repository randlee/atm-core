---
id: AG.21
title: Collapse Duplicate Dispatch Routing And Inbound Persistence Paths
status: complete
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

## Hard Dependencies

- AG.20 merged

## Exact Targets

- `crates/atm-daemon/src/runtime_health/dispatch_delivery.rs:70-117`
- `crates/atm-daemon/src/runtime_health/dispatch_delivery.rs:27-39`
- source findings:
  - `CROSSHOST-UNIFY-3`
  - `CROSSHOST-UNIFY-4`
  - boundary-review item 1

## Deliverables

- one daemon send dispatch decision point
- one inbound send persistence path for all send-shaped requests
- elimination of duplicate local-vs-remote branching above the shared outbound
  delivery layer

## Required Work

- collapse dispatcher send handling to one send request family
- move local-vs-remote routing into one shared outbound delivery function
- ensure inbound peer requests land on the same send persistence path rather
  than a second handler family

## Explicit Code Samples

```rust
fn dispatch_send(
    &self,
    request: SendRequest,
) -> Result<ResponseEnvelope, AtmError>;
```

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

### Hard Merge Gate

- net LOC in duplicate daemon dispatch / inbound persistence routing trends
  toward reduction or any increase is explicitly justified and QA-approved
  before merge
- every completion, validation, and QA verdict must report:
  - `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- quality-mgr QA must independently sweep for any new duplicate routing branch
  or inbound persistence handler introduced by the fix

## Required Validation

- `just test`
- `just lint`
- `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- `rg -n "dispatch_compose_send|build_remote_deferred_outcome|SendRequestRoute::Local|SendRequestRoute::Remote" crates/atm-daemon/src/runtime_health/dispatch_delivery.rs`
