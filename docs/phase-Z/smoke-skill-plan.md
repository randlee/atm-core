---
id: phase-z-smoke-skill-plan
title: Phase Z Smoke Skill And just smoke Plan
status: planned
target: develop
---

# Phase Z Smoke Skill And `just smoke` Plan

## Goal

Define a repeatable executable smoke harness for `Phase Z` that produces the
comprehensive row-by-row results the rollout line needs, instead of ad hoc
status notes.

The planned end state is:

- one reusable smoke-test skill under `.claude/skills/smoke-test/`
- one repo-native `just smoke` entrypoint with level-specific variants
- one machine-readable report format plus human-readable summary
- explicit integration with the frozen `Phase Z` smoke and canary checklists
- explicit sprint ownership for landing the work in `Z.18` through `Z.20`

## Scope Summary

This planning document covers only the smoke automation/reporting line that
follows `Z.17` and must land before final release sign-off in `Z.4`.

It does not change the accepted `Z.1`, `Z.2`, `Z.16`, or `Z.17` results.
Instead, it defines how future smoke runs must be executed and reported so the
operator, QA, and release gate can see the full test matrix and exact row
results.

## Hard Dependencies

- `docs/plan-phase-Z.md`
- `docs/project-plan.md`
- `docs/phase-Z/smoke-checklist.md`
- `docs/phase-Z/canary-dogfood-checklist.md`
- `docs/phase-Z/readiness.md`
- accepted `Z.16` and `Z.17` smoke rerun expectations on the execution line

## Smoke Levels

### `fast`

Purpose:

- prove binaries build
- prove daemon can start and stop cleanly on a disposable baseline

Expected runtime:

- approximately 30 seconds

Minimum coverage:

- release build for the smoke binaries
- daemon bring-up
- daemon readiness check
- daemon shutdown

Checklist alignment:

- partial automation support for the startup/build subset of
  `docs/phase-Z/smoke-checklist.md`

### `normal`

Purpose:

- provide the default operator smoke lane
- validate the core executable flows that should work on every normal sprint
  rerun

Expected runtime:

- approximately 2 to 3 minutes

Minimum coverage:

- everything in `fast`
- disposable-team setup
- `doctor`
- core retained team/member inspection
- core `send` / `read` / `ack` flow
- summary of any skipped rows that require complete-level fixtures

Checklist alignment:

- covers the normal operator-facing subset of `docs/phase-Z/smoke-checklist.md`
- becomes the default `just smoke` target

### `complete`

Purpose:

- run the full operator smoke matrix against the accepted baseline under test
- produce row-by-row evidence for the whole frozen smoke checklist

Expected runtime:

- approximately 5 to 10 minutes

Minimum coverage:

- everything in `normal`
- every operator flow from `docs/phase-Z/smoke-checklist.md`
- copied-state fixture lane when the checklist requires it
- explicit PASS / FAIL / SKIP row verdict for every checklist row

Checklist alignment:

- authoritative automated companion to the frozen smoke checklist
- must map every report row to the checklist row identifier

## Required Row Coverage Map

The smoke runner must report these frozen `Phase Z` smoke rows explicitly:

| Row | Flow Summary | `fast` | `normal` | `complete` |
| --- | --- | --- | --- | --- |
| `Z1-001` | build approved smoke baseline | yes | yes | yes |
| `Z1-002` | clean-room daemon/runtime bring-up | yes | yes | yes |
| `Z1-003` | retained team/member inspection on clean-room baseline | no | yes | yes |
| `Z1-004` | empty-mailbox retained CLI surface | no | yes | yes |
| `Z1-005` | first clean-room send to config-defined recipient | no | yes | yes |
| `Z1-006` | degraded notification after durable send | no | no | yes |
| `Z1-007` | retained CLI validation and recovery guidance | no | yes | yes |
| `Z1-008` | copied-state durable baseline bring-up | no | no | yes |
| `Z1-009` | reconcile/runtime retry-visible smoke coverage | no | no | yes |

Rules:

- `fast` may report non-applicable rows as omitted only when those rows are not
  part of the level contract
- `normal` must report any unrun required row as `SKIP`, not silently omit it
- `complete` must never omit a frozen smoke row

## Report Contract

Every smoke run must write:

- machine-readable JSON to `.smoke-reports/<timestamp>.json`
- human-readable summary to stdout

Required JSON schema:

```json
{
  "level": "normal",
  "timestamp": "2026-05-24T12:34:56Z",
  "binary_sha": "0123456789abcdef",
  "duration_secs": 123,
  "rows": [
    {
      "id": "Z1-005",
      "flow": "Send the first message to a config-defined recipient on a clean-room baseline",
      "verdict": "PASS",
      "notes": "send/read/ack round-trip succeeded"
    }
  ],
  "summary": {
    "pass": 8,
    "fail": 0,
    "skip": 1
  }
}
```

Required behavior:

- exit `0` when every executed row passes
- exit `1` when any row fails
- keep skipped rows explicit in both the JSON and the human-readable summary
- capture the accepted binary baseline being exercised through `binary_sha`

## Human-Readable Summary Contract

The stdout summary must be concise but complete enough for ATM handoff:

- smoke level
- binary SHA
- total duration
- pass/fail/skip counts
- each failed or skipped row with its checklist ID and short reason

The summary must be good enough to paste into ATM without rereading the raw
JSON file.

## Planned Skill Layout

Planned canonical skill location:

- `.claude/skills/smoke-test/SKILL.md`

Planned skill support files:

- `.claude/skills/smoke-test/references/level-matrix.md`
- `.claude/skills/smoke-test/references/report-schema.md`
- `.claude/skills/smoke-test/references/phase-z-row-map.md`

Planned implementation support files:

- `scripts/smoke/run.py`
- `scripts/smoke/report.py`
- `scripts/smoke/fixtures.py`

The skill must tell an operator:

- when to run `just smoke-fast`
- when to run `just smoke`
- when to run `just smoke-complete`
- how to interpret `.smoke-reports/<timestamp>.json`

## Planned Just Targets

Required targets:

- `just smoke-fast`
- `just smoke`
- `just smoke-complete`

Required target semantics:

- `just smoke-fast`
  - executes the `fast` level
- `just smoke`
  - defaults to the `normal` level
- `just smoke-complete`
  - executes the `complete` level

Optional follow-on targets may be added later only if they do not obscure the
three required entrypoints.

## Checklist And Manual Integration

`just smoke` does not replace the governing `Phase Z` checklist artifacts.
Instead, it augments them.

Rules:

- `docs/phase-Z/smoke-checklist.md` remains the authoritative source for row
  IDs, row descriptions, and expected executable behavior
- `just smoke-complete` must emit one report row per checklist row
- the human-readable summary must make it obvious which checklist rows failed
  or were skipped
- `docs/phase-Z/canary-dogfood-checklist.md` remains the authoritative source
  for canary/dogfood execution, but the smoke skill must define how the same
  reporting format can later be reused for canary-adjacent executable checks
- manual operator notes remain allowed, but the smoke report becomes the
  baseline evidence artifact for the automated portion of the run

## Binary Baseline Tracking

Every smoke report must record the exact binary baseline under test.

Minimum tracked fields:

- git commit SHA via `binary_sha`
- smoke level
- report timestamp

Planned follow-on behavior:

- `Z.20` will define how the accepted binary SHA is tied back to
  `docs/phase-Z/readiness.md`, the smoke checklist, and canary entry records

## Sprint Sequence

### Z.18 Smoke Skill Scaffold And Fast/Normal Runner

Purpose:

- land the smoke skill scaffold
- land report writing and summary output
- land `just smoke-fast` and default `just smoke`

Execution branch:

- `feature/pZ-s18-smoke-skill-and-fast-normal-runner`

Execution worktree:

- `../atm-core-worktrees/feature/pZ-s18-smoke-skill-and-fast-normal-runner`

Primary deliverables:

- `.claude/skills/smoke-test/`
- `scripts/smoke/run.py`
- `Justfile` smoke targets
- `.smoke-reports/<timestamp>.json` output contract

### Z.19 Complete Smoke Checklist Automation And Reporting

Purpose:

- extend the runner to full `complete` coverage
- map every frozen smoke-checklist row to a report row
- tighten the human-readable summary for ATM/QA use

Execution branch:

- `feature/pZ-s19-complete-smoke-checklist-automation-and-reporting`

Execution worktree:

- `../atm-core-worktrees/feature/pZ-s19-complete-smoke-checklist-automation-and-reporting`

Primary deliverables:

- full checklist row map
- `just smoke-complete`
- complete-level copied-state fixture coverage
- row-level PASS / FAIL / SKIP output

### Z.20 Canary Smoke Integration And Binary Baseline Tracking

Purpose:

- align smoke reporting with canary/release evidence needs
- tie binary baseline tracking back to `Phase Z` readiness artifacts
- document how automated smoke augments manual canary execution

Execution branch:

- `feature/pZ-s20-canary-smoke-integration-and-binary-baseline-tracking`

Execution worktree:

- `../atm-core-worktrees/feature/pZ-s20-canary-smoke-integration-and-binary-baseline-tracking`

Primary deliverables:

- canary-checklist integration guidance
- readiness/baseline tracking wiring
- finalized artifact list and operator usage guidance

### Optional Z.21 Cross-Platform Smoke Promotion

This sprint is optional and should be created only if `Z.18` through `Z.20`
cannot land cross-platform runtime stability, fixture portability, or CI smoke
promotion cleanly without overloading one of those sprints.

If needed, `Z.21` should own only:

- platform-specific fixture/runtime stabilization
- CI smoke promotion
- no new smoke-report schema churn

## Artifact List

Planned new files:

- `docs/phase-Z/smoke-skill-plan.md`
- `docs/phase-Z/sprint-Z18.md`
- `docs/phase-Z/sprint-Z19.md`
- `docs/phase-Z/sprint-Z20.md`
- `.claude/skills/smoke-test/SKILL.md`
- `.claude/skills/smoke-test/references/level-matrix.md`
- `.claude/skills/smoke-test/references/report-schema.md`
- `.claude/skills/smoke-test/references/phase-z-row-map.md`
- `scripts/smoke/run.py`
- `scripts/smoke/report.py`
- `scripts/smoke/fixtures.py`

Planned Just targets:

- `smoke-fast`
- `smoke`
- `smoke-complete`

Planned output location:

- `.smoke-reports/`

## Acceptance Criteria

- the plan defines `fast`, `normal`, and `complete` smoke levels with expected
  runtime and coverage
- the plan defines the exact JSON report schema and human-readable summary
  contract
- the plan defines the required `just smoke-fast`, `just smoke`, and
  `just smoke-complete` targets
- the plan defines `.smoke-reports/<timestamp>.json` as the required report
  path
- the plan defines exit-code behavior for pass/fail results
- the plan defines the `Z.18` through `Z.20` sprint sequence and optional
  `Z.21` split rule
- the plan defines integration points with both
  `docs/phase-Z/smoke-checklist.md` and
  `docs/phase-Z/canary-dogfood-checklist.md`
- the plan defines binary baseline tracking expectations

## Risks And Watchouts

- do not let smoke automation silently replace the checklist artifacts
- keep the report row IDs aligned to the checklist IDs from the start
- keep the default `just smoke` target useful enough for normal operator use,
  not only for exhaustive release runs
- keep skipped rows explicit; hidden partial coverage would recreate the same
  low-value smoke summaries this plan is trying to replace
