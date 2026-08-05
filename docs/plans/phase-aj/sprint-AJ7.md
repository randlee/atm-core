---
id: AJ.7
title: Runtime Observation Source-Use Guard
status: complete
branch: feature/pAJ-s7-runtime-observation-source-guard
worktree: ../atm-core-worktrees/feature/pAJ-s7-runtime-observation-source-guard
target: integrate/phase-aj
---

# Sprint AJ.7 — Runtime Observation Source-Use Guard

## Goal

Add the narrow static source-use guard that prevents runtime observation from
escaping its non-authoritative cache/snapshot boundary. AJ.7 owns only this
enforcement test and its lint registration; AJ.8 owns boundary-record wording.

After AJ.6, `phase-aj-research.md` remains planning context rather than a hard
dependency: AJ.7 validates the merged implementation and its enforcement
shape.

## Hard Dependencies

- AJ.1 through AJ.6 development heads merged forward into this branch
- AJ.6's snapshot and roster projection are present through the immediate
  AJ.6 → AJ.7 merge-forward; AJ.6 QA/PR completion is not a dev-start gate
- `docs/plans/phase-aj/plan-phase-aj.md`

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
- `.just/check_runtime_observation_boundary.py`
- `.just/run_lint.py` only if explicit registration is required by the existing
  test collector; do not alter unrelated lint execution.

## Interfaces To Add Or Modify

AJ.7 adds no production Rust type, wire field, transport behavior, or boundary
record. `.just/check_runtime_observation_boundary.py` is the production lint
checker; its test file verifies the checker. The checker is default-deny: it
scans all Rust sources under `crates/` for observation identifiers, permits
them only at an explicit file/symbol allowlist, and fails any unlisted use.
Its tests must follow the existing environment-boundary guard configuration
pattern with explicit required-positive file/symbol checks. The forbidden
consumer regression fixtures include these concrete paths:

- `crates/atm-daemon/src/runtime_health/peer_delivery_router.rs`
- `crates/atm-daemon/src/runtime_health/peer_authority.rs`
- `crates/atm-daemon/src/peer_drain_coordinator.rs`
- `crates/atm-daemon/src/post_send_emitter.rs`
- `crates/atm-core/src/delivery_policy.rs` (`DeliveryPolicyCoordinator`,
  `DeliveryEventFamily`)
- `crates/atm/src/commands/internal_nudge.rs`

The positive list must require these concrete files and symbols, rather than
merely allowing their directories:

- `crates/atm-core/src/caller_context.rs` — `ActivityObservation`
- `crates/atm-core/src/send/mod.rs` — `WriteRequest` observation field
- `crates/atm-core/src/read/mod.rs` — `ReadQuery` observation field
- `crates/atm-core/src/ack/mod.rs` — `AckRequest` conversion into `WriteRequest`
- `crates/atm-daemon/src/https_transport.rs` — HTTPS Write/Receive stripping
- `crates/atm-daemon/src/runtime_health.rs` — local dispatcher forwarding
- `crates/atm-daemon/src/runtime_status_cache.rs` — merge and snapshot projection

## Deliverables

- The guard permits observation references only at the enumerated positive
  file/symbol targets, plus the roster projection target explicitly named by
  the test. Its default-deny scan rejects every other use; the listed consumer
  paths are mandatory regression fixtures, not a hand-maintained complete
  denylist.
- The required-positive configuration must fail at the Phase AJ entry baseline,
  preventing shape-only closure.

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
