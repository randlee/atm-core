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

The release packaging follow-up for the optional archive checksum manifest is
tracked at [Wyvern #141](https://github.com/randlee/wyvern/issues/141); the
atm-core bootstrap pins the archive SHA-256 values in `tools/bootstrap.toml`
and treats `checksums.txt` as an optional cross-check.

The issue specifies the version-1 `PickerInput` and `PickerOutput` schemas,
schema-version rejection semantics, diagnostics/cancel behavior, bounded
parseable `--version`, approximately one-second cold start, and the shared
fixture corpus. The AQ6 adapter now uses the released Wyvern wizard contract:
it writes `PickerInput` under the generated wizard JSON's `config`, invokes
`wyvern <wizard.json> --ui-root <asset directory>`, and unwraps the returned
`WizardResult.data` as `PickerOutput`; it does not rely on the nonexistent
`--picker` flag or bare stdout shape.

## PR CI evidence

Final implementation head: `29ac4a7c58796f446f3cdd6725f265ea6db2a66a` (PR
#1066). Previous implementation head (history):
`a80d6b2ef93b957485351184c79ea056ef3e7719`. The diff between `a80d6b2ef9` and
`29ac4a7c58` is merge-forwards only, with zero change to AQ6's own files
(confirmed by quality-mgr QA-4 diff-scope check).

- [CI workflow run 33208724696](https://github.com/randlee/atm-core/actions/runs/33208724696) —
  all jobs success: Format check (`98976354412`), Clippy (`98976437399`),
  Just lint ubuntu (`98976354185`), macos (`98976354312`), windows
  (`98976354419`), Test ubuntu (`98977494098`), macos (`98977493925`),
  windows (`98977493977`).
- [Phase AQ evidence workflow run 33208724703](https://github.com/randlee/atm-core/actions/runs/33208724703) —
  all jobs success: ubuntu (`98976354582`), macos (`98976354619`), windows
  (`98976354387`).

These run IDs are the authoritative evidence for PR #1066's final pushed
head; lane completion status is tracked by the linked workflows.

## Fix-forward record

No upstream regression was observed in the current-release dry run. If a
future latest Wyvern or sc-ecosystem release regresses an integration target,
pin back to the last known-good exact version, reuse
`maybe_file_dep_currency_issue` with `ATMD_GH_AUTOFIX_ISSUES=1`, and append the
regression, pinned-back version, and returned issue URL here before release
continues.

## CI evidence for final head

- head: `29ac4a7c58796f446f3cdd6725f265ea6db2a66a` (PR #1066)
- previous implementation head (history): `a80d6b2ef93b957485351184c79ea056ef3e7719`
- CI run: [33208724696](https://github.com/randlee/atm-core/actions/runs/33208724696) — all jobs
  success: Format check (`98976354412`), Clippy (`98976437399`), Just lint
  ubuntu (`98976354185`) / macos (`98976354312`) / windows (`98976354419`),
  Test ubuntu (`98977494098`) / macos (`98977493925`) / windows
  (`98977493977`)
- Phase AQ Evidence run: [33208724703](https://github.com/randlee/atm-core/actions/runs/33208724703) —
  all jobs success: ubuntu (`98976354582`), macos (`98976354619`), windows
  (`98976354387`)
