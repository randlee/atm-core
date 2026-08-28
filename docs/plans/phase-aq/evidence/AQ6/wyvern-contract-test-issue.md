# Wyvern CI contract-test request

## Request

Please add a Wyvern CI contract test for the ATM Send-To picker. The test
should execute the released picker with the shared fixtures and fail on any
wire-contract drift. This issue deliberately contains the complete contract
so the Wyvern implementation does not need to depend on atm-core source.

## `PickerInput` (schema version 1)

The picker receives exactly one JSON object on stdin:

```json
{"schema_version":1,"teams":[{"id":"…","name":"…","members":[{"id":"…","name":"…","host":"…","cwd":"…","status":"active|idle|dead"}]}]}
```

`schema_version` is the compatibility gate. The page must explicitly declare
the version it implements and reject an unknown input version rather than
guessing at field meanings. Additive contract evolution increments the
version; it must not silently widen version 1.

## `PickerOutput` (schema version 1)

On confirmation, the picker writes exactly one JSON object to stdout:

```json
{"schema_version":1,"recipients":["member-id","…"],"note":"optional one-liner"}
```

Version 1 has exactly those three keys. `recipients` is a non-empty
multi-select list of member IDs and `note` is optional free text. The shared
fixture corpus in atm-core is:

- `docs/plans/phase-aq/fixtures/picker-input-v1.json`
- `docs/plans/phase-aq/fixtures/picker-output-v1.json`
- `docs/plans/phase-aq/fixtures/picker-output-unknown-schema.json`

The CI test should consume the same bytes (or a synchronized copy checked by
hash) rather than maintaining a subtly different schema fixture.

## Process contract

- stdin contains one `PickerInput` object and nothing else;
- stdout contains one `PickerOutput` object and nothing else — no logs, HTML,
  progress, or diagnostics;
- diagnostics belong on stderr;
- cancel exits nonzero and emits no stdout at all;
- `--version` is parseable as `MAJOR.MINOR.PATCH`, returns quickly, and is
  suitable for a bounded compatibility probe;
- launch-to-interactive is approximately 1 second or less; record the
  measurement in CI and fail or flag a regression according to the release
  policy.

The test should cover confirmation, cancel, unknown schema rejection, exact
stdout shape, stderr-only diagnostics, the version probe, and the cold-start
budget. Please link the resulting workflow job or PR here when implemented.
