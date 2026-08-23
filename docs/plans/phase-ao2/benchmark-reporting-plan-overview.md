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
| D1 | Baseline seed (macOS M5): sqlite 35,000 · uds 18,000 · tcp 17,500 · tcp-tls 17,500 msg/s p50. These are deliberately below the best recorded empirical M5 values (37,483 / 18,937 / 18,116 / 17,934) — per-target headroom: sqlite ~6.6%, uds ~4.9%, tcp ~3.4%, tcp-tls ~2.4% — an explicit operator decision to absorb run-to-run variance at seed time; the ratchet (D3) closes the gap as passing runs land, and the candlestick keeps best-achieved values visible so a dip below them is never hidden. quality-mgr confirms the seed against the recorded AO2.7/AO2.8 figures when approving `baselines.json` revision 1. Windows seeds derived from its best historical passing runs during AO2.12, quality-mgr approved. |
| D2 | PASS requires **100% durable writes** (every requested message admitted and durable) AND p50 ≥ baseline. There is no retention percentage. |
| D3 | Baselines may only increase over time (ratchet), and only via quality-mgr-approved change to `baselines.json`. |
| D4 | INCOMPLETE runs are rendered (panel with reason note at the bottom), committed and pushed, viewable in wyvern — but excluded from the candlestick and from the historical record. Periodic cleanup deletes them later; never silently discard. |
| D5 | Candlestick: 4 charts in a 2×2 grid — tcp, tcp-tls on top; uds, sqlite below. One series per computer. Historical campaigns contribute only their final/best run; the current phase contributes all measured runs. |
| D6 | Phase report file: `site/reports/send-message-benchmark/phase-<id>.html` (e.g. `phase-ao2.html`). `site/reports/send-message-benchmark/index.html` shows the latest graphs plus links to historical phase reports. The legacy `site/reports/send-message-benchmark.html` aggregate is retired. |
| D7 | ALL historical result JSON is normalized to the new schema with values and timestamps unchanged. One `historical-record.json` is the single historical record; quality-mgr is responsible for verifying its values/dates are accurate. |
| D8 | Historical pass/fail display uses the ratchet: baseline-as-of-run = best passing p50 seen so far for that (host, target); best runs show PASS, dips from best show FAIL. |
| D9 | Wyvern displays the most recent panel on demand. Wyvern lacks `.xhtml` suffix support today — [randlee/wyvern#115](https://github.com/randlee/wyvern/issues/115) filed; interim: render/copy an `.html` twin for viewing. |
| D10 | sc-compose j2 templates carry all formatting; agents never hand-author report output. validated JSON → `sc-compose render` → output. |
| D11 | All timestamps in JSON artifacts are UTC (ISO-8601, `Z` suffix). Rendered XHTML/HTML displays times human-formatted in 24-hour style: each timestamp is emitted as `<time datetime="<UTC>">Aug 23, 2026 · 11:59 PDT</time>` with the text pre-rendered in `America/Los_Angeles`, and a small inline script (no external dependencies) upgrades the text to the viewer's local zone at view time; with scripts unavailable the Pacific fallback text stands. In `.xhtml` output (strict XML, `xmlns` declared) the inline script MUST be `<![CDATA[...]]>`-wrapped so `<`/`&` in script code cannot break well-formedness, and rendered `.xhtml` must pass a strict XML parse gate. Sorting/grouping always uses the UTC values; stored data is never localized. |
| D12 | Target naming: the machine identifier is `tcp-tls` everywhere in code, schemas, filenames, and JSON — matching the base-branch matrix runner (`BENCHMARK_TARGETS`), whose identifiers are never renamed. Rendered reports may display the human label “TCP + TLS”; the `+` form never appears in data or code. |

## Sprint map

| Sprint | Title | Depends | Recommended |
|--------|-------|---------|-------------|
| AO2.10 | Benchmark data contract and runner emission | AO2.6 merged (must_follow) | arch-ctm / deep-reasoning |
| AO2.11 | Rendering pipeline: panels, phase report, candlestick, index | must_follow AO2.10 | arch-ctm / deep-reasoning |
| AO2.12 | Historical data migration and single historical record | must_follow AO2.10, AO2.11 | Cipher-311d / fast |
| AO2.13 | Canonical benchmark-run skill and operator workflow | must_follow AO2.11; parallel_safe with AO2.12 | Cipher-311d / fast |

**Base-branch precondition (blocking for AO2.10 dispatch):** the branch
**`origin/fix/ao2-7-direct-peer-benchmark-harness`** must merge into
`integrate/phase-ao2` before AO2.10 development starts. That branch — and only
that branch — is the precondition's single source of truth: it carries the
four-target matrix runner (`BENCHMARK_TARGETS`: sqlite, uds, tcp, tcp-tls; one
unskippable matrix per `just benchmark`) and already contains the
report-provenance fixes merged into it by PR #1003 (head branch
`fix/ao2-7-report-provenance`, now behind it — do not merge that head branch
instead). Neither `integrate/phase-ao2` nor develop has the matrix runner
today; all four sprint docs assume it as their starting point.

Rationale for the split: AO2.10 closes the data/schema seam, AO2.11 closes
rendering, AO2.12 closes history, AO2.13 closes operator procedure. Each is a
distinct closure type with non-intersecting primary files; merging any two
would mix closure types and blur ownership (sprint-planning-guidelines: Split
Early).

All sprint PRs target `integrate/phase-ao2`. Standing constraint (memory:
findings-never-regress-hit-numbers): no change may regress the achieved
AO2.7/AO2.8 benchmark numbers; this plan touches only run/report tooling, not
the benchmarked runtime.

## Plan QA history

| Round | Reviewer | Commit reviewed | Result | Disposition |
|-------|----------|-----------------|--------|-------------|
| 1 | plan-scope-reviewer (sonnet) | `eead638e` | FAIL — 1 Blocking (missing normative `HistoricalRecord` contract), 2 Important (AO2.11↔AO2.13 dependency mislabeled `parallel_safe`; AI.49/AO2.5.4 dropped from supersession list), 1 minor (`benchmark-report` recipe disposition unstated) | All fixed in round-1 fix commit: `HistoricalRecord` contract added to AO2.12 with implementation ownership in AO2.11; dependency relabeled must_follow; supersession list expanded to seven docs; recipe note added. |
| 1 | critical-plan-reviewer (sonnet) | `eead638e` | FAIL — 5 Blocking (matrix runner absent from base branch; AC didn't pin N=4/3; campaign target-coverage roll-up unenforced; missing-BaselineEntry behavior undefined; D1 seed below recorded empirical baseline unjustified), 2 Important (sqlite-target metrics shape undefined; `HistoricalRecord` stub ownership), 2 minor | All fixed in round-1 fix commit: base-branch precondition added (merge `fix/ao2-7-report-provenance` before AO2.10); AC #2 pins 4/3 via real matrix run; `BenchmarkCampaign` coverage `model_validator` + truth-table case; missing baseline = hard emission error + test + seeded-label live-verify rule; D1 justification added (deliberate variance headroom, quality-mgr confirms vs AO2.7/AO2.8 figures at revision 1); sqlite variant rule (network fields None, validator-enforced); implementation-ownership rule for `HistoricalRecord`; Windows textual-verification defined; line-number refs replaced with grep locators. |

Operator additions after round 1: D11 (UTC-only JSON; rendered reports show
Pacific 24-hour fallback inside `<time>` elements upgraded to viewer-local
time by a small inline script).

| Round | Reviewer | Commit reviewed | Result | Disposition |
|-------|----------|-----------------|--------|-------------|
| 2 | plan-scope-reviewer (sonnet) | `19eeeda0` | FAIL — round-1 closures confirmed; new: 1 Blocking (base-branch precondition named two branches ambiguously), 3 Important (stale "five docs" restatement in AO2.13 deliverable 3; D11 had no AC; `HistoricalRecord` implementation outside AO2.11's deliverables list), 1 minor (duplicate deliverable numbering) | All fixed in round-2 fix commit: precondition unified on `origin/fix/ao2-7-direct-peer-benchmark-harness` (single source of truth, both docs); AO2.13 deliverable 3 now cross-references deliverable 1's seven-doc list; AO2.11 AC #7 (time display) added; `HistoricalRecord` implementation promoted to AO2.11 deliverable 7 + AC #8; deliverables renumbered 1–7. |
| 2 | critical-plan-reviewer (sonnet) | `19eeeda0` | FAIL — all 10 round-1 findings verified closed; new: 2 Blocking (`tcp+tls` in docs vs `tcp-tls` in the base branch's code/schemas/filenames — zero `tcp+tls` occurrences there; inline time script would break strict-XHTML well-formedness with no CDATA rule or XML-parse gate), 1 minor (D1 "~5–8%" headroom claim vs actual ~2.4–6.6%) | All fixed in round-2 fix commit: canonical identifier `tcp-tls` adopted across all docs + new D12 (machine id never renamed; "TCP + TLS" is display-label only); D11/AO2.11 now require `<![CDATA[...]]>` wrapping in `.xhtml` output and a strict XML parse gate in AC #7; D1 states per-target headroom percentages. |

## Out of scope

- Any change to the Rust runtime, daemon, writer, TLS, or client framing.
- The AO2.9 two-phase git-lock publication protocol (intent.json, `.pending`
  remote locks, evidence-branch PR gate, host-manifest binding digests). PR
  #989 is closed in favor of this plan; ADR-054's trust discussion is retained
  as background only.
- Independent result verification by a second host (explicitly deferred, as in
  ADR-054).
- Deleting INCOMPLETE-run artifacts (future periodic cleanup task).
