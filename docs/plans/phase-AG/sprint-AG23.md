---
id: AG.23
title: Remove Synthetic Deferred Receipt Construction From Daemon Dispatch
status: complete
branch: feature/pAG-s23-remove-synthetic-deferred-receipts
worktree: ../atm-core-worktrees/feature/pAG-s23-remove-synthetic-deferred-receipts
target: develop
---

# Sprint AG.23 — Remove Synthetic Deferred Receipt Construction From Daemon Dispatch

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.23
worktree: ../atm-core-worktrees/feature/pAG-s23-remove-synthetic-deferred-receipts
branch: feature/pAG-s23-remove-synthetic-deferred-receipts
status: complete
estimated_scope: medium
```

## Goal

Delete mailbox-visible deferred-receipt synthesis from daemon dispatch so the
dispatcher returns delivery facts rather than creating semantic receipt
messages itself.

## Hard Dependencies

- AG.22 merged

## Exact Targets

- `crates/atm-daemon/src/runtime_health/dispatch_delivery.rs:91-114`
- `crates/atm-daemon/src/runtime_health/dispatch_delivery.rs:140-181`
- source findings:
  - boundary-review item 2
  - `CROSSHOST-UNIFY-4` follow-on closure

## Deliverables

- no daemon-dispatch helper that persists remote deferred receipts
- deferred/unknown result shaping moved to one shared policy layer outside
  dispatch

## Required Work

- delete `build_remote_deferred_outcome(...)`
- remove `persist_remote_delivery_receipt_with_runtime(...)` from dispatch

## Explicit Code Samples

```rust
match deliver_remote_send_request(...) ? {
    Delivered(response) => Ok(*response),
    Deferred(state) => Ok(shared_outcome_policy::deferred(state)?),
    OutcomeUnknown(state) => Ok(shared_outcome_policy::unknown(state)?),
    RejectedTerminal(error) => Err(error),
}
```

## This Sprint Does Not Close

- AG.24 owns request-shape preservation
- AG.25 owns live proof

## Acceptance Criteria

- daemon dispatch does not persist mailbox-visible deferred receipts directly
- there is no dispatch-local helper that manufactures deferred send outcomes

### Hard Merge Gate

- net LOC in daemon-dispatch receipt-synthesis logic trends toward reduction or
  any increase is explicitly justified and QA-approved before merge
- every completion, validation, and QA verdict must report:
  - `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- quality-mgr QA must independently sweep for any new synthetic mailbox
  receipt path introduced in dispatch or adjacent glue

## Required Validation

- `just test`
- `just lint`
- `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- `rg -n "build_remote_deferred_outcome|persist_remote_delivery_receipt_with_runtime" crates/atm-daemon/src/runtime_health`
