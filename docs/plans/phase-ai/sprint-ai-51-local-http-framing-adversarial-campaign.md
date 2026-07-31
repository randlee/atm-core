---
title: AI.51 local HTTP framing adversarial campaign
status: planned
branch: feature/pAI-s51-local-http-framing-adversarial-campaign
recommended_agent: arch-ctm
recommended_model: deep-reasoning
execution_mode: after_merge
execution_dependencies:
  - AI.39
  - AI.50
dependencies_relation:
  - sprint: AI.39
    relation: must_follow
    rationale: Campaign targets the merged shared frame reader.
  - sprint: AI.50
    relation: must_follow
    rationale: Campaign evidence uses the completed fuzz renderer.
target: integrate/phase-ai-31-33
depends_on: AI.39, AI.50
---

# AI.51 — Local HTTP framing adversarial campaign

## Goal

Run the first bounded real campaign against AI.39 and promote only confirmed,
minimized defects to deterministic owning-crate tests.

## Execution Dependencies

AI.51 `must_follow`s AI.39 and AI.50. Merge-forward after both development
pushes; its PR completes after both PRs merge.

## Exact Targets

- AI.39 frame-reader tests in their owning crates
- AI.48 `just fuzz` workflow
- AI.50 report package under `site/reports/`

## Deliverables

1. Run one approved-worktree local framing campaign against the real reader
   with deterministic seed, baseline, worker cap, platform, and CPU features.
2. Reproduce/minimize every candidate three times. Promote only confirmed bugs
   to deterministic owning-crate tests; retain other outcomes in the report.
3. Use AI.50's copied templates and invoke `just reports-index` after each
   complete, failed, or incomplete campaign artifact write.

## Required Validation

- repeat one seed/baseline campaign and compare classifications
- targeted promoted tests, `just reports-index --check`, `just lint`, `just test`

## Acceptance Criteria

All four probe outcomes are durable and no candidate is silently counted PASS.

## Non-goals

No automatic production fix, unbounded fuzzing, or benchmark work.
