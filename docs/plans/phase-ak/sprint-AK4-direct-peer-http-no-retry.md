---
title: AK.4 Direct peer HTTP without retry
status: proposed
target: integrate/phase-ak
recommended_agent: arch-ctm
recommended_model: deep-reasoning
must_follow: AK.3
parallel_safe: false
---

# AK.4 — direct peer HTTP without retry

## Closure

Prove the smallest production peer-delivery path before resend caching: use
AK.3's persisted full hostname, send ordinary HTTP once, and let the existing
receiver persist and nudge. Failure returns an error and remains undelivered.

## Fixed contract

```rust
fn send_peer_http(
    endpoint: &PeerEndpoint,
    write: &WriteRequest,
) -> Result<ResponseEnvelope, AtmError>;
```

The normal HTTP connect resolves the full hostname to its current IP address.
ATM creates no DNS thread. There is no queue, timer, retry state, worker,
thread, channel, scan, or alternate receiver route.

## Deliverables

1. Add one direct plain trusted-LAN HTTP sender using the existing JSON
   `WriteRequest`/`ResponseEnvelope` contract and receiver handler.
2. Route host-qualified admission directly to it with the in-memory write; do
   not reload SQLite.
3. On failure, leave the admitted record explicitly undelivered and return a
   delivery error. Do not add retry behavior.
4. Update requirements, ADRs, architecture/boundaries, OpenAPI if affected,
   and smoke documentation for the proven no-retry baseline.

## Required validation

- Integration: production send and curl submit the same JSON and receive the
  same response shape.
- Smoke: M4→M5 and M5→M4 each prove production send, remote read, acknowledged
  reply, full-host rendering, and one receiver nudge; curl is the independent
  request-path proof.
- `just lint` and `just test` pass.

## Dependencies

Before every AK.4 development/fix round, merge AK.3 into AK.4. Start AK.4 as
soon as AK.3 is pushed; do not wait for QA. AK.4 PR completion waits for AK.3
merge. Push AK.4, then start AK.5 with AK.4→AK.5 merge-forward.
`must_follow` is required because AK.5 retries AK.4's one verified function;
it is not parallel-safe because both own the active peer send path.
