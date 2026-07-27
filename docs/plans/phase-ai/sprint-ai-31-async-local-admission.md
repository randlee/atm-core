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

## Deliverables

1. First commit sets every releasable assembly to `1.4.0-beta-ai.31` and
   records matching CLI/daemon values from `atm doctor --json`.
2. In `crates/atm-daemon/src/runtime_health.rs`, keep
   `MessageWriter::write(...)` as the only synchronous write-path operation.
   It prepares/persists the canonical record, calls the post-write router, and
   immediately builds the existing `SendResponseEnvelope::{Sent,Acknowledged}`
   admission response. Do not add a remote-success response variant.
3. Replace the foreground coordinator trait method with a signal-only seam in
   `crates/atm-daemon/src/peer_drain_coordinator.rs`:

   ```rust
   trait PeerDeliveryCoordinator: Send + Sync {
       fn signal_after_persist(&self, peer: HostName);
       fn sync_peer(&self, peer: &HostName, deadline: RequestDeadline)
           -> Result<u16, AtmError>;
   }
   ```

   `signal_after_persist` is non-blocking, cannot perform network I/O, and
   cannot make a successfully committed local write fail. It records only a
   bounded non-durable scheduling signal; no `WriteRequest`, payload, receipt,
   or per-message retry record is retained there.
4. In `runtime_health/peer_delivery_router.rs`, replace
   `deliver_to_peer(..., deadline, ...)` with one post-commit
   `signal_after_persist(peer)` call for every host-qualified origin write.
   Local/no-host nudge handling stays unchanged. No special localhost or
   self-IP route is permitted.
5. Retain AI.27 events, but make their meanings precise: `write_persisted` is
   emitted before response; later worker outcomes produce
   `peer_delivery_confirmed` or `peer_delivery_unconfirmed`. No admission
   response, event, or CLI prose may call local persistence “remote sent.”
6. Update `ADR-038`, `ADR-041`, and the requirements to state that the local
   request deadline ends at admission response and never owns background peer
   work.

## Required tests

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

## Acceptance criteria

- SQLite commit is the sole synchronous post-validation operation before a
  local send/ack response.
- A slow or unavailable peer cannot delay, cancel, or relabel a committed local
  admission response.
- No outbox, durable queue, receipt, delivery state, or extra public endpoint
  is introduced.
- `just lint`, `just test`, and branch-daemon `just smoke localhost` pass.

## Non-goals

This sprint does not define worker concurrency, delivery order, capacity, or
cross-host evidence. Those belong to AI.32 and AI.33.
