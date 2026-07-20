---
title: AI.6 REST router and local UDS
status: proposed
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
    fn route(&self, request: ApiRequest) -> Result<ApiResponse, AtmError>;
}
```

The UDS adapter translates HTTP to/from `ApiRequest` and calls `ApiRouter`; it
does not own a storage, nudge, acknowledgement, or routing branch.

## Acceptance criteria

- `atm` and graft local daemon calls use HTTP/UDS; no production local client
  uses the retired frame codec.
- Windows CI proves AF_UNIX HTTP message, chat-address, and error behavior.
- No source/dependency reference to named pipes or the retired custom frame
  protocol remains.
- Router tests prove adapters cannot reach storage or post-send boundaries.

## Non-closure

AI.6 supplies the one HTTP/UDS ingress and routes each operation to the
retained handler surface. AI.7 alone consolidates write semantics; AI.6 must
not introduce a temporary second write handler.

## Required validation

Unix and Windows UDS REST integration tests; OpenAPI contract tests; `just
lint`; `just test`; local CLI send/read/ack smoke.
