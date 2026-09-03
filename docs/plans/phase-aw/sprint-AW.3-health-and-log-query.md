---
sprint: AW.3
title: "Health exposure, merged atm log query, and retained-log contract docs"
branch: feature/aw3-health-and-log-query
base: feature/aw4-graft-fallback-observability
issues: "#905 (items: health counters, atm log query over SQLite, loss semantics docs); #904 (item: atm log merges fallback stream)"
must_follow: [AW.2, AW.4]
parallel_safe: []
---

# AW.3 — Health exposure and merged log query

## Deliverables

1. **Health**: `RuntimeStatusSnapshot`
   (`crates/atm-http-runtime/src/runtime_health.rs`) and
   `AtmObservabilityHealth` in the doctor report
   (`crates/atm-core/src/doctor/report.rs`) gain an `observability` block:
   `jsonl_queue_full_drops_total`, `bridge_reentrant_drops_total`,
   `timeline_queue_full_drops_total`, `timeline_persist_failures_total`,
   `timeline_rows`, `timeline_oldest_ms`, `sink_state`,
   `last_degraded_at`, `last_recovered_at`. `atm doctor` prints a single
   `observability: ok|degraded (…)` line; `--json` carries the block
   verbatim.
2. **`atm log --source {jsonl,timeline,merged}`** (default `jsonl`, so
   existing behaviour and scripts are unchanged):
   - `timeline` queries `DiagnosticTimelineStore` through the daemon's
     existing HTTP query surface (a new read-only route in
     `atm-http-runtime`, `GET /v1/diagnostics?…`, same auth as other
     local routes) — the CLI never opens the SQLite file;
   - `merged` reads canonical JSONL + the graft fallback satellite
     (`graft_fallback_log_path`, absent file is not an error) + the
     timeline, and yields a single timestamp-ordered stream with a
     `source` column (`jsonl|graft|timeline`). Records with identical
     `(ts, code, correlation_id, message)` across JSONL and timeline are
     collapsed to one line tagged `jsonl+timeline`; the collapse is
     documented as best-effort.
   - existing `--level/--match/--since/--limit/--json` filters apply to
     every source; `--source timeline` also accepts `--code`.
   - when the daemon is unreachable, `--source timeline|merged` degrades to
     the file sources and prints one stderr notice naming the missing
     source; exit code stays 0.
3. **Docs**: rewrite `docs/atm-daemon/logging.md` retained-log contract and
   update `docs/user-documents/doctor-and-log.md`,
   `docs/atm-core/modules/{observability,log}.md`, and ADR-011 addendum:
   which events are retained where, the three sources, honest loss
   semantics (JSONL primary; timeline bounded, pruned, may drop under
   overload; counters are the truth), and the pre-bootstrap stderr
   allowlist from AW.1.

## Contract samples

```text
$ atm log --source merged --since 10m --level warn
2026-09-02T14:03:11.412Z  warn   jsonl+timeline  ATM_DELIVERY_RETRY      c-8f1a  delivery retry scheduled
2026-09-02T14:03:12.001Z  error  graft           ATM_GRAFT_DAEMON_UNAVAILABLE  c-8f1b  daemon unavailable, recovery=retry_once
2026-09-02T14:03:12.140Z  warn   timeline        ATM_SQLITE_WRITER_TIMEOUT  -       writer lane acquisition timed out
```

```json
{"observability":{"sink_state":"ok","jsonl_queue_full_drops_total":0,"timeline_queue_full_drops_total":3,"timeline_persist_failures_total":0,"timeline_rows":1842,"timeline_oldest_ms":1756220000000,"last_degraded_at":null,"last_recovered_at":null}}
```

## Acceptance criteria

- AC1: Counters in `atm doctor --json` match the values reported by the
  AW.1/AW.2 stats structs after an induced drop fixture. (Closes #905
  "health exposes drop counts".)
- AC2: `atm log --source timeline --code ATM_DELIVERY_RETRY --since 1h`
  returns rows matching a direct `DiagnosticTimelineStore::query` in the
  same order. (Closes #905 "atm log can query SQLite".)
- AC3: `atm log --source merged` over a fixture with all three sources
  produces strictly non-decreasing timestamps, correct `source` tags, and
  collapses exact duplicates; each source alone is also correct. (Closes
  #904 "atm log merges fallback stream".)
- AC4: Missing fallback file and unreachable daemon both degrade as
  specified with exit 0 and one notice.
- AC5: `atm log` with no `--source` is byte-identical to the pre-AW output
  for the same JSONL fixture.
- AC6: Docs updated; `req-qa` confirms every #905/#904 checkbox is claimed
  by AW.1–AW.4 acceptance criteria with no gaps or double claims.
- AC7: No file under `crates/atm-daemon/` is modified.

## Required validation

- `cargo test -p atm -p atm-core -p atm-http-runtime`
- `just lint`, function-length gate (`log.rs` split into `log/` module if
  it approaches 1 000 non-test lines).
- Manual: run the AW.4 Python fixture against a stopped daemon, then
  `atm log --source merged` shows the graft records interleaved.

## Out of scope

- Any new retention policy (AW.2 owns it).
- Hermes-side consumption of the merged output.
