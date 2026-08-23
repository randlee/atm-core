# Benchmark Reporting Consolidation — Plan Overview (AO2.10–AO2.13)

Date: 2026-08-23 · Author: fenix · Status: draft for quality-mgr review
Basis: [benchmark-process-audit.md](./benchmark-process-audit.md) and operator
answers of 2026-08-23. Supersedes the AO2.9 design (PR #989, to be closed).

## Target architecture

```
run (one computer, 3–4 targets)
  └─ campaign JSON  (array of per-target results, Pydantic-validated, immutable)
        └─ sc-compose render ─ per-run XHTML panel (one per campaign)
              └─ sc-compose render ─ phase report HTML (all panels of the phase,
                 newest first, 2×2 candlestick grid at top)
                    └─ sc-compose render ─ index.html (latest graphs +
                       links to historical phase reports)
baselines.json  (read-only, versioned, quality-mgr-approved changes only)
historical-record.json (all prior campaigns, normalized, values unchanged)
```

Every rendered artifact is regenerable from JSON alone; no rendering step ever
re-runs a benchmark or mutates result data.

## Operator decisions (binding for all four sprints)

| # | Decision |
|---|----------|
| D1 | Baseline seed (macOS M5): sqlite 35,000 · uds 18,000 · tcp 17,500 · tcp+tls 17,500 msg/s p50. Windows seeds derived from its best historical passing runs during AO2.12, quality-mgr approved. |
| D2 | PASS requires **100% durable writes** (every requested message admitted and durable) AND p50 ≥ baseline. There is no retention percentage. |
| D3 | Baselines may only increase over time (ratchet), and only via quality-mgr-approved change to `baselines.json`. |
| D4 | INCOMPLETE runs are rendered (panel with reason note at the bottom), committed and pushed, viewable in wyvern — but excluded from the candlestick and from the historical record. Periodic cleanup deletes them later; never silently discard. |
| D5 | Candlestick: 4 charts in a 2×2 grid — tcp, tcp+tls on top; uds, sqlite below. One series per computer. Historical campaigns contribute only their final/best run; the current phase contributes all measured runs. |
| D6 | Phase report file: `site/reports/send-message-benchmark/phase-<id>.html` (e.g. `phase-ao2.html`). `site/reports/send-message-benchmark/index.html` shows the latest graphs plus links to historical phase reports. The legacy `site/reports/send-message-benchmark.html` aggregate is retired. |
| D7 | ALL historical result JSON is normalized to the new schema with values and timestamps unchanged. One `historical-record.json` is the single historical record; quality-mgr is responsible for verifying its values/dates are accurate. |
| D8 | Historical pass/fail display uses the ratchet: baseline-as-of-run = best passing p50 seen so far for that (host, target); best runs show PASS, dips from best show FAIL. |
| D9 | Wyvern displays the most recent panel on demand. Wyvern lacks `.xhtml` suffix support today — [randlee/wyvern#115](https://github.com/randlee/wyvern/issues/115) filed; interim: render/copy an `.html` twin for viewing. |
| D10 | sc-compose j2 templates carry all formatting; agents never hand-author report output. validated JSON → `sc-compose render` → output. |

## Sprint map

| Sprint | Title | Depends | Recommended |
|--------|-------|---------|-------------|
| AO2.10 | Benchmark data contract and runner emission | AO2.6 merged (must_follow) | arch-ctm / deep-reasoning |
| AO2.11 | Rendering pipeline: panels, phase report, candlestick, index | must_follow AO2.10 | arch-ctm / deep-reasoning |
| AO2.12 | Historical data migration and single historical record | must_follow AO2.10, AO2.11 | Cipher-311d / fast |
| AO2.13 | Canonical benchmark-run skill and operator workflow | must_follow AO2.11; parallel_safe with AO2.12 | Cipher-311d / fast |

Rationale for the split: AO2.10 closes the data/schema seam, AO2.11 closes
rendering, AO2.12 closes history, AO2.13 closes operator procedure. Each is a
distinct closure type with non-intersecting primary files; merging any two
would mix closure types and blur ownership (sprint-planning-guidelines: Split
Early).

All sprint PRs target `integrate/phase-ao2`. Standing constraint (memory:
findings-never-regress-hit-numbers): no change may regress the achieved
AO2.7/AO2.8 benchmark numbers; this plan touches only run/report tooling, not
the benchmarked runtime.

## Out of scope

- Any change to the Rust runtime, daemon, writer, TLS, or client framing.
- The AO2.9 two-phase git-lock publication protocol (intent.json, `.pending`
  remote locks, evidence-branch PR gate, host-manifest binding digests). PR
  #989 is closed in favor of this plan; ADR-054's trust discussion is retained
  as background only.
- Independent result verification by a second host (explicitly deferred, as in
  ADR-054).
- Deleting INCOMPLETE-run artifacts (future periodic cleanup task).
