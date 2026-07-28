---
title: AI.31 asynchronous durable local admission
status: proposed
branch: feature/pAI-s31-async-local-admission
target: integrate/phase-AI
depends_on: AI.23, AI.27, AI.28
---

# AI.31 — asynchronous durable local admission

## Closure

A host-qualified `atm send` or `atm ack` returns after its immutable origin
record commits to SQLite. It does not wait for another message, peer DNS,
connection, TLS, remote handler, receipt, or nudge. The response means
**locally admitted**, never remotely delivered.

## Why

The current foreground `deliver_after_persist` call holds the local IPC
response open while it performs peer work. A healthy local daemon can then
appear unavailable when its client response deadline expires. Persisting the
message first already gives the system its recovery source; peer work belongs
after the response, not inside it.

## Current root cause and required simplification

The current path is slow because `DaemonRequestDispatcher::route_write` calls
`PostWriteRouter::dispatch` before `PreparedWrite::finish`. For a
host-qualified write, that call currently performs all of the following before
the local IPC handler can write response headers:

| Current site | Foreground work today | Required change |
| --- | --- | --- |
| `atm-core/src/send/mod.rs::prepare_send_context` | Config/hook lookup and recipient/delivery snapshot resolution | Read only daemon-owned, reloadable in-memory runtime view on admission. Move hook/nudge configuration use to post-commit work. No filesystem or SQLite read per admitted write. |
| `atm-core/src/ack/mod.rs::{resolve_acknowledgement_write,resolve_received_acknowledgement_write}` | Loads source and source record before persistence, then defers mutation | Replace with one storage transaction that resolves source data, inserts the immutable ACK reply, and conditionally marks source acknowledged. No application-layer source read before transaction. |
| `runtime_health/peer_delivery_router.rs::dispatch` | `list_trusted_peers`, authority resolution, delivery-event setup, then direct delivery | Validate syntax and authority from the in-memory runtime view; emit `write_persisted`; signal work by immutable message ID and return. |
| `peer_drain_coordinator.rs::acquire` | Slot creation, generation bookkeeping, mutex/condition-variable wait behind another request | Delete it from admission entirely. Admission never waits for another message. |
| `peer_drain_coordinator.rs::{drain,deliver_current}` | Policy/store reads, outbound page scan, JSON decode, DNS, TCP connect, TLS, HTTP write/read | Run only in post-commit worker jobs. None may execute before the local response. |
| `atm-core/src/send/mod.rs::PreparedWrite::finish` / `ack::ResolvedAcknowledgement::finish` | ACK source mutation occurs after peer delivery | Move ACK record plus source-state mutation into the one SQLite admission transaction. |
| `PreparedWrite::emit_local_post_write` | Nudge/hook emission runs in the request handler | Queue only the immutable message ID; worker loads/uses the committed record after response. |

The client currently gives local IPC roughly its request deadline plus a
250-ms response grace. That is where the observed `os error 35` originates;
raising that deadline is forbidden as a remediation. The accepted design is a
smaller request path, not a more patient client.

## Deliverables

1. First commit sets every releasable assembly to `1.4.0-beta-ai.31` and
   records matching CLI/daemon values from `atm doctor --json`.
2. In `crates/atm-daemon/src/runtime_health.rs`, reduce the request path to
   admission validation from an in-memory runtime view, one SQLite transaction,
   one `write_persisted` event, one non-blocking work signal, and the existing
   `SendResponseEnvelope::{Sent,Acknowledged}` response. Do not add a
   remote-success response variant.
   `crates/atm-daemon/src/composition.rs` constructs one immutable
   `AdmissionRuntimeView` from existing configuration/roster/trust data; the
   existing runtime-reload path atomically swaps that view. The view is a
   lookup cache, not another persistence store or delivery state machine.
3. Replace the foreground coordinator trait method with a signal-only seam in
   `crates/atm-daemon/src/peer_drain_coordinator.rs`:

   ```rust
   pub(crate) trait PeerDeliveryCoordinator: Send + Sync {
       fn signal_after_persist(&self, peer: HostName);
       fn sync_peer(&self, peer: &HostName, deadline: RequestDeadline)
           -> Result<PeerSyncOutcome, AtmError>;
   }

   enum PeerSyncOutcome {
       Confirmed,
       Unconfirmed { code: PeerDeliveryErrorCode },
       Expired { code: PeerDeliveryErrorCode },
   }
   ```

   `signal_after_persist` is non-blocking, cannot perform network I/O or a
   store read, and cannot make a successfully committed local write fail. It
   records only a bounded non-durable scheduling signal; no `WriteRequest`,
   payload, receipt, or per-message retry record is retained there.

   ```rust
   enum PostCommitWorkKey {
       LocalNudge(AtmMessageId),
       PeerDelivery { peer: HostName, message_id: AtmMessageId },
   }

   trait PostCommitWorkQueue: Send + Sync {
       fn signal(&self, work: PostCommitWorkKey);
   }
   ```

   The queue holds identifiers only. On bounded-queue pressure it coalesces a
   peer rescan signal rather than blocking, dropping a durable message, or
   retaining its payload. It has no state transitions beyond queued/in-flight
   coalescing.
4. In `runtime_health/peer_delivery_router.rs`, replace
   `deliver_to_peer(..., deadline, ...)` with one post-commit
   `signal_after_persist(peer)` call for every host-qualified origin write.
   Empty-host routing likewise emits only `LocalNudge(message_id)` work; the
   worker performs its existing nudge/hook behavior after the response. No
   special localhost or self-IP route is permitted.
5. Move `ResolvedAcknowledgement::finish` into the same SQLite transaction as
   the acknowledgement record. An ACK admission response therefore proves both
   writes committed; peer delivery of the reply remains post-commit work.
   Replace `resolve_acknowledgement_write` and
   `resolve_received_acknowledgement_write` with one sealed storage/runtime
   operation whose transaction returns the reply target and typed
   already-acknowledged/not-found outcome. Do not perform a source lookup in
   `atm-core` before calling that operation.
6. Retain AI.27 events, but make their meanings precise: `write_persisted` is
   emitted before response; later worker outcomes produce
   `peer_delivery_confirmed` or `peer_delivery_unconfirmed`. No admission
   response, event, or CLI prose may call local persistence “remote sent.”
7. Update `ADR-038`, `ADR-041`, and the requirements to state that the local
   request deadline ends at admission response and never owns background peer
   work.
8. Extend the daemon boundary TOML record and
   `crates/atm-architecture/tests/boundary_enforcement.rs`: the admission
   files may signal `PostCommitWorkQueue` but must reject direct
   `PeerHttpTransport`/delivery, peer-store scans, DNS/socket/TLS, hooks, and
   nudge calls. The scheduler must reject concrete SQLite imports and durable
   payload/receipt/retry state.

## Paths to delete

- `PeerDeliveryCoordinator::deliver_after_persist(...)` and every foreground
  call site.
- `PostWriteRouter::deliver_to_peer(..., RequestDeadline, ...)`; its peer
  transport/error classification belongs to the worker, not local admission.
- `PreparedWrite::finish` as a deferred ACK mutation boundary; acknowledgement
  source mutation becomes part of the admission transaction.
- Application-layer `load_ack_source` and `load_ack_source_record` calls on
  the admission path.
- Foreground post-send hook/config lookup from `prepare_send_context` and
  `PreparedWrite::emit_local_post_write`.

## Implementation map

- `crates/atm-daemon/src/composition.rs`: construct/swap the immutable
  admission runtime view and one post-commit queue.
- `crates/atm-daemon/src/runtime_health.rs`: use that view and return the
  admission response immediately after the transaction and work signal.
- `crates/atm-daemon/src/runtime_health/peer_delivery_router.rs`: classify
  local-nudge versus host-qualified work and signal only `PostCommitWorkKey`.
- `crates/atm-core/src/send/mod.rs`: remove deferred completion semantics;
  retain only data needed for the admission transaction and post-commit key.
- `crates/atm-core/src/ack/mod.rs` and the sealed storage/runtime boundary:
  replace source-read/finish with the atomic acknowledgement transaction.

## Required validation

- A real local IPC integration test installs a peer transport that deliberately
  blocks beyond the old response grace period. The same host-qualified send and
  host-qualified ack must receive successful local admission before that peer
  transport is released; the immutable record is present in SQLite.
- The existing normal remote HTTP endpoint, dispatcher, persistence method, and
  post-write router remain the only peer path for localhost, self-IP, and a
  remote host. Add an AST/source boundary test that rejects a direct transport
  call from `runtime_health.rs` or `peer_delivery_router.rs`.
- Peer failure after admission creates exactly one retained unconfirmed event
  with the original ULID and does not convert the local response to
  `ATM_DAEMON_UNAVAILABLE`.
- Bare same-agent/no-host self-send remains rejected; host-qualified localhost
  and self-IP remain admitted through the ordinary host route.
- Runtime-view tests prove one admission performs no peer-config store read,
  policy read, outbound-page read, DNS call, socket open, TLS operation, or
  hook/nudge execution before its response is written.
- ACK tests prove source resolution, reply insertion, and source transition are
  atomic: a typed pending/source error leaves no reply row; a successful ACK
  has both rows committed before response; no test double observes an
  application-layer source read before the storage transaction.
- `just lint`, `just test`, and branch-daemon `just smoke localhost` pass.

## Acceptance criteria

- SQLite commit is the sole synchronous post-validation operation before a
  local send/ack response.
- An ACK admission transaction persists both the immutable reply and its source
  acknowledgement state before response; it never waits for reply delivery.
- A slow or unavailable peer cannot delay, cancel, or relabel a committed local
  admission response.
- No outbox, durable queue, receipt, delivery state, or extra public endpoint
  is introduced.

## Non-goals

This sprint does not define worker concurrency, delivery order, capacity, or
cross-host evidence. Those belong to AI.32 and AI.33.
