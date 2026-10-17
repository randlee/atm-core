---
version: 0.6.0
---

# Reference catalog

## Workflow entrypoints

- `references/capture-and-maintenance-workflow.md` - capture, lock, record, and maintain workflow
- `references/question-tuning-workflow.md` - calibrated independent rubric and question-tuning workflow
- `references/planning-review-workflow.md` - source-neutral planning gate and quality loop
- `references/development-review-workflow.md` - delivered-work review and reporting workflow
- `references/agent-delegation-workflow.md` - runner boundary, harness-specific agents, and parallel execution

## References

- `references/anchor-capture.md` - detecting, proposing, confirming, and wording anchors
- `references/anchor-document.md` - intent.md, anchors.json schema, anchors.md, IDs, lifecycle
- `references/bead-layout.md` - the anchor bead tree, bd commands, labels, overrides in beads, reading beads
- `references/question-tuning.md` - the testability rubric, per-anchor question overrides, proving the wording
- `references/triage-and-iteration.md` - the run, triage, apply, re-run, diff loop and the stop conditions
- `references/plan-size-and-shape.md` - oversized beads and phases: triage order, what Jev reads, `shape` metrics
- `references/measured-behaviour.md` - the numbers behind every rule
- `references/planning-gate.md` - inputs, run, exit codes, reading results
- `references/plan-source-contract.md` - Markdown and bd/br phase discovery, parity, safe copy, and canonical Jev bodies
- `references/development-gate.md` - inputs, evidence, run, exit codes, reading results
- `references/plan-anchor-questions.md` - the planning-gate question set and why it is orthogonal
- `references/code-review-questions.md` - the development-gate question set and its evidence
- `references/verdict-gating.md` - answers to verdicts, thresholds, coverage, diffs
- `references/report-format.md` - the provenance-quoting report and what to do with each verdict
- `references/jev-api.md` - endpoint, request and response, limits, retries, privacy opt-in, every flag and error code
- `references/installation-and-troubleshooting.md` - installing python3, bd, and the API key; PATH; Codex
- `references/agent-runner-contract.md` - one trusted agent resolution, strict fenced envelopes, and redacted attestation
- `assets/questions/plan.json`, `assets/questions/dev.json`, `assets/questions/testability.json`, `assets/questions/dedupe.json` - the question wording of record
- `assets/templates/intent.md`, `assets/templates/anchor-questions-bead.md`, `assets/examples/` - starting points and a worked example
- `<repository root>/tests/sc-plan-anchor/` - maintainer-only offline test suite; intentionally excluded from the redistributed plugin
