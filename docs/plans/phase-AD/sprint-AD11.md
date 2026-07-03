---
id: AD.11
title: Smoke And Readiness Closeout
status: planned
branch: feature/pAD-s11-smoke-and-readiness-closeout
worktree: ../atm-core-worktrees/feature/pAD-s11-smoke-and-readiness-closeout
target: integrate/phase-AD
---

# Sprint AD.11 — Smoke And Readiness Closeout

## Goal

- prove the repaired AD behavior on the accepted line and record the readiness
  verdict

## Hard Dependencies

- `AD.1` through `AD.10` complete
- `docs/plans/phase-AD/plan-phase-AD.md`

## Exact Targets

- `Justfile`
- `scripts/smoke/run.py`
- `scripts/smoke/run_thorough.py`
- `scripts/smoke/run_thorough_graft.py`
- `reports/smoke/`
- `docs/plans/phase-AD/`
- `boundaries/atm-core/post-send-hook-emitter.toml`
- `docs/atm-core/boundaries.md`
- readiness/project-plan docs touched by final verdict

## Added Or Modified Artifacts

- modify smoke runners so they prove:
  - caller-owned identity success with invoking-shell `ATM_IDENTITY`
  - caller-owned missing-identity failure before daemon dispatch
  - local tmux post-send emission
  - graft advisory post-send emission
  - sender-visible warning behavior on forced emission failure
- add final report artifacts that map each AD closure claim to concrete smoke
  evidence
- modify readiness docs so `Phase AD` cannot be marked closed without all AD
  evidence present

## Obsolescence Instructions

- any smoke/readiness step that still treats daemon ambient identity, Claude
  mailbox append, or queued notification-runtime behavior as acceptable must be
  marked obsolete and removed from the accepted release gate

## Deliverables

- smoke evidence proving bare identity commands work correctly
- smoke evidence proving caller-owned commands fail locally when identity is
  missing
- smoke evidence proving configured post-send emission either succeeds or warns
- smoke evidence proving local tmux recipients on the accepted line
- smoke evidence proving graft-backed recipients on the accepted line
- recorded readiness verdict for `Phase AD`

## Required Work

- prove bare `send`/`read`/`ack` caller identity on the accepted baseline
- prove caller-owned commands reject missing identity before daemon dispatch
- prove local tmux-backed post-send emission
- prove graft-backed post-send emission
- prove sender-visible warning behavior on forced emission failure
- update readiness artifacts with final AD closure state
- add or retain a readiness/boundary gate that fails closed when the
  `PostSendHookEmitter` boundary TOML or matching inventory entry is missing

## This Sprint Does Not Close

- unrelated cross-host transport work
- new notification feature expansion

## Acceptance Criteria

- `just smoke normal` passes and its report artifacts prove the repaired local
  identity plus local-emitter lane
- smoke artifacts prove missing-identity caller commands fail locally before
  daemon dispatch
- `just smoke thorough` passes and its report artifacts prove the repaired
  graft-backed lane
- smoke artifacts record sender-visible warning behavior on forced emission
  failure
- readiness artifacts record `Phase AD` as closed only if all AD lanes above
  passed on the accepted line
- readiness/boundary evidence records the presence of
  `boundaries/atm-core/post-send-hook-emitter.toml` and the matching
  `docs/atm-core/boundaries.md` inventory entry

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- `just smoke normal`
- `just smoke thorough`
- `just validate all`
- boundary governance check for `PostSendHookEmitter`
- `git diff --check`
