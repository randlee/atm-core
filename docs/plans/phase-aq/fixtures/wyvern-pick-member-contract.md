# Wyvern `pick-member.html` contract

This is the shared contract for the optional Wyvern runtime picker.  The
page is maintained in the Wyvern repository; atm-core intentionally does not
vendor the page or make Wyvern a build/test dependency.

## Invocation

The atm adapter invokes the pinned binary as:

```text
wyvern --picker /absolute/path/to/pick-member.html
```

It writes one `PickerInput` JSON object to stdin. The page must write exactly
one `PickerOutput` JSON object to stdout and must write diagnostics to stderr.
There must be no logging, HTML, or progress text on stdout. Cancel is a
nonzero exit with no stdout. The binary's `--version` output must contain a
parseable `MAJOR.MINOR.PATCH` version and return promptly; atm probes it with
a 1.5 second deadline.

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

Upstream tracking issue / PR: [`randlee/wyvern#140`](https://github.com/randlee/wyvern/pull/140)
(references atm-core issue #139), adding
`examples/wizards/atm-pick-member/pages/pick-member.html` and the Playwright
L2 contract test `tests/l2/wizard-atm-pick-member.spec.ts`. Both idle and
dead rows render `disabled` (not merely styled), and an unrecognized
`schema_version` is rejected without guessing.

**Invocation-shape correction (found while building the linked PR):** real
Wyvern (v0.5.0) has no `--picker <path>` flag described above; that
invocation is this doc's illustrative sketch, not a working command.
Wyvern's own stdin is read as its `Command` JSON only when no
positional/extension argument is given, so a caller cannot both name a page
positionally and pipe `PickerInput` to it. The real integration point is the
wizard command's opaque `config` field, requiring the adapter to generate a
small `wizard.json` (`config` = `PickerInput`) rather than piping to
`wyvern pick-member.html` directly; the terminal stdout is the full
`WizardResult` envelope (`{"button":"finish","data":<PickerOutput>,"stack":[...]}`),
so the adapter must read `.data`, not the bare stdout object. See the linked
PR's README for the exact shape. `scripts/send-to/atm-send-to.sh`'s current
`--picker`/bare-stdout assumption does not yet match this and is tracked as
a follow-up in `validation-evidence.md` (AQ5.2a) rather than fabricated as
closed here.
