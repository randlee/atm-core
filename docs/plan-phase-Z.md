# Phase Z Plan

## Goal

Validate the first daemon + SQLite mail-SSOT release in real executable use
after the `Phase Y` implementation line closes and the final `develop` gate is
explicitly opened.

Phase `Z` owns the progressive rollout and release-readiness work that should
not be mixed into the architectural cleanup history:

- daemon bring-up on the real binaries
- executable smoke coverage across the supported feature set
- smoke finding closure and revalidation
- `atm-dev` canary / dogfood on the new executables
- final release-fix loop and ship/no-ship verdict

## Baseline

- planning branch: `plan/phase-Z`
- prerequisite implementation line:
  - `Phase Y` accepted through the final `Phase Yd` develop gate
  - `Phase Ye` closed on `develop`
- blocking closeout line before `Phase Z` may begin:
  - `Phase Yd`
- future integration branch: `integrate/phase-Z` (not yet created)

## Phase Entry Criteria

`Phase Z` does not begin until the accepted `Phase Y` line is develop-ready and
the final `Phase Yd` record says `Phase Z` may begin:

- the write-owner boundary is enforced
- the delivery-policy coordinator and required state machines are landed
- the compatibility field set is finalized
- the append-only/export contract decision is complete
- the later `Phase Yb` / `Phase Yc` message-path and production-readiness
  follow-up work is closed on the accepted `Phase Y` line
- the blocking issues in `docs/phase-Y/issues.md` are closed
- the readiness record in `docs/phase-Yd/readiness.md` explicitly states:
  - `Phase Y` may land on `develop`
  - `Phase Z` may begin
- the post-`Phase Y` daemon ownership simplification line in `Phase Ye` is
  complete and no longer changes the rollout gate

Current gate status:

- `Phase Yd` final accepted candidate line: `19376e42`
- `Phase Y` may land on `develop`
- `Phase Z` may begin
- `Phase Ye` is complete and merged on the current `develop` baseline

## Pre-Phase JSON I/O Status

The CLI JSON I/O audit is already complete:

- audit record: `docs/phase-Z/cli-json-io-audit.md`
- retained-command `--json` output is already implemented on all 9 commands
- no `Phase Y` or `Phase Z` output retrofit work is required
- structured JSON input remains absent and is explicitly deferred until after
  `Phase Z`

The planning consequence is intentional:

- `Phase Z` is not blocked on a JSON-output expansion sprint
- `Phase Z` smoke/dogfood should validate the existing public JSON outputs as
  part of normal executable coverage
- any future JSON-input work must start from a separate public DTO design and
  must not be smuggled into the smoke/release validation line

## Sprint Sequence

### Z.1 Smoke Bring-Up

Purpose:

- developer-coordinated daemon bring-up
- feature-by-feature executable smoke pass
- corner-case and recovery verification on the real binaries
- freeze the authoritative smoke checklist and smoke findings ledger used by
  `Z.2`

Execution branch:
- `feature/pZ-s1-smoke-bring-up`

Execution worktree:
- `../atm-core-worktrees/feature/pZ-s1-smoke-bring-up`

### Z.2 Fix And Revalidate

Purpose:

- close smoke findings from `Z.1`
- re-run full executable validation on the fixed branch
- carry forward only the frozen `Z.1` smoke findings ledger

Execution branch:
- `feature/pZ-s2-fix-and-revalidate`

Execution worktree:
- `../atm-core-worktrees/feature/pZ-s2-fix-and-revalidate`

### Z.3 `atm-dev` Canary and Dogfood

Purpose:

- move from single-operator smoke to `atm-dev` team use on the new binaries
- verify UX, recovery text, and operational behavior under real use
- produce the canary participant list, operator-report path, and canary
  findings ledger used by `Z.4`

Execution branch:
- `feature/pZ-s3-atm-dev-canary-and-dogfood`

Execution worktree:
- `../atm-core-worktrees/feature/pZ-s3-atm-dev-canary-and-dogfood`

### Z.4 Final Fixes And Release Sign-Off

Purpose:

- close `Z.3` findings
- produce the final release-readiness verdict
- rerun the final executable validation and release checklist on the closeout
  branch

Execution branch:
- `feature/pZ-s4-final-fixes-and-release-sign-off`

Execution worktree:
- `../atm-core-worktrees/feature/pZ-s4-final-fixes-and-release-sign-off`

## Sprint Artifact Summary

`Phase Z` uses one named artifact set throughout execution:

- `Z.1` / `Z.2`:
  - `docs/phase-Z/smoke-checklist.md`
  - `docs/phase-Z/smoke-findings-ledger.md`
- `Z.3`:
  - `docs/phase-Z/canary-dogfood-checklist.md`
  - `docs/phase-Z/canary-findings-ledger.md`
- `Z.4`:
  - `docs/phase-Z/release-checklist.md`
  - `docs/phase-Z/readiness.md`

The sprint docs remain the only authoritative source for per-sprint
deliverables, acceptance criteria, and closure rules.

## Phase Rules

- all validation is against the real built executables, not only harness/unit
  tests
- smoke findings feed only the immediately following fix sprint
- dogfood findings feed only the final fix/sign-off sprint
- release readiness is not declared until the documented executable flows and
  recovery behavior are revalidated after each fix round

## Initial Planning Outputs

- `docs/plan-phase-Z.md`
- `docs/phase-Z/cli-json-io-audit.md`
- `docs/phase-Z/smoke-checklist.md`
- `docs/phase-Z/smoke-findings-ledger.md`
- `docs/phase-Z/canary-dogfood-checklist.md`
- `docs/phase-Z/canary-findings-ledger.md`
- `docs/phase-Z/release-checklist.md`
- `docs/phase-Z/readiness.md`
- `docs/phase-Z/sprint-Z1.md`
- `docs/phase-Z/sprint-Z2.md`
- `docs/phase-Z/sprint-Z3.md`
- `docs/phase-Z/sprint-Z4.md`
