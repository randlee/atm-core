# ADR-035 — Canonical Write Ingress And Host Routing

| Field | Value |
| --- | --- |
| ID | ADR-035 |
| Status | Proposed |
| Scope | Repository-wide |
| Relates to | ADR-012, ADR-018, ADR-019, ADR-033, ADR-034, Phase AI |

## Decision

Every write—CLI send, CLI ack, graft send, local UDS HTTP, and inbound
HTTPS—uses one canonical `WriteRequest`, one write handler, and one storage
method. The write handler persists the immutable sender or receiver record
idempotently, applies the optional acknowledgement transition, and emits the
existing post-write event. Inbound HTTPS then has no cross-host-specific
mailbox, acknowledgement, or nudge branch.

Routing is decided exactly once by the post-write event router:

- an empty destination host selects local nudge delivery;
- every validated present destination host selects the HTTPS transport adapter,
  including `localhost` and this daemon's advertised or bound IP address.

For a remote destination, the local write records the sender's immutable
outbound message before this decision; the remote daemon performs the
recipient-side write when it receives the same `WriteRequest`. A failed HTTPS
attempt does not add local delivery state, a remote recipient row, a receipt,
or a retry queue. It returns one transport error; a repeated write reuses the
same immutable message ULID.

The destination host is stable message metadata. A transient authenticated peer
identity is request context for authorization and observability only; it cannot
replace or rewrite the destination host. `localhost` and a daemon's own
advertised address are ordinary host values and exercise the HTTPS route rather
than a special loopback implementation.

Source and destination chat IDs are stable address metadata under ADR-037. The
write handler persists them unchanged; the post-write router ignores them when
choosing local versus remote transport. This preserves `hendrix:12345` and
`hendrix:98765` as independent reply and nudge targets without creating a
second routing or acknowledgement path.

## Prohibitions

- No Compose/DirectDeliver split or equivalent renamed pair.
- No separate ack sender, ack transport, or sender-side ack state mutation.
- No second routing decision in CLI, graft, HTTP, TLS, storage, or nudge code.
- No cross-host-specific persistence or inbound nudge handler.

Architecture checks must reject these shapes structurally, not merely by a
denylist of historical identifiers.
