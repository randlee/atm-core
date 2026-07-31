---
title: AI.46 GitHub Pages report index
status: planned
branch: feature/pAI-s46-pages-report-index
recommended_agent: Cipher-311d
recommended_model: fast
execution_mode: parallel
target: integrate/phase-ai-31-33
---

# AI.46 — GitHub Pages report index

## Recommended Agent / Model

`Cipher-311d` / fast: bounded static-site workflow and deterministic index
generation; no daemon/runtime change.

## Execution Dependencies

AI.46 has no parent merge gate. It defines the generic report-index contract;
report producers must invoke it after every report artifact write.

## Goal

Publish `site/` through GitHub Pages. Generate `site/index.html` and
`site/reports/index.html` from durable report artifacts so new report files
appear without manual index edits.

## Exact Targets

- `site/index.html`
- `site/reports/index.html` and report-index generator/tests
- `.github/workflows/pages.yml`
- GitHub Pages deployment documentation/configuration

## Deliverables

1. Generate `site/index.html` with a clear relative link to
   `site/reports/index.html`.
2. Generate `site/reports/index.html` from `site/reports/` artifacts. It has
   one section per `report_type`; entries are newest-first by their UTC
   timestamp and link to the report HTML.
3. Use the durable naming/envelope contract, not a hand-maintained list:
   benchmark run artifacts below `send-message-benchmark/` aggregate to the
   one `send-message-benchmark.html` entry; each fuzz report HTML has its own
   same-named directory containing JSON/XHTML evidence and an index entry.
4. Keep report HTML in `site/reports/` and all supporting JSON/XHTML in its
   same-named directory. The generator links only; it neither copies evidence
   nor re-renders individual reports.
5. Expose one repository command used by every report producer and the Pages
   workflow. It regenerates `site/reports/index.html` after each successful or
   failed report artifact write; no hand-maintained index or agent reminder is
   permitted.
6. Add a GitHub Pages workflow that validates/generates `site/`, uploads that
   directory as the Pages artifact, and deploys from the configured default
   branch. Document the one repository Pages setting required to select the
   GitHub Actions source.

## Required Validation

- Fixture tests cover empty reports, multiple report types, newest-first UTC
  ordering, benchmark aggregation, malformed envelope handling, and relative
  links.
- Validate generated HTML and report links.
- `just lint`
- `just test`

## Acceptance Criteria

- Pages serves `site/index.html`; its reports link resolves.
- `site/reports/index.html` groups all recognized report types and lists every
  recognized report newest-first without manual index maintenance.
- Every entry links to an HTML page whose supporting evidence stays in the
  matching directory.

## Non-goals

No browser runtime, charts, database, daemon change, or duplicate report
renderer. `artifacts/view` remains a transient development view.
