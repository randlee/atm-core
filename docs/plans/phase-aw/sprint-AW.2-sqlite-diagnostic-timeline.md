---
sprint: AW.2
title: "SQLite diagnostic timeline and production SqliteObservability adapter"
branch: feature/aw2-sqlite-diagnostic-timeline
base: feature/aw1-tracing-bridge
issues: "#905 (items: persist warn/error diagnostics in SQLite, bounded/pruned, production SqliteObservability, saturation transition diagnostic, drop counters)"
must_follow: [AW.1]
parallel_safe: [AW.4]
---

# AW.2 — SQLite diagnostic timeline

## Deliverables

1. **Schema**: new migration appended to `DB_MIGRATIONS`
   (`crates/atm-storage-rusqlite/src/shared_db.rs`) creating
   `diagnostic_events(id INTEGER PRIMARY KEY, recorded_at_ms INTEGER NOT
   NULL, level TEXT NOT NULL CHECK(level IN ('warn','error','info')),
   component TEXT NOT NULL, code TEXT, action TEXT, correlation_id TEXT,
   outcome TEXT, origin TEXT NOT NULL, detail TEXT)` with an index on
   `recorded_at_ms` and one on `(code, recorded_at_ms)`. `detail` is
   bounded to `DIAGNOSTIC_DETAIL_MAX_BYTES = 2048` (truncated with a
   `…[truncated]` marker; UTF-8-boundary safe). A schema note is added
   next to the existing `shared_db.rs:843` guidance.
2. **Contract**: `DiagnosticTimelineStore` trait in
   `crates/atm-storage/src/contract.rs` (sealed like the neighbours):
   `record_batch(&[DiagnosticEvent]) -> Result<RecordedCount>`,
   `query(DiagnosticQuery) -> Result<Vec<DiagnosticEvent>>` (filter by
   min level, code, component, since/until, limit ≤ 10 000, newest first),
   `prune(DiagnosticRetention) -> Result<PrunedCount>`.
   `DiagnosticEvent` is a plain struct with the allowlisted fields only —
   there is no free-form map on the SQLite side.
3. **Writer**: `DiagnosticTimelineWriter` (Tokio task, owned by
   `atm-daemon-bootstrap`) with a bounded `mpsc` channel
   (`DIAGNOSTIC_QUEUE_CAPACITY = 1024`, `try_send` only). It batches up
   to `DIAGNOSTIC_BATCH_MAX = 128` events or `DIAGNOSTIC_FLUSH_INTERVAL =
   250 ms`, and submits **one write op through the existing SharedDb single
   writer lane** at low priority. It never opens a second connection and
   never awaits channel capacity. Overflow increments
   `diagnostic_queue_full_drops_total`; write-op failure increments
   `diagnostic_persist_failures_total` and emits **one** JSONL-only
   diagnostic (`origin = "timeline"`), rate-limited to once per
   `DIAGNOSTIC_FAILURE_NOTICE_INTERVAL = 60 s`.
4. **Retention**: every `DIAGNOSTIC_PRUNE_INTERVAL = 60 s` the writer
   submits one bounded prune (`DELETE … LIMIT 1000` semantics via rowid
   subquery) enforcing `DIAGNOSTIC_MAX_ROWS = 20 000` and
   `DIAGNOSTIC_MAX_AGE = 7 days`. Defaults live in one `DiagnosticRetention`
   struct with `ATM_DIAGNOSTIC_MAX_ROWS` / `ATM_DIAGNOSTIC_MAX_AGE_HOURS`
   overrides parsed and validated at bootstrap (invalid → diagnostic +
   default, never panic).
5. **Fan-out**: AW.1's `TracingBridgeLayer` gains an optional secondary
   `DiagnosticSink` hook; the daemon wires it to the timeline writer's
   `try_send`. JSONL remains the primary path; a timeline drop never
   affects JSONL emission and vice versa.
6. **Production adapter**: `DaemonSqliteObservability` implementing
   `SqliteObservability`, mapping every `SqliteObservabilityEvent` to a
   `tracing::warn!/error!` with `origin = "sqlite"` and stable codes
   (`ATM_SQLITE_WRITER_TIMEOUT`, `ATM_SQLITE_WAL_CHECKPOINT_FAILED`, …
   enumerated in the sprint PR). Events with `origin = "sqlite"` are
   **JSONL-only** (the bridge does not fan them out to the timeline) —
   this is the recursion break. All three production construction sites
   (`lib.rs:741,750`, `shared_db_reader_lanes.rs:43`) take the adapter;
   `NullSqliteObservability` stays available for tests only (`#[cfg(test)]`
   or a `testing` feature — dev decides, boundary-guard reviews).
7. **Saturation transition diagnostic**: `DaemonObservability` tracks
   sink health per `sc_observability` report; on transition into
   `queue_full`/degraded and on recovery it emits one retained JSONL record
   (`ATM_OBSERVABILITY_DEGRADED` / `ATM_OBSERVABILITY_RECOVERED`) with the
   drop counter delta, rate-limited to once per 60 s per direction.

## Contract samples

```sql
INSERT INTO diagnostic_events(recorded_at_ms, level, component, code, action, correlation_id, outcome, origin, detail)
VALUES (1756822991412, 'warn', 'atm_http_runtime::delivery', 'ATM_DELIVERY_RETRY', 'deliver', 'c-8f1a', 'retry', 'tracing', 'attempt=3');
```

JSONL-only record from the storage adapter:

```json
{"ts":"…","level":"error","component":"atm_storage_rusqlite::writer","code":"ATM_SQLITE_WRITER_TIMEOUT","origin":"sqlite","elapsed_ms":5003,"message":"writer lane acquisition timed out"}
```

## Acceptance criteria

- AC1: A fresh DB and a pre-AW DB both migrate; `diagnostic_events` exists
  with the declared columns (reuse `ensure_columns_match_canonical`).
- AC2: An induced `warn!` on the runtime path appears in both
  `atm.log.jsonl` and `diagnostic_events` with matching `code`,
  `correlation_id`, `recorded_at_ms` within 1 s. (Closes #905 "persist in
  SQLite".)
- AC3: Non-interference: with the timeline channel full (capacity 1024
  pre-filled) and with the writer lane faulted, 1 000 sends complete with
  identical results and latencies within the existing benchmark tolerance
  vs a run with the timeline disabled; drop counters account for every
  undelivered diagnostic.
- AC4: Retention: 25 000 inserted rows prune down to ≤ 20 000 within two
  prune intervals; rows older than max-age are removed first; each prune op
  deletes ≤ 1 000 rows.
- AC5: `detail` longer than 2048 bytes is stored truncated at a UTF-8
  boundary with the marker; a multibyte fixture does not corrupt.
- AC6: A writer-lane failure emitted by `DaemonSqliteObservability` reaches
  JSONL and does not produce a timeline write attempt (assert writer op
  count unchanged) — recursion break proven.
- AC7: Saturation: forcing `QueueFull` via `RetainedSinkFaultInjector`
  yields exactly one `ATM_OBSERVABILITY_DEGRADED` record and one
  `ATM_OBSERVABILITY_RECOVERED` record across a 3-minute cycling fixture.
- AC8: No production code path constructs `NullSqliteObservability`
  (grep gate in the sprint's test file).
- AC9: Redaction: a `SqliteObservabilityEvent` carrying an SQL statement
  or a path is retained with the statement replaced by its verb and the
  path replaced by its file name only.
- AC10: No file under `crates/atm-daemon/` is modified.

## Required validation

- `cargo test -p atm-storage -p atm-storage-rusqlite -p atm-daemon-bootstrap`
- `just lint` including boundary TOML for the new trait (owner crate
  `atm-storage`, impl `atm-storage-rusqlite`); `rusqlite::Connection`
  literal must not appear outside the owner crate (use the
  `crate::shared_db::SqliteConnection` alias).
- Function-length and file-length gates (`shared_db.rs` is at 919 non-test
  lines — the migration DDL goes in a new `diagnostic_events_schema.rs`
  module, not inline).
- Storage benchmark spot check on the dev host with timeline enabled vs
  disabled (evidence only; official numbers stay on m5-atmbench).

## Out of scope

- Health/doctor surfacing and `atm log` query (AW.3).
- Graft fallback events (AW.4).
