---
id: Z.23
title: Cross-Platform Test Coverage Reporting
status: planned
branch: feature/pZ-s23-cross-platform-test-coverage-reporting
worktree: ../atm-core-worktrees/feature/pZ-s23-cross-platform-test-coverage-reporting
target: integrate/phase-Z
---

# Sprint Z.23 — Cross-Platform Test Coverage Reporting

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.23
worktree: ../atm-core-worktrees/feature/pZ-s23-cross-platform-test-coverage-reporting
branch: feature/pZ-s23-cross-platform-test-coverage-reporting
status: planned
estimated_scope: medium
```

## Goal

- add explicit coverage-report generation as a separate command surface
- keep coverage reporting out of ordinary `just test`
- persist tracked-latest and timestamped cross-platform coverage reports under
  `reports/coverage/`

## Scope Summary

This sprint adds local coverage reporting without changing the ordinary test
contract. It must not make routine `just test` slower or noisier by silently
collecting coverage.

## Governing Requirements

- `REQ-P-COVERAGE-001`

## Governing ADRs

- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`

## Governing Boundaries

- `RuntimeFactory`

## Prerequisites

- `Z.22` complete

## Hard Dependencies

- `docs/phase-Z/smoke-skill-plan.md`
- `docs/plan-phase-Z.md`
- `docs/project-plan.md`

## Exact Targets

- `Justfile`
- `templates/coverage-report/mac.md.j2`
- `templates/coverage-report/win.md.j2`
- `scripts/coverage/run.py`
- `scripts/coverage/render_report.py`
- `reports/coverage/`
- `.gitignore`

## Deliverables

Every listed deliverable is expected to land at a production-ready level for
the scope this sprint claims. If that cannot be done cleanly in one sprint,
the sprint must be split before implementation begins. No deliverable may be
silently dropped or partially deferred.

- `just test coverage`
- tracked latest coverage reports:
  - `reports/coverage/mac.md`
  - `reports/coverage/win.md`
- gitignored timestamped coverage reports:
  - `reports/coverage/YYYY-MM-DD-HH-MM-SS-mac.md`
  - `reports/coverage/YYYY-MM-DD-HH-MM-SS-win.md`
- J2 coverage report templates
- shared timestamp convention compatible with smoke reporting

## Required Work

- add `just test coverage` as an explicit separate interface
- keep `just test` unchanged and free of automatic coverage collection
- choose and wire the initial coverage collector, expected to be
  `cargo llvm-cov`, behind the script layer
- render coverage reports through J2 templates
- write tracked latest platform reports under `reports/coverage/`
- write timestamped platform reports using the same run timestamp convention as
  smoke reports
- define how a shared run timestamp is reused when smoke and coverage are
  generated as part of the same reporting campaign

## Explicit Code Samples

```text
just test coverage
```

```json
{
  "platform": "mac",
  "coverage_level": "local-explicit",
  "timestamp": "2026-05-24T12:34:56Z",
  "commit": "0123456789abcdef",
  "duration_secs": 245,
  "collector": "cargo llvm-cov",
  "summary": {
    "line_percent": 82.4,
    "function_percent": 79.1
  },
  "crates": [
    {
      "name": "atm-core",
      "line_percent": 84.2,
      "function_percent": 81.0
    },
    {
      "name": "atm-daemon",
      "line_percent": 78.6,
      "function_percent": 75.4
    }
  ]
}
```

## This Sprint Does Not Close

- adding coverage collection to ordinary `just test`
- automatic coupling between smoke runs and coverage runs
- CI publishing or upload workflows unless explicitly widened later

## Acceptance Criteria

- `just test coverage` exists and is not run automatically by `just test`
- the coverage run emits the canonical JSON payload with platform,
  coverage-level, timestamp, commit, duration, and per-crate coverage fields
- coverage reports render through J2 templates
- `reports/coverage/mac.md` and `reports/coverage/win.md` are the tracked
  latest platform reports
- timestamped platform reports use the same timestamp convention as smoke
  reports
- the report layout and generation flow are documented clearly enough for
  future CI promotion

## Required Validation

- `cargo test --workspace`
- `python3 .just/run_lint.py all`
- `git diff --check`

## Split Recommendation

If CI upload/publishing or per-platform collector stabilization becomes large
enough to obscure the basic local reporting contract, split that into a later
follow-on sprint.

## Production-Ready Expectation

Every listed `Z.23` deliverable is expected to land at a production-ready
level for explicit local coverage-report generation and persisted report
artifacts.

## Required Document Updates

- `docs/requirements.md`
- `docs/architecture.md`
- `docs/testing-guidelines.md`
- `docs/phase-Z/smoke-skill-plan.md`
- `docs/plan-phase-Z.md`
- `docs/project-plan.md`
- `.gitignore`

## Risks And Watchouts

- do not let coverage collection silently slow ordinary `just test`
- keep platform report naming deterministic
- keep the shared timestamp model explicit so smoke and coverage artifacts are
  easy to correlate later
