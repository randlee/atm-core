# AQ5 real Wyvern invocation -- local transcript

**LOCAL transcript.** Produced on a single macOS developer machine by
manually building the linked upstream PR
([`randlee/wyvern#140`](https://github.com/randlee/wyvern/pull/140),
commit `958b5102e977f30f812213d5ae08c1420828bead`) and running
`scripts/send-to/atm-send-to.sh` end-to-end against the real `wyvern`
binary it produced. This is **not** a CI artifact -- CI never installs
Wyvern (per the sprint's Wyvern dependency contract, no atm build or test
lane requires it) -- and is not reproducible by `just test`/CI on its own.
It exists to prove the corrected invocation shape documented in
[`wyvern-pick-member-contract.md`](../../fixtures/wyvern-pick-member-contract.md)
actually works against a real Wyvern process, not only the hermetic stub
fixtures in `.just/tests/test_send_to_surface.py` and
`scripts/phase-aq/run_aq5_wyvern_degradation_evidence.py`.

- atm-core commit: this file's own commit (the working tree, including the
  `scripts/send-to/atm-send-to.sh`/`picker.py`/`pick-member.html`
  real-Wyvern-invocation changes, at the time this transcript was captured)
- Wyvern binary: `cargo build -p wyvern-cli --bin wyvern` from
  `randlee/wyvern` commit `958b5102e977f30f812213d5ae08c1420828bead`
  (`feat/atm-pick-member-contract`, PR #140)
- `wyvern --version` -> `wyvern 0.5.0` (matches `WYVERN_PIN` in
  `atm-send-to.sh`/`atm-send-to.ps1` exactly)
- Host: macOS (26.5.1), local developer machine

## What was driven

`atm-send-to.sh` was run unmodified (no picker overrides), with only
`ATM_SEND_TO_WYVERN_BIN` pointed at a thin `tee`-through spy wrapper
around the locally built `wyvern` binary (to capture its raw stdout
independent of what `atm-send-to.sh` does with it afterward) and a stub
`atm` script standing in for `atm teams --json --members`/`atm send
--from-json` (same shape the automated harness stubs use). Because a real
Wyvern wizard window needs a human click and this run has none, Wyvern was
launched with its own `WYVERN_VIEWER=none`/`WYVERN_DIALOG_URL_FILE`
headless-CI contract (documented in the Wyvern repo's own
`docs/plans/phase-C/c9-testing-headless.md`) and driven by a small
concurrent `curl` script hitting the exact same `/api/wizard/state` and
`/api/wizard/finish` HTTP endpoints the page's own JS
(`wyvernWizardState`/`wyvernWizardFinish` in Wyvern's
`ui/shared/wyvern-api.js`) calls -- this is not a shortcut around the
contract, it is the same HTTP surface a real browser click drives, and it
is exactly what Wyvern's own Playwright L2 test
(`tests/l2/wizard-atm-pick-member.spec.ts` in the linked PR) also does at
a higher level. `atm-send-to.sh` itself never knows the difference
between a human click and this driver.

The roster fixture matches the committed
[`picker-input-v1.json`](../../fixtures/picker-input-v1.json) fixture: one
`active` member (`cipher@atm-dev`), one `idle` member (`fenix@atm-dev`),
one `dead` member (`offline@atm-dev`, null host/cwd).

## Sequence

1. `atm-send-to.sh plan.md` is invoked. It resolves `atm teams --json
   --members` (stub), passes the bounded `probe_wyvern.py` pin/asset
   check for the real `wyvern` binary and the vendored
   `scripts/send-to/pick-member.html` asset, then -- inside one
   trap-cleaned subshell, so any failure here degrades to the native
   picker exactly like an unavailable Wyvern does, never a hard abort --
   creates `$ATM_TEMP/send-to/wyvern-wizard.XXXXXX/{pages/pick-member.html,wizard.json}`,
   generates `wizard.json` via `picker.py --make-wizard-json` (`config`
   = the PickerInput roster verbatim), and invokes
   `wyvern <wizard.json> --ui-root <wizard dir>` (no `--picker` flag --
   it does not exist).
2. Wyvern serves the dialog over HTTP. `GET /api/wizard/state` returns
   the wizard state with `config` equal to the generated PickerInput
   (captured verbatim below) -- proving the `config`-field channel
   documented in the contract doc actually round-trips through a real
   Wyvern process, not just the stub.
3. The driver `POST`s `/api/wizard/finish` with
   `{"button":"finish","data":{"schema_version":1,"recipients":["cipher@atm-dev"],"note":"see the attached plan"},"stack":[...]}`
   -- the same shape `pick-member.html`'s `collectCurrentPageData()` +
   `wizard-nav.js`'s `wyvernWizardFinish` would submit for selecting the
   one `active` member and typing a note.
4. `wyvern` exits 0 and prints the full `WizardResult` envelope on
   stdout (captured below, via the spy wrapper) -- confirming the terminal
   stdout really is `{"button":...,"data":...,"stack":...}`, not a bare
   `PickerOutput`.
5. `atm-send-to.sh` pipes that stdout through `picker.py
   --unwrap-wizard-result`, which extracts and validates `.data` as
   `PickerOutput`, then re-validates the whole picker output one more
   time (the existing unconditional `--validate` safety check) before
   invoking `atm send --from-json`.
6. The stub `atm` records the exact argv and stdin `atm-send-to.sh`
   invoked it with.
7. The generated `wyvern-wizard.XXXXXX` directory (wizard.json + the
   copied page) is removed by the subshell's `trap ... EXIT` before
   `atm-send-to.sh` exits -- verified by listing `$ATM_TEMP` afterward:
   only the empty parent `send-to/` directory remained.

## Captured artifacts (verbatim from this run)

### `GET /api/wizard/state` response

```json
{"type":"wizard","config":{"schema_version":1,"teams":[{"id":"atm-dev","members":[{"cwd":"/work/atm-core","host":"m4","id":"cipher@atm-dev","name":"cipher","status":"active"},{"cwd":"/work/atm-core","host":"m5","id":"fenix@atm-dev","name":"fenix","status":"idle"},{"cwd":null,"host":null,"id":"offline@atm-dev","name":"offline","status":"dead"}],"name":"atm-dev"}]},"page":{"id":"pick-member","title":"ATM Send-To","html":"pages/pick-member.html"},"page_data":{},"stack":[]}
```

### `POST /api/wizard/finish` response (echoed request body wyvern accepted)

```json
{"button":"finish","data":{"note":"see the attached plan","recipients":["cipher@atm-dev"],"schema_version":1},"stack":[{"page":{"id":"pick-member","title":"ATM Send-To","html":"pages/pick-member.html"},"data":{"note":"see the attached plan","recipients":["cipher@atm-dev"],"schema_version":1}}]}
```

### `wyvern`'s raw stdout (the full `WizardResult`, captured by the spy wrapper)

```json
{"button":"finish","data":{"note":"see the attached plan","recipients":["cipher@atm-dev"],"schema_version":1},"stack":[{"page":{"id":"pick-member","title":"ATM Send-To","html":"pages/pick-member.html"},"data":{"note":"see the attached plan","recipients":["cipher@atm-dev"],"schema_version":1}}]}
```

### `atm-send-to.sh` stderr

```text
WYVERN_DIALOG_URL=http://127.0.0.1:64561/wizard/pages/pick-member.html
atm send: message delivered (stub)
```

### Final `atm send --from-json` invocation (stub `atm`, captured argv + stdin)

argv:

```text
send
--from-json
--attach
/tmp/aq5-real-e2e.3c0S50/plan.md
```

stdin (`PickerOutput`, unwrapped from `WizardResult.data`):

```json
{"note":"see the attached plan","recipients":["cipher@atm-dev"],"schema_version":1}
```

### `atm-send-to.sh` exit code

```text
0
```

## Result

**PASS.** The real Wyvern binary from the linked PR served the generated
`PickerInput` under `config`, returned a `WizardResult` with the
selected recipient under `.data`, and `atm-send-to.sh` unwrapped and
delivered it through the unchanged final `atm send --from-json` stage --
end to end, with no fabricated steps, and with its scratch directory
cleaned up afterward. Deliverable 3 (Wyvern `pick-member.html`) is
implemented and verified against a real `wyvern#140` build, not left as
an open invocation-shape gap.
