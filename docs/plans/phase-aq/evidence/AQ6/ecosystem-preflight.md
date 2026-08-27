# AQ6 ecosystem preflight evidence

## Current-release dry run

Command executed on 2026-08-27 from `feature/aq-6-ecosystem-preflight`:

```text
$ python3 scripts/validate_release.py ecosystem-preflight --dry-run \
    --findings /tmp/aq6-ecosystem-preflight-findings.json
wrote findings: /tmp/aq6-ecosystem-preflight-findings.json
release validation blockers:
- [sc-ecosystem-preflight] Wyvern is required on PATH for ecosystem preflight
exit=1
```

The registry lookups passed for the recorded current pins:

| dependency | recorded exact pin | latest lookup |
| --- | --- | --- |
| `sc-composer` | `=1.5.0` | `1.5.0` |
| `sc-observability` | `=1.2.0` | `1.2.0` |
| `sc-observability-types` | `=1.2.0` | `1.2.0` |
| Wyvern | `0.5.0` | `v0.5.0` |

The dry run intentionally failed because this workstation does not have the
optional `wyvern` executable on `PATH`. This is the required blocking,
actionable result (`install wyvern before running preflight`), while AQ5's
runtime and test lanes remain runnable without Wyvern.

## Upstream contract issue

The detailed Wyvern CI contract-test request is filed at:

<https://github.com/randlee/wyvern/issues/139>

The issue specifies the version-1 `PickerInput` and `PickerOutput` schemas,
schema-version rejection semantics, one-object stdin/stdout discipline,
stderr-only diagnostics, cancel behavior, bounded parseable `--version`,
approximately one-second cold start, and the shared fixture corpus.

## Fix-forward record

No upstream regression was observed in the current-release dry run. If a
future latest Wyvern or sc-ecosystem release regresses an integration target,
pin back to the last known-good exact version, reuse
`maybe_file_dep_currency_issue` with `ATMD_GH_AUTOFIX_ISSUES=1`, and append the
regression, pinned-back version, and returned issue URL here before release
continues.
