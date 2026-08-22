# Sprint AO2.9 — Benchmark Report Template and Procedure

Status: DRAFT — requirements captured on-the-fly by Rand; not yet hardened,
reviewed, or dispatched. Must be formalized before Phase AO2 closes.

## Background

During AO2.7/AO2.8 execution, quality-mgr identified a process gap: benchmark
campaigns were run and discarded/never published because no standard,
required reporting artifact or publish location existed for `just benchmark`
runs (see `.triage`/ATM history around the six-unpublished-campaigns finding,
2026-08-22). Rand separately requested a consistent, templated report format
and a formalized target matrix per OS. This sprint formalizes both.

## Requirements

1. **Report template**: introduce a `<file>.xhtml.j2` template (rendered via
   the repo's existing `sc-compose` templating convention) that produces a
   consistent, human-readable benchmark report for a single run. Exact
   filename/location TBD during hardening — mirror the existing
   `site/reports/smoke/*`, `site/reports/fuzz/*` convention
   (`.just/generate_report_index.py`).

2. **Mandatory per-run publish**: every `just benchmark` run — pass, fail, or
   incomplete — must render its xhtml report immediately after the attempt
   and commit/push it into `site/reports/` under a benchmark-specific,
   per-computer subpath (e.g. `site/reports/benchmark/<host>/<timestamp>/`).
   A run that is not published is itself a documented process violation, not
   merely an incomplete result. No campaign result may be discarded or
   silently represented as valid.

3. **Compiled/aggregate report**: individual per-run xhtml reports must be
   compiled into a single final report (index/summary) — reuse or extend
   `.just/generate_report_index.py`.

4. **Target matrix, per OS** (this is the authoritative matrix for AO2.7/AO2.8
   completion — supersedes any narrower prior framing):
   - **macOS / Linux**: all 4 targets — `sqlite`, `uds`, `tcp`, `tcp+tls`.
   - **Windows**: 3 targets — `sqlite`, `tcp`, `tcp+tls` (no `uds` — not a
     meaningful/available transport on Windows).

## Open items for hardening pass

- Exact xhtml.j2 template location and variable contract.
- Exact `site/reports/benchmark/...` path convention (host naming, timestamp
  format, retention/pruning policy if any).
- Whether AO2.7 (M5/macOS) and AO2.8 (Windows) sprint docs get amended in
  place to reference this matrix/template, or whether this doc is the sole
  source of truth and they defer to it.
- CI wiring, if any, vs. manual/agent-triggered publish.

## Acceptance criteria (draft — refine during hardening)

- [ ] `<file>.xhtml.j2` template exists and renders a complete single-run report.
- [ ] `just benchmark` (or equivalent wrapper) publishes the rendered report to
      `site/reports/benchmark/...` immediately after every attempt, including
      failed/incomplete ones.
- [ ] An aggregate/compiled report is generated from all published per-run
      reports.
- [ ] AO2.7 target matrix = sqlite, uds, tcp, tcp+tls (macOS/Linux).
- [ ] AO2.8 target matrix = sqlite, tcp, tcp+tls (Windows).
- [ ] Plan doc reviewed by quality-mgr before dispatch to dev.

## References

- quality-mgr process-gap finding, ATM message 01M0NGE2MEV5QER37YXFYTHTK3
  (2026-08-22T19:51:16Z)
- `docs/plans/phase-ao2/sprint-AO2-7-m5-tcp-benchmark-parity.md`
- `docs/plans/phase-ao2/sprint-AO2-8-windows-tcp-benchmark-parity.md`
- `.just/generate_report_index.py`, `site/reports/smoke/*`, `site/reports/fuzz/*`
