---
title: AI.28 bounded peer recovery after connectivity loss
status: proposed
branch: feature/pAI-s28-bounded-peer-recovery
target: integrate/phase-AI
depends_on: AI.21-pre, AI.26, AI.27
---

# AI.28 — bounded peer recovery after connectivity loss

## Release candidate

- First commit: set every releasable ATM assembly to `1.3.2-beta-28`; record
  matching client/daemon values from `atm doctor --json` in runtime evidence.

## Closure

After a peer-connectivity failure, one daemon-owned, single-flight drain per
configured peer re-sends recent immutable outbound records oldest-first over
one persistent peer HTTP(S) connection in the active wire-security profile. It keeps no message retry queue, outbox,
receipt, or per-message delivery state.

## Deliverables

1. Extend the existing durable `PeerSyncPolicy` with an operator-selected send
   window and bounded recovery cadence. The initial smoke policy is 10 minutes
   and batch cap 100. The policy enables recovery only for the exact trusted
   hostname; it does not retain an IP address.
2. Add `crates/atm-daemon/src/peer_drain_coordinator.rs`, owned and started by
   `composition.rs`. `runtime_health.rs::DaemonRequestDispatcher::route_write`
   must hand host-qualified writes to this coordinator after canonical local
   persistence instead of calling a second transport/write implementation.
   The sole post-write handoff is:

   ```rust
   trait PeerDeliveryCoordinator: Send + Sync {
       fn deliver_after_persist(
           &self,
           request: &WriteRequest,
           deadline: RequestDeadline,
       ) -> Result<(), AtmError>;
   }
   ```

   `deliver_after_persist` first drains all eligible older records and then the
   just-persisted record in `(created_at, message_ulid)` order on the same
   coordinator-owned connection. It returns peer acceptance for that request,
   or AI.26's `REMOTE_DELIVERY_UNCONFIRMED` at its deadline. If another lease
   is active, it signals its generation and waits only within this request's
   existing deadline; it never opens a second socket or creates a durable or
   per-message delivery record. The request-local waiter is discarded with the
   request and is not coordinator slot state.

   The coordinator owns exactly one non-durable slot per `HostName`:

   ```rust
   struct PeerDrainSlot {
       running: bool,
       requested_generation: u64,
       observed_generation: u64,
       next_attempt_at: Option<Instant>,
       backoff: Duration,
   }
   ```

   It must contain no message ULID, payload, cursor, receipt, or attempt
   history. A per-host single-flight lease covers scan, connection, ordered
   sends, and final rescan; different hosts may drain independently.
3. Replace `OutboundMessageQuery::recent_outbound_for_peer` with the following
   backend-neutral paged contract in `atm-core`/`atm-storage`; remove the old
   method and its implementations/tests in this sprint so the two queries do
   not coexist:

   ```rust
   fn page_for_peer(
       &self,
       peer: &HostName,
       not_before: IsoTimestamp,
       after: Option<(IsoTimestamp, MessageId)>,
       limit: NonZeroU16,
   ) -> Result<Vec<StoredPeerWrite>, AtmError>;
   ```

   It selects only the exact-peer,
   locally-originated immutable records within `max_message_age`, ordered by
   `(created_at, message_ulid)` ascending. The query accepts a transient
   exclusive `(created_at, message_ulid)` lower bound and returns at most
   `max_batch_messages`; no lower bound survives the active drain.
4. Extend `crates/atm-daemon/src/https_transport.rs` so one drain opens one
   HTTP(S) connection in the active wire-security profile and submits the
   normal canonical `WriteRequest` sequentially, waiting for each ordinary
   response before the next. It advances the transient lower bound through
   repeated pages until an empty page, transport failure, or cancellation. Do
   **not** add a `Vec<WriteRequest>` endpoint, batch envelope, or recovery-only
   router/handler. A transport failure stops that drain at the first
   unconfirmed write; a later run safely reuses its original ULID.
5. A successful local host-qualified persistence signals the slot generation.
   After the drain sees an empty page it compares `observed_generation` with
   `requested_generation`; a change requires another scan before release. A
   write after release obtains or signals the next lease. This is the required
   lost-wakeup closure: no write can fall between the final scan and lease
   release.
6. On failure, schedule the same host no earlier than 60 seconds; later
   failures use exponential backoff capped at 15 minutes. On any peer HTTP
   acceptance reset backoff to 60 seconds and continue the current ordered
   drain. A daemon restart creates no immediate probe: it waits at least 60
   seconds and only when eligible records exist. There is no periodic ping or
   empty-peer monitor.
7. Extend AI.27's foreground-only `PeerDeliveryEventKind` with
   `peer_recovery_scheduled`, `peer_recovery_attempt`,
   `peer_recovery_confirmed`, and `peer_recovery_unconfirmed`, and add the
   recovery-only bounded candidate-count and next-attempt fields. Emit those
   events with hostname, bounded candidate count, delay, and typed error but
   never body, certificate material, or message payload. Deliver each event to
   AI.27's `DaemonRequestDispatcher::record_peer_delivery_event`; that function
   alone updates the `PeerLinkStatus` projection.
8. Stop and discard the in-memory slot when the window is empty, policy is
   disabled, peer is revoked, or daemon shuts down. `atm peer sync <peer>` is
   one immediate use of the same coordinator/connection/endpoint, never a
   second loop.

## Implementation map

- `crates/atm-daemon/src/peer_drain_coordinator.rs` (new): the only owner of
  `PeerDrainSlot`, per-host lease acquisition, generation comparison, timing,
  and AI.27 event emission.
- `crates/atm-daemon/src/composition.rs`: constructs and starts exactly one
  coordinator; `runtime_health.rs` receives it by trait/object seam rather
  than constructing transport or storage coordination itself.
- `crates/atm-daemon/src/runtime_health.rs`: after the canonical write handler
  persists a host-qualified origin record, signal the coordinator; never call a
  second remote write implementation.
- `crates/atm-storage/src/contract.rs`: replace
  `OutboundMessageQuery::recent_outbound_for_peer` with `page_for_peer`; its
  SQLite implementation belongs in the storage crate and daemon/transport
  imports no SQLite type.
- `crates/atm-daemon/src/https_transport.rs`: expose one connection-scoped
  sequential canonical-write operation used by both foreground and drain work;
  it owns no scheduling or storage query.

## Acceptance criteria

- No network retry starts sooner than one minute after Wi-Fi/VPN loss, and no
  retry occurs while the eligible window is empty.
- Exactly one active drain/socket exists for a host. It drains ordered bounded
  pages oldest-to-newest on one connection until empty, failure, or
  cancellation; every entry is the ordinary `WriteRequest` endpoint and
  handler, not a batch/recovery endpoint.
- A write persisted during a drain is included before that lease exits; a
  write after it exits wakes the next lease. Tests force both race windows.
- A newly persisted write behind existing eligible backlog is sent only in its
  canonical ordered position. A concurrent foreground caller cannot bypass an
  active lease by opening a second socket; it receives peer confirmation only
  if its request completes within the shared deadline, otherwise the one
  truthful unconfirmed outcome.
- Backoff grows after failures, caps at 15 minutes, and resets only after peer
  HTTP acceptance.
- Recovery reuses original ULIDs and receiver duplicates remain idempotent.
- Events never call local persistence a recovery success and expose no body or
  certificate material.
- Exactly one coordinator and HTTPS delivery call path exist; transport imports
  no SQLite type and the coordinator accesses records only through
  `OutboundMessageQuery`.

## Required validation

Fake-clock tests for minimum/cap/reset/revoke/empty-window/restart;
single-flight same-host contention; deterministic oldest-first paging;
generation changes during final scan and after lease release; foreground write
behind older backlog; active-lease foreground deadline; one-connection
sequential writes; integration test with original ULID; event-schema and
doctor status tests; `just lint`; `just test`.

## Non-closure

No heartbeat/ping protocol, TCP probe loop, alternate transport, remote
delivery table, batch endpoint, or separate write path is added. This is a
bounded recovery coordinator, not a durable replay system or general
connectivity monitor.
