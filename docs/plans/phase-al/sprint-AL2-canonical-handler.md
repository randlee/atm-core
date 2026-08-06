---
title: AL.2 Canonical Typed HTTP Handler
status: complete
branch: feature/pal-s2-canonical-handler
worktree: ../atm-core-worktrees/feature/pal-s2-canonical-handler
target: integrate/phase-al
---

# AL.2 — Canonical Typed HTTP Handler

**recommended_agent:** arch-ctm/deep-reasoning
**must_follow:** AL.1; merge AL.1's pushed integration line before each dev or
fix round.
**unblocks:** AL.3 and AL.4.
**parallel_safe:** none; this establishes the sole application ingress.

**traceability:** `REQ-CORE-TRANSPORT-001`, `001B`, `002`, `004`,
`REQ-CORE-BOUNDARY-002`, ADR-032, ADR-033. The public type/serialization
oracle recorded by AL.1 is binding.

## Deliverables

1. Implement the framework-owned `POST /v1/atm/messages` route.
2. Extract and serialize the exact current route-specific HTTP types directly.
   The names below are placeholders for the AL.1 inventory entries, not new
   types and not a request to add a generic envelope:

```rust
async fn post_messages(
    State(state): State<RuntimeState>,
    Json(request): Json<ExistingPostMessagesRequest>,
) -> Result<Json<ExistingPostMessagesSuccess>, HttpApiError>;
```

   Existing `RequestEnvelope` / `ResponseEnvelope` values may be used inside
   `ApiRouter` exactly where current application code uses them, but ADR-033
   forbids exposing those as the HTTP body.

3. Normalize authenticated connector provenance before exactly one
   `ApiRouter` dispatch; no listener/peer-specific decoder may deserialize a
   second request form.
4. Preserve existing typed error/result semantics through one response mapper.
5. Apply bounded overload control with maintained Tower/Axum layers: fixed body
   limit, bounded in-flight request/concurrency limit, and load-shed rejection
   when capacity is exhausted. Reject overload with the existing ADR-032 error
   contract; do not create an ATM queue, worker pool, retry, or polling loop.

## Acceptance criteria

- A local and authenticated-peer fixture submit byte-equivalent current
  route-body JSON and receive the same current success/error JSON snapshot.
- The handler has one core dispatch call; no peer router, peer body, or
  `PeerMessageArray` support is introduced.
- Malformed HTTP, headers, JSON, and body limits are rejected by framework
  configuration/extractors, not an ATM frame reader.
- Overload reaches the configured load-shed result within the request budget;
  it never creates an unbounded queue or background work.

## Required validation

- handler integration tests via the Axum/Tower test service
- serialization equality test for local and peer connector fixtures
- OpenAPI/Serde regression snapshot from the AL.1 compatibility oracle
- boundary search/test confirming no `HttpFrameReader` use in the new crate
- overload/body-limit integration tests that assert bounded in-flight work and
  the retained ADR-032 response schema

## Non-closure

No production listener has moved in this sprint, and no notification behavior
is added before post-persistence semantics are proven in AL.3.
