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
  - caller-context success with invoking-shell `ATM_IDENTITY` plus `ATM_TEAM`
  - caller-context failure when either `ATM_IDENTITY` or `ATM_TEAM` is
    missing
  - explicit override success when env caller context is absent but the
    command-line override surface is present
  - command-matrix coverage for the retained ATM command inventory:
    - `send`
    - `read`
    - `ack`
    - `list`
    - `clear`
    - `log`
    - `doctor`
    - `members`
    - `teams`
    - `teams add-member`
    - `teams update-member`
    - `teams backup`
    - `teams restore`
  - local tmux post-send emission
  - cross-team, cross-repo local post-send emission from a sender working in a
    different repository with a different durable `home_dir`
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

- smoke evidence proving bare caller-context commands work correctly
- smoke evidence proving retained ATM commands fail locally when caller
  identity or caller team is missing
- smoke evidence proving explicit override paths still work when env caller
  context is absent
- smoke evidence proving the retained ATM command matrix stays on the declared
  caller context instead of guessed fallback context
- smoke evidence proving configured post-send emission either succeeds or warns
- smoke evidence proving local tmux recipients on the accepted line
- smoke evidence proving sender repository/home-dir differences do not change
  post-send emission behavior
- smoke evidence proving graft-backed recipients on the accepted line
- recorded readiness verdict for `Phase AD`

## Required Work

- prove bare `send`, `read`, `ack`, `list`, `clear`, `log`, `doctor`,
  `members`, `teams`, `teams add-member`, `teams update-member`,
  `teams backup`, and `teams restore` all execute on the declared caller
  context on the accepted baseline
- prove retained ATM commands reject missing identity before retained
  execution or daemon dispatch
- prove retained ATM commands reject missing team before retained execution or
  daemon dispatch
- prove explicit override paths still work when env caller context is absent
- prove local tmux-backed post-send emission
- prove `atm send` from another team/repository with a different sender
  `home_dir` still fires the same recipient post-send behavior
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
  caller-context plus local-emitter lane
- smoke artifacts prove missing-identity caller commands fail locally before
  retained execution or daemon dispatch
- smoke artifacts prove missing-team caller commands fail locally before
  retained execution or daemon dispatch
- smoke artifacts prove explicit override paths still work when env caller
  context is absent
- smoke artifacts or targeted CLI-matrix artifacts prove the retained ATM
  command inventory executes against the declared caller context:
  - `send`
  - `read`
  - `ack`
  - `list`
  - `clear`
  - `log`
  - `doctor`
  - `members`
  - `teams`
  - `teams add-member`
  - `teams update-member`
  - `teams backup`
  - `teams restore`
- smoke artifacts prove that sending from another team/repository with a
  different sender `home_dir` does not change whether local post-send
  emission is attempted
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
- targeted CLI integration coverage for the retained ATM command matrix
- `just validate all`
- boundary governance check for `PostSendHookEmitter`
- `git diff --check`
