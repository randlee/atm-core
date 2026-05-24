---
id: phase-z-smoke-skill-plan
title: Phase Z Smoke Skill And just smoke Plan
status: complete
target: develop
---

# Phase Z Smoke Skill And `just smoke` Plan

## Goal

Define a repeatable executable smoke harness for `Phase Z` that produces
comprehensive row-by-row results, clear root-cause notes for deviations, and
tracked report artifacts that are useful to operators, QA, and the release
gate.

The planned end state is:

- one reusable smoke-test skill under `.claude/skills/smoke-test/`
- one repo-native `just smoke` entrypoint with subcommands
- one canonical JSON payload plus rendered markdown smoke reports
- explicit integration with the frozen `Phase Z` smoke and canary artifacts
- explicit sprint ownership for landing the smoke and coverage work in `Z.18`
  through `Z.23`

## Scope Summary

This planning document covers the smoke automation/reporting line that follows
`Z.17` and must land before final `Z.4` release sign-off is considered
complete.

It does not change the accepted `Z.1`, `Z.2`, `Z.16`, or `Z.17` results.
Instead, it defines how future smoke runs must be executed, reported, and
triaged so the operator, QA, and release gate can see:

- what ran
- what did not run
- what passed
- what failed
- what was skipped
- how thorough the run was
- the likely root cause for each deviation

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

- prove the product works on a clean-room happy-path baseline
- give an agent a fast end-to-end "everything basically works" run
- verify retained logs contain the expected events and no warnings/errors

Expected runtime:

- approximately 30 seconds

Minimum coverage:

- release build for the smoke binaries
- clean new disposable DB / runtime state
- daemon bring-up
- clean-room team creation/setup
- successful `doctor`
- successful `atm send` without `--requires-ack`
- successful `atm send` with `--requires-ack`
- successful `atm read`
- successful `atm ack`
- successful nudge-visible flow
- successful daemon shutdown
- script-based retained-log analysis proving:
  - expected lifecycle/send/read/ack/nudge events were logged
  - no warnings
  - no errors

Checklist alignment:

- fast happy-path subset of `docs/phase-Z/smoke-checklist.md`
- additional smoke-runner log-analysis checks that summarize whether the
  retained event trail matches the happy-path contract

### `normal`

Purpose:

- provide the default operator smoke lane
- extend the fast happy path into the broader retained/operator surface
- validate important validation/error-path behavior without paying the full
  thorough-level cost
- include root-cause notes for every deviation from expected behavior

Expected runtime:

- approximately 2 to 3 minutes

Minimum coverage:

- everything in `fast`
- retained team/member inspection
- retained mailbox/log surface beyond the fast happy-path minimum
- broader retained/admin/operator flows that matter in routine use
- at least one important validation/error-path check
- summary of any skipped rows that require copied-state or degraded/recovery
  fixtures

Checklist alignment:

- covers the normal operator-facing subset of `docs/phase-Z/smoke-checklist.md`
- becomes the default `just smoke` target

### `thorough`

Purpose:

- run the full operator smoke matrix against the accepted baseline under test
- produce row-by-row evidence for the whole frozen smoke checklist
- root-cause every discrepancy from expected behavior

Expected runtime:

- approximately 5 to 10 minutes

Minimum coverage:

- everything in `normal`
- every operator flow from `docs/phase-Z/smoke-checklist.md`
- every CLI interface on happy path plus common error paths
- copied-state fixture lane when the checklist requires it
- explicit PASS / FAIL / SKIP row verdict for every checklist row

Checklist alignment:

- authoritative automated companion to the frozen smoke checklist
- must map every report row to the checklist row identifier

## Required Row Coverage Map

The smoke runner must report these frozen `Phase Z` smoke rows explicitly:

| Row | Flow Summary | `fast` | `normal` | `thorough` |
| --- | --- | --- | --- | --- |
| `Z1-001` | build approved smoke baseline | yes | yes | yes |
| `Z1-002` | clean-room daemon/runtime bring-up | yes | yes | yes |
| `Z1-003` | retained team/member inspection on clean-room baseline | yes | yes | yes |
| `Z1-004` | empty-mailbox retained CLI surface | yes | yes | yes |
| `Z1-005` | first clean-room send to config-defined recipient | yes | yes | yes |
| `Z1-006` | degraded notification after durable send | no | no | yes |
| `Z1-007` | retained CLI validation and recovery guidance | no | yes | yes |
| `Z1-008` | copied-state durable baseline bring-up | no | no | yes |
| `Z1-009` | reconcile/runtime retry-visible smoke coverage | no | no | yes |

Rules:

- `fast` may omit rows that are outside the level contract
- `normal` must report any unrun required row as `SKIP`, not silently omit it
- `thorough` must never omit a frozen smoke row

Additional required smoke-runner checks outside the frozen row IDs:

- `FAST-LOG-001`
  - retained logs contain the expected happy-path lifecycle/send/read/ack/nudge
    events for the run
- `FAST-LOG-002`
  - retained logs contain no warnings or errors for the run

## Report Contract

Every smoke run must write:

- machine-readable JSON to `reports/smoke/<timestamp>-smoke*.json`
- human-readable markdown reports to `reports/smoke/`
- human-readable summary to stdout

Required JSON schema:

```json
{
  "level": "normal",
  "timestamp": "2026-05-24T12:34:56Z",
  "binary_sha": "0123456789abcdef",
  "duration_secs": 123,
  "status": "scaffold-only",
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
- for every non-pass row in `normal` or `thorough`, include:
  - observed behavior
  - expected behavior
  - likely root cause
  - artifact pointer such as retained log evidence, stderr, or report field

## Human-Readable Summary Contract

The stdout summary must be concise but complete enough for ATM handoff:

- smoke level
- runner status
- binary SHA
- total duration
- pass/fail/skip counts
- each failed or skipped row with its checklist ID and short reason

The summary must be good enough to paste into ATM without rereading the raw
JSON file.

## Report Output Layout

Tracked latest snapshots:

- `reports/smoke/smoke-fast.md`
- `reports/smoke/smoke.md`
- `reports/smoke/smoke-thorough.md`

Timestamped markdown artifacts:

- `reports/smoke/YYYY-MM-DD-HH-MM-SS-smoke-fast.md`
- `reports/smoke/YYYY-MM-DD-HH-MM-SS-smoke.md`
- `reports/smoke/YYYY-MM-DD-HH-MM-SS-smoke-thorough.md`

Timestamped JSON source artifacts:

- `reports/smoke/YYYY-MM-DD-HH-MM-SS-smoke-fast.json`
- `reports/smoke/YYYY-MM-DD-HH-MM-SS-smoke.json`
- `reports/smoke/YYYY-MM-DD-HH-MM-SS-smoke-thorough.json`

Git policy:

- the latest snapshot markdown files are tracked in git
- the timestamped markdown artifacts are gitignored
- the timestamped JSON artifacts are gitignored unless a later sprint promotes
  them

Render behavior:

- each run overwrites the matching latest snapshot report
- each run writes one timestamped markdown artifact
- each run may write one timestamped JSON source artifact
- stdout prints the same summary content rendered into the matching latest
  snapshot report

## Planned Skill Layout

Planned canonical skill location:

- `.claude/skills/smoke-test/SKILL.md`

Planned skill support files:

- `.claude/skills/smoke-test/references/level-matrix.md`
- `.claude/skills/smoke-test/references/report-schema.md`
- `.claude/skills/smoke-test/references/phase-z-row-map.md`

Planned implementation support files:

- `scripts/smoke/run.py`
- `scripts/smoke/render_report.py`
- `scripts/smoke/fixtures.py`
- `scripts/smoke/analyze_logs.py`

Planned report templates:

- `templates/smoke-report/smoke-fast.md.j2`
- `templates/smoke-report/smoke.md.j2`
- `templates/smoke-report/smoke-thorough.md.j2`

The skill must tell an operator:

- when to run `just smoke`
- when to run `just smoke fast`
- when to run `just smoke thorough`
- how to interpret `reports/smoke/`

## Planned Just Targets

Required interface:

- `just smoke`
- `just smoke fast`
- `just smoke thorough`

Required command semantics:

- `just smoke`
  - defaults to the `normal` level
- `just smoke fast`
  - executes the `fast` level
- `just smoke thorough`
  - executes the `thorough` level

Optional follow-on targets may be added later only if they do not obscure the
single `just smoke` entrypoint.

## Checklist And Manual Integration

`just smoke` does not replace the governing `Phase Z` checklist artifacts.
Instead, it augments them.

Rules:

- `docs/phase-Z/smoke-checklist.md` remains the authoritative source for row
  IDs, row descriptions, and expected executable behavior
- `just smoke thorough` must emit one report row per checklist row
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

- `Z.22` will define how accepted smoke findings, binary-baseline notes, and
  canary/release evidence link back to `docs/phase-Z/readiness.md`

## Smoke Findings Handling

Rules:

- small localized issues that block predictable smoke execution should be fixed
  inside the active smoke sprint
- missing log messages should be added inside the active smoke sprint at the
  appropriate log level when that is a local, low-risk change
- minor requirement or architecture violations encountered during smoke work
  may be fixed inside the active smoke sprint when the remediation is local and
  does not widen scope materially
- larger requirement or architecture violations must be promoted into the
  findings-review artifact instead of silently expanding the active sprint

The durable review queue for those larger findings is:

- `docs/phase-Z/smoke-findings-review.md`

Logging expectations:

- `fast` smoke may enable verbose/debug logging and require detailed
  send/read/ack/nudge/lifecycle event visibility
- normal production runtime logging should remain quiet at routine verbosity
  and should not log every send/read/ack event at ordinary operator levels

## Sprint Sequence

### Z.18 Smoke Skill Scaffold And Report Infrastructure

Purpose:

- land the smoke skill scaffold
- land template rendering, report writing, summary output, and artifact layout
- land the shared smoke runner infrastructure that later smoke entrypoints use

Execution branch:

- `feature/pZ-s18-smoke-skill-and-report-infrastructure`

Execution worktree:

- `../atm-core-worktrees/feature/pZ-s18-smoke-skill-and-report-infrastructure`

Primary deliverables:

- `.claude/skills/smoke-test/`
- `scripts/smoke/run.py`
- `scripts/smoke/analyze_logs.py`
- `scripts/smoke/render_report.py`
- `scripts/smoke/fixtures.py`
- `templates/smoke-report/smoke-fast.md.j2`
- `templates/smoke-report/smoke.md.j2`
- `templates/smoke-report/smoke-thorough.md.j2`
- `reports/smoke/` output contract
- `.gitignore` report-ignore rules
- tracked-latest scaffold snapshots in `reports/smoke/` that later smoke
  execution sprints overwrite with real run results

### Z.19 Fast Smoke Happy-Path Execution

Purpose:

- implement `just smoke fast`
- prove the clean-room happy path quickly and reliably
- fix minor smoke-blocking issues in-sprint when they are small and localized

Execution branch:

- `feature/pZ-s19-fast-smoke-happy-path-execution`

Execution worktree:

- `../atm-core-worktrees/feature/pZ-s19-fast-smoke-happy-path-execution`

Primary deliverables:

- `just smoke fast`
- minimal clean-room team shell plus `atm teams add-member` setup contract
- `reports/smoke/smoke-fast.md`
- timestamped fast smoke artifacts
- deterministic fast log-analysis gate
- minor in-sprint fixes needed for fast predictability

### Z.20 Normal Smoke Systemic Execution

Purpose:

- implement the default `just smoke` run
- exercise most important feature/system behavior beyond the fast happy path
- root-cause every deviation from expected behavior

Execution branch:

- `feature/pZ-s20-normal-smoke-systemic-execution`

Execution worktree:

- `../atm-core-worktrees/feature/pZ-s20-normal-smoke-systemic-execution`

Primary deliverables:

- `just smoke`
- `reports/smoke/smoke.md`
- timestamped normal smoke artifacts
- root-cause reporting for every deviation
- minor in-sprint fixes needed for normal predictability

### Z.21 Thorough Smoke CLI Coverage And Reporting

Purpose:

- implement `just smoke thorough`
- cover every CLI interface on happy path plus common error paths
- root-cause discrepancies from expected behavior

Execution branch:

- `feature/pZ-s21-thorough-smoke-cli-coverage-and-reporting`

Execution worktree:

- `../atm-core-worktrees/feature/pZ-s21-thorough-smoke-cli-coverage-and-reporting`

Primary deliverables:

- `just smoke thorough`
- `reports/smoke/smoke-thorough.md`
- timestamped thorough smoke artifacts
- row-level PASS / FAIL / SKIP output
- root-cause notes for every deviation from expected behavior

### Z.22 Smoke Findings Review And Major Rework Triage

Purpose:

- provide the durable place to record smoke findings that are too large to fix
  inside active smoke sprints
- separate minor in-sprint fixes from significant rework
- connect smoke findings to canary/release evidence and accepted binary notes

Execution branch:

- `feature/pZ-s22-smoke-findings-review-and-major-rework-triage`

Execution worktree:

- `../atm-core-worktrees/feature/pZ-s22-smoke-findings-review-and-major-rework-triage`

Primary deliverables:

- `docs/phase-Z/smoke-findings-review.md`
- major smoke finding triage contract
- accepted binary-baseline linkage notes

### Z.23 Cross-Platform Test Coverage Reporting

Purpose:

- add explicit coverage-report generation as a separate command surface
- keep coverage reporting out of ordinary `just test`
- persist tracked latest and timestamped cross-platform coverage reports under
  `reports/coverage/`

Execution branch:

- `feature/pZ-s23-cross-platform-test-coverage-reporting`

Execution worktree:

- `../atm-core-worktrees/feature/pZ-s23-cross-platform-test-coverage-reporting`

Primary deliverables:

- `just test coverage`
- `reports/coverage/mac.md`
- `reports/coverage/win.md`
- timestamped coverage artifacts

## Artifact List

Planned new files:

- `docs/phase-Z/smoke-skill-plan.md`
- `docs/phase-Z/sprint-Z18.md`
- `docs/phase-Z/sprint-Z19.md`
- `docs/phase-Z/sprint-Z20.md`
- `docs/phase-Z/sprint-Z21.md`
- `docs/phase-Z/sprint-Z22.md`
- `docs/phase-Z/sprint-Z23.md`
- `docs/phase-Z/smoke-findings-review.md`
- `.claude/skills/smoke-test/SKILL.md`
- `.claude/skills/smoke-test/references/level-matrix.md`
- `.claude/skills/smoke-test/references/report-schema.md`
- `.claude/skills/smoke-test/references/phase-z-row-map.md`
- `scripts/smoke/run.py`
- `scripts/smoke/render_report.py`
- `scripts/smoke/fixtures.py`
- `scripts/smoke/analyze_logs.py`
- `scripts/coverage/run.py`
- `scripts/coverage/render_report.py`
- `templates/smoke-report/smoke-fast.md.j2`
- `templates/smoke-report/smoke.md.j2`
- `templates/smoke-report/smoke-thorough.md.j2`
- `templates/coverage-report/mac.md.j2`
- `templates/coverage-report/win.md.j2`
- `docs/testing-guidelines.md`

Planned Just interface:

- `smoke`
- `smoke fast`
- `smoke thorough`
- `test coverage`

Planned output locations:

- `reports/smoke/`
- `reports/coverage/`

## Acceptance Criteria

- the plan defines `fast`, `normal`, and `thorough` smoke levels with expected
  runtime and coverage
- the plan defines the exact JSON payload contract and rendered markdown report
  contract
- the plan defines the required `just smoke`, `just smoke fast`, and
  `just smoke thorough` interface
- the plan defines `reports/smoke/` as the required report destination with
  tracked latest snapshots plus gitignored timestamped artifacts
- the plan defines `just test coverage` as a separate interface and
  `reports/coverage/` as its report destination
- the plan defines exit-code behavior for pass/fail results
- the plan defines the `Z.18` through `Z.23` sprint sequence
- the plan defines the rule that minor smoke-blocking issues are fixed inside
  the active smoke sprint and larger rework findings are promoted to the
  findings review artifact
- the plan defines integration points with both
  `docs/phase-Z/smoke-checklist.md` and
  `docs/phase-Z/canary-dogfood-checklist.md`
- the plan defines binary baseline tracking expectations

## Risks And Watchouts

- do not let smoke automation silently replace the checklist artifacts
- keep the report row IDs aligned to the checklist IDs from the start
- keep the default `just smoke` target useful enough for normal operator use
- keep skipped rows explicit; hidden partial coverage would recreate the same
  low-value smoke summaries this plan is trying to replace
- keep `thorough` broad, but do not label it `exhaustive`; it should not claim
  impossible total-path coverage
