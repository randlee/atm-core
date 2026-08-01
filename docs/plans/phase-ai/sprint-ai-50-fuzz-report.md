---
title: AI.50 fuzz report
status: planned
branch: feature/pAI-s50-fuzz-report
recommended_agent: Cipher-311d
recommended_model: fast
execution_mode: after_merge
execution_dependencies:
  - AI.49
  - AI.48
dependencies_relation:
  - sprint: AI.49
    relation: must_follow
    rationale: Preserves the requested report-tooling sequence.
  - sprint: AI.48
    relation: must_follow
    rationale: Renders its validated coordinator/probe results.
target: integrate/phase-ai-31-33
depends_on: AI.49, AI.48
---

# AI.50 — Fuzz report

## Goal

Render AI.48 fuzz results using the established sc-compose HTML-report package.

Source: `randlee/sc-compose`.

## Exact Targets

- `.claude/skills/html-report/templates/fuzz-run-report.html.j2`
- `.claude/skills/html-report/templates/fuzz-run-agent.xhtml.j2`
- `.claude/skills/html-report/fuzz-run-agent-contract.md`
- fuzz report renderer/tests and `site/reports/` fuzz artifacts

## Deliverables

1. Copy the three named sc-compose files verbatim; AI.48 worker JSON conforms
   to the copied contract without template changes.
2. Render one top-level fuzz HTML report, JSON sidecar, and one XHTML panel per
   fragment/limit/transport/differential probe in its same-named directory.
3. Invoke `just reports-index` after each complete, failed, or incomplete
   campaign artifact write.

## Required Validation

- fixture rendering for all worker outcomes and invalid envelopes
- HTML/XHTML/JSON validation and relative-link checks
- `just reports-index --check`
- `just lint`

## Acceptance Criteria

Every AI.48 result state renders through the copied templates into a complete,
public-safe, browsable report package.

## Non-goals

No parser fix, unbounded campaign, benchmark renderer, or Pages deployment.
