---
name: smoke-test
description: Run or inspect the repo-native smoke harness for atm-core. Use when implementing or operating `just smoke`, including live `localhost` and `local-ip` hardware checks; when rendering or inspecting `site/reports/smoke/*`; or when triaging smoke findings against retained evidence.
---

# Smoke Test Skill

Use this skill for the repo-native smoke harness and its report artifacts.

## Start Here

- Read `references/level-matrix.md` to select `fast`, `normal`, or `thorough`.
- Read `references/phase-z-row-map.md` when you need the frozen row IDs and
  level-to-row coverage expectations.
- Read `references/report-schema.md` when you need the canonical evidence
  layout and JSON contract.

## Live daemon prerequisite

Before any live `just smoke localhost`, `local-ip`, `peer-preflight`,
`crosshost-send`, or `crosshost-ack` run, use the
[`/daemon-switch`](../daemon-switch/SKILL.md) skill. It selects the matching
CLI and daemon as one pair, restarts only the one managed daemon, and requires
`atm doctor --json` to report the selected pair ready. Do not manually start a
daemon, point a service at a worktree binary, or run smoke with a split
CLI/daemon pair.

## Implementation Surface

- runner:
  - `scripts/smoke/run.py`
  - `scripts/smoke/run_feature_smoke.py`
- retained-log analysis:
  - `scripts/smoke/analyze_logs.py`
- fixture and artifact path helpers:
  - `scripts/smoke/fixtures.py`
- markdown rendering:
  - `scripts/smoke/render_report.py`

## Artifact Rules

- Live hardware smoke uses the fuzz-style, self-contained report layout:
  `site/reports/smoke/<platform>/<host>/<run-id>-pid<PID>-<feature>/`.
- The directory contains the JSON result, per-host XHTML panels, feature HTML,
  its own `index.html`, and a `smoke.envelope.json` registration record. The
  generated master navigation at `site/reports/index.html` links every run;
  no smoke payload or panel is written to the site root or top-level
  `site/reports` directory.
- `platform` is the operating-system label (`macos`, `windows`, or `linux` as
  reported by the runner), `host` is the sanitized local hostname, and
  `run-id` is `ATM_SMOKE_RUN_ID` when supplied or a UTC microsecond timestamp.
  The PID suffix preserves non-overlap even for simultaneous same-host runs.
- Report consumers must use the path printed by `just smoke`; never infer a
  shared “latest” artifact.
- Fixture levels (`fast`, `normal`, `thorough`) use the same platform/host/run
  directory layout and retain their Markdown and JSON inside that directory.

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
