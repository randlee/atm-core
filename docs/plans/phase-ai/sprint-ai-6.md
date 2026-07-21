---
title: AI.6 REST router and local UDS
status: complete
branch: feature/pAI-s6-http-uds-router
worktree: ../atm-core-worktrees/feature/pAI-s6-http-uds-router
target: integrate/phase-AI
---

# AI.6 — REST router and local UDS

## Deliverables

1. Finalize the versioned OpenAPI 3.1 contract in
   `docs/atm-daemon/http-api.md` rooted at `/v1/atm`: messages, message
   detail/read/ack, doctor, teams, and team detail, including ADR-037's
   structured addresses.
2. Implement one REST router from that contract. It calls application handlers
   and never SQLite or nudge code directly.
3. Replace the current custom local framing/client codec with HTTP over UDS.
   The same UDS contract must run on Unix and Windows AF_UNIX.
4. Delete Windows named-pipe mapping/fallback and the custom ATM frame codec
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
- Windows CI proves AF_UNIX HTTP message, chat-address, and error behavior.
- No source/dependency reference to named pipes or the retired custom frame
  protocol remains.
- Router tests prove adapters cannot reach storage or post-send boundaries.
- UDS startup/shutdown tests prove a bounded body is rejected before decode and
  tracked requests drain or cancel within the documented deadline.

## Non-closure

AI.6 supplies the one HTTP/UDS ingress and routes each operation to the
retained handler surface. AI.7 alone consolidates write semantics; AI.6 must
not introduce a temporary second write handler.

## Required validation

Unix and Windows UDS REST integration tests; OpenAPI contract tests; `just
lint`; `just test`; local CLI send/read/ack smoke.

## Closure waiver

Deliverable 8's full AST graph check names `MessageWriter::write` and
`PostWriteRouter::dispatch`, which are AI.7-scope types and do not exist on
the AI.6 branch. AI.6 closes the buildable part of the contract by declaring a
real `ApiRequest::Write` discriminant and routing HTTP method/path pairs into
typed `ApiRequest` variants before daemon dispatch. The complete write-ingress
AST graph remains deferred to AI.7, where the writer and post-write router
types are introduced.
