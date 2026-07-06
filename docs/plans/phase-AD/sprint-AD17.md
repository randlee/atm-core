---
id: AD.17
title: Boundary Reset Verification Closeout
status: planned
branch: feature/pAD-s17-boundary-reset-verification-closeout
worktree: ../atm-core-worktrees/feature/pAD-s17-boundary-reset-verification-closeout
target: integrate/phase-AD
---

# Sprint AD.17 — Boundary Reset Verification Closeout

## Goal

- prove the graft boundary reset on the accepted line and close the added
  `AD.12` through `AD.16` corrective scope

## Hard Dependencies

- `AD.13` complete
- `AD.14` complete
- `AD.15` complete
- `AD.16` complete
- `docs/plans/phase-AD/plan-phase-AD.md`

## Exact Targets

- `scripts/smoke/run.py`
- `scripts/smoke/run_thorough.py`
- `scripts/smoke/run_thorough_graft.py`
- `reports/smoke/`
- `docs/plans/phase-AD/`
- readiness/project-plan docs touched by final verdict

## Interfaces To Verify

The accepted verification target after this sprint is:

- retained ATM message identity is ULID-only on the accepted line
- daemon local IPC remains framed unary request/response for retained ATM
  command paths
- post-send emission still happens after persistence through the accepted
  `PostSendHookEmitter` seam
- graft-backed receiver behavior is verified without reintroducing shared
  advisory session protocol families

## Paths To Delete

- any smoke or regression test that still expects daemon-owned graft advisory
  register/unregister/fetch/drain/stream packet families
- any smoke or readiness step that treats a dedicated advisory-stream daemon
  socket as release-required architecture

## Deliverables

- smoke evidence proving the ULID-only message identity line holds after the
  boundary reset
- smoke evidence proving local tmux post-send still works after the boundary
  reset
- smoke evidence proving graft-backed post-send still works after the boundary
  reset
- regression evidence proving the local IPC receive loop no longer hangs on the
  deleted graft stream path
- final readiness verdict for the `AD.12` through `AD.16` corrective line

## This Sprint Does Not Close

- unrelated cross-host feature work outside the reset scope

## Acceptance Criteria

- smoke artifacts prove retained ATM message ids remain ULID-only on the
  accepted line
- smoke artifacts prove the corrected post-send path still works for local
  tmux-backed recipients
- smoke artifacts prove the corrected graft-backed lane works without shared
  advisory session packet families
- targeted Windows/local-IPC regression coverage proves the removed stream path
  no longer blocks command completion
- readiness artifacts record `Phase AD` as closed only if the original
  `AD.1` through `AD.11` gates and the added `AD.12` through `AD.16` reset
  gates all pass on the accepted line

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- `just smoke normal`
- `just smoke thorough`
- targeted Windows/local-IPC regression coverage for the former advisory-stream lane
- `git diff --check`
