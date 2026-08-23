# Sprint AO2.11 — Rendering Pipeline: Panels, Phase Report, Candlestick, Index

Status: draft · Branch: `feature/ao2-11-benchmark-rendering` off
`integrate/phase-ao2` (after AO2.10 merge-forward) · PR target:
`integrate/phase-ao2`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

Implements decisions D4–D6, D9, D10 of
[benchmark-reporting-plan-overview.md](./benchmark-reporting-plan-overview.md).
Fixes audit findings B (dual `--rebuild`/`--input` render paths) and C
(no phase report, no chart, no wyvern step).

## Deliverables

1. **Three sc-compose j2 templates** under `templates/benchmark-report/`
   (replacing the current two):
   - `benchmark-run.xhtml.j2` — one panel per campaign: campaign header
     (host, phase, revision, roll-up status), one row per target with p50 /
     p95 / p99, durable-write proof, baseline floor and margin. If status is
     INCOMPLETE, a visible note block at the BOTTOM of the panel with
     `incomplete_reason` (D4).
   - `benchmark-phase-report.html.j2` — `phase-<id>.html`: 2×2 candlestick
     grid at top (tcp, tcp-tls / uds, sqlite — D5), then every panel of the
     phase embedded newest-first by `started_at` descending.
   - `benchmark-index.html.j2` — `index.html` in
     `site/reports/send-message-benchmark/`: the latest phase's 2×2 grid plus
     a links list to all historical phase reports, newest first (D6).
2. **Time display (D11):** every timestamp shown in panels, phase reports,
   the index, and chart axes is emitted as a `<time datetime="<UTC ISO>">`
   element whose text is pre-rendered in `America/Los_Angeles`, 24-hour
   format (e.g. `Aug 23, 2026 · 11:59 PDT`), by a single shared formatting
   helper feeding the j2 variables. One small **inline** script per page
   (allowed by the no-external-URL gate, which forbids only `<script src=`)
   rewrites each `<time>` text to the viewer's local zone, same 24-hour
   format; without script execution the Pacific text stands. In the
   `.xhtml.j2` panel template (strict XHTML: `xmlns` declared, parsed as
   XML), the inline script body MUST be wrapped in `<![CDATA[ ... ]]>` so
   `<` and `&` in script code cannot break XML well-formedness; the `.html`
   templates need no wrapping. Sorting and grouping always use the UTC
   values.
3. **Chart data preparation** in `scripts/smoke/benchmark_report.py`: a pure
   function computes candlestick geometry as plain JSON variables; the j2
   template emits self-contained inline SVG. No external JS/CSS dependencies;
   the site must render offline.
4. **Single render path.** `benchmark_report.py` exposes exactly one flow:
   read validated JSON (campaign files + `historical-record.json` when
   present) → render panels → render phase report(s) → render index → run
   `just reports-index`. The `--input`/`--rebuild` split is removed;
   `--rebuild` remains as the only (default) verb. Rendering never mutates
   result JSON (regeneration property). The `benchmark-report` Justfile
   recipe is kept as a thin passthrough to this single flow and its comment
   is updated to describe the post-AO2.11 behavior (the retired aggregate is
   no longer mentioned).
5. **Wyvern preview.** `just benchmark-show` renders/copies the newest
   campaign panel to an `.html` twin under
   `artifacts/benchmark/preview/latest.html` and opens it with `wyvern`
   (interim for [randlee/wyvern#115](https://github.com/randlee/wyvern/issues/115);
   switch to direct `.xhtml` when that lands). `just benchmark` prints the
   exact `just benchmark-show` command at the end of every run.
6. **Retire** `site/reports/send-message-benchmark.html` (delete file and its
   generation code path); `index.html` and `phase-<id>.html` replace it (D6).
7. **`HistoricalRecord` model classes.** Implement `RatchetPoint`,
   `HistoricalResultEntry`, `HistoricalCampaignEntry`, `UnattributedEntry`,
   and `HistoricalRecord` in `scripts/smoke/benchmark_schema.py` exactly per
   the normative contract in
   [sprint-AO2-12-historical-data-migration.md](./sprint-AO2-12-historical-data-migration.md)
   § "HistoricalRecord contract", plus an empty-record fixture used for
   pre-AO2.12 rendering. AO2.12 consumes these classes unchanged.

## Candlestick contract (normative)

- One chart per target; grid order: row 1 = tcp, tcp-tls; row 2 = uds, sqlite.
  Windows series simply absent from the uds chart.
- One series (color) per `host_label`; x-axis = campaign `started_at`
  (chronological); y-axis = admissions/sec.
- Candle mapping from `metrics.admissions_per_second` (MetricDistribution):
  wick low = `min`, wick high = `max`, body spans `p50`–`p95`, with the p50
  edge emphasized. Body fill: PASS = series color, FAIL = red outline.
- Point inclusion (D5): campaigns in `historical-record.json` contribute only
  the entry flagged `final_best`; current-phase campaigns all appear except
  status INCOMPLETE (D4). Baseline floor drawn as a horizontal reference line
  per series at the current `baselines.json` value.

```python
def candlestick_series(
    charts: Sequence[Literal["tcp", "tcp-tls", "uds", "sqlite"]],
    historical: HistoricalRecord,      # normative model defined in AO2.12
    phase_campaigns: Sequence[BenchmarkCampaign],
    baselines: BaselineSet,
) -> dict[str, ChartVars]:             # JSON-serializable j2 vars only
    ...
```

`HistoricalRecord` (with `HistoricalCampaignEntry.final_best`, the `ratchet`
trace, `evidence_gap`, and `unattributed`) is defined normatively in
[sprint-AO2-12-historical-data-migration.md](./sprint-AO2-12-historical-data-migration.md)
§ "HistoricalRecord contract". This sprint **implements** those model classes
in `scripts/smoke/benchmark_schema.py` exactly per that normative section
(plus an empty-record fixture for pre-AO2.12 rendering); AO2.12 consumes them
unchanged. Fixtures must validate against the implemented model — no ad-hoc
stub shapes.

## Acceptance criteria

1. `python3 scripts/smoke/benchmark_report.py --rebuild` regenerates every
   panel, `phase-ao2.html`, and `index.html` from JSON alone; a second
   invocation is byte-identical (deterministic; no timestamps-of-generation
   in output bodies).
2. Panels for INCOMPLETE campaigns render with the reason note at the bottom
   and are absent from every candlestick (template test with fixture data).
3. Phase report orders panels strictly by `started_at` descending (test with
   ≥3 fixture campaigns out of filename order).
4. SVG charts: fixture-driven golden test rendering 2 hosts × 4 targets ×
   mixed PASS/FAIL, asserting candle count, series count, FAIL styling, and
   baseline line presence; page contains no `<script src=` / external URL
   (grep gate).
5. `just benchmark-show` opens the newest panel in wyvern on macOS (manual
   evidence: operator-visible screenshot or wyvern stdout committed to the
   sprint evidence notes).
6. `site/reports/send-message-benchmark.html` no longer exists on the branch
   and nothing regenerates it (grep gate on the filename).
7. Time display (D11): template/unit tests assert every rendered timestamp
   is a `<time datetime="<UTC ISO Z>">` element whose text is the Pacific
   24-hour format; a fixture with campaigns whose UTC order differs from
   their Pacific calendar-date order proves sorting/grouping uses the UTC
   values; a DOM/grep check asserts exactly one inline (non-`src`) time
   upgrade script per page; every rendered `.xhtml` panel (including an
   INCOMPLETE-status fixture and one containing the inline script) passes a
   strict XML parse (`xml.etree.ElementTree.parse` or equivalent) as an
   automated test — a well-formedness failure fails the suite, not just a
   grep gate.
8. `HistoricalRecord` classes exist in `benchmark_schema.py`, field-for-field
   equal to AO2.12's normative contract (docstring cross-references it), and
   the empty-record fixture round-trips through model validation.
9. `.just/tests` suite green on macOS and Windows CI lanes.

## Required validation

- `.just/tests` python suite both platforms; golden-SVG fixtures committed.
- One live macOS `just benchmark` followed by `--rebuild` twice proving
  determinism, evidence committed on the sprint branch.
- Live-verify gate (memory: live-verify-before-QA): operator confirms the
  wyvern display and phase report visually before quality-mgr dispatch.

## Non-closure / out of scope

- Historical candlestick points: charts show only current-phase campaigns
  until AO2.12 delivers `historical-record.json` (function accepts it empty).
- No change to runner/schema (AO2.10 owns) or run procedure docs (AO2.13).
- Wyvern `.xhtml` native support (external, wyvern#115).

## Dependencies

- must_follow: AO2.10 (consumes v4 models, baselines.json, campaign files).
  Merge-forward trigger: AO2.10 dev push.
- AO2.13 is a must_follow child of this sprint (PR-completion trigger: this
  sprint's PR merges first) — see AO2.13's Dependencies section, which is
  authoritative for that relation.
