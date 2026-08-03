---
title: AI.32 bounded independent peer jobs
status: in_progress
branch: feature/ai32-independent-peer-jobs
target: integrate/phase-ai-31-33
depends_on: AI.31
---

# AI.32 — bounded independent peer jobs

## Closure

The daemon schedules independent immutable-ULID peer-delivery jobs after
local admission. It bounds work globally and per host, but does not promise
delivery order between separate CLI/API writes and does not create a stream,
outbox, durable retry system, or delivery state machine.

## Why

Two CLI invocations milliseconds apart are independent messages. The only
ordering that matters is byte order within one HTTP exchange and the causal
relationship between a message and an acknowledgement that names its ULID.
Serializing unrelated messages adds latency and state without an agreed
requirement.

## Deliverables

1. First commit sets every releasable assembly to `1.4.0-beta-ai.32` and
   records matching branch CLI/daemon values through `atm doctor --json`.
2. Replace `crates/atm-daemon/src/peer_drain_coordinator.rs` with the only
   owner of a small bounded, non-durable work queue. It may use a bounded
   channel, work semaphore, and in-flight `HashSet<MessageId>`/host accounting.
   Its state may contain only hostname, message ULID, one next-eligible time,
   and concurrency counters—never a slot state machine, payload, receipt,
   remote result, checkpoint, cursor, generation, or retry history.
3. Replace AI.28's ordered `PeerDrainSlot`/cursor contract with storage paging
   that returns eligible immutable records for one exact hostname. Enqueue an
   independent job for each ULID, coalescing an already-in-flight ULID. Do not
   expose a batch endpoint or send a `Vec<WriteRequest>`.
4. Each job invokes the existing `PeerHttpTransport` and ordinary canonical
   `WriteRequest` endpoint. HTTP keep-alive is allowed as a transport
   optimization but not as a message-stream abstraction or ordering contract.
   Acknowledgements are ordinary independent write jobs with
   `acknowledges_message_id` set.
5. A job completion clears its in-flight marker and emits the AI.27 confirmed
   or unconfirmed event with the existing stable `AtmErrorCode`. When an eligible
   record ages beyond `PeerSyncPolicy.max_message_age`, emit terminal
   `peer_delivery_expired` with that code and stop scheduling it until a new
   persisted write or explicit policy change makes it eligible. The existing
   retained-event sink idempotently records that terminal `(ULID, code)` event;
   the scheduler retains no terminal marker. A failure may
   schedule the existing bounded reconciliation delay, but must not attach an
   attempt count or delivery state to the message record.
6. Define `POST_COMMIT_QUEUE_DEPTH = 256`, `MAX_ACTIVE_PEER_JOBS = 64`,
   `MAX_ACTIVE_PEER_JOBS_PER_HOST = 8`, and
   `PEER_DELIVERY_WORKER_DEADLINE = 10s` in the coordinator. The deadline
   spans the whole one job and cannot be extended per leg. Document why the
   limits bound file descriptors and peer load. They are capacity controls,
   not ordering controls. Different hosts must make progress independently.
7. `sync_peer` is a crate-private explicit one-shot scheduler request. It
   returns `PeerSyncOutcome`, uses the same queue and 10-second worker budget,
   and cannot bypass the ordinary canonical endpoint or introduce a foreground
   delivery path.

## Paths to delete

- AI.28's `PeerDrainSlot` ordered cursor/lower-bound state and any
  oldest-first or one-socket assertion.
- `acquire`, `release`, generation comparison, `Condvar` waiting,
  `run_scheduled_recovery`, and 250-ms polling from
  `peer_drain_coordinator.rs`.
- Any product contract or test that treats independent same-peer sends as a
  FIFO stream rather than independent immutable ULID jobs.

## Required validation

- Three same-peer writes submitted concurrently can reach a controlled peer in
  any completion order; all retain their original distinct ULIDs and produce
  exactly one recipient persistence/nudge each.
- Two signals for the same ULID while its job is in-flight create one outbound
  attempt; a signal after a confirmed or unconfirmed job completes may safely
  rediscover an eligible ULID, whose receiver duplicate remains idempotent.
- Per-host and global limits block excess *worker* starts without blocking a
  new local SQLite admission response.
- A source-boundary test rejects `PeerDrainSlot`, `Condvar`, generation fields,
  ordered cursor state, and fixed polling from the replacement scheduler.
- The daemon boundary TOML record and
  `crates/atm-architecture/tests/boundary_enforcement.rs` reject concrete
  SQLite imports, payload/receipt/retry-history fields, and public exposure of
  the scheduler or its outcome types.
- One stalled host cannot prevent a second host job from starting within its
  own bound.
- A peer that remains unavailable until its configured window expires emits one
  terminal `peer_delivery_expired` event with a stable typed code; it creates
  no receipt, attempt history, or caller-visible remote-success claim.
- A send followed by its generated acknowledgement proves the ACK refers to
  the delivered message ULID; no test asserts incidental order between
  independently submitted sends.
- `just lint`, `just test`, and branch-daemon `just smoke localhost` pass.

## Acceptance criteria

- No global/per-peer FIFO delivery promise, cursor, ordered backlog, or stream
  abstraction remains in product code or requirements.
- All peer work is bounded, non-durable, and rebuildable from immutable SQLite
  records after restart.
- Ordinary localhost, self-IP, and cross-host traffic use the same HTTP route.
- `PeerSyncOutcome` and `AtmErrorCode` remain crate-private typed
  seams; no raw HTTP status or transport implementation leaks into routing.

## Non-goals

This sprint does not claim a throughput target or physical peer evidence. It
creates the simple bounded worker model that AI.33 measures.
