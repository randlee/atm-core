---
name: smoke-test
description: Run or inspect the Phase Z smoke harness for atm-core. Use when implementing or operating `just smoke`, `just smoke fast`, or `just smoke thorough`, when rendering `reports/smoke/*`, when checking the frozen Phase Z row map, or when triaging smoke findings against retained logs and report artifacts.
---

# Smoke Test Skill

Use this skill for the repo-native smoke harness and its report artifacts.
The sole operator entry point is `just smoke`; do not invoke a module under
`scripts/smoke/` directly.

## Start Here

- Select the feature through `just smoke [feature]`; use `just smoke` for the
  normal lane, `just smoke fast`, or `just smoke thorough`.
- Read `references/level-matrix.md` to select `fast`, `normal`, or `thorough`.
- Read `references/phase-z-row-map.md` when you need the frozen row IDs and
  level-to-row coverage expectations.
- Read `references/report-schema.md` when you need the canonical JSON contract
  or the tracked-latest vs timestamped artifact rules.

## Command Surface

```bash
just smoke
just smoke fast
just smoke thorough
just smoke localhost
just smoke local-ip
just smoke peer-preflight <host...>
just smoke crosshost-curl-plain <host...>
just smoke crosshost-send <host...>
just smoke crosshost-ack <host...>
just smoke peer-pair <args...>
just smoke inbound-peer <args...>
just smoke inbound-peer-combine <args...>
just smoke graft-hermes <args...>
```

Use `just benchmark` only for the separate performance gate. See
[`docs/smoke-testing.md`](../../../docs/smoke-testing.md) for the stable map
from Just features to internal implementation modules.

## Internal Implementation Surface

- fixture runner: `scripts/smoke/run.py`
- feature dispatcher: `scripts/smoke/run_feature_smoke.py`
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
