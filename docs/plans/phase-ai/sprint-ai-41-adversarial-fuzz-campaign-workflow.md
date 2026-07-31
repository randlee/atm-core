---
title: AI.41 adversarial fuzz campaign workflow
status: proposed
branch: feature/pAI-s41-adversarial-fuzz-campaign-workflow
recommended_agent: Cipher-311d
recommended_model: fast
execution_mode: after_merge
execution_dependencies:
  - AI.46
dependencies_relation:
  - sprint: AI.39
    relation: parallel_safe
    rationale: AI.41 owns campaign tooling and report contracts; AI.39 owns Rust framing adapters.
  - sprint: AI.46
    relation: must_follow
    rationale: Emits AI.46's generated report-index contract after every report.
target: integrate/phase-ai-31-33
---

# AI.41 — Adversarial fuzz campaign workflow

## Recommended Agent / Model

`Cipher-311d` / fast: this bounded tooling, agent-contract, and reporting work
is well-scoped without a production parser change. This is a planning-time
recommendation, not a binding assignment.

## Execution Dependencies

AI.41 is `parallel_safe` with AI.39: it owns `.claude` campaign/report tooling;
AI.39 owns Rust framing adapters. It `must_follow`s AI.46: merge-forward after
AI.46's development push, not QA; its PR completes only after AI.46's PR
merges. AI.42 then follows both parents.

## Dependency Relations

| Sprint | Relation | Rationale |
| --- | --- | --- |
| AI.39 | parallel_safe | AI.41 owns `.claude` campaign/report tooling; AI.39 owns `atm-core` framing and local transport adapters. |
| AI.46 | must_follow | It owns the generated report-index command invoked after every report. |

```yaml
plan_type: sprint_plan
phase: AI
sprint: AI.41
worktree: feature/pAI-s41-adversarial-fuzz-campaign-workflow
branch: feature/pAI-s41-adversarial-fuzz-campaign-workflow
status: proposed
estimated_scope: campaign contracts, registered agents, and durable report artifacts
```

## Goal

Create a reusable, bounded adversarial-fuzzing workflow for ATM. It follows
the established sc-compose coordinator/worker pattern: a registered coordinator
validates one campaign contract, launches at most four focused background
workers, reproduces and minimizes candidate failures, and writes a complete
human and machine-readable report package. This is tooling, not a product
parser or a substitute for deterministic unit tests.

## Governing requirements and ADRs

- `REQ-CORE-TRANSPORT-005B`
- ADR-033 — HTTP endpoint contract
- ADR-035 — canonical write ingress
- `.just/build_view_site.py` / `artifacts/view` ToolPanel contract; extend it,
  do not add a second generic report renderer.
- sc-compose `adversarial-fuzzing` campaign contract, used as the design
  precedent (coordinator/worker bounds, classification, promotion, and report
  shape—not as a runtime dependency)

## Deliverables

1. Add a versioned, discoverable `adversarial-fuzzing` skill, a registered
   coordinator agent, and a single-responsibility probe agent. The coordinator
   accepts exactly one fenced JSON campaign contract, validates that
   `worktree_path` is an approved absolute ATM worktree, rejects path traversal
   and unsafe limits, and fails closed when its registered workers are missing.

   ```json
   {
     "worktree_path": "/absolute/path/to/atm-core-worktree",
     "target": "local-http-framing | full",
     "baseline_ref": "optional git ref",
     "seed": 157,
     "max_workers": 4,
     "cases_per_worker": 100,
     "per_worker_timeout_s": 120,
     "promote_regressions": true
   }
   ```

2. Define an ATM-specific worker portfolio for the local HTTP boundary. The
   coordinator assigns only relevant workers; a `full` campaign uses all four:

   | Correlation ID | Focus | Required probes |
   | --- | --- | --- |
   | `fragment-probe` | frame shape | delimiter/body splits, coalesced frames, EOF |
   | `limit-probe` | negative boundary | caps, malformed start/header/length, bounded allocation |
   | `transport-probe` | parity | UDS/TCP equivalence on Unix and TCP on Windows |
   | `differential-probe` | regression oracle | baseline/head, deterministic and metamorphic relations, panic/hang/timeout |

   Workers run in background, receive unique correlation IDs and deterministic
   seeds, are capped at four concurrent workers, may retry a recoverable error
   once, return one structured result, and never edit product code or commit.
   This sprint deliberately does **not** add `just fuzz`, a standalone Python
   fuzz runner, or a cargo-fuzz target merely to imitate benchmark tooling.

3. Adopt sc-compose’s evidence and triage rules. A candidate is a confirmed
   bug only for a repeatable panic/hang/timeout, accepted-input contract
   violation, specified transport-parity failure, regression against baseline,
   or violated stable error boundary. Every candidate carries command, seed,
   minimal input, observed result, expected oracle, requirement/ADR trace, and
   three-reproduction result. Intentional invalid-input rejections remain
   visible as `intentional_boundary`; insufficient-oracle cases remain
   `inconclusive`. Neither is silently counted as PASS.

4. Vendor the established sc-compose fuzz-report package; do not implement a
   second renderer. Copy these files **verbatim** from `randlee/sc-compose`
   PR #165 source:

   - `.claude/skills/html-report/templates/fuzz-run-report.html.j2`
   - `.claude/skills/html-report/templates/fuzz-run-agent.xhtml.j2`
   - `.claude/skills/html-report/fuzz-run-agent-contract.md`
   - `.claude/skills/adversarial-fuzzing/SKILL.md`
   - `.claude/agents/sc-adversarial-fuzz-coordinator.md` and
     `.claude/agents/sc-adversarial-fuzz-probe.md`

   The coordinator/probe output conforms to
   `fuzz-run-agent-contract.md` unchanged, so the copied templates render
   without ATM-specific template edits. A campaign writes a top-level
   `site/reports/YYYYMMDD-N-fuzz-report.html`, a JSON sidecar at
   `site/reports/YYYYMMDD-N-fuzz-report/YYYYMMDD-N-fuzz-report.json`, and one
   XHTML panel per worker in that same derived directory. Its JSON envelope
   includes AI.46's `schema_version`, `report_type: fuzz`, `generated_at`,
   relative `report_html`, and ADR-044-safe opaque `host_label`. The report
   summary is a compact table of target, seed, case budget, completed/pass
   counts, and PASS/FAIL; each worker panel holds its durable JSON envelope
   and context. AI.46 links only to HTML rendered by these copied templates.

   `site/reports` is the durable report/evidence root; its same-named directory
   holds the JSON and XHTML supporting each HTML page. `artifacts/view` stays
   transient and registers one ToolPanel link only; it does not copy or render
   a second report.

   Invoke AI.46's report-index command after every report artifact write,
   including incomplete/failed campaigns.

5. Add structural tests for campaign input validation, registered-worker
   resolution, deterministic correlation ordering, worker timeout/partial
   failure retention, result schema, artifact-path derivation, and report
   rendering. Validate generated HTML and XHTML with the repository’s adopted
   HTML/XHTML validators before reporting a completed campaign.

6. Keep this a separate campaign skill: `codex-orchestration` and
   `graph-orchestration` own planning/dispatch and graph queries, not bounded
   fuzz input validation, worker lifecycle, minimization, or evidence.

## Required validation

- A dry-run fixture campaign exercises all four worker result shapes,
  including timeout and malformed worker output, without invoking product
  fuzzing; assert no result is dropped and ordering is deterministic.
- Validate safe worktree containment, traversal rejection, worker cap, timeout
  validation, and rejection of unregistered agent paths.
- Render one fixture report and validate its HTML, JSON sidecar, and every
  XHTML panel; assert all links are relative and no absolute local path leaks.
- Run `just reports-index --check` in the producer PR gate.
- Run the repository’s required formatting, unit, lint, and boundary gates for
  the files introduced by this sprint.

## Acceptance criteria

- A campaign cannot start with an unsafe worktree, invalid limits, or an
  unregistered worker.
- The full worker portfolio is bounded, seeded, ordered, and non-lossy for
  success, failure, timeout, and malformed-result states.
- The report package is a self-contained, browsable record with machine
  evidence and per-worker panels.
- AI.41 adds no production HTTP framing/parser logic and makes no claim that a
  real campaign has run.

## Non-goals

No production parser change, HTTP benchmark change, automatic production fix,
or release claim. AI.39 owns framing behavior, AI.40 owns throughput evidence,
and AI.42 owns the first real HTTP-framing campaign plus any promoted
deterministic tests.
