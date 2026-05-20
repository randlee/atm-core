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

- planning branch: `feature/pY-s0-planning`
- prerequisite implementation line: completed `Phase Y`
- blocking closeout line before `Phase Z` may begin:
  - `Phase Yd`
- future integration branch: `integrate/phase-Z`

## Phase Entry Criteria

`Phase Z` does not begin until `Phase Y` is closed and `Phase Yd` says the line
may land on `develop`:

- the write-owner boundary is enforced
- the delivery-policy coordinator and required state machines are landed
- the compatibility field set is finalized
- the append-only/export contract decision is complete
- `Y.0` trivial fixes and `Y.1` through `Y.6` implementation work are merged
  onto the authoritative integration line
- the blocking issues in `docs/phase-Y/issues.md` are closed
- the readiness record in `docs/phase-Yd/readiness.md` explicitly states:
  - `Phase Y` may land on `develop`
  - `Phase Z` may begin

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

### Z.2 Fix And Revalidate

Purpose:

- close smoke findings from `Z.1`
- re-run full executable validation on the fixed branch

### Z.3 `atm-dev` Canary / Dogfood

Purpose:

- move from single-operator smoke to `atm-dev` team use on the new binaries
- verify UX, recovery text, and operational behavior under real use

### Z.4 Final Fixes And Release Sign-Off

Purpose:

- close `Z.3` findings
- produce the final release-readiness verdict

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
- `docs/phase-Z/sprint-Z1.md`
- `docs/phase-Z/sprint-Z2.md`
- `docs/phase-Z/sprint-Z3.md`
- `docs/phase-Z/sprint-Z4.md`
