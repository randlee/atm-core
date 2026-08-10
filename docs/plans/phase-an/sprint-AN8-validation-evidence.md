---
title: AN.8 Phase Validation — Motivating Queries and Evidence
status: draft
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
2. **Parser-replacement test:** the captured agent-written Python tmp-file
   parser is re-implemented as queries; both are run against a corpus
   seeded by real templated sends and must produce equivalent answers with
   zero file parsing.
3. **Template-agnosticism check:** a synthetic template set with a
   deliberately different metadata/var vocabulary answers analogous
   span/count/rollup questions using the same generic surface, with no atm
   changes — proving no orchestration semantics leaked into core.
4. Cross-host story smoke: a same-team, same-host templated send is
   decomposed and queryable. A templated send to both a same-team foreign-host
   recipient and a foreign-team foreign-host recipient arrives as rendered
   plain text and is readable there.
5. Phase evidence per current conventions (smoke reports on macOS, Linux,
   and Windows lanes; retained artifacts), plus `docs/project-plan.md` and
   `CHANGELOG.md` entries for the phase.

## Acceptance criteria

- Q1–Q4 return correct results against both the seeded corpus and the
  real-template fixture corpus, with expected values hand-computed in the
  fixtures.
- Parser-replacement equivalence holds on the shared corpus; the query
  artifact reads no files outside SQLite.
- The synthetic-vocabulary check passes with zero atm-core diffs.
- The three-row routing matrix (same-team/same-host → decomposed;
  same-team/cross-host → plain; foreign-team/cross-host → plain) passes on
  all three smoke lanes.
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
