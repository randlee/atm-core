# Benchmark Run/Report Process Audit (pre-planning, AO2.10+)

Date: 2026-08-23 · Author: fenix (planning) · Scope: the benchmarking and
reporting **process** for the SQLite / UDS / TCP / TCP+TLS matrix on macOS and
Windows. Not an audit of the code being benchmarked. Read-only; no code changes.

Inputs: arch-ctm's read-only briefing (ATM 01M0QZAV05J892MVNXSAR63GXX), four
codebase/data sweeps, AO2.9 reference worktree (`plan/ao2-9-benchmark-report-template`,
PR #989), sprint docs AI.40 / AI.49 / AI.52 / AL.9 / AO2.5.4 / AO2.7 / AO2.8.

## A. Process inconsistencies

1. **No single canonical run procedure.** Five sprint docs each restate the
   lifecycle with differences: artifact ownership (AI.40 defers publication to
   AI.49; AI.52 has the operator `git add` directly), snapshot/restore
   (mandatory and prescriptive in AO2.5.4; only implied in AI.52), hook-mode
   pairs (required by AL.9, owned by no sprint), and failure triage left to
   operator judgment (AO2.7). Agents running benchmarks must interpret, which
   is exactly the observed drift.
2. **Publication gap in the default path.** `just benchmark` ends with
   `benchmark_report.py --rebuild`, which renders HTML but never writes
   discovery envelopes; only the rarely-used `--input` path calls
   `persist_result()`. Result: 134 compact JSON vs 97 envelopes; 20 (develop)
   to 42 (integrate/phase-ao2) result files are invisible to
   `site/reports/index.html`; the index's newest benchmark entry lags real runs.
3. **Campaign identity is not persisted.** Campaign membership is *inferred* at
   render time from newest host/transport/revision grouping
   (`benchmark_report.py:235-263`) and appears only in rendered HTML. There is
   no machine-readable campaign/run summary JSON, so regenerating reports can
   silently regroup history, and "one run = one computer × 3-4 targets" has no
   artifact.
4. **Schema drift in published data.** At least four distinct compact-JSON
   shapes coexist (23-key mac, 25-key windows, 26-key local+direct-sqlite,
   35-key phase-ao2 with `benchmark_target`/`hook_mode`/`peer_wire_security`).
   The runner writes `schema_version: 2`, the canonical Pydantic model is v3,
   migration runs on every load, and `benchmark_policy` still special-cases v2.
5. **Three competing baseline models.**
   - Static constants in `benchmark_report.py:49-54` (45k / 24k / 22.5k / 22.5k).
   - Empirical M5 historical baseline in `historical-imports.json`
     (37,483 / 18,937 / 18,116 / 17,934; 95% retention) — which exists **only on
     the unmerged `fix/ao2-7-report-provenance` branch**, not on develop or
     integrate/phase-ao2.
   - Ad-hoc `--baseline <file>` argument pointing at an arbitrary prior run.
   Additionally `run_admission_capacity.py:1556,1584` hardcodes Windows
   comparison defaults (`mac-arm64-01`, comparison optional on Windows) that
   conflict with AO2.8's mandatory 80%-of-M5 floor.
6. **Pass/fail is computed once and split across layers.** Thresholds are
   evaluated at run time (`benchmark_policy.evaluate_profile_thresholds`),
   cross-checked by schema validation, and only *echoed* at render; runner and
   report each have independent comparison/grouping logic that can diverge.
7. **Duplicated derivations.** Artifact-id timestamp munging is implemented
   twice (`run_admission_capacity.py:1086-1098`, `benchmark_report.py:136-139`);
   host-label sanitization exists in three places with different rules.

## B. Overly complicated / dead result-processing code

- `benchmark_suite.py` defines an intent/ledger/threshold model that the normal
  path never uses beyond `TargetResult` validation — a second, unused seam.
- Dual render paths (`--rebuild` vs `--input`) duplicating render+index logic,
  with envelope creation only on one path (root cause of A.2).
- v1/v2→v3 migration executed on every load of every file, forever, because the
  writer still emits v2 (A.4).
- Dead code: `parse_utc`, `safe_label` in `benchmark_report.py`.
- The AO2.9 reference design (PR #989) layers a two-phase git-publication
  protocol on top: pre-run `intent.json` commit+push, `.pending/<run_id>.json`
  remote lock, push-gated finalizer, host-manifest binding digests, evidence
  branch + PR gates (ADR-054). That is far heavier than "validated JSON array →
  sc-compose render", and it contains no campaign JSON, no phase-level report,
  no graph, and no wyvern step. Per operator direction PR #989 will be closed
  in favor of the new plan.

Worth **keeping** from AO2.9: immutable per-run result JSON; machine-classified
PASS/FAIL/INCOMPLETE (never caller-selected); OS-specific target matrix
(Windows has no UDS); permanent retention of failed runs; sc-compose as the
sole rendering seam; extending (not replacing) `.just/generate_report_index.py`.

## C. Gaps vs the target design (operator expectations)

Target: each run emits one simple JSON array of results validated against a
Pydantic model → sc-compose j2 render to an XHTML panel → phase-level final
HTML aggregating all panels of the current phase (newest first) with a
candlestick chart of history per computer → read-only versioned baseline JSON
changed only with quality-mgr approval → historical JSON normalized to the same
schema (values unchanged) so reports can be regenerated without re-running.

Missing today: campaign/run-level JSON (A.3); phase-level report; any chart;
wyvern display step; single reviewed baseline file (A.5); normalized history
(A.4); envelope/index publication in the default path (A.2).

## D. Open questions (awaiting operator answers before plan finalization)

1. Baseline seed values and pass rule (which numbers, what retention %).
2. Do measurement-less INCOMPLETE attempts appear as phase-report panels or
   only in an attempt ledger?
3. Candlestick semantics: metric (TCP f8 p50 vs all four targets), one chart
   per target vs grouped, candle field mapping.
4. Location/name of the phase report and fate of `send-message-benchmark.html`.
5. Historical migration scope: all files vs final/best per historical campaign.
6. Disposition of `historical-imports.json` (unmerged branch) into the new
   baseline file.
7. Exact wyvern invocation expected for "display latest panel on demand".
