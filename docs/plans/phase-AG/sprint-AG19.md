---
id: AG.19
title: Delete Separate Remote-Ack Execution Path
status: complete
branch: feature/pAG-s19-delete-remote-ack-path
worktree: ../atm-core-worktrees/feature/pAG-s19-delete-remote-ack-path
target: develop
---

# Sprint AG.19 — Delete Separate Remote-Ack Execution Path

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.19
worktree: ../atm-core-worktrees/feature/pAG-s19-delete-remote-ack-path
branch: feature/pAG-s19-delete-remote-ack-path
status: complete
estimated_scope: medium
```

## Goal

Make remote ack use the same outbound send path as ordinary send and restore
the receive-side-only ack-state mutation rule.

## Hard Dependencies

- AG.18 merged

## Exact Targets

- `crates/atm-core/src/ack/mod.rs:577-623`
- `crates/atm-core/src/ack/mod.rs:641-647`
- `crates/atm-core/src/ack/mod.rs:649-682`
- source findings:
  - `CROSSHOST-UNIFY-1`
  - `ARCH-AG15-004`
  - boundary-review items 5, 6, 7

## Deliverables

- no separate remote-ack execution function
- one outbound send path for send and ack
- ack-state mutation only after confirmed delivered reply
- regression tests proving deferred/unknown remote ack leaves the source
  message pending

## Required Work

- delete `execute_remote_ack_reply_plan(...)`
- delete the `route_send_request(&reply.reply_request)` branch from
  `execute_ack_reply_plan(...)`
- route ack reply through the same outbound send execution used by ordinary
  send
- gate local ack state strictly on confirmed reply delivery

## Explicit Code Samples

```rust
let reply_request = SendRequest {
    acknowledges_message_id: Some(source_message_id),
    ..shared_send_shape
};

let delivery = execute_outbound_send(runtime, reply_request)?;
if delivery.is_confirmed_delivered() {
    commit_source_ack_state(...)?;
}
```

## This Sprint Does Not Close

- AG.20 owns `CROSSHOST-UNIFY-2`
- AG.21 owns boundary-review item 1
- AG.22 owns `CROSSHOST-UNIFY-5`
- AG.23 owns boundary-review item 2
- AG.24 owns boundary-review item 4
- AG.25 owns `CROSSHOST-UNIFY-8` and `CROSSHOST-UNIFY-9`

## Acceptance Criteria

- there is no remaining production function dedicated to remote ack execution
- remote ack reply uses the same outbound send path as ordinary send
- deferred or unknown remote ack outcomes do not mark the source message
  acknowledged locally
- tests prove the `ARCH-AG15-004` data-integrity bug is closed

### Hard Merge Gate

- net LOC in the separate ack-routing/execution surface trends toward
  reduction or any increase is explicitly justified and QA-approved before
  merge
- every completion, validation, and QA verdict must report:
  - `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- quality-mgr QA must independently sweep for any new duplicate ack/send path
  or hidden state-mutation rule introduced by the fix

## Required Validation

- `just test`
- `just lint`
- `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- `rg -n "execute_remote_ack_reply_plan|commit_source_ack_state|route_send_request\\(&reply.reply_request\\)" crates/atm-core/src/ack/mod.rs`
