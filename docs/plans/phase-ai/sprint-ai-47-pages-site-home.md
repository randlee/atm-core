---
title: AI.47 GitHub Pages site home
status: planned
branch: feature/pAI-s47-pages-site-home
recommended_agent: Cipher-311d
recommended_model: fast
execution_mode: after_merge
execution_dependencies:
  - AI.46
dependencies_relation:
  - sprint: AI.46
    relation: must_follow
    rationale: Publishes AI.46's generated reports index.
target: integrate/phase-ai-31-33
depends_on: AI.46
---

# AI.47 — GitHub Pages site home

## Goal

Publish the generated static `site/` tree through GitHub Pages.

## Execution Dependencies

AI.47 `must_follow`s AI.46. Merge-forward after AI.46 development is pushed;
its PR completes after AI.46 merges.

## Governing requirements and ADRs

- `REQ-CORE-REPORT-001`
- ADR-044 — public verification-report classification

## Exact Targets

- `site/index.html`
- `.github/workflows/pages.yml`
- GitHub Pages configuration documentation

## Deliverables

1. Generate `site/index.html` with a relative link to AI.46's
   `site/reports/index.html`.
2. Add a Pages workflow that runs `just reports-index --check`, uploads only
   `site/`, and deploys through GitHub Actions. Document the one repository
   setting that selects the GitHub Actions source.

## Required Validation

- `just reports-index --check`
- workflow/config fixture validation
- `just lint`

## Acceptance Criteria

The published site has a valid home-to-reports link and no alternate publisher.

## Non-goals

No report renderer, report producer, fuzz skill, or benchmark work.
