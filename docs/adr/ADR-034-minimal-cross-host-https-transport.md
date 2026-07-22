# ADR-034 — Minimal Cross-Host HTTPS Transport

| Field | Value |
| --- | --- |
| ID | ADR-034 |
| Status | Accepted |
| Scope | Repository-wide |
| Relates to | ADR-018, ADR-028, ADR-029, ADR-030, ADR-033, Phase AI |

## Decision

Cross-host communication is HTTPS requests to the same daemon HTTP router used
by Unix UDS and loopback-TCP local clients. A daemon is simultaneously an HTTPS
listener for allowed peers and an HTTPS client when its post-write router
selects a remote host.
There is no second daemon and no cross-host application service.

The only cross-host transport responsibilities are:

1. bind enabled HTTPS interfaces;
2. authenticate TLS peers and enforce the exact peer allowlist;
3. serialize a canonical HTTP request and write it to a socket; and
4. hand an accepted inbound request to the common router.

Interface bindings, local certificate identity, and peer trust records are
durable configuration behind the storage trait, managed by CLI. SQLite is the
initial backend; neither the HTTPS adapter nor daemon runtime imports SQLite
types. Environment variables are not an operator configuration surface. Peer
trust is exact host identity plus pinned certificate fingerprint; adding or
replacing a trust record requires explicit operator confirmation. The initial
certificate may be self-signed and generated on demand. Unauthenticated or
untrusted TLS peers are rejected before routing.

The daemon has no cross-host outbox, replay store, retry state, receipt
synthesis, per-host acknowledgement state, or duplicate-delivery subsystem.
Messages are immutable and carry their origin-created ULID identity. The exact
ULID and immutable payload are persisted on both hosts. Storage treats an
exact duplicate as idempotent with no new nudge or acknowledgement transition;
the same ULID with any differing immutable field is a typed conflict, is
logged structurally, preserves the original record, and has no side effect or
panic.

The HTTPS adapter applies bounded `5s` connect, TLS-handshake, and request
deadlines, and rejects an over-limit body before decode. Listener startup
validates every enabled interface and certificate reference before publishing
any listener; an invalid configuration or bind failure leaves no partial HTTPS
service. On shutdown it stops accepts then drains or cancels tracked work
within the daemon shutdown deadline.

## Consequences

An unavailable remote peer returns a normal transport error for that write
attempt. The canonical local write may already have stored the sender's
immutable outbound record, but it creates no recipient-inbox row, remote state
machine, receipt, or retry state. Operators may retry the same immutable
message identity through the ordinary write path. ADR-038 is the sole permitted
bounded, user-selected reconciliation mechanism: it re-sends existing immutable
records through this adapter and creates no outbox, queue, checkpoint, receipt,
or per-message delivery state.

ADR-028, ADR-029, ADR-030, and ADR-031 are superseded by this integrated
interface, trust, TLS, and transport decision.
