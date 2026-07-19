---
id: AG.18
title: Collapse Compose And DirectDeliver Into One Envelope And Handler
status: complete
branch: feature/pAG-s18-unify-envelope-handler
worktree: ../atm-core-worktrees/feature/pAG-s18-unify-envelope-handler
target: develop
---

# Sprint AG.18 — Collapse Compose And DirectDeliver Into One Envelope And Handler

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.18
worktree: ../atm-core-worktrees/feature/pAG-s18-unify-envelope-handler
branch: feature/pAG-s18-unify-envelope-handler
status: complete
estimated_scope: medium
```

## Goal

Delete the duplicate message-semantic abstraction split so send and ack both
travel as one canonical request envelope through one handler family.

## Hard Dependencies

- none; this sprint lands first and is the required base for AG.19-AG.25

## Exact Targets

- `crates/atm-daemon-client/src/rpc.rs`
- `crates/atm-daemon/src/runtime_health/dispatch_delivery.rs:27-39`
- `crates/atm-core/src/ack/mod.rs`
  - `CROSSHOST-UNIFY-1` cites `persist_sent_ack_reply` at `504-566`
- `crates/atm-daemon/src/non_claude_outbound_runtime.rs`
  - `CROSSHOST-UNIFY-1` cites `74-98` as the separate ack outbound path
- `crates/atm-core/src/protocol.rs`
- source findings:
  - `CROSSHOST-UNIFY-1`
  - `CROSSHOST-UNIFY-3`
  - `CROSSHOST-UNIFY-4`
  - `CROSSHOST-UNIFY-7`

## Deliverables

- one canonical outbound ATM send envelope for:
  - ordinary send
  - ack reply send
- one canonical inbound send handler family
- deletion of the `Compose` vs `DirectDeliver` semantic split
- updated protocol/requirements/ADR references naming the single envelope
- LOC-delta evidence showing the duplicate envelope/handler surface trends down

## Required Work

- remove the `DirectDeliver` semantic path completely
- keep `acknowledges_message_id` as payload data inside the canonical send
  request rather than as a separate wire family
- update the daemon dispatcher so a send-shaped request enters one code path
  before any local-vs-remote transport decision
- update tests so they prove one envelope/handler family instead of both old
  surfaces

## Explicit Code Samples

```rust
pub enum RequestEnvelope {
    Send(Box<SendRequest>),
    Heartbeat(HeartbeatRequest),
    CompatibilityPreflight(CompatibilityPreflight),
    List(ListQuery),
    Peek(ReadQuery),
    Receive(ReadQuery),
    Clear(ClearQuery),
    Doctor(DoctorQuery),
}

pub struct SendRequest {
    pub to: AgentAddress,
    pub remote_host: Option<RemoteTargetHost>,
    pub acknowledges_message_id: Option<AtmMessageId>,
}
```

## This Sprint Does Not Close

- AG.19 owns deletion of the separate remote-ack execution branch and
  `ARCH-AG15-004`
- AG.20 owns `CROSSHOST-UNIFY-2` and boundary-review item 3
- AG.21 owns boundary-review item 1
- AG.22 owns `CROSSHOST-UNIFY-5` and boundary-review item 8
- AG.23 owns boundary-review item 2
- AG.24 owns boundary-review item 4
- AG.25 owns `CROSSHOST-UNIFY-8` and `CROSSHOST-UNIFY-9`

## Acceptance Criteria

- there is no remaining `DirectDeliver` message-semantic envelope or parallel
  handler family in production code
- ack reply construction uses the same canonical send request type as ordinary
  send
- daemon entry no longer forks send semantics by envelope family before routing
- requirements / architecture / ADR language names the single envelope/handler
  family explicitly

### Hard Merge Gate

- net LOC in the removed duplicate envelope/handler surface trends toward
  reduction or any increase is explicitly justified and QA-approved before
  merge
- every completion, validation, and QA verdict must report:
  - `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- quality-mgr QA must independently sweep for any new duplicate abstraction,
  wire shape, or parallel code path introduced while deleting the old one

## Required Validation

- `just test`
- `just lint`
- `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- `rg -n "DirectDeliver|SendRequestEnvelope::Compose|SendRequestEnvelope::Acknowledge" crates`
