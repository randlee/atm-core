---
id: AG.19
title: Delete Separate Remote-Ack Execution Path
status: complete
execution_status: not_started  # plan doc is complete/ready-for-review; code has not landed on any feature/pAG-sN branch yet
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

## Specific Deletions Required

- `crates/atm-core/src/ack/mod.rs:618-623`
  - delete unconditional source-state commit after warning-only remote outcomes
- `crates/atm-core/src/ack/mod.rs:642-680`
  - delete the separate remote-ack branch and
    `execute_remote_ack_reply_plan(...)`
- `crates/atm-core/src/ack/mod.rs:577`
  - remove any ack execution fork that bypasses the canonical outbound send
    path
- `crates/atm-core/src/ack/mod.rs:533-682`
  - delete the reply-only finalization stack once ack reply is just a
    canonical send result plus receive-side state mutation:
    `finalize_ack_outcome(...)`,
    `finalize_suppressed_self_ack_outcome(...)`,
    `finalize_sent_ack_outcome(...)`,
    `AckReplyStateMachine`,
    `build_reply_delivery_plan(...)`,
    `reply_post_send_messages(...)`

## Logic / Branches / State That Do Not Belong

- any ack-specific remote transport execution path
- any branch that treats deferred or outcome-unknown remote ack as caller
  success
- any local ack-state mutation before confirmed remote acceptance
- any reply-only delivery-plan state machine that reconstructs send semantics
  after persistence instead of using the canonical outbound send result
- any split ack outcome family that forces caller/test code to reason about
  `SuppressedSelfAck` vs `Sent` as distinct transport behaviors

## Deliverables

- no separate remote-ack execution function
- one outbound send path for send and ack
- ack-state mutation only after confirmed delivered reply
- regression tests proving deferred/unknown remote ack leaves the source
  message pending
- explicit caller-visible non-success result for remote-ack deferred and
  outcome-unknown cases
- no ack-only reply finalization state machine surviving in production code

## Required Work

- delete `execute_remote_ack_reply_plan(...)`
- delete the `route_send_request(&reply.reply_request)` branch from
  `execute_ack_reply_plan(...)`
- route ack reply through the same outbound send execution used by ordinary
  send
- gate local ack state strictly on confirmed reply delivery
- collapse reply finalization so the canonical send result is consumed once,
  without rebuilding a second reply-only delivery plan

## Explicit Code Samples

```rust
let reply_request = SendRequest {
    acknowledges_message_id: Some(source_message_id),
    ..shared_send_shape
};

let delivery = execute_outbound_send(dispatcher, reply_request, post_send_emitter)?;
if delivery.is_confirmed_delivered() {
    commit_source_ack_state(...)?;
}
```

## Supporting types and staged removal

- remove in AG.19 if the execution path can be collapsed in one patch:
  - `crates/atm-core/src/ack/mod.rs:39-46`
    - `AckReplyDisposition`
    - this currently encodes a dedicated ack-success path instead of reusing
      the canonical outbound delivery result
  - `crates/atm-core/src/ack/mod.rs:240-259`
    - `FinalizeAckContext`
    - collapse once reply-send execution no longer needs a distinct remote-ack
      orchestration context
  - `crates/atm-core/src/ack/mod.rs:550-682`
    - `finalize_ack_outcome(...)`
    - `finalize_sent_ack_outcome(...)`
    - `execute_ack_reply_plan(...)`
    - `execute_remote_ack_reply_plan(...)`
  - `crates/atm-core/src/ack/mod.rs:533-682`
    - `finalize_suppressed_self_ack_outcome(...)`
    - `AckReplyStateMachine`
    - `build_reply_delivery_plan(...)`
    - `reply_post_send_messages(...)`
    - these rebuild a reply-only plan/execution state machine after
      persistence instead of consuming the canonical outbound send result

- if removal must be staged, deprecate in AG.19 and delete no later than the
sprint that collapses the last separate ack-send branch:
  - `AckReplyDisposition`
    - only if caller-facing output cannot be flattened in the same patch
  - `FinalizeAckContext`
    - only if post-send warning/planning data still needs temporary isolation

- retained in AG.19:
  - `AckRequest`
    - retained as the user-facing mutation command input unless and until a
      later sprint folds it into a broader command contract
  - `AckOutcome`
    - retained only if it becomes a thin wrapper over the canonical outbound
    send result plus source-state mutation status
  - `ReplyTarget`
    - retained while ack still needs reply-address derivation; revisit later if
      target resolution can be folded into shared send helpers
  - `SentAckReply`
    - retained only if it becomes a pure data record with no remote-path
      orchestration behavior
  - loopback/self-poison tests in `crates/atm/src/composition.rs`
    - retained, but updated to assert unified send behavior rather than
      `AckReplyDisposition` branch semantics

## Exact Keep / Delete Decisions

### Canonical path to keep

- retain exactly one ack-reply send path:
  - build canonical `SendRequest` with `acknowledges_message_id`
  - pass that request through the same outbound send executor used by ordinary
    send
  - commit source ack state only after confirmed remote/local delivery
  - surface deferred / unknown / terminal outcomes to the caller as explicit
    non-success results

### Ack orchestration layer

- keep:
  - request loading and reply-request construction
  - source-state commit only after confirmed delivery
- delete:
  - `crates/atm-core/src/ack/mod.rs:642-680`
    - dedicated remote-ack execution branch
  - `crates/atm-core/src/ack/mod.rs:618-623`
    - unconditional source-state commit after warning-only outcomes
  - `crates/atm-core/src/ack/mod.rs:533-682`
    - reply-only finalize/build-plan state machine
  - any branch where ack reply bypasses the canonical outbound send executor

### Caller / protocol layer to revisit after AG.18

- if AG.18 has not yet physically removed the old split, AG.19 may temporarily
  consume the transitional wrapper
- AG.19 must not introduce any new ack-specific caller, protocol, or transport
  type while cleaning up the execution path

### Test / caller surfaces that must be rewritten with the path collapse

- `crates/atm/src/composition.rs:1562-1603`
  - stop asserting `AckReplyDisposition::{Sent,SuppressedSelfAck}` as transport
    control flow; assert the canonical send result and mailbox state instead
- `crates/atm-core/src/ack/mod.rs:1394-1646`
  - remove tests whose primary assertion is the split finalizer/disposition
    machinery rather than unified send semantics

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
- there is no surviving production reply-only state machine under
  `crates/atm-core/src/ack/mod.rs`

## Hard Merge Gate

- this sprint must deliver at least `-100` net LOC across `crates/` in its
  named target files and contribute to the AG.18-AG.25 ladder-wide aggregate
  reduction; any result above `-100` net LOC fails the sprint
- every completion, validation, and QA verdict must report:
  - `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- every added line must be scrutinized for absolute necessity; lines added only
  to preserve parallel paths, socket-only semantics, or transport-local policy
  fail the sprint
- every production caller/path that can send or route an ack must be
  enumerated and proven to use the unified send path; any surviving alternate
  production ack path is a merge blocker
- every retained boundary and wire contract must stay compatible with a future
  HTTP transport phase; any new socket-only semantic, custom state machine, or
  transport-specific message shape is a merge blocker
- quality-mgr QA must independently sweep for any new duplicate ack/send path
  or hidden state-mutation rule introduced by the fix

## Required Validation

- `just test`
- `just lint`
- `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- `rg -n "execute_remote_ack_reply_plan|commit_source_ack_state|route_send_request\\(&reply.reply_request\\)|AckReplyStateMachine|build_reply_delivery_plan|reply_post_send_messages" crates/atm-core/src/ack/mod.rs`
