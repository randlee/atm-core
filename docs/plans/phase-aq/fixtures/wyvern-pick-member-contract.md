# Wyvern `pick-member.html` contract

This is the **single authoritative statement** of the real Wyvern
integration contract, superseding the illustrative `wyvern --picker
page.html < input.json > output.json` sketch in the PRD (`prd-atm-send-to.md`
§4.1) -- that sketch never matched Wyvern's actual CLI surface and is not a
working command. The page is maintained in the Wyvern repository (vendored
locally only as the small static asset `scripts/send-to/pick-member.html`,
kept in sync with upstream -- see that file's own header comment); atm-core
does not depend on the `wyvern` crate/binary at build or test time.

## Invocation (implemented, verified against `wyvern#140`)

Wyvern has **no `--picker <path>` flag**. The real, working invocation --
implemented in `scripts/send-to/atm-send-to.sh` and `atm-send-to.ps1`, and
verified end to end against a real `wyvern` build in
[`docs/plans/phase-aq/evidence/AQ5/wyvern-real-invocation-local.md`](../evidence/AQ5/wyvern-real-invocation-local.md)
-- is:

1. Generate a wizard command document (`config` = the `PickerInput` JSON,
   verbatim) via `picker.py --make-wizard-json`:

   ```json
   {"type":"wizard","page":{"id":"pick-member","title":"ATM Send-To","html":"pages/pick-member.html"},"config":<PickerInput>}
   ```

2. Write it, plus a copy of the vendored `pick-member.html`, into a fresh
   `$ATM_TEMP/send-to/wyvern-wizard.<random>/{wizard.json,pages/pick-member.html}`
   directory.
3. Invoke `wyvern <wizard.json> --ui-root <that directory>` (a plain
   positional argument, not `--picker`). `WizardCommand::config` is "opaque
   wizard-wide config, never inspected by the host" -- the only channel a
   wizard page has for caller-supplied data; the page reads it via
   `window.wyvern.config` after `wyvernWizardState()`.
4. On success, `wyvern` prints the full `WizardResult` envelope on stdout,
   **not** a bare `PickerOutput`:

   ```json
   {"button":"finish","data":<PickerOutput>,"stack":[...]}
   ```

   `picker.py --unwrap-wizard-result` extracts and validates `.data`;
   a non-`"finish"` `button` (cancel/dismiss) is treated exactly like a
   cancelled native picker.
5. The binary's `--version` output must still contain a parseable
   `MAJOR.MINOR.PATCH` version and return promptly; `probe_wyvern.py`'s
   1.5 second bounded deadline and every degradation case (absent,
   below-pin, unparsable version, hanging `--version`, missing page asset,
   unknown `PickerOutput.schema_version`) are unchanged by this shape --
   they gate the same way, before any wizard.json is ever generated.

Cancel and every degradation path fall back to the native picker with a
one-line stderr note, exactly as before; no atm build or test lane requires
Wyvern to be installed.

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

The page groups members by team, permits multi-select, and renders `idle`
and `dead` members as genuinely `disabled` checkboxes (not merely styled --
HTML's native `disabled` attribute removes them from selection, keyboard
focus, and form submission), with a one-line note field. It rejects an
unknown input `schema_version` rather than guessing at field meanings.

## Upstream

[`randlee/wyvern#140`](https://github.com/randlee/wyvern/pull/140)
(references atm-core issue #139) adds
`examples/wizards/atm-pick-member/pages/pick-member.html` and the Playwright
L2 contract test `tests/l2/wizard-atm-pick-member.spec.ts`. Both idle and
dead rows render `disabled`, and an unrecognized `schema_version` is
rejected without guessing. The vendored copy at
`scripts/send-to/pick-member.html` is that same file (commit
`958b5102e977f30f812213d5ae08c1420828bead` on
`feat/atm-pick-member-contract`); its header comment records provenance and
must be kept in sync if the upstream page changes before the PR merges.
