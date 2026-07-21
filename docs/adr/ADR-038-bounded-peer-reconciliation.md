# ADR-038 — Bounded Peer Reconciliation

| Field | Value |
| --- | --- |
| ID | ADR-038 |
| Status | Proposed |
| Scope | Repository-wide |
| Relates to | ADR-034, ADR-035, ADR-036, Phase AI.16 |

## Decision

ATM may reconcile missed peer deliveries only by re-sending already persisted,
immutable canonical records through the ordinary HTTPS transport. The feature
is controlled by a durable, backend-neutral peer policy:

```rust
pub struct PeerSyncPolicy {
    pub max_message_age: Duration, // zero disables reconciliation
}
```

The policy defaults to disabled. An operator enables it with the peer CLI and
may request a one-shot sync. After an ordinary HTTPS write succeeds, the daemon
may run the same bounded sync for that peer. A sync queries storage for local
outbound canonical records addressed to that exact peer and newer than
`now - max_message_age`, then submits each unchanged record to
`PeerHttpTransport`.

The storage trait—not the daemon or transport—owns the backend-neutral query.
There is no outbox, replay store, retry queue, background monitor, cursor,
checkpoint, receipt, retry budget, or per-message delivery state. A duplicate
exact ULID/payload is idempotent under ADR-034; a same-ULID/different-payload
conflict remains a typed error with no side effects.

## Consequences

Reconciliation is bounded by explicit user policy and ordinary stored message
age. It provides a small recovery window after a peer reconnects without
creating a second write path or transport state machine. It never changes the
message ID or immutable payload, and it cannot synthesize delivery success.
