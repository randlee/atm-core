---
title: AI.49 benchmark report
status: planned
branch: feature/pAI-s49-benchmark-report
recommended_agent: Cipher-311d
recommended_model: fast
execution_mode: after_merge
execution_dependencies:
  - AI.48
  - AI.40
dependencies_relation:
  - sprint: AI.48
    relation: must_follow
    rationale: Preserves the requested report-tooling sequence.
  - sprint: AI.40
    relation: must_follow
    rationale: Consumes AI.40's schema-valid benchmark-run result.
target: integrate/phase-ai-31-33
depends_on: AI.48, AI.40
---

# AI.49 — Benchmark report

## Goal

Persist AI.40 benchmark runs as public-safe durable JSON and render one
aggregate benchmark report.

## Exact Targets

- `site/reports/send-message-benchmark.html`
- `site/reports/send-message-benchmark/` JSON/XHTML artifacts
- benchmark report renderer/tests

## Deliverables

1. Persist one immutable approximately 20-second JSON artifact per
   host-label/transport/frames profile using AI.46's envelope.
2. Render one aggregate benchmark HTML page and per-run XHTML panels from
   those JSON artifacts; aggregate by UTC timestamp for regression trends.
3. Invoke `just reports-index` after every successful or failed artifact write.

## Required Validation

- fixture JSON migration, aggregation, transport separation, and failed-run retention
- `just reports-index --check`
- `just lint`

## Acceptance Criteria

Each benchmark run has one durable public-safe JSON artifact; one HTML page
shows its dated history without hand-maintained index edits.

## Non-goals

No admission-path optimization, fuzz tooling, or Pages deployment.
