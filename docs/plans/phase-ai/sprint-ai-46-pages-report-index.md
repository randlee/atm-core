---
title: AI.46 generated reports index
status: complete
branch: feature/pAI-s46-reports-index
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pAI-s46-reports-index
recommended_agent: Cipher-311d
recommended_model: fast
execution_mode: parallel
target: integrate/phase-ai-31-33
---

# AI.46 — Generated reports index

## Goal

Create the durable report envelope and generated `site/reports/index.html`.
This is report discovery only; Pages publication and individual renderers are
separate sprints. AI.47 owns `site/index.html`.

## Governing requirements and ADRs

- `REQ-CORE-REPORT-001`
- ADR-044 — public verification-report classification

## Exact Targets

- `.just/generate_report_index.py` and its tests
- `Justfile` `reports-index` / `reports-index --check` recipes
- `site/reports/index.html`

## Deliverables

1. Define and validate the public envelope: `schema_version`, `report_type`,
   `generated_at` UTC, safe opaque `host_label`, and relative `report_html`.
   Initial types are `benchmark` and `fuzz`.
2. Generate `site/reports/index.html` from envelopes, grouped by type and
   newest-first. Reject stale indexes, unsafe metadata, malformed envelopes,
   missing report HTML, and missing same-named evidence directories.
3. `just reports-index` regenerates the index; `--check` verifies it. Fixture
   coverage includes empty input, both types, aggregation, bad envelopes,
   stale-index detection, ordering, and links.

## Required Validation

- `just reports-index --check`
- `just lint`
- `just test`

## Acceptance Criteria

One tested command creates or verifies the complete public report index with
no hand-maintained entries.

## Non-goals

No Pages deployment, report renderer, benchmark runner, fuzz skill, or fuzz
campaign.
