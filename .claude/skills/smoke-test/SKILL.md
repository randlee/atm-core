---
name: smoke-test
description: Run or inspect the Phase Z smoke harness for atm-core. Use when implementing or operating `just smoke`, `just smoke fast`, or `just smoke thorough`, when rendering `site/reports/smoke*`, when checking the frozen Phase Z row map, or when triaging smoke findings against retained logs and report artifacts.
---

# Smoke Test Skill

Use this skill for the repo-native smoke harness and its report artifacts.

## Start Here

- Read `references/level-matrix.md` to select `fast`, `normal`, or `thorough`.
- Read `references/phase-z-row-map.md` when you need the frozen row IDs and
  level-to-row coverage expectations.
- Read `references/report-schema.md` when you need the canonical JSON contract
  or the site-published, timestamp- and host-labeled artifact naming rules.

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

- smoke reports publish to `site/reports/smoke-<level>/`, matching the
  `site/reports/send-message-benchmark/` benchmark-harness pattern: a flat
  directory of `<timestamp>-<host_label>-<slug>.{md,json,envelope.json}`
  files, with no fixed "latest" filename
- every run is timestamp- and host-labeled (`platform.node()`, sanitized the
  same way as the benchmark harness), so concurrent runs from different
  machines never collide or overwrite each other
- each level also gets a root-level discovery page
  (`site/reports/smoke-fast.html`, `site/reports/smoke.html`,
  `site/reports/smoke-thorough.html`) that `just reports-index` wires into
  `site/reports/index.html`

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
