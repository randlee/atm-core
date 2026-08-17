---
id: AG.18
title: Collapse Compose And DirectDeliver Into One Envelope And Handler
status: complete
execution_status: not_started  # plan doc is complete/ready-for-review; code has not landed on any feature/pAG-sN branch yet
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

All line citations in this sprint are pre-ladder baseline references and must
be re-resolved against the actual branch tip immediately before execution; run
the existing `rg -n ...` validation first as the anti-staleness check.

## Hard Dependencies

- none; this sprint lands first and is the required base for AG.19-AG.25

## Exact Targets

- `crates/atm-daemon-client/src/rpc.rs`
- `crates/atm-daemon/src/runtime_health.rs`
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

## Specific Deletions Required

- `crates/atm-core/src/protocol.rs:30-37`
  - delete the `SendRequestEnvelope::{Compose,Acknowledge}` request split
- `crates/atm-core/src/protocol.rs:314-331`
  - delete split request/response message kinds:
    `SendComposeRequest`, `SendAcknowledgeRequest`,
    `SendSentResponse`, `SendAcknowledgedResponse`
- `crates/atm-core/src/protocol.rs:504-510`
  - delete split frame decoding and nested payload selection keyed by compose vs
    acknowledge
- `crates/atm-core/src/protocol.rs:743-755`
  - delete split request message-kind selection logic
- `crates/atm-core/src/protocol.rs:757-770`
  - delete split response message-kind selection logic
- `crates/atm-daemon/src/runtime_health.rs`
  - delete the top-level send-vs-ack semantic fork in daemon dispatch
- `crates/atm/src/composition.rs:293-319`
  - delete dual CLI request construction paths for compose vs acknowledge
- `crates/atm-graft/src/lib.rs:227-247`
  - delete dual graft request construction paths for compose vs acknowledge
- `crates/atm-daemon-client/src/rpc.rs:223-305`
  - delete compose-specific request encode/decode tests and replace them with
    one canonical send-shaped request path
- `crates/atm-daemon/src/peer_transport.rs`
  - remove any compose-only peer transport annotation or request-shape logic;
    inbound handling must apply to the single retained send-shaped request
- test surfaces to rewrite after deletion:
  - `crates/atm-daemon/src/tests/runtime_root.rs:76-76,129-129,180-180`
  - `crates/atm-daemon/src/tests/runtime_root.rs:255-255,333-335,378-378,472-476`
  - `crates/atm-graft/src/lib.rs:605-661`

## Exact Keep / Delete Decisions

### Canonical path to keep

- retain exactly one end-to-end send-shaped path:
  - caller builds canonical `SendRequest`
  - protocol serializes one send request kind
  - daemon dispatch accepts one send-shaped request entry
  - one outbound send executor performs local-vs-remote routing
  - inbound peer handling annotates the same canonical request shape
  - one send-shaped response contract carries the delivery result back
- ack remains data on that path via `acknowledges_message_id`; it is not a
  second semantic request family

### Caller entry layer

- keep:
  - one CLI send-shaped request entry built from canonical `SendRequest`
  - one graft send-shaped request entry built from canonical `SendRequest`
- delete:
  - `crates/atm/src/composition.rs:293-293`
    - compose-only caller path
  - `crates/atm/src/composition.rs:316-319`
    - acknowledge-only caller path
  - `crates/atm-graft/src/lib.rs:227-233`
    - compose-only graft caller path
  - `crates/atm-graft/src/lib.rs:243-249`
    - acknowledge-only graft caller path

### Protocol / wire layer

- keep:
  - one send-shaped request frame kind
  - one send-shaped response frame kind
- delete:
  - `crates/atm-core/src/protocol.rs:30-37`
    - `SendRequestEnvelope::{Compose,Acknowledge}`
  - `crates/atm-core/src/protocol.rs:37-37`
    - `SendResponseEnvelope::{Sent,Acknowledged}`
  - `crates/atm-core/src/protocol.rs:314-331`
    - `SendComposeRequest`
    - `SendAcknowledgeRequest`
    - `SendSentResponse`
    - `SendAcknowledgedResponse`
  - `crates/atm-core/src/protocol.rs:343-344`
    - split request-kind classification
  - `crates/atm-core/src/protocol.rs:365-375`
    - split numeric message-kind decoding
  - `crates/atm-core/src/protocol.rs:504-506`
    - split request decode branch
  - `crates/atm-core/src/protocol.rs:504-506`
    - split nested payload selection for compose vs acknowledge
  - `crates/atm-core/src/protocol.rs:743-755`
    - split request message-kind selection
  - `crates/atm-core/src/protocol.rs:757-770`
    - split response message-kind selection
  - `crates/atm-core/src/protocol.rs:1222-1243`
    - compose-shaped protocol round-trip test assumptions

### Supporting types and staged removal

- remove in AG.18 if the call graph can be collapsed in one patch:
  - `crates/atm-core/src/protocol.rs:30-37`
    - `SendRequestEnvelope`
  - `crates/atm-core/src/protocol.rs:37-37`
    - `SendResponseEnvelope`
  - `crates/atm-core/src/protocol.rs:314-331`
    - split `MessageKind` variants for compose/ack and sent/acknowledged
  - `crates/atm-daemon/src/runtime_health.rs`
    - `dispatch_compose_send(...)`

- if removal must be staged, deprecate in AG.18 and delete no later than the
  immediate follow-on sprint that collapses the last caller:
  - `SendRequestEnvelope`
    - deprecate once all callers can build canonical `SendRequest` directly
  - `SendResponseEnvelope`
    - deprecate once callers can consume one send-shaped delivery result
  - split `MessageKind` variants
    - deprecate once frame encode/decode supports one request kind and one
      response kind

- not deletion targets in AG.18:
  - `RequestEnvelope`
    - retained as the outer transport envelope
  - `ResponseEnvelope`
    - retained as the outer transport envelope
  - `SendRequest`
    - retained as the canonical request type
  - `SendOutcome`
    - retained as the canonical outbound delivery result until later cleanup
  - `AckOutcome`
    - retained in AG.18; AG.19 decides whether it survives as a distinct
      caller-facing result or is folded behind the same outbound result family

### Daemon request-dispatch layer

- keep:
  - one daemon send-shaped handler entry that accepts canonical `SendRequest`
- delete:
  - `crates/atm-daemon/src/runtime_health.rs`
    - top-level compose vs acknowledge fork
  - `crates/atm-daemon/src/runtime_health.rs`
    - `dispatch_compose_send(...)` as a compose-named semantic branch

### Inbound peer-server layer

- keep:
  - one send-shaped inbound annotation path that sets `source_remote_host`
    without caring about old compose/ack envelope families
- delete:
  - `crates/atm-daemon/src/peer_transport.rs`
    - compose-only peer transport match/annotation logic used for source-host
      handling

### Daemon-client / RPC layer

- keep:
  - one canonical send-request encode/decode path
- delete:
  - `crates/atm-daemon-client/src/rpc.rs:171`
    - `MessageKind::SendComposeRequest` as the hardcoded canonical request kind
  - `crates/atm-daemon-client/src/rpc.rs:227-257`
    - compose-only request round-trip test
  - `crates/atm-daemon-client/src/rpc.rs:270-311`
    - compose-only stdin-materialization request test
  - `crates/atm-daemon-client/src/rpc.rs:317-339`
    - response round-trip assumptions that canonical send result is the old
      split response family
  - any remaining RPC test assumptions that canonical send means `Compose`

### Derived helper code to revisit after primary deletions

- `crates/atm-daemon/src/peer_transport.rs:288-338`
  - replay persistence helpers currently assume the old compose-shaped request
    family; once AG.18 removes the split, they must either use the retained
    single request type or be queued for deletion by the later replay-policy
    sprint
- `crates/atm-daemon/src/peer_transport.rs:733-741`
  - remote send currently persists outcome-unknown requests through the same
    flat peer transport file; once the split is removed this path must use the
    retained single request representation only

### Test / harness contract layer

- keep:
  - tests that assert one canonical send-shaped contract
- delete or rewrite:
  - any test asserting `SendRequestEnvelope::Compose` is the required send path
  - any test asserting `SendRequestEnvelope::Acknowledge` is a separate
    required semantic path
  - specifically:
    - `crates/atm-daemon/src/tests/runtime_root.rs:76-76,129-129,180-180`
    - `crates/atm-daemon/src/tests/runtime_root.rs:255-255,333-335,378-378,472-476`
    - `crates/atm-graft/src/lib.rs:605-661`
    - `crates/atm-daemon/src/tests_post_send_graft_warning.rs:123,160,184,258`
    - `crates/atm-daemon/src/peer_transport.rs:1117-1468`
    - `crates/atm-daemon/src/peer_transport.rs:1380-1468`
      because the flat peer transport test/replay surfaces still encode the
      old compose-only request family assumptions

## Logic / Branches / State That Do Not Belong

- any production branch that decides message semantics from envelope family
- any dedicated ack wire family instead of `SendRequest` plus
  `acknowledges_message_id`
- any caller surface that still constructs a distinct ack request envelope
- any protocol frame kind that exists only to preserve compose-vs-ack semantic
  splitting
- any compose-only inbound annotation branch in peer server handling
- any test or harness that encodes the old split as the required contract
- any replay or deferred helper that assumes only one old split variant is the
  canonical persisted request representation

## Deliverables

- one canonical outbound ATM send envelope for:
  - ordinary send
  - ack reply send
- one canonical inbound send handler family
- deletion of the `Compose` vs `DirectDeliver` semantic split
- updated protocol/requirements/ADR references naming the single envelope
- LOC-delta evidence showing the duplicate envelope/handler surface trends down

## Explicit Code Samples

```rust
fn execute_outbound_send(
    dispatcher: &impl boundary::OutboundSendDispatcher,
    request: SendRequest,
    post_send_emitter: &dyn boundary::PostSendEmitter,
) -> Result<ResponseEnvelope, AtmError>;
```

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

## Hard Merge Gate

- this sprint must deliver at least `-100` net LOC across `crates/` in its
  named target files and contribute to the AG.18-AG.25 ladder-wide aggregate
  reduction; any result above `-100` net LOC fails the sprint
- every completion, validation, and QA verdict must report:
  - `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- every added line must be scrutinized for absolute necessity; lines added only
  to preserve parallel paths, socket-only semantics, or transport-local policy
  fail the sprint
- every production caller/path for the retired envelope/handler branch must be
  enumerated and proven deleted or unreachable; any surviving alternate
  production path is a merge blocker
- every retained boundary and wire contract must stay compatible with a future
  HTTP transport phase; any new socket-only semantic, custom state machine, or
  transport-specific message shape is a merge blocker
- quality-mgr QA must independently sweep for any new duplicate abstraction,
  wire shape, or parallel code path introduced while deleting the old one

## Required Validation

- `just test`
- `just lint`
- `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- `rg -n "DirectDeliver|SendRequestEnvelope::Compose|SendRequestEnvelope::Acknowledge" crates`
