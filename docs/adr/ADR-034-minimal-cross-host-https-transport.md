# ADR-034 — Minimal Cross-Host HTTPS Transport

| Field | Value |
| --- | --- |
| ID | ADR-034 |
| Status | Proposed |
| Scope | Repository-wide |
| Relates to | ADR-018, ADR-028, ADR-029, ADR-030, ADR-033, Phase AI |

## Decision

Cross-host communication is HTTPS requests to the same daemon HTTP router used
by local UDS clients. A daemon is simultaneously an HTTPS listener for allowed
peers and an HTTPS client when its post-write router selects a remote host.
There is no second daemon and no cross-host application service.

The only cross-host transport responsibilities are:

1. bind enabled HTTPS interfaces;
2. authenticate TLS peers and enforce the exact peer allowlist;
3. serialize a canonical HTTP request and write it to a socket; and
4. hand an accepted inbound request to the common router.

Interface bindings, local certificate identity, and peer trust records are
SQLite-backed configuration managed by CLI. Environment variables are not an
operator configuration surface. Peer trust is exact host identity plus pinned
certificate fingerprint; adding or replacing a trust record requires explicit
operator confirmation. The initial certificate may be self-signed and generated
on demand. Unauthenticated or untrusted TLS peers are rejected before routing.

The daemon has no cross-host outbox, replay store, retry state, receipt
synthesis, per-host acknowledgement state, or duplicate-delivery subsystem.
Messages are immutable and carry their existing ULID identity. Storage accepts
the same message identity idempotently on either host.

## Consequences

An unavailable remote peer returns a normal transport error for that write
attempt. It does not mutate a local remote-message state machine. Operators may
retry by issuing the same immutable message identity through the ordinary write
path.

ADR-028, ADR-029, ADR-030, and ADR-031 are superseded by this integrated
interface, trust, TLS, and transport decision.
