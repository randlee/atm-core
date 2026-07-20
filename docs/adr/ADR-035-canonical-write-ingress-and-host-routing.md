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
method. The write handler persists the immutable message idempotently, applies
the optional acknowledgement transition, and emits the existing post-write
event. Inbound HTTPS then has no cross-host-specific mailbox, acknowledgement,
or nudge branch.

Routing is decided exactly once by the post-write event router:

- an empty destination host or this daemon's configured host identity selects
  local nudge delivery;
- any other validated destination host selects the HTTPS transport adapter.

The destination host is stable message metadata. A transient authenticated peer
identity is request context for authorization and observability only; it cannot
replace or rewrite the destination host. `localhost` and a daemon's own
advertised address are ordinary host values and exercise the HTTPS route rather
than a special loopback implementation.

## Prohibitions

- No Compose/DirectDeliver split or equivalent renamed pair.
- No separate ack sender, ack transport, or sender-side ack state mutation.
- No second routing decision in CLI, graft, HTTP, TLS, storage, or nudge code.
- No cross-host-specific persistence or inbound nudge handler.

Architecture checks must reject these shapes structurally, not merely by a
denylist of historical identifiers.
