# AL.2 — Canonical Typed HTTP Handler

**recommended_agent:** arch-ctm/deep-reasoning
**must_follow:** AL.1; merge AL.1's pushed integration line before each dev or
fix round.
**unblocks:** AL.3 and AL.4.
**parallel_safe:** none; this establishes the sole application ingress.

## Deliverables

1. Implement the framework-owned `POST /v1/atm/messages` route.
2. Extract/serialize the existing shared types directly:

```rust
async fn messages(
    State(state): State<RuntimeState>,
    Json(request): Json<RequestEnvelope>,
) -> Result<Json<ResponseEnvelope>, HttpApiError>;
```

3. Normalize authenticated connector provenance before exactly one
   `ApiRouter` dispatch; no listener/peer-specific decoder may deserialize a
   second request form.
4. Preserve existing typed error/result semantics through one response mapper.

## Acceptance criteria

- A local and authenticated-peer fixture submit byte-equivalent serialized
  `RequestEnvelope::Write` values and receive the same `ResponseEnvelope`.
- The handler has one core dispatch call; no peer router, peer body, or
  `PeerMessageArray` support is introduced.
- Malformed HTTP, headers, JSON, and body limits are rejected by framework
  configuration/extractors, not an ATM frame reader.

## Required validation

- handler integration tests via the Axum/Tower test service
- serialization equality test for local and peer connector fixtures
- boundary search/test confirming no `HttpFrameReader` use in the new crate

## Non-closure

No production listener has moved in this sprint, and no notification behavior
is added before post-persistence semantics are proven in AL.3.
