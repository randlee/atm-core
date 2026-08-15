---
title: AN.8 Phase Validation — Motivating Queries and Evidence
status: complete
branch: feature/pan-s8-validation-evidence
worktree: ../atm-core-worktrees/feature/pan-s8-validation-evidence
target: integrate/phase-an
---

# AN.8 — Phase Validation: Motivating Queries and Evidence

**recommended_agent:** Cipher-311d/fast (evidence assembly; escalate query
authoring questions to arch-ctm/deep-reasoning).
**must_follow:** AN.3, AN.4, AN.6, AN.7 (all pushed integration lines merged
forward; AN.5 arrives transitively via AN.6).
**unblocks:** phase completion PR `integrate/phase-an → develop`.
**parallel_safe:** none; this sprint validates the composed whole.

**traceability:** plan-phase-an.md "Why this phase exists" (Q1–Q5) and the
template-agnostic invariant; AN.1 fixture captures. Requirement IDs assigned
during plan hardening.

## Deliverables

1. The four motivating queries, expressed as an **orchestration-layer
   artifact** (SQL against `decomposed_messages` and/or `atm search`
   invocations, committed under `docs/plans/phase-an/fixtures/queries/`) —
   explicitly not atm code:
   - Q1: time span of every sprint (first assignment → completion message)
   - Q2: QA iterations per sprint
   - Q3: findings per QA round by Blocking/Important/Minor
   - Q4: dev agent per sprint
   Authored against the real templates captured in AN.1, using only
   introspection output to discover keys.
2. **File-parser replacement test:** inspect the AN.1-captured Python helper
   before treating it as evidence. It is a JSON mailbox reader/atomic writer,
   not an analytical parser, so it has no Q1–Q4 answer to compare. Replace
   its file-oriented data source with the durable decomposed-view artifacts:
   a shared task/QA-shaped corpus yields the documented Q1–Q4 answers with
   zero file parsing by the query path.
3. **Template-agnosticism check:** a synthetic template set with a
   deliberately different metadata/var vocabulary answers analogous
   span/count/rollup questions using the same generic surface, with no atm
   changes — proving no orchestration semantics leaked into core.
4. Routing-matrix smoke: a same-team, same-host templated send is decomposed
   and queryable. The other three cells — same-team/cross-host,
   foreign-team/same-host, and foreign-team/cross-host — arrive as rendered
   plain text and are readable there. Each plain-text assertion inspects the
   stored row (`template_sha` and `vars_json` NULL, rendered `message_text`)
   and proves the send created no catalog admission, not merely CLI output.
5. Phase evidence per current conventions (smoke reports on macOS, Linux,
   and Windows lanes; retained artifacts), plus `docs/project-plan.md` and
   `CHANGELOG.md` entries for the phase.

Implementation evidence is retained in
[`validation-evidence.md`](./validation-evidence.md). The four-cell Tokio
runtime test runs in the repository's Linux/macOS/Windows CI matrix; it is the
platform evidence for this code-level routing invariant. Physical cross-host
template synchronization remains a declared Phase AN non-goal.

## Acceptance criteria

- Q1–Q4 return correct results against the shared task/QA-shaped corpus, with
  expected values hand-computed in the fixtures. The retained real templates
  are independently rendered byte-for-byte by the Phase AN compose
  passthrough test; the query corpus uses their public template names and
  discovered durable keys without inventing an unavailable historical send.
- The historical helper's actual file-oriented scope is recorded accurately;
  the replacement query artifacts answer Q1–Q4 from the shared decomposed
  corpus and read no files outside SQLite.
- The synthetic-vocabulary check passes with zero atm-core diffs.
- The four-cell routing matrix (same-team/same-host → decomposed;
  same-team/cross-host → plain; foreign-team/same-host → plain;
  foreign-team/cross-host → plain) passes on all three smoke lanes, with
  stored-row and catalog-admission assertions for every plain-text cell.
- Evidence ledger accepted; project-plan and CHANGELOG entries merged.

## Required validation

- query-correctness fixtures (hand-computed expectations)
- parser-vs-query equivalence run
- synthetic-vocabulary no-diff check (`git diff --exit-code` on atm crates)
- full smoke suite, all platforms
- cargo test/format/lint suite

## Non-closure

This sprint closes phase validation only. Deferred items remain deferred per
the plan's Non-goals: dolt allowlist enforcement, cross-host template sync,
HTTP aggregation language, path-body rejection, historical backfill.
