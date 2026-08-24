# ADR-053 — Peer-Write Redial and Delivery-Attempt Invariant

| Field | Value |
| --- | --- |
| Status | Accepted |
| Scope | Outbound daemon-owned direct-peer HTTP/1 connections |
| Relates to | ADR-032, ADR-035, ADR-047 |

## Context

The Tokio/Axum daemon persists a host-qualified write before it performs its
best-effort peer acknowledgement. A connection pool can avoid repeated TCP,
mTLS, and HTTP/1 setup, but an automatic resend after an I/O failure would
make a durable request whose delivery status is uncertain execute twice.

The receiver does not promise an idempotency protocol for an arbitrary
possibly-delivered peer request. Connection reuse must therefore improve only
transport establishment; it must not change the existing one-attempt delivery
contract or ADR-032 error taxonomy.

## Decision

The daemon pools negotiated HTTP/1 senders by configured `HostName` authority,
never by resolved address. A pooled sender is checked only while it is being
acquired, before a request is handed to `exchange`. If that check finds a
closed sender, the pool discards it and performs one replacement dial using
the request's remaining absolute deadline.

Once `exchange` receives a request, the attempt count is exactly one. The
daemon does not retry a request-write, response-read, or response-decode
failure. The caller receives the existing error variant and message mapping.
This includes the kept-alive race in which a peer closes a connection after
acquire but during an exchange: the resulting failure is not evidence that a
retry is safe.

Pool capacity bounds retained entries, not writes. A capacity-constrained
write dials an unpooled connection instead of waiting behind a pool slot or
being rejected. A retained reservation is released exactly once when its
connection is discarded or evicted. If a stale-entry replacement dial fails,
the pool releases the stale reservation before surfacing the normal direct
peer connect failure; no guard exists on that path to perform a later release.

Guard drop is synchronous and does not wait. Daemon shutdown first stops HTTP
admission and drains requests, then invokes the pool shutdown path: it closes
every retained sender and boundedly waits for its connection-driver task,
aborting a driver that exceeds the shutdown allowance. This provides bounded
teardown without awaiting while a pool mutex or guard drop is active.

## Consequences and failure modes

- **No retry after exchange.** A mid-request connection failure can leave
  delivery unknown, so the pool reports it once rather than issuing a possible
  duplicate write. The residual kept-alive race is intentionally visible to
  the caller.
- **Replacement-dial failure.** A sender found stale before exchange can be
  redialed safely. If that redial fails, the old reservation is released in
  `acquire` before the existing `PeerConnect` or `PeerConnectTimeout` failure
  is returned.
- **Bounded teardown drain.** Idle entries are closed, their driver tasks are
  awaited only until the daemon's fixed shutdown deadline, and overdue tasks
  are aborted with a short bounded join grace. This prevents shutdown from
  leaking detached retained-connection tasks.
- **Pool exhaustion.** An overflow connection is deliberately not retained;
  it preserves availability of an already-durable write while retaining the
  configured memory and idle-connection bounds.

## Rejected alternatives

1. **Retry every failed exchange.** Rejected because a failed response read
   cannot prove the receiving peer did not apply the request.
2. **Pool raw authenticated streams.** Rejected because each use would require
   a new HTTP/1 handshake and would orphan or duplicate the connection driver.
3. **Wait for a retained slot.** Rejected because pool capacity is an
   optimization limit, not a new durable-write backpressure policy.
4. **Await a driver in guard `Drop`.** Rejected because Rust drop is
   synchronous and must not block request completion or hold a pool lock.
