---
title: AK.5 Direct peer resend cache and timer aggregate
status: proposed
target: integrate/phase-ak
recommended_agent: arch-ctm
recommended_model: deep-reasoning
must_follow: AK.4
parallel_safe: false
---

# AK.5 — direct peer resend cache and timer aggregate

## Closure

Add optional default-on resend caching to AK.4's proven direct HTTP function.
It adds one endpoint aggregate and one timer, never a worker or alternate path.

## Fixed contract

```rust
enum PeerConnectionState { Connected, Disconnected, Queued }
```

`peer_resend_cache` defaults to `true`. Connected sends call AK.4 directly;
transient failure queues one endpoint aggregate and arms one timer; queued
sends only append identities. The timer fires no sooner than 60 seconds, loads
the oldest-first immutable origin records for that endpoint once, and calls
AK.4's same HTTP function. Full
success sets `Connected`; failure retains and re-arms. Disabled caching is
exactly AK.4: failed messages remain undelivered and return an error.

## Deliverables

1. Add the three-value endpoint state, one timer-backed aggregate, and the
   default-on `peer_resend_cache` daemon setting.
2. Route immediate and timer sends exclusively through AK.4's HTTP function.
3. Keep this transport state separate from agent/session/roster/nudge state.
4. Update requirements/architecture/ADR language for optional resend caching;
   state that AK.6 deletes obsolete legacy support.

## Required validation

- Unit: connected direct send, disconnected no-connect, and queued two-ULID
  oldest-first timer resend.
- Unit: disabled caching leaves no aggregate/timer and returns an error.
- Unit: no coordinator, worker, per-message thread, channel, or immediate
  SQLite reload exists.
- Integration: immediate and timer resend use the same receiver/nudge path.
- `just lint` and `just test` pass.

## Dependencies

Before every AK.5 development/fix round, merge AK.4 into AK.5. Start AK.5 as
soon as AK.4 is pushed; do not wait for QA. AK.5 PR completion waits for AK.4
merge. Push AK.5, then start AK.6 with AK.5→AK.6 merge-forward.
`must_follow` is required because AK.6 removes only code superseded by the
AK.4/AK.5 path; it is not parallel-safe because both touch peer transport.
