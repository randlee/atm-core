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
peer-delivery failure signals the one bounded per-host drain coordinator. The
coordinator queries storage for local outbound canonical records addressed to
that exact peer and newer than `now - max_message_age`, then submits each
unchanged record to `PeerHttpTransport`.

Each scan advances a transient exclusive `(created_at, message_ulid)` lower
bound through pages of at most `max_batch_messages`, ordered oldest first,
until it observes an empty page, a transport failure, or cancellation. One
in-memory lease per `HostName` covers storage paging, one HTTP(S) connection in
the active wire-security profile, sequential ordinary `WriteRequest`
submissions, and final rescan. It carries only running
state, a wake generation, next attempt time, and backoff; it stores no message
ID, payload, cursor, receipt, or delivery result. A persisted write increments
the generation. Before lease release, an empty final scan must observe the same
generation; otherwise it scans again. A post-release signal starts the next
lease.

The post-write router calls the one coordinator handoff for every
host-qualified origin write after canonical persistence. A foreground request
waits behind that same host lease and older ordered records only within its
existing request deadline; it never opens a second socket. This transient
request-local wait is neither a coordinator slot field nor durable delivery
state. Its timeout is the one truthful unconfirmed outcome defined by ADR-041.

After a delivery failure, the coordinator schedules the same host no earlier
than 60 seconds, then with exponential backoff capped at 15 minutes while
eligible records remain. Restart waits the same minimum before an eligible
attempt. Explicit one-shot sync uses the same lease and connection. There is no
ping, empty-peer monitor, batch endpoint, or recovery-specific router.

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
