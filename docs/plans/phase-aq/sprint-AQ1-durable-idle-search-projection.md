---
title: AQ.1 Durable Idle Search Projection
status: planned
branch: feature/aq1-durable-idle-search-projection
target: integrate/phase-aq
worktree: ../atm-core-worktrees/feature/aq1-durable-idle-search-projection
external_blockers: []
---

# AQ.1 — Durable Idle Search Projection

**recommended_agent:** arch-ctm/deep-reasoning (SQLite transaction and
foreground/maintenance ownership).
**must_follow:** Phase AN's merged `develop` tip. Before every development or
fix round, merge the selected AQ integration baseline so the implementation tests the released
checked-render/search surface, including its current HTTP fuzz coverage.
**unblocks:** AQ.2 performance and recovery evidence.
**parallel_safe:** none. This sprint changes the only search-projection
writer, the `MessageSearchStore` contract, and every local presentation of
its freshness state.

**traceability:** ADR-047; ADR-001, ADR-008, ADR-018, ADR-036,
ADR-ATM-RUSQLITE-002; `REQ-P-SEARCH-INDEX-001`,
`REQ-RUSQLITE-SEARCH-INDEX-001`, `REQ-P-DAEMON-LANES-001`, and
`REQ-P-PLATFORM-002`.

**schema approval gate:** this sprint is planning-only until the product owner
explicitly approves the additive SQLite work-ledger schema required by
`REQ-RUSQLITE-SEARCH-INDEX-001`. That approval must be recorded in the
tracked [AQ.1 schema-approval artifact](./aq1-schema-approval.md) and copied
into the implementation PR description under `AQ.1 schema approval` before
any schema migration lands. Every checklist field in that artifact is a hard
gate; an unchecked field blocks the migration and enables no drain.

## Deliverables

1. Add the approved additive SQLite desired-state work ledger and migration.
   It contains only message compound identities and template SHAs, coalesces
   duplicate work, and is private to `atm-storage-rusqlite`. Every canonical
   message/template mutation that currently invokes synchronous projection
   maintenance instead atomically enqueues the affected source identity in
   the same writer transaction. No user-facing SQL/FTS string, renderer,
   workflow value, or HTTP shape enters the ledger.
   The migration preserves existing valid AN.5 projection rows and begins with
   an empty ledger; normal startup must not rebuild/backfill all FTS rows.
   `reindex-search` remains the explicit recovery/backfill path.

2. Replace foreground `sync_message_projection*` / template sync calls with
   one private, bounded idle drain owned by the existing `SqliteWriter`. The
   drain runs only after the documented quiet interval; it yields to foreground
   submissions, processes a bounded batch in one transaction, and is
   idempotent. For each item it applies the current source state, deletes a
   projection whose source disappeared, and removes the work row only in the
   successful transaction. Reopen/restart resumes remaining work. No new
   daemon, raw process, detached task, HTTP DB access, or legacy-daemon edit
   is permitted.

   AQ.1 defines one crate-private `SearchProjectionScheduleConfig` validated
   synchronously while `SharedDb` starts, before the writer accepts work:
   `idle_quiet_interval` defaults to 250 ms and must be within 1 ms through
   60 s; `max_batch_items` defaults to 256 and must be within 1 through
   1,024. Zero, negative/invalid duration encodings, and values outside those
   bounds return one typed configuration error and prevent writer startup.
   There is no silent clamping, fallback, or partially running indexer.

   A failed idle-drain item is never retried in a transaction loop. The ledger
   records an attempt count and next-eligible time. For classified transient
   SQLite `BUSY`/`LOCKED` and I/O failures, it schedules at most eight
   automatic attempts per current desired state with exponential delays of
   50 ms, 100 ms, 200 ms, 400 ms, 800 ms, 1.6 s, 3.2 s, and 5 s (capped).
   Every failed batch rolls back its projection changes and leaves its work
   durable. After the eighth failure the item is durable-but-blocked: it is
   excluded from automatic drain until a new canonical mutation or explicit
   rebuild resets its attempt state. The status surface reports blocked work
   separately from healthy pending work, so a failure cannot spin forever or
   silently masquerade as eventual catch-up.

3. Extend only the existing sealed `MessageSearchStore` capability with a
   leaf `SearchProjectionStatus` DTO and a read-only status method. It has
   this minimum public shape; SQLite timing/schema details remain private:

   ```rust
   pub enum SearchProjectionFreshness {
       Fresh,
       CatchingUp,
       Stale,
   }

   pub struct SearchProjectionStatus {
       pub sampled_at: IsoTimestamp,
       pub pending_count: u64,
       pub blocked_count: u64,
       pub oldest_pending_at: Option<IsoTimestamp>,
       pub last_successful_drain_at: Option<IsoTimestamp>,
       pub completed_watermark_at: Option<IsoTimestamp>,
       pub freshness: SearchProjectionFreshness,
   }

   pub trait MessageSearchStore: sealed::Sealed + Send + Sync {
       fn search(&self, query: &MessageSearchQuery)
           -> Result<MessageSearchPage, AtmError>;
       fn projection_status(&self) -> Result<SearchProjectionStatus, AtmError>;
   }

   #[async_trait::async_trait]
   pub trait AsyncMessageSearchStore: MessageSearchStore {
       async fn projection_status_async(
           &self,
           deadline: SearchDeadline,
       ) -> Result<SearchProjectionStatus, AtmError>;
   }
   ```

   Update every authorized production implementation and fake, boundary
   manifest, contract test, and core mapping. The async method uses the
   existing bounded reader lane; HTTP must not call a synchronous SQLite
   method after awaiting a search. Do not add a public maintenance trait or
   expose a direct SQLite handle.

   Every adapter derives the same classification from the status snapshot:
   `Fresh` means `pending_count == 0` and `blocked_count == 0`;
   `CatchingUp` means there is pending work, no blocked work, and the oldest
   pending item is at most 30 seconds old; `Stale` means any blocked work or
   an oldest pending item older than 30 seconds. The fixed 30-second threshold
   is deliberately an observation contract, not an admission timeout or a
   promise that newly accepted content is immediately searchable.

4. Make the status additive and visible on every existing **local** search
   presentation: the CLI, local HTTP response, Python read/query result, and
   doctor diagnostic. A nonzero backlog states that FTS is eventually
   consistent; it does not block send/read/ack or downgrade daemon readiness.
   Remote search remains forbidden. The status must not expose message
   content, a raw database path, or private SQLite error causes.
   CLI, Python, local HTTP, and doctor use the one `Fresh`/`CatchingUp`/
   `Stale` classification above; they must not invent per-surface thresholds.

5. Preserve the local search-projection rebuild operation as recovery. It
   must converge to the same projection state as an idle drain without
   creating a second writer or leaving false-complete ledger rows. Any future
   user-facing command is separately owned by the CLI/core layer, never by
   `atm-storage-rusqlite`. Record the one operator explanation: canonical
   rows are immediate; FTS search can lag until the durable backlog drains;
   rebuild is recovery, not an admission prerequisite.

## Acceptance criteria

- Foreground message/template admission persists canonical data and one
  coalesced durable work item atomically, then returns without FTS row
  synchronization. A forced transaction failure leaves neither partial source
  mutation nor a misleading ledger item.
- Migrating an existing indexed database does not synchronously rewrite the
  full FTS projection at daemon startup. Existing rows remain searchable;
  explicit reindex is the only full-rebuild path.
- Repeated mutation of a source leaves one desired-state item; delete/recreate
  and decomposed-admission updates converge to the newest canonical state.
  Crash/reopen before, during, and after a drain converges with no source loss,
  duplicate externally visible search result, or false-empty ledger.
- A transient drain failure rolls back the batch and follows the exact bounded
  retry schedule. An eighth consecutive failure leaves durable blocked work
  visible to operators; a source mutation or explicit rebuild resets the
  retry state and converges without data loss or an unbounded retry loop.
- The writer runs maintenance only after the configured quiet interval, drains
  no more than the documented batch bound, and processes newly queued
  foreground work before another maintenance batch. Shutdown stops intake,
  does not leave a detached worker, and preserves unfinished durable items.
- Invalid `idle_quiet_interval` or `max_batch_items` values fail startup
  before the writer accepts a submission; defaults and both inclusive valid
  bounds start deterministically.
- Fake and SQLite implementations expose the same status semantics. CLI,
  local HTTP, Python, and doctor make nonempty backlog observable without
  exposing an internal cause or changing canonical command success.
- CLI, Python, local HTTP, doctor, fake, and SQLite tests prove the one
  classification contract: empty is `Fresh`; a <=30-second unblocked backlog
  is `CatchingUp`; an older backlog or any blocked work is `Stale`.
- Boundary lint proves `atm-storage-rusqlite` is the only direct SQLite/FTS
  owner; no legacy daemon or `atm-http-runtime` source constructs/controls the
  indexer. The sealed-trait manifests name the amended method and all
  authorized implementations/test doubles.
- `reindex-search`, after any drain/restart history, produces the same
  projection as a clean rebuild of current canonical rows.

## Required validation

- Owning-crate deterministic tests for atomic enqueue, failure rollback,
  coalescing, deletion, decomposed update, bounded idle preemption, reopen,
  shutdown, and reindex equivalence. Use controlled clock/worker hooks, not
  sleep-based timing assertions.
- `atm-storage` fake/contract tests; `atm-core` mapping tests; CLI, local
  Tokio/Axum HTTP, Maturin/Python, and doctor response tests for the explicit
  status. Include a nonempty backlog assertion and prove no remote query route
  becomes available.
- Configuration tests cover the default, both valid bounds, zero, invalid
  duration encodings, and out-of-range values, proving rejection occurs
  before the writer accepts any work.
- Controlled-clock drain tests cover each retry delay, rollback-on-failure,
  the eighth-failure block, blocked-work status, and reset through both a new
  canonical mutation and explicit rebuild; no retry test may use a sleep.
- `cargo test -p atm-storage -p atm-storage-rusqlite -p atm-core -p atm -p atm-http-runtime`,
  `cargo test -p atm-architecture --test boundary_enforcement`, `just lint`,
  and `just test` on Linux, macOS, and Windows CI.

## Paths to delete

Delete the synchronous foreground projection calls from normal admission paths
only after the durable enqueue and idle-drain tests pass. Retain the private
projection functions as the drain/reindex implementation; do not duplicate
them in a new worker.

## Non-closure

AQ.1 does not add remote search, a generic jobs framework, user-configurable
SQL/FTS syntax, a second database process, template lineage policy, or a
promise that a just-admitted message is immediately searchable. It does not
claim the M5 throughput target; AQ.2 owns measured performance/recovery
closure.
