# ADR-035 — Canonical Write Ingress And Host Routing

| Field | Value |
| --- | --- |
| ID | ADR-035 |
| Status | Accepted |
| Scope | Repository-wide |
| Relates to | ADR-012, ADR-018, ADR-019, ADR-033, ADR-034, Phase AI |

## Decision

Every write—CLI send, CLI ack, graft send, Unix UDS HTTP, loopback-TCP HTTP,
and inbound HTTPS—uses one canonical `WriteRequest`, one write handler, and
one storage method. The handler orders work exactly as: idempotent persistence,
optional receiver-side acknowledgement transition, then one post-write event.
An event cannot precede a visible persisted write. Inbound HTTPS has no
cross-host-specific mailbox, acknowledgement, or nudge branch.

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

The source host is durable message provenance. The destination host is an
origin-side routing selector consumed before an authenticated peer ingress is
given to the receiver-side post-write router; it is not re-forwarded by that
receiver. A transient authenticated peer identity is request context for
authorization and observability only; it cannot replace source provenance or
invent a routing selector. `localhost` and a daemon's own advertised address
are ordinary host values and exercise the HTTPS route rather than a special
loopback implementation.

The destination routing selector is not part of the immutable receiver message
payload. Peer transport carries the exact origin ULID and immutable message
fields, and removes the consumed selector before receiver-side canonical write
handling. This prevents an inbound peer write from selecting another peer path
without adding an inbound special handler.

The origin creates the ULID once. Repeating the same ULID with an identical
immutable payload is a no-op after the original result; reusing it with a
different immutable payload returns a typed conflict, logs the discrepancy,
preserves the original, and emits no nudge, acknowledgement transition, or
peer delivery. The origin never preflights the remote roster: the receiving
daemon validates its own roster in this same handler and returns its ordinary
error response.

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
- No host inspection before persistence or outside `PostWriteRouter`.
- No socket-family or socket-address inference of local versus peer ingress.

Architecture checks must reject these shapes structurally, not merely by a
denylist of historical identifiers.

The router receives an authenticated ingress context. The peer form is an
opaque `AuthenticatedPeer` constructed only by the HTTPS adapter after mTLS and
exact configured-host/fingerprint verification; adapters cannot fabricate it.
This type preserves the authenticate-before-route invariant without adding a
peer-specific application handler.

## Compliance status

This ADR is the accepted target contract. At the current integrated tip, the
pre-persistence `route_write` remote-host branch and no-op
`PostWriteRouter::dispatch` remain; AI.12 is the sole owning sprint for their
deletion and the structural enforcement of this ADR. Until AI.12 closes, no
current code claim may describe the post-write routing invariants as enforced.
