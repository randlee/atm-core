---
title: AI.32 bounded independent peer jobs
status: proposed
branch: feature/pAI-s32-independent-peer-jobs
target: integrate/phase-AI
depends_on: AI.31
---

# AI.32 — bounded independent peer jobs

## Closure

The daemon schedules independent immutable-ULID peer-delivery jobs after
local admission. It bounds work globally and per host, but does not promise
delivery order between separate CLI/API writes and does not create a stream,
outbox, or durable retry system.

## Why

Two CLI invocations milliseconds apart are independent messages. The only
ordering that matters is byte order within one HTTP exchange and the causal
relationship between a message and an acknowledgement that names its ULID.
Serializing unrelated messages adds latency and state without an agreed
requirement.

## Deliverables

1. First commit sets every releasable assembly to `1.4.0-beta-ai.32` and
   records matching branch CLI/daemon values through `atm doctor --json`.
2. Refactor `crates/atm-daemon/src/peer_drain_coordinator.rs` into the only
   owner of a bounded, non-durable scheduler. It may use a bounded channel,
   work semaphore, and in-flight `HashSet<MessageId>`/host accounting. Its
   state may contain only hostname, message ULID, timing/backoff, and
   concurrency counters—never payload, receipt, remote result, checkpoint,
   or retry history.
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
   or unconfirmed event. A failure may schedule the existing bounded
   reconciliation delay, but must not attach an attempt count or delivery state
   to the message record.
6. Define named constants for global and per-host limits in the coordinator;
   document why they bound file descriptors and peer load. They are capacity
   controls, not ordering controls. Different hosts must make progress
   independently.

## Required tests

- Three same-peer writes submitted concurrently can reach a controlled peer in
  any completion order; all retain their original distinct ULIDs and produce
  exactly one recipient persistence/nudge each.
- Two signals for the same ULID while its job is in-flight create one outbound
  attempt; a signal after terminal completion may safely rediscover that ULID,
  whose receiver duplicate remains idempotent.
- Per-host and global limits block excess *worker* starts without blocking a
  new local SQLite admission response.
- One stalled host cannot prevent a second host job from starting within its
  own bound.
- A send followed by its generated acknowledgement proves the ACK refers to
  the delivered message ULID; no test asserts incidental order between
  independently submitted sends.

## Acceptance criteria

- No global/per-peer FIFO delivery promise, cursor, ordered backlog, or stream
  abstraction remains in product code or requirements.
- All peer work is bounded, non-durable, and rebuildable from immutable SQLite
  records after restart.
- Ordinary localhost, self-IP, and cross-host traffic use the same HTTP route.
- `just lint`, `just test`, and branch-daemon `just smoke localhost` pass.

## Non-goals

This sprint does not claim a throughput target or physical peer evidence. It
creates the simple bounded worker model that AI.33 measures.
