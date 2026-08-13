---
title: AN.12 Workflow Metadata Validation And Retained Evidence
status: planned
branch: feature/an12-workflow-validation-evidence
target: integrate/phase-an
---

# AN.12 — Workflow Metadata Validation And Retained Evidence

**recommended_agent:** Cipher-311d/fast (evidence assembly; escalate failing
generic lifecycle semantics to arch-ctm/deep-reasoning).
**must_follow:** AN.11 merged. Merge its integration tip before every dev/fix
round; this sprint validates the composed extension rather than a branch-local
subset.
**unblocks:** Phase AN extension completion review.
**parallel_safe:** none. Evidence must reference the final integrated API and
migration state.

**traceability:** ADR-046 and all AN.9–AN.11 requirements.

## Deliverables

1. Commit two retained fixture families whose terminology intentionally differs
   from each other and from the recommended `dev`/`qa` examples. Each family
   declares literal template tags, workflow scope/state/stage/transition, and
   optional iteration values. Hand-computed expected facts demonstrate tags,
   provenance, revision history, durations, incomplete cycles, and iteration
   counts without ATM code knowing the vocabulary.
2. Add an end-to-end same-host decomposed evidence lane: template registration,
   admission, local CLI/HTTP filters, read-only Python query, lifecycle
   projection, and no-op plus configured-test telemetry sink. Retain commands,
   expected outputs, and redacted artifacts under the Phase AN evidence
   directory.
3. Prove safety/compatibility: migration from an AN.8 fixture, legacy/plain
   rows with no fabricated snapshot, all four routing cells, immutable
   revision history, reserved-prefix rejection, and exporter failure isolation.
4. Update the Phase AN plan/project-plan status and the author guide only with
   evidence actually produced. Any missing physical cross-host proof remains
   explicitly blocked/non-goal rather than silently inferred from a local
   lane.

## Acceptance criteria

- Both unrelated vocabularies pass the exact same generic admission/query/
  pairing implementation with no ATM source change between fixtures.
- Every expected duration, incomplete observation, tag source, and effective
  tag is fixture-hand-computed and matched by retained results.
- AN.8 database compatibility and four-cell routing expectations pass on the
  Linux/macOS/Windows CI matrix.
- Telemetry attributes originate solely from stored snapshots/timestamps; the
  output contains no body or merged-variable payload; failure isolation is
  demonstrated.
- Documentation labels only completed proof as complete and links the one
  authoring guide plus ADR/requirements/sprint evidence.

## Required validation

- end-to-end fixture integration tests on Linux/macOS/Windows CI
- AN.8 database migration/reopen test
- retained local CLI/HTTP/Python query transcripts and expected-output checks
- no-op, configured-test-sink, and failing-sink tests
- `just test`, `just lint`, and documentation-link/reference validation

## Paths to delete

None. Retained AN.8 evidence remains historical evidence; AN.12 adds its own
versioned workflow-metadata evidence rather than rewriting prior results.

## Non-closure

This sprint closes the optional workflow-metadata extension only. It does not
backfill prose, make telemetry mandatory, authorize remote analytics, or add
new workflow-specific behavior to ATM.
