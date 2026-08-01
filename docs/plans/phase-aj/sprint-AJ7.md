---
id: AJ.7
title: Runtime Observation Source-Use Guard
status: planned
branch: feature/pAJ-s7-runtime-observation-source-guard
worktree: ../atm-core-worktrees/feature/pAJ-s7-runtime-observation-source-guard
target: integrate/phase-AJ
---

# Sprint AJ.7 — Runtime Observation Source-Use Guard

## Goal

Add the narrow static source-use guard that prevents runtime observation from
escaping its non-authoritative cache/snapshot boundary. AJ.7 owns only this
enforcement test and its lint registration; AJ.8 owns boundary-record wording.

## Hard Dependencies

- AJ.1 through AJ.6 development heads merged forward into this branch
- AJ.6's snapshot and roster projection are present through the immediate
  AJ.6 → AJ.7 merge-forward; AJ.6 QA/PR completion is not a dev-start gate
- `docs/plans/phase-aj/plan-phase-aj.md`
- `phase-aj-research.md` ends as a hard dependency after AJ.6: AJ.7 validates
  merged implementation, not pre-implementation research.

## Dependency Relation

- `must_follow` AJ.6 because the guard requires the implemented DTO, cache
  merge, HTTPS stripping, snapshot, and roster projection targets.
- AJ.8 `must_follow`s AJ.7 because a boundary record may name this enforcement
  gate only after it is real and passing.
- AJ.7 begins immediately after AJ.6 → AJ.7 merge-forward; it does not wait
  for AJ.6 QA. Repeat that merge before every AJ.7 dev/fix round. AJ.7's PR
  completes only after AJ.6's PR merges. No AJ pair is `parallel_safe`.
- On AJ.7 development-head push, AJ.8 begins immediately by merging
  AJ.7 → AJ.8; AJ.8 must complete that merge before any dev/fix round and does
  not wait for AJ.7 QA.

## Exact Targets

- `.just/tests/test_runtime_observation_boundary.py`
- `.just/run_lint.py` only if explicit registration is required by the existing
  test collector; do not alter unrelated lint execution.

## Interfaces To Add Or Modify

None. AJ.7 adds no production Rust type, wire field, transport behavior, or
boundary record. It inspects source text using the repository's existing narrow
static-test conventions.

## Deliverables

- The guard permits observation references only in DTO/caller construction,
  local request construction, HTTPS Write/Receive stripping, local dispatcher
  forwarding, `runtime_status_cache.rs` merge/snapshot, and roster projection.
- It rejects observation references in peer delivery, post-write routing,
  nudge, retry, admission, notification, and policy modules.
- Required-positive checks require `ActivityObservation`, both request DTO
  fields, `AckRequest` conversion, HTTPS Write/Receive stripping, the local
  dispatcher merge, and snapshot projection. The test fails at the Phase AJ
  entry baseline, preventing shape-only closure.

## Required Validation

- Run the new test through its normal `just lint` collection path.
- Add a fixture or source-copy test proving one forbidden policy reference
  fails and one listed required-positive target is mandatory.
- `just lint`
- `git diff --check`

## Acceptance Criteria

- The guard is narrow, deterministic, and cannot pass merely because AJ code
  is absent.
- AJ.7 is production-ready only for static enforcement; it does not alter
  boundary records, governing documents, or phase status.
- AJ.7 must_follow AJ.6 under the merge-forward and PR-completion rule above.
