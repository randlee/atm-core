# ADR-038 — Bounded Peer Reconciliation

| Field | Value |
| --- | --- |
| ID | ADR-038 |
| Status | Proposed |
| Scope | Repository-wide |
| Relates to | ADR-034, ADR-035, ADR-036, ADR-041, Phase AI.28 |

## Decision

ATM may reconcile missed peer deliveries only by re-sending already persisted,
immutable canonical records through the ordinary HTTPS transport. The feature
is controlled by a durable, backend-neutral peer policy:

```rust
pub struct PeerSyncPolicy {
    pub max_message_age: Duration, // zero disables reconciliation
    pub max_batch_messages: NonZeroU16, // default: 100
}
```

The policy defaults to disabled. An operator enables it with the peer CLI and
may request a one-shot sync. A host-qualified local outbound persistence or a
peer-delivery failure signals the bounded non-durable scheduler. The scheduler
queries storage for local outbound canonical records addressed to that exact
peer and newer than `now - max_message_age`, then submits unchanged records to
`PeerHttpTransport` as independent jobs.

Each scheduler scan pages eligible records through the storage trait and
enqueues ordinary `WriteRequest` jobs. Global and per-host bounds limit
concurrent jobs; an in-flight ULID may be coalesced. The scheduler makes no
delivery-order promise across independent messages, does not require a stream,
and does not define a durable cursor. It holds only transient host/message-ID
work markers, bounded timing, and backoff—never a payload, receipt, retry
history, or delivery result. A persisted write signals the host. A signal that
arrives during a scan or active job requires another eligibility scan before
the host becomes idle; a restart safely drops transient work and later
rediscovers immutable records through normal idempotent delivery.

The post-write router calls the one coordinator handoff for every
host-qualified origin write after canonical persistence. It signals bounded
background work and returns; foreground admission never waits for another
message, DNS, connection, TLS, or peer receipt. A remote outcome is recorded
asynchronously and remains distinct from local admission under ADR-041.

After a delivery failure, the coordinator schedules the same host no earlier
than 60 seconds, then with exponential backoff capped at 15 minutes while
eligible records remain. Restart waits the same minimum before an eligible
attempt. Explicit one-shot sync uses the same scheduler and ordinary endpoint.
There is no ping, empty-peer monitor, batch endpoint, or recovery-specific
router.

The storage trait—not the daemon or transport—owns the backend-neutral query.
There is no outbox, replay store, retry queue, background monitor, cursor,
checkpoint, receipt, retry budget, or per-message delivery state. A duplicate
exact ULID/payload is idempotent under ADR-034; a same-ULID/different-payload
conflict remains a typed error with no side effects.

## Consequences

Reconciliation is bounded by explicit user policy and ordinary stored message
age. It provides a small recovery window after Wi-Fi/VPN connectivity returns without
creating a second write path or transport state machine. It never changes the
message ID or immutable payload, and it cannot synthesize delivery success.
Its retained events distinguish a scheduled/attempted scan from peer HTTP
acceptance.
