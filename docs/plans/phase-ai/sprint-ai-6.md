---
title: AI.6 REST router and local UDS
status: historical-superseded-by-ai-11
branch: feature/pAI-s6-http-uds-router
worktree: ../atm-core-worktrees/feature/pAI-s6-http-uds-router
target: integrate/phase-AI
---

# AI.6 — REST router and local UDS (historical)

> **Superseded local-platform target:** AI.11 replaces this sprint's Windows
> UDS-only assumptions with loopback-TCP-only local HTTP. AI.6 records the
> initial router landing; it is not authority for the accepted local transport
> contract, resource inventory, or validation matrix.

## Deliverables

1. Finalize the versioned OpenAPI 3.1 contract in
   `docs/atm-daemon/http-api.md` rooted at `/v1/atm`: messages, message
   detail/read/ack, doctor, teams, and team detail, including ADR-037's
   structured addresses.
2. Implement one REST router from that contract. It calls application handlers
   and never SQLite or nudge code directly.
3. **Historical original target:** replace the custom local framing/client
   codec with HTTP over UDS. AI.11 supersedes the Windows local-transport portion with
   Windows loopback-TCP local HTTP.
4. Delete retired Windows local-transport mapping/fallback and the custom ATM frame codec
   after all retained operations are represented by the REST contract. The
   deletion inventory includes the `AtmProtocol`, `ClientTransport`,
   `ServerTransport`, and `RequestDispatcher` frame-boundary records, their
   codec implementations, and `docs/atm-daemon/protocol-icd.md`; retain none
   under a new name or fallback feature.
5. Embed/publish the OpenAPI JSON through `atm api spec` and contract-test it
   against router requests/responses.
6. Introduce the sealed `DaemonApiClient` as the one client-facing application
   API for CLI, graft, UDS, HTTPS, and test adapters. It may not own a second
   handler or transport decision.
7. Enforce a 3s same-host request deadline and the documented `1_048_576` byte
   body limit before decoding. On shutdown, stop UDS accepts and drain or
   cancel tracked requests within the one documented shutdown deadline.
8. Add an AST-based architecture test that resolves production module/use
   paths and proves every write ingress reaches `ApiRouter::route`, then the
   canonical handler, `MessageWriter::write`, and `PostWriteRouter::dispatch`;
   it must reject another direct storage write or post-write call. String
   matching is not an acceptable graph check.

## Contract

```rust
pub enum ApiRequest {
    Messages(MessageQuery),
    Write(WriteRequest),
    Message(MessageId),
    Clear(MessageId),
    Doctor,
    Teams(TeamRequest),
}

pub trait ApiRouter: Send + Sync {
    fn route(
        &self,
        request: ApiRequest,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError>;
}

pub struct RequestDeadline(/* monotonic absolute deadline */);

pub enum AuthenticatedIngress {
    Local(/* UDS-authenticated local caller */),
    Peer(AuthenticatedPeer),
}

pub struct AuthenticatedPeer { /* private construction */ }

pub const MAX_HTTP_REQUEST_BODY_BYTES: usize = 1_048_576;
```

The UDS adapter translates HTTP to/from `ApiRequest` and calls `ApiRouter`; it
does not own a storage, nudge, acknowledgement, or routing branch.
`AuthenticatedIngress` is transport proof only. Its peer variant is
declared here but remains unconstructable until AI.9's mTLS adapter; the router
still dispatches one application handler.

## Acceptance criteria

- `atm` and graft local daemon calls use HTTP/UDS; no production local client
  uses the retired frame codec.
- **Historical original proof:** Windows local HTTP behavior. AI.11 replaces
  this acceptance item with Windows loopback-TCP HTTP proof.
- No source/dependency reference to the retired Windows local transport or custom frame
  protocol remains.
- Router tests prove adapters cannot reach storage or post-send boundaries.
- UDS startup/shutdown tests prove a bounded body is rejected before decode and
  tracked requests drain or cancel within the documented deadline.

## Non-closure

AI.6 supplies the one HTTP/UDS ingress and routes each operation to the
retained handler surface. AI.7 alone consolidates write semantics; AI.6 must
not introduce a temporary second write handler.

## Required validation

Historical validation was Unix and Windows UDS REST integration tests. The
accepted validation is AI.11's Unix UDS plus loopback-TCP and Windows
loopback-TCP matrix, OpenAPI contract tests, `just lint`, `just test`, and
local CLI send/read/ack smoke.
