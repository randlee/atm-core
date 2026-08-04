---
id: AJ.8
title: Runtime Observation Boundary Record
status: planned
branch: feature/pAJ-s8-runtime-observation-boundary-record
worktree: ../atm-core-worktrees/feature/pAJ-s8-runtime-observation-boundary-record
target: integrate/phase-AJ
---

# Sprint AJ.8 — Runtime Observation Boundary Record

## Goal

Reclassify the daemon boundary record from the explicit pre-AJ planned target
to the implemented non-authoritative runtime-observation contract. AJ.8 owns
only the machine-readable and matching human boundary records.

After AJ.6, `phase-aj-research.md` remains planning context rather than a hard
dependency: AJ.8 validates merged implementation and contracts.

## Hard Dependencies

- AJ.1 through AJ.7 development heads merged forward into this branch
- AJ.7's passing source-use guard is present through the immediate AJ.7 → AJ.8
  merge-forward; AJ.7 QA/PR completion is not a dev-start gate
- `docs/plans/phase-aj/plan-phase-aj.md`

## Dependency Relation

- `must_follow` AJ.7 because this sprint names its real enforcement gate in
  the machine-readable boundary contract.
- AJ.9 `must_follow`s AJ.8 because governing documents must cite the final
  boundary contract rather than a planned placeholder.
- AJ.8 begins immediately after AJ.7 → AJ.8 merge-forward; it does not wait
  for AJ.7 QA. Repeat that merge before every AJ.8 dev/fix round. AJ.8's PR
  completes only after AJ.7's PR merges. No AJ pair is `parallel_safe`.
- On AJ.8 development-head push, AJ.9 begins immediately by merging
  AJ.8 → AJ.9; AJ.9 must complete that merge before any dev/fix round and does
  not wait for AJ.8 QA.

## Exact Targets

- `boundaries/atm-daemon/daemon-status-source.toml`
- `docs/atm-daemon/boundaries.md`

## Interfaces To Add Or Modify

None. AJ.8 changes no Rust API, I/O ownership, dependency edge, or runtime
behavior.

## Deliverables

- Replace the explicit pre-AJ planned labels with the implemented contract only
  after the AJ.7 source-use guard passes on the merged-forward child branch.
- Add `runtime_observation_non_authoritative` to
  `BOUNDARY-StatusSource-Daemon.review_gates`.
- State that cache merge/snapshot projection may inspect observation while
  routing, nudge, notification, retry, admission, delivery, and policy may
  not. Do not broaden `io_owns`, `io_forbidden`, or dependencies.

## Required Validation

- Parse the TOML through the repository boundary-lint path.
- `just lint`
- `git diff --check`

## Acceptance Criteria

- Human and machine-readable boundary records agree exactly on the named guard
  and non-authoritative observation rule.
- AJ.8 is production-ready only for boundary record reclassification; it does
  not reconcile governing requirements/ADR/architecture or close the phase.
- AJ.8 must_follow AJ.7 under the merge-forward and PR-completion rule above.
