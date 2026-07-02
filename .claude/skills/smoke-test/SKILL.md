---
name: smoke-test
description: Run or inspect the Phase Z smoke harness for atm-core. Use when implementing or operating `just smoke`, `just smoke fast`, or `just smoke thorough`, when rendering `reports/smoke/*`, when checking the frozen Phase Z row map, or when triaging smoke findings against retained logs and report artifacts.
---

# Smoke Test Skill

Use this skill for the repo-native smoke harness and its report artifacts.

## Start Here

- Read `references/level-matrix.md` to select `fast`, `normal`, or `thorough`.
- Read `references/phase-z-row-map.md` when you need the frozen row IDs and
  level-to-row coverage expectations.
- Read `references/report-schema.md` when you need the canonical JSON contract
  or the tracked-latest vs timestamped artifact rules.

## Implementation Surface

- runner:
  - `scripts/smoke/run.py`
- retained-log analysis:
  - `scripts/smoke/analyze_logs.py`
- fixture and artifact path helpers:
  - `scripts/smoke/fixtures.py`
- markdown rendering:
  - `scripts/smoke/render_report.py`

## Artifact Rules

- tracked latest smoke reports:
  - `reports/smoke/smoke-fast.md`
  - `reports/smoke/smoke.md`
  - `reports/smoke/smoke-thorough.md`
- timestamped smoke markdown and JSON artifacts use the shared
  `YYYY-MM-DD-HH-MM-SS-*` convention
- timestamped artifacts are local/transient; tracked latest reports are the
  committed human-facing snapshots

## Notes

- `fast` is the quick clean-room happy-path lane
- `normal` is the default `just smoke` lane
- `thorough` is the full CLI plus common-error-path lane
- `thorough` also includes one real same-host `atm-graft` advisory plus unary
  ICD lane
- shared fixture labels are sprint-scoped on purpose:
  - `fast` uses `z19-team`
  - `normal` uses `z20-team`
- current daemon shutdown handling in the smoke runner is POSIX-only; fail
  closed on non-POSIX hosts instead of pretending Windows support exists
- major findings promote into `docs/plans/phase-Z/smoke-findings-review.md`
