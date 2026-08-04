# ADR-035 — Canonical Write Ingress And Host Routing

| Field | Value |
| --- | --- |
| ID | ADR-035 |
| Status | Accepted; amended by ADR-047 |
| Scope | Repository-wide |
| Relates to | ADR-012, ADR-018, ADR-019, ADR-033, ADR-034, Phase AI |

## Decision

Every write—CLI send, CLI ack, graft send, Unix UDS HTTP, loopback-TCP HTTP,
and configured peer HTTP—uses one canonical `WriteRequest`, one write handler, and
one storage method. The handler orders work exactly as: idempotent persistence,
optional receiver-side acknowledgement transition, then one post-write event.
An event cannot precede a visible persisted write. Inbound peer HTTP has no
cross-host-specific mailbox, acknowledgement, or nudge branch.

Before the canonical write, a host-qualified origin destination is normalized
once against the daemon-owned immutable `PeerDirectory`. Explicit configured
host/IP aliases map to one canonical hostname; the canonical hostname is the
only host retained in immutable origin metadata. This configuration lookup has
no DNS, SQLite query, peer scan, worker, or socket operation.

Routing is decided exactly once by the post-write event router:

- an empty destination host selects the ordinary local post-write nudge; and
- every validated present destination host, including `localhost` and this
  daemon's advertised or bound IP address, selects the one direct configured
  peer HTTP call.

For a remote destination, the local write records the sender's immutable
outbound message before this decision. The initiating request worker then
makes one bounded direct peer HTTP call with that exact immutable
`WriteRequest`; a matching canonical response confirms delivery. A failure
returns `REMOTE_DELIVERY_UNCONFIRMED` after persistence and leaves only the
existing `peerOutbound` marker. It adds no local delivery state, remote row,
receipt, worker, scheduler, or retry queue.

The source host is durable message provenance. The destination host is an
origin-side routing selector consumed before an authenticated peer ingress is
given to the receiver-side post-write router; it is not re-forwarded by that
receiver. A transient authenticated peer identity is request context for
authorization and observability only; it cannot replace source provenance or
invent a routing selector. `localhost` and a daemon's own advertised address
are ordinary host values and exercise the configured peer HTTP route rather than a special
loopback implementation.

The destination routing selector is not part of the immutable receiver message
payload. Peer transport carries the exact origin ULID and immutable message
fields, and removes the consumed selector before receiver-side canonical write
handling. This prevents an inbound peer write from selecting another peer path
without adding an inbound special handler.

The origin creates the ULID once. Repeating the same ULID with an identical
immutable payload logs a skipped database write. The same-store receipt log is
`peer_duplicate_write_skipped` with the ULID, source/destination host,
`same_store_peer_receipt=true`, `database_write=skipped`, and
`delivery=continued`. An already-delivered remote
duplicate is otherwise a no-op. The narrow same-host peer receipt that finds
this daemon's retained host-qualified origin record continues the ordinary
inbound local nudge after the skipped write, without mutating the origin
record or re-entering peer delivery. A later ACK reads that retained origin
destination host as its reply-routing target and creates the same canonical
write with `acknowledges_message_id`; it does not fabricate source provenance
or add an ACK transport branch. Reusing a ULID with different immutable
payload returns a typed conflict, logs the discrepancy, preserves the
original, and emits no nudge, acknowledgement transition, or peer delivery.
The origin never preflights the remote roster: the receiving daemon validates
its own roster in this same handler and returns its ordinary error response.

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
- No host inspection before persistence or outside `PostWriteRouter`. The
  router may read only its immutable daemon-owned runtime view; it must not
  read a configuration/policy store, query outbound records, or invoke DNS,
  socket, TLS, hook, nudge, or peer transport code.
- No socket-family or socket-address inference of local versus peer ingress.

Architecture checks must reject these shapes structurally, not merely by a
denylist of historical identifiers.

The configured peer HTTP adapter supplies `AuthenticatedIngress::Peer` after
validating its finite local bind configuration. Its `X-ATM-Peer-Source-Host`
header is display provenance only: it cannot authorize a sender, select a
recipient, or choose a second route. Every accepted local, loopback, same-IP,
and cross-host frame reaches the same router, handler, and post-write nudge.

## Compliance status

This ADR is the accepted target contract. AI.23 is the sole owner for removing
any remaining pre-persistence host branch and for enforcing adapter convergence
on the one `ApiRouter`/dispatcher/write/post-write chain. AI.24 owns the
narrow same-store host-qualified duplicate receipt proof. Until their runtime
evidence and structural gates close, no current code claim may describe these
post-write routing invariants as enforced.
