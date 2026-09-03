---
sprint: AW.2
title: "SQLite diagnostic timeline and production SqliteObservability adapter"
branch: feature/aw2-sqlite-diagnostic-timeline
base: integrate/phase-aw
issues: "#905 ids 905-3, 905-4, 905-5, 905-6 (diagnostic), 905-8 (non-interference/retention/saturation)"
must_follow: [AW.1]
parallel_safe: [AW.4]
---

# AW.2 — SQLite diagnostic timeline

## Deliverables

1. **Schema**: migration appending `diagnostic_events` to `DB_MIGRATIONS`
   (`crates/atm-storage-rusqlite/src/lib.rs`):
   `id INTEGER PK, ts_unix_ms INTEGER NOT NULL, level TEXT NOT NULL,
   component TEXT NOT NULL, code TEXT, correlation_id TEXT, origin TEXT NOT
   NULL, message TEXT NOT NULL, detail TEXT` with index
   `(ts_unix_ms)` and `(component, ts_unix_ms)`. `detail` holds the
   allowlisted extra fields as a compact JSON object, capped at
   `DIAGNOSTIC_DETAIL_MAX_BYTES = 1024` (truncated with `…` marker, never
   split inside a UTF-8 sequence).
2. **Store contract** in `crates/atm-storage`
   (`diagnostics.rs`): `DiagnosticEvent`, `DiagnosticQuery { since, until,
   level_at_least, component_prefix, limit }`, and trait
   `DiagnosticTimelineStore { record_batch, query, prune }`. Implemented in
   `atm-storage-rusqlite` as `SqliteDiagnosticTimeline`.
3. **Writer-lane priority (concrete mechanism).** No second SQLite
   connection. `crates/atm-storage-rusqlite/src/writer/ops.rs` gains
   `WriteOp::RecordDiagnostics(Vec<DiagnosticEvent>)`. The writer
   (`writer/mod.rs`) gains a second bounded `tokio::sync::mpsc` channel
   `diagnostic_tx/diagnostic_rx` whose items are **batches**
   (`Vec<DiagnosticEvent>` of 1..=`DIAGNOSTIC_BATCH_MAX = 128` events);
   channel capacity `DIAGNOSTIC_QUEUE_BATCHES = 8`, so at most
   `8 × 128 = 1024` events are in flight toward the writer (the only
   in-flight bound; D4's buffer is upstream of this channel and holds at
   most one batch). The writer loop becomes:

   ```rust
   loop {
       tokio::select! {
           biased;
           Some(op) = primary_rx.recv() => handle(op),
           Some(batch) = diagnostic_rx.recv(), if primary_rx_is_empty() => handle_diagnostics(batch),
           else => break,
       }
   }
   ```

   i.e. a diagnostic batch is drained only when the primary channel has no
   pending op, at most one batch per idle tick, and a diagnostic batch that
   fails to persist is counted and dropped (never retried, never surfaced as
   a primary-lane error). Producers use `try_send` only; `Full` drops the
   whole batch and adds its length to `timeline_dropped_queue_full_total`.
4. **`DiagnosticTimelineWriter`** (bootstrap side): implements the
   `DiagnosticSink` contract from the phase plan §2 (`offer` copies the
   `RetainedEvent` into an owned `DiagnosticEvent`, never blocks); buffers
   at most one batch (`DIAGNOSTIC_BATCH_MAX` events; `offer` returns
   `Dropped(QueueFull)` when the buffer is full and the channel refuses),
   flushes the batch on `DIAGNOSTIC_BATCH_MAX` or
   `DIAGNOSTIC_FLUSH_INTERVAL_MS = 250`, whichever first; records
   `timeline_written_total`, `timeline_dropped_queue_full_total`,
   `timeline_dropped_persist_error_total` in a shared
   `DiagnosticTimelineStats`, and implements
   `DiagnosticCountersSource` (phase plan §2) over bridge + timeline stats.
5. **Production `SqliteObservability` adapter**
   (`DaemonSqliteObservability` in `atm-daemon-bootstrap`) replaces
   `NullSqliteObservability` at its two production construction sites
   (`lib.rs:741,750`); the `#[cfg(test)]` helper in
   `shared_db_reader_lanes.rs` keeps the null adapter. Every `SqliteObservability` method emits
   `tracing::warn!`/`error!` with `origin = "sqlite"` and a `code`
   (`ATM_SQLITE_WRITER_TIMEOUT`, `ATM_SQLITE_WAL_CHECKPOINT_FAILED`,
   `ATM_SQLITE_WRITE_FAILED`, `ATM_SQLITE_QUEUE_SATURATED`). Per the phase
   plan §2 "No recursion" rule these go to JSONL only and are never fanned
   out to the timeline (the bridge skips `DiagnosticSink` for
   `origin ∈ {"sqlite", "timeline"}`).
6. **Policy-selected minor events**: `DiagnosticPolicy` (bootstrap) lists
   the INFO events (by `target`, reusing `RETAINED_INFO_TARGETS`) that are
   also recorded to the timeline; default equals `RETAINED_INFO_TARGETS`.
7. **Saturation/degradation transition diagnostic** (905-6 diagnostic
   half): `DegradationMonitor` observes the bridge stats (AW.1) and timeline
   stats; on the first drop after a zero-drop window it emits one retained
   `warn!(code = "ATM_LOG_SINK_DEGRADED", origin = "timeline", sink =
   "jsonl"|"timeline", dropped = n)`; recovery (no drops for
   `DEGRADATION_RECOVERY_WINDOW_SECS = 60`) emits
   `ATM_LOG_SINK_RECOVERED`; rate-limited to one transition pair per
   `DEGRADATION_RATE_LIMIT_SECS = 60` per sink. Stats are exposed for AW.3
   health output.
8. **Retention/prune**: `DIAGNOSTIC_MAX_ROWS = 20_000`,
   `DIAGNOSTIC_MAX_AGE_DAYS = 7`, `DIAGNOSTIC_PRUNE_BATCH = 1000`; prune
   runs as a `RecordDiagnostics`-lane op (same priority) after every flush
   that crosses a `DIAGNOSTIC_PRUNE_CHECK_EVERY = 500` written-row boundary.
9. **ADR amendment**: append a "Phase AW amendment (2026-09)" section to
   `docs/adr/ADR-ATM-RUSQLITE-002.md` recording the second, lower-priority
   diagnostic lane on the same single write worker/connection, the biased
   drain rule, the batch bound, and that diagnostic persistence failures
   are counted and dropped rather than surfaced as store errors. The
   `adr-index` lint must pass.

## Acceptance criteria

- AC1: migration applies on a fresh DB and on a DB at the current head;
  `sqlite3 .schema diagnostic_events` matches deliverable 1.
- AC2 (905-3): an induced bridged `warn!` (origin tracing) and a
  policy-selected INFO event both appear as timeline rows; an INFO event
  outside the policy does not.
- AC3 (905-4, 905-8 non-interference): with the diagnostic channel full and
  with `RecordDiagnostics` forced to fail, a concurrent send/read/ack and a
  mailbox write complete with unchanged results; diagnostic drops are
  counted.
- AC4 (905-4, 905-8 retention): 25_000 inserted rows prune to ≤ 20_000 in
  batches of ≤ 1000; rows older than 7 days prune regardless of count.
- AC5 (905-4 redaction): `detail` never contains a key outside
  `RETAINED_FIELD_ALLOWLIST`; detail > 1024 bytes is truncated at a char
  boundary.
- AC6 (905-5): a writer timeout, a WAL checkpoint failure and a write
  failure each produce a JSONL record with the deliverable-5 `code` and
  `origin = "sqlite"`, and no timeline row (recursion rule).
- AC7 (905-6, 905-8 saturation): with the writer paused, offering more
  than `DIAGNOSTIC_QUEUE_BATCHES × DIAGNOSTIC_BATCH_MAX + DIAGNOSTIC_BATCH_MAX`
  events drops the excess (`timeline_dropped_queue_full_total` equals the
  excess) and emits exactly one `ATM_LOG_SINK_DEGRADED` within the
  rate-limit window and one `ATM_LOG_SINK_RECOVERED` after the recovery
  window.
- AC8 (905-5): `NullSqliteObservability` has no production construction
  site: a source-scan test asserts every non-`#[cfg(test)]` reference in
  `crates/atm-storage-rusqlite/src/` is the type definition itself or a
  test-only helper.
- AC9 (905-4): writer-lane priority test: with 10_000 queued primary ops and
  all `DIAGNOSTIC_QUEUE_BATCHES` batch slots full, all primary ops complete
  before any diagnostic batch is written (assert via a recording writer
  hook).
- AC10: no file under `crates/atm-daemon/` is modified; boundary lint
  passes (`atm-storage` gains no new dependency; `atm-daemon-bootstrap` →
  `atm-observability` edge already allowlisted by AW.1).
- AC11: `ADR-ATM-RUSQLITE-002.md` contains the amendment section and the
  `adr-index` lint passes.

## Required validation

- `cargo test -p atm-storage -p atm-storage-rusqlite -p atm-daemon-bootstrap`
- `just lint`; `grep -n 'sc-observability' Cargo.toml` shows `=1.2.0`
  unchanged, no diff to those lines.
- Benchmark sanity: `just bench-smoke` (or the AO2 quick profile) shows no
  regression beyond noise on the send path with the timeline enabled.

## Out of scope

- Health/doctor exposure and `atm log` (AW.3).
