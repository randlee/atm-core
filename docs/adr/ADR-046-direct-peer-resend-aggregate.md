# ADR-046 — Direct Peer Resend Aggregate

| Field | Value |
| --- | --- |
| Status | Accepted |
| Scope | Repository-wide |
| Amends | ADR-047 |

## Decision

`peerOutbound` remains the sole durable record of an undelivered peer write.
The product setting `peer_resend_cache` defaults on, but disabled mode makes
the direct ADR-047 call unchanged: it neither locks resend state nor reads a
backlog nor retries.

Enabled caching owns one daemon-memory aggregate per configured canonical
endpoint. Its only states are `Connected`, `Disconnected` (one attempt is in
progress), and `Queued { due_at }`. The aggregate stores no write payload,
ULID, receipt, health claim, agent state, or session state. A failed direct
attempt queues that endpoint for a 60-second base plus deterministic
endpoint-only jitter bounded at 6 seconds; a queued or in-progress admission
keeps its immutable marker and returns `REMOTE_DELIVERY_UNCONFIRMED`.

The existing local ingress serve loop is the sole timer owner. It runs one due
endpoint and one oldest-first page of at most 64 frames through ADR-047's
existing `send_peer_http_frames` function using a 250-ms callback deadline.
No coordinator, worker, task, channel, connection pool, DNS thread, peer scan,
or alternate sender is permitted.

On daemon construction and config reload, enabled caching performs exactly one
read-only distinct-host query of retained `peerOutbound` records. It resolves
each host only through the immutable configured peer directory; an absent host
stays durable and unqueued. This restart bootstrap is not a recurring poll.

## Consequences

- Direct delivery remains the cache-disabled fast path and first cross-host
  proof; caching is additive, not an admission prerequisite.
- Retry state disappears at daemon exit. Durable immutable writes may be
  rediscovered after restart without a retry table or cursor.
- Receiver persistence and nudging remain the ordinary shared peer ingress
  path, never a resend signal or special receiver route.
