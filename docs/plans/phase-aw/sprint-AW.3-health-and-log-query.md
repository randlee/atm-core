---
sprint: AW.3
title: "Health counters, atm log query modes, retained-log contract docs"
status: complete
branch: feature/aw3-health-and-log-query
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/aw3-health-and-log-query
base: integrate/phase-aw
issues: "#905 ids 905-6 (health), 905-7, 905-8 (merge/order), 905-9; #904 ids 904-4, 904-7 (merge/order)"
must_follow: [AW.2, AW.4]
parallel_safe: [AW.5]
---

# AW.3 — Health counters and `atm log` query modes

## Deliverables

1. **Health block**: `GET /v1/health` (atm-http-runtime) and `atm doctor`
   gain `observability: { jsonl: { forwarded_total, dropped_queue_full_total,
   dropped_reentrant_total }, timeline: { written_total,
   dropped_queue_full_total, dropped_persist_error_total }, degraded:
   ["jsonl"|"timeline"...] }` sourced from the
   `Arc<dyn DiagnosticCountersSource>` (phase plan §2) that
   `atm-daemon-bootstrap` injects into `RuntimeHealth` via
   `HttpRuntimeBuilder::with_diagnostic_counters(...)`; `atm-http-runtime`
   depends only on the `atm-core` snapshot type it already reaches — no new
   crate edge, no `.just/lint-config.toml` change. Doctor prints a WARN line
   when `degraded` is non-empty.
2. **`atm log --source jsonl|timeline|merged`** (default `jsonl`, preserving
   current behaviour). `timeline` queries `DiagnosticTimelineStore` through
   the daemon (`GET /v1/diagnostics?since&until&level&component&limit`, new
   read-only route in `crates/atm-http-runtime/src/diagnostics_route.rs`)
   whose handler takes the `Arc<dyn DiagnosticTimelineStore>` from the
   `atm-runtime` router state (bootstrap registers it next to the existing
   store handles); `atm-http-runtime` never imports `atm-storage-rusqlite`
   (existing `forbidden_edges` in `boundaries/atm-http-runtime/http-runtime.toml`).
   `merged` reads canonical JSONL, the graft fallback
   satellite (`graft_fallback_log_path`, AW.1/AW.4), and the timeline, then
   sorts by `ts` ascending with a stable tiebreak `(source_rank, seq)`
   (`jsonl=0, graft=1, timeline=2`); each merged record carries
   `source`. Existing `--level/--since/--component` filters apply to every
   source. Output header (non-JSON mode) and a `"note"` field (JSON mode)
   state: "sources are independently bounded; merged view is not lossless
   under overload".
3. **cli_surface baseline** regenerated for the new flag.
4. **Docs**: `docs/atm-daemon/logging.md` retained-log contract rewritten
   with the exact guarantees (levels, targets, allowlist), the exceptions
   (pre-bootstrap stderr allowlist, `origin` rules), and loss/degradation
   behaviour (queue sizes, drop counters, `ATM_LOG_SINK_*` codes, timeline
   retention). `docs/graft-observability.md` (new, from AW.4 contract)
   cross-linked.

## Acceptance criteria

- AC1 (905-6 health): health JSON and doctor show the counters; an induced
  drop flips `degraded` and the doctor WARN line.
- AC2 (905-7): `atm log --source timeline --level warn` returns rows
  written by AW.2; `--source jsonl` output is byte-identical to pre-sprint
  output for the same file.
- AC3 (905-7, 905-8, 904-4, 904-7): fixture with interleaved JSONL, graft
  fallback and timeline records yields a single ascending-`ts` result with
  correct `source` tags and deterministic tiebreak order.
- AC4: `/v1/diagnostics` is read-only, bounded (`limit ≤ 5000`), and
  rejects unknown query keys.
- AC5 (905-7): no doc or CLI text claims lossless equivalence; the note
  is present in both output modes (test asserts substring).
- AC6 (905-9): `logging.md` contains the guarantees/exceptions/loss sections
  with the constant values matching the code (doc test greps constants).
- AC7: no file under `crates/atm-daemon/` is modified; boundary lint passes
  with no allowlist change (`atm` CLI → `atm-storage`, `atm-http-runtime` →
  `atm-core`/`atm-runtime` are existing edges); the PR diff touches none of
  the AW.5-reserved paths in the phase plan §4 table.

## Required validation

- `cargo test -p agent-team-mail -p atm-http-runtime -p atm-core`
- `just lint`; cli_surface test green; `grep -n 'sc-observability'
  Cargo.toml` shows `=1.2.0` unchanged.

## Out of scope

- Any change to list/read/send outcome structs (AW.5 owns those).
