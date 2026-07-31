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

## Governing requirements and ADRs

- `REQ-CORE-REPORT-001` — durable report publication and index contract
- `docs/architecture.md` §1.3 — verification report-site ownership
- ADR-044 — public report classification and Pages exposure decision
- `.just/build_view_site.py` ToolPanel contract; it links to durable reports
  and does not render or copy them

## Goal

Publish `site/` through GitHub Pages. Generate `site/index.html` and
`site/reports/index.html` from durable report artifacts so new report files
appear without manual index edits.

## Exact Targets

- `site/index.html`
- `site/reports/index.html` and `.just/generate_report_index.py` with tests
- `.github/workflows/pages.yml`
- GitHub Pages deployment documentation/configuration
- AI.41-vendored sc-compose fuzz templates:
  `.claude/skills/html-report/templates/fuzz-run-report.html.j2` and
  `fuzz-run-agent.xhtml.j2`, with
  `.claude/skills/html-report/fuzz-run-agent-contract.md`

## Deliverables

1. Generate `site/index.html` with a clear relative link to
   `site/reports/index.html`.
2. Define the versioned report-envelope interface: `schema_version`,
   `report_type` (`benchmark` or `fuzz`), `generated_at` (UTC), an ADR-044-safe
   opaque `host_label`, and a relative `report_html` path. Generate
   `site/reports/index.html` from those envelopes.
   It has one section per report type; entries are newest-first by UTC and link
   to the validated report HTML.
3. Use the durable naming/envelope contract, not a hand-maintained list:
   benchmark run artifacts below `send-message-benchmark/` aggregate to the
   one `send-message-benchmark.html` entry; each fuzz report HTML has its own
   same-named directory containing JSON/XHTML evidence and an index entry.
4. Keep report HTML in `site/reports/` and all supporting JSON/XHTML in its
   same-named directory. The generator links only; it neither copies evidence
   nor re-renders individual reports. For `fuzz` entries, it links to HTML
   rendered by AI.41's verbatim-vendored `randlee/sc-compose@c19b743`
   templates:
   `.claude/skills/html-report/templates/fuzz-run-report.html.j2` and
   `fuzz-run-agent.xhtml.j2`; it must not introduce a bespoke fuzz renderer.
5. Expose `just reports-index` and `just reports-index --check`, used by every
   report producer and the Pages workflow. The first regenerates
   `site/reports/index.html` after each successful or failed report artifact
   write; check mode fails on a stale index, malformed or public-unsafe
   envelope, missing HTML, or missing same-named evidence directory. Producer
   PR gates run check mode and producer tests assert that their write path
   invokes the command. No hand-maintained index or agent reminder is
   permitted.
6. Add a GitHub Pages workflow that validates/generates `site/`, uploads that
   directory as the Pages artifact, and deploys from the configured default
   branch. Document the one repository Pages setting required to select the
   GitHub Actions source.

## Required Validation

- Fixture tests cover empty reports, both report types, newest-first UTC
  ordering, benchmark aggregation, malformed-envelope handling, missing
  linked artifacts, stale-index detection, and relative links.
- Validate generated HTML and report links.
- `just reports-index --check`
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
