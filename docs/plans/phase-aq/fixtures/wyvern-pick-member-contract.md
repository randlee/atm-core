# Wyvern `pick-member.html` contract

This is the shared contract for the optional Wyvern runtime picker.  The
page is maintained in the Wyvern repository; atm-core intentionally does not
vendor the page or make Wyvern a build/test dependency.

## Invocation

The atm adapter generates a short-lived wizard command and invokes the pinned
binary as:

```text
wyvern /temporary/path/wizard.json --ui-root /directory/containing/pick-member.html
```

The generated `wizard.json` carries one `PickerInput` object under the wizard
command's opaque `config` field. The wizard writes a `WizardResult` envelope
to stdout; the adapter accepts only `button: "finish"` and unwraps its
`data` field as the `PickerOutput`. Cancel/dismissed buttons are treated as a
nonzero selection failure with no stdout. Diagnostics, HTML, and progress
text must not be forwarded to stdout. The binary's `--version` output must
contain a parseable `MAJOR.MINOR.PATCH` version and return promptly; atm probes
it with a 1.5 second deadline.

## JSON fixtures

The canonical bytes are [`picker-input-v1.json`](picker-input-v1.json) and
[`picker-output-v1.json`](picker-output-v1.json). The input is:

```json
{"schema_version":1,"teams":[{"id":"…","name":"…","members":[{"id":"…","name":"…","host":"…","cwd":"…","status":"active|idle|dead"}]}]}
```

The output is:

```json
{"schema_version":1,"recipients":["member-id","…"],"note":"optional one-liner"}
```

The page groups members by team, permits multi-select, displays `idle` and
`dead` members greyed/non-routable, and provides a one-line note field. It
must reject an unknown input schema rather than guessing at field meanings.

Upstream tracking issue / PR: **OPEN — Wyvern maintainer owner required**.
