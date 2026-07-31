---
title: AI.42 local HTTP framing adversarial campaign
status: proposed
branch: feature/pAI-s42-local-http-framing-adversarial-campaign
recommended_agent: arch-ctm
recommended_model: deep-reasoning
execution_mode: after_merge
execution_dependencies:
  - AI.39
  - AI.41
  - AI.46
dependencies_relation:
  - sprint: AI.39
    relation: must_follow
    rationale: Campaign targets AI.39's merged shared frame reader.
  - sprint: AI.41
    relation: must_follow
    rationale: Campaign execution and reports use AI.41's merged workflow.
  - sprint: AI.46
    relation: must_follow
    rationale: Emits AI.46's generated report-index contract after every report.
target: integrate/phase-ai-31-33
depends_on: AI.39, AI.41, AI.46
---

# AI.42 — Local HTTP framing adversarial campaign

## Recommended Agent / Model

`arch-ctm` / deep-reasoning: classifying parser failures and promoting only
deterministic cross-platform regressions requires deep boundary analysis. This
is a planning-time recommendation, not a binding assignment.

## Execution Dependencies

AI.42 `must_follow`s AI.39, AI.41, and AI.46. Merge-forward trigger: all
development pushes, not QA; before every round merge all three into this
branch. PR-completion trigger: all three PRs merge into
`integrate/phase-ai-31-33` first. It runs AI.41's campaign against AI.39's
reader and emits AI.46's report index.

## Dependency Relations

| Sprint | Relation | Rationale |
| --- | --- | --- |
| AI.39 | must_follow | The real shared frame reader is the campaign target. |
| AI.41 | must_follow | The bounded coordinator/worker workflow and report package are required. |
| AI.46 | must_follow | It owns the generated report-index command invoked after every report. |

```yaml
plan_type: sprint_plan
phase: AI
sprint: AI.42
worktree: feature/pAI-s42-local-http-framing-adversarial-campaign
branch: feature/pAI-s42-local-http-framing-adversarial-campaign
status: proposed
estimated_scope: first bounded local HTTP campaign and regression-test promotion
```

## Goal

Run the first real, bounded adversarial campaign against the merged AI.39 local
HTTP frame reader using the AI.41 workflow. It provides reproducible evidence
for framing safety and transport parity, converts only confirmed and minimized
defects into deterministic owning-crate tests, and reports remaining defects
without silently changing production behavior.

## Governing requirements and ADRs

- `REQ-CORE-TRANSPORT-001` and `REQ-CORE-TRANSPORT-005B`
- ADR-032 — unified error contract
- ADR-033 — HTTP endpoint contract
- ADR-035 — canonical write ingress
- AI.39 shared bounded frame-reader contract
- AI.41 adversarial campaign workflow and report contract
- `.just/build_view_site.py` / `artifacts/view` ToolPanel contract; extend it,
  do not add a second generic report renderer.

## Deliverables

1. Run a reproducible `local-http-framing` campaign in an approved isolated
   worktree against the actual AI.39 reader—not a mock router or reimplemented
   parser. Record the integration baseline ref, campaign ID, deterministic
   seed, worker cap, case budget, per-worker timeout, platform, and enabled
   CPU features in its JSON evidence.

2. Exercise the four AI.41 worker surfaces with bounded generated inputs:
   fragmented header delimiter and body boundaries; coalesced frames and
   retained surplus; empty/truncated/oversized/malformed frames; declared
   content-length limits; default-close and bounded keep-alive; UDS/TCP parity
   on Unix; and TCP parity on Windows. The scalar compatibility path runs
   before any normal runtime-dispatched vector path. A vector-capable platform
   must prove identical parsed frames and typed errors; a non-vector platform
   records that it ran the scalar-compatible path rather than skipping.

3. Reproduce every candidate at least three times, minimize it, classify it,
   and trace it to a requirement or ADR. Promote only confirmed bugs to the
   nearest existing deterministic Rust test suite with the minimized input and
   expected user-visible result. An intentional boundary, inconclusive result,
   timeout, or worker failure stays visible in the report with a next owner;
   it cannot be described as a passing no-finding run.

4. Produce the AI.41 report package in `site/reports/`, with `report_type:
   fuzz` and AI.46's versioned envelope fields. The top-level report
   distinguishes complete/no-finding, complete/confirmed-finding, and
   incomplete campaign states. It includes a compact summary table and one
   XHTML panel per worker; the JSON sidecar remains the source of truth. Its
   same-named directory holds all JSON/XHTML evidence. `artifacts/view`
   registers a ToolPanel link only and never copies or re-renders this report.
   Invoke `just reports-index` after every artifact write, including an
   incomplete or failed campaign; producer tests prove this invocation.

5. Run targeted promoted tests and then the repository’s required formatting,
   test, lint, and boundary gates. If campaign evidence identifies a production
   defect, file it with minimized reproducer and owner; a separate fix sprint
   changes production code.

## Required validation

- Repeat the complete campaign once with the same seed and baseline; assert
  deterministic worker ordering, classification, and no unexplained result
  divergence.
- Verify the campaign runs the real parser and captures all four worker
  envelopes; fail evidence if a worker is absent, times out, or returns an
  invalid envelope.
- Validate every output HTML/XHTML/JSON artifact and run the promoted-test
  suite plus repository formatting, lint, test, and boundary gates.
- Review every promoted test for determinism, cross-platform validity, and
  direct connection to its minimized confirmed finding.

## Acceptance criteria

- A complete campaign supplies durable evidence for all four worker surfaces
  and both applicable local transports without claiming unsupported platform
  coverage.
- Every confirmed finding is minimized, reproduced three times, and either
  promoted as a deterministic test or assigned to a dedicated fix sprint.
- No campaign timeout, malformed result, or inconclusive candidate is hidden
  behind a PASS verdict.
- AI.42 does not change production framing behavior except through explicitly
  authorized, separately tracked fix work.

## Non-goals

No unbounded fuzzing, generic network-service fuzzing, performance benchmark,
or automatic bug fixing. This campaign is a safety and regression-evidence
gate; AI.40 remains the throughput closure sprint.
