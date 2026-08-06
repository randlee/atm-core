# AL.4 — Shared Client and Framework Listeners

**recommended_agent:** arch-ctm/deep-reasoning
**must_follow:** AL.2.
**unblocks:** AL.5.
**parallel_safe:** AL.3; modules and responsibilities do not overlap.

## Deliverables

1. Provide one typed request execution operation for all callers:

```rust
async fn execute(
    &self,
    endpoint: &Endpoint,
    request: &RequestEnvelope,
) -> Result<ResponseEnvelope, AtmError>;
```

2. Configure Unix UDS, loopback TCP, and authenticated TLS TCP as connector or
listener selection only. Each uses the same route, existing envelope codec,
and response decoder.
3. Migrate CLI/graft local clients and direct cross-host send callers to this
shared operation.
4. Configure deadlines, connection limits, tracing, cancellation, and graceful
shutdown through Tokio/framework facilities—not ATM-owned per-connection
threads, polling loops, or coordinators.

## Acceptance criteria

- A source-level test proves all three connector kinds call one client encode /
  decode implementation.
- Cross-host direct send issues one ordinary `RequestEnvelope::Write` to the
  canonical route; it does not schedule retry, replay, batching, or a peer
  request body.
- Listener startup uses framework/Tokio listener services; no manual HTTP
  read/write loop is introduced.

## Required validation

- local UDS (Unix) and loopback-TCP integration tests
- authenticated TCP fixture test
- timeout/cancellation test using Tokio time control
- static test that transport client code has no raw framing symbols

## Non-closure

Legacy transports remain present but unused only after AL.5 proves this path.
No deletion occurs here.
