---
title: AI.12 canonical post-write router
status: complete
branch: feature/pAI-s12-post-write-router
worktree: ../atm-core-worktrees/feature/pAI-s12-post-write-router
target: integrate/phase-AI
depends_on: AI.11
---

# AI.12 — canonical post-write router

AI.12 changes write ordering and routing ownership only; AI.11 remains the
owner of HTTP resource schemas and local transport admission.

## Purpose

Make one canonical write pipeline enforce the required order for every ingress:

```text
decode shared HTTP request -> canonical write handler -> idempotent persistence
-> optional receiver-side acknowledgement mutation -> PostWriteRouter
```

The router runs only after successful persistence/mutation. It makes the sole
destination-host decision and performs one post-write action: local nudge for
an empty host, or peer HTTPS delivery for a present host. Send and ack use the
same `WriteRequest`; `acknowledges_message_id` is their only semantic
difference.

## Deliverables

1. Delete `route_write`'s pre-persistence host branch and the no-op
   `PostWriteRouter` implementation. `route_write` must call the canonical
   writer for every write, then invoke `PostWriteRouter::dispatch` once.
2. Keep persistence and optional acknowledgement mutation inside the canonical
   writer. Neither local nudge nor HTTPS transport can run before it succeeds.
3. Move the only delivery `destination.host` inspection and local-vs-peer
  selection into `PostWriteRouter::dispatch`. The authenticated receiving
  ingress may inspect a host-qualified address solely to select its local
  persistence-admission rule; it must not nudge, deliver, or select a route.
  The peer adapter carries the exact origin ULID
  and immutable message fields, but consumes the origin-only destination
  routing selector before receiver-side dispatch to prevent forwarding loops.
4. For an empty destination host, the router emits exactly one local post-write
   nudge for a newly persisted message and no peer request.
5. For a present destination host, the router invokes only `PeerHttpTransport`
   after persistence. It does not create replay, retry, receipt, queue, or
   remote-ack state. The receiving daemon enters the same canonical handler;
   its peer adapter removes transport-routing metadata before router dispatch so
   it cannot forward the same write back to the peer.
6. Keep all callers—CLI, graft, Unix UDS, loopback TCP, and HTTPS—on the same
   `ApiRouter` -> canonical writer -> `PostWriteRouter` graph. No adapter may
   call storage, nudge, acknowledgement mutation, or peer transport directly.
7. Preserve roster ownership: a local-origin write for a present destination
   host records only the sender's immutable outbound record and does not query
   that remote recipient roster. The authenticated receiving peer invocation of
   the same canonical handler validates its own recipient roster before it
   persists the recipient record. This distinction is derived from
   `AuthenticatedIngress`, never socket family or a second write path.
8. Rewrite—not supplement—
   `crates/atm-architecture/tests/boundary_enforcement.rs::canonical_write_router_has_one_host_routing_decision`.
   Its AST assertion and failure text must change from "only `route_write` may
   select local or remote write dispatch" to proving that only
   `PostWriteRouter::dispatch` selects local nudge versus peer delivery after
   the canonical writer. The replacement rejects a `route_write` host branch,
   direct `PeerHttpTransport::deliver`, or direct nudge call outside that
   router. This closes `AI11-BLOCKING-03-POSTWRITEROUTER-NOOP` rather than
   creating a second overlapping architecture gate.

## Invariants

- A nudge is emitted only after the associated message is durable and visible
  to a read. Duplicate ULID writes do not create a second row or nudge.
- The origin creates the message ULID and timestamp once. Peer transport
  carries those exact immutable fields, and the receiving host persists that
  same ID and timestamp; no receiving adapter, router, or storage method may
  generate replacements.
- A repeated ULID is idempotent only when every immutable message field matches
  the existing record. A mismatched payload for an existing ULID is a typed
  conflict: retain the original row, emit a structured error log for follow-up,
  return an error, and perform no nudge, acknowledgement mutation, or peer
  delivery. It is never a panic or overwrite.
- A receiver-side acknowledgement mutation occurs only after its acknowledgement
  write is durable and its one router-selected peer delivery succeeds. The
  router consumes the canonical resolved reply payload, so acknowledgement and
  send do not have separate outbound paths. A failed peer delivery never marks
  the remote message acknowledged.
- Router failure is returned as the post-write delivery result; it does not
  introduce an outbox or alter the immutable message payload.
- Socket family, listener address, and peer address never decide message
  routing. Only the canonical destination host field does.
- The source host is durable message provenance and remains visible to reads,
  nudges, and acknowledgement reply construction. The destination host is an
  outbound routing selector: the sending router consumes it to choose the peer,
  and the authenticated receiving adapter removes that selector before the
  receiving post-write router runs. Confirm ADR-035 and the address projection
  contract remain consistent with this distinction when AI.12 lands.
- A remote-recipient roster rejection is produced only by the receiving host's
  canonical handler. The origin retains no recipient inbox row and makes no
  remote acknowledgement mutation.

## Required tests

| Test | Required proof |
| --- | --- |
| Ordering | Instrument canonical persistence, acknowledgement mutation, nudge, and peer transport; assert persistence/mutation precede exactly one router action for both send and ack. |
| Local write | Empty host persists once, emits one nudge after persistence, and never calls peer transport. |
| Peer write | Present host persists once, calls peer transport once after persistence, and never emits a local recipient nudge before remote acceptance. |
| Inbound peer | An mTLS-delivered write reaches the same writer and post-write router without re-forwarding to its source host. |
| Failure | Failed peer send returns the transport error, does not synthesize replay/receipt state, and does not mutate the target acknowledgement state. |
| Roster ownership | Origin sends to an unknown remote recipient without a local roster lookup; receiving peer rejects it from its local roster and leaves recipient state unchanged. |
| Idempotency | A peer-delivered write retains the origin ULID exactly. Replaying the same immutable payload returns the existing record and does not repeat nudge, acknowledgement mutation, or peer delivery. Reusing the ULID with different immutable data returns a typed conflict, logs the discrepancy, preserves the original, and has no side effects. |
| Ingress parity | CLI, graft, UDS, loopback TCP, and HTTPS fixtures traverse the same typed writer/router instrumentation points. |

## Boundary enforcement

Use an AST-aware check, not raw text matching, to prove:

1. exactly one production `ApiRouter`, `MessageWriter`, and `PostWriteRouter`
   implementation owns write flow;
2. delivery selection on `destination.host` occurs only in the
   `PostWriteRouter` implementation; authenticated receiving ingress may use a
   host-qualified origin only for local roster admission, never routing;
3. storage write and acknowledgement mutation calls occur only in the canonical
   writer; and
4. nudge and `PeerHttpTransport::deliver` calls occur only in the post-write
   router.

The check resolves module/use aliases before evaluating calls and has negative
fixtures for a second writer, a pre-write nudge, a pre-write peer send, and a
delivery host check outside the router, while retaining a positive fixture for
the documented receiving-host roster-admission check.

## Non-goals

- No new transport protocol, message envelope, acknowledgement type, delivery
  state machine, replay store, retry queue, or receipt.
- No changes to HTTP resource schemas beyond AI.11's HTTP-contract remediation.
- No live two-machine release proof; AI.13–AI.15 own reusable smoke definition
  and host-pair execution.

## Acceptance criteria

- The pre-router remote branch and no-op router are deleted.
- All write ingress paths satisfy the ordered shared pipeline.
- `just lint`, `just test`, AST boundary tests, and ordering/idempotency tests
  pass.
- The implementation removes the pre-persistence `route_write` remote branch
  and no-op router; report the deleted symbols and net LOC with closure
  evidence.

## Implementation record

- `route_write` no longer branches on `destination.host` before the canonical
  writer. `PostWriteRouter::dispatch` is the sole daemon host-routing point
  and invokes peer delivery only after a successful durable write.
- A host-qualified origin writes a non-roster-backed immutable outbound record
  into the existing message storage contract. This is not an outbox, queue,
  replay store, or receipt state: it is the same canonical message record,
  retained before its one peer-delivery attempt.
- `WriteRequest.origin_message_id` is assigned once by the origin canonical
  writer, carried by the peer HTTP payload, and accepted only from authenticated
  peer ingress. Local clients cannot provide it. The receiver therefore stores
  the same ULID while the peer adapter removes only the destination routing
  selector.

## Required validation

Run the ordering, idempotency/conflict, local, peer, inbound-peer, failure, and
ingress-parity tests listed above; run the AST boundary negatives and
`cargo test -p atm-architecture canonical_write_router_has_one_host_routing_decision`;
then run `just lint` and `just test`.
