# Phase AQ validation evidence register

This is the AQ5 evidence ledger. A PASS means the named automated artifact
was run on the AQ5 worktree; OPEN means the physical host or upstream artifact
is still required. No manual GUI run or cross-host delivery is represented as
passed without a real transcript and screenshot.

## Sprint verdicts

| Sprint | Verdict | Evidence head / artifact |
| --- | --- | --- |
| AQ1 trait foundation + queue CLI | PASS for the merged implementation line | [`a36e45faa456fabccbee53fc2787656b346dff30`](https://github.com/randlee/atm-core/commit/a36e45faa456fabccbee53fc2787656b346dff30) |
| AQ1.5–AQ1.9 graft chain | PASS for automated/local evidence; AQ1.9 m5 restart row OPEN | [`bdeac0a2605df019bc859e3daa00b6273e642de8`](https://github.com/randlee/atm-core/commit/bdeac0a2605df019bc859e3daa00b6273e642de8) *(recorded short head was reconciled locally; live row remains pending)* |
| AQ2 queue graft | PASS | [`f41bead9e825655bf423acb90fa58f8784ed2538`](https://github.com/randlee/atm-core/commit/f41bead9e825655bf423acb90fa58f8784ed2538) |
| AQ2.5 delivery triggers | PASS | [`01e69ed967c56c91824e1a44317ee97178c8324a`](https://github.com/randlee/atm-core/commit/01e69ed967c56c91824e1a44317ee97178c8324a) |
| AQ2.6 Herdr steer | PASS / referenced upstream evidence | [`cfdcafcd49dc330a18dff6bddbcbb8f5145222e0`](https://github.com/randlee/atm-core/commit/cfdcafcd49dc330a18dff6bddbcbb8f5145222e0) |
| AQ2.7 Herdr queue wake | PASS / referenced upstream evidence | [`ea941497c5f231367dea44c4c1744cd3a8333832`](https://github.com/randlee/atm-core/commit/ea941497c5f231367dea44c4c1744cd3a8333832) |
| AQ3 retained tmux idle drain | PASS for clean-runner harness; live tmux transcript referenced | [`ec3ba5173abb1ec8858b24445d93e9c801e344ab`](https://github.com/randlee/atm-core/commit/ec3ba5173abb1ec8858b24445d93e9c801e344ab) |
| AQ4 Send-To core | PASS for merged core/CLI tests | [`1c737c7bb6a702e295ad25998bf90be6ba428c3b`](https://github.com/randlee/atm-core/commit/1c737c7bb6a702e295ad25998bf90be6ba428c3b) |

The AQ3 clean-runner job definition is [`PR #1058`](https://github.com/randlee/atm-core/pull/1058), with its committed job artifact at [`3a14746daf214080159bce2933ec9cc91261e06c`](https://github.com/randlee/atm-core/commit/3a14746daf214080159bce2933ec9cc91261e06c). The AQ2.7 Herdr transcript is the existing fixture/evidence referenced by its sprint; AQ5 does not rerun Herdr or tmux infrastructure.

## AQ5 requirements

Requirement text and IDs are PRD §5 (`prd-atm-send-to.md`) verbatim. Every
Must row below points at a named passing test or a committed evidence file;
no row is marked PASS without one.

| Requirement | Closing test or artifact | Verdict |
| --- | --- | --- |
| R1, one gesture on macOS/Windows/Linux (Must) | `scripts/send-to/README.md`, `atm-send-to.command`, `atm-send-to.ps1`, `nautilus-atm-send-to.sh`, `atm-send-to.desktop`; the `PickerInput` host/cwd/status projection (`crates/atm-core/src/picker_projection.rs`) that feeds every shell entry; physical Finder/Explorer/Nautilus delivery still needs host runs (tracked as `AQ5-gui-e2e` in the manual register below) | OPEN — owners: Rand (macOS Finder first, `AQ5-gui-e2e`), QA Windows/Ubuntu |
| R2, multi-select recipients and multi-file `$@` (Must) | `.just/tests/test_send_to_surface.py::test_multiple_files_and_recipients_reach_one_final_send`; `picker-output-v1.json` | PASS — [`2b280eba2adaffbf180b4812421849a613f842e2`](https://github.com/randlee/atm-core/commit/2b280eba2adaffbf180b4812421849a613f842e2) |
| R3, cross-host delivery via configured per-host transfer script; unconfigured fails closed with the setup-doc error (Must) | `crates/atm/src/commands/send_to.rs::transfer_not_enabled_error_matches_the_canonical_text_verbatim` (canonical error text), `::invoke_transfer_script_happy_path_returns_the_landed_directory` (configured happy path), `crates/atm-core/src/transfer_script.rs::missing_script_is_not_configured` (unconfigured detection); `docs/cross-host-file-transfer.md` | PASS for automated configured/unconfigured coverage — live US-2 cross-host transcript (a real second host) remains OPEN, tracked under `AQ5-gui-e2e`/host-operator rows below, not fabricated here |
| R4, dead/idle members visibly disabled in picker (Must) | `.just/tests/test_picker_exclusion.py` (all four picker adapters — `picker.py`, `picker-macos.sh`, `picker-linux.sh`, `picker-windows.ps1` — plus direct `selectable_rows`/`unavailable_rows` unit coverage) exercised against the committed `picker-input-v1.json` fixture (one `active`, one `idle`, one `dead` member); `.just/tests/test_send_to_surface.py::test_reference_picker_emits_versioned_output` (idle-exclusion regression); `PickerMemberStatus` projection tests in `crates/atm-core/src/picker_projection.rs` | PASS — dead/idle members are filtered out of every picker's selectable set (not merely labeled); `choose from list`/zenity/`Out-GridView` cannot render a disabled row, so exclusion is enforced by omission plus a separate stderr "unavailable" notice |
| R5, cancel never results in a send (Must) | `.just/tests/test_send_to_surface.py::test_cancel_exits_without_invoking_send`; picker output validation before the final `atm send` invocation | PASS — [`2b280eba2adaffbf180b4812421849a613f842e2`](https://github.com/randlee/atm-core/commit/2b280eba2adaffbf180b4812421849a613f842e2) |
| R6, `atm teams --json --members` and `atm send --from-json` usable without Wyvern — TUI, Raycast, scripts (Must) | `picker-macos.sh`, `picker-linux.sh`, `picker-windows.ps1` are each a thin native-only adapter with no Wyvern dependency; `.just/tests/test_send_to_surface.py::test_reference_picker_emits_versioned_output` and `::test_multiple_files_and_recipients_reach_one_final_send` exercise the full pipeline with a stub picker and zero Wyvern involvement | PASS — the native path is the default reference implementation, not a fallback bolted on afterward |
| R7, periodic sweep of `$ATM_TEMP` removes entries older than 30 days (Should) | `crates/atm-core/src/atm_temp_sweeper.rs::expired_file_is_reclaimed_and_fresh_file_is_kept`, `::zero_ttl_is_rejected`, `::valid_config_converts_days_to_seconds` (30-day `TTL` constant), landed in AQ4; AQ5.2a's Wyvern degradation harness is now registered alongside the AQ1.9/AQ2.5 harnesses in `.github/workflows/phase-aq-evidence.yml` (`run_aq5_wyvern_degradation_evidence.py`), producing its transcript under `docs/plans/phase-aq/evidence/AQ5/` | PASS — sweeper coverage is AQ4's; AQ5 does not change or re-test the sweeper itself. The queued-attachment TTL *interaction* (does an unread queued attachment survive its 30-day window) is a separate, still-open item — see the manual register below |
| R8, attachment contents flagged as untrusted in agent conventions (Should) | [`CLAUDE.md`](../../../CLAUDE.md) and [`docs/agent-conventions.md`](../../agent-conventions.md), both naming `$ATM_TEMP/send-to` (grep-checkable per AC3) | PASS — [`2b280eba2adaffbf180b4812421849a613f842e2`](https://github.com/randlee/atm-core/commit/2b280eba2adaffbf180b4812421849a613f842e2) |
| R13, pipeline stages are side-effect-free except the final send (Must) | pipeline captures and validates picker output before constructing the final send argv; `test_cancel_exits_without_invoking_send` proves zero sends on cancel | PASS — [`2b280eba2adaffbf180b4812421849a613f842e2`](https://github.com/randlee/atm-core/commit/2b280eba2adaffbf180b4812421849a613f842e2) |
| R14, `atm queue` mirrors the full `atm send` surface (Must) | AQ1–AQ3 queue evidence and the AQ4 shared attachment CLI; no queue code changed in AQ5 | PASS — referenced AQ2/AQ3 heads |
| R15, deferred nudges drain one-per-idle-transition with restart-safe markers (Must) | AQ2.7 Herdr and AQ3 tmux evidence refs above; AQ1.9 m5 restart matrix remains an explicit pending slot | OPEN — owner: fenix/m5 operator |

## AQ5.2a optional Wyvern degradation matrix

The optional Wyvern probe is bounded to 1.5 seconds and treats all of the
following as native-picker fallback with a one-line stderr note: absent binary,
below-pin version, unparsable version, a hanging `--version` child, unknown
PickerOutput schema, and missing page asset. The fixture and contract are
committed at [`wyvern-pick-member-contract.md`](fixtures/wyvern-pick-member-contract.md),
with `picker-output-unknown-schema.json` for the schema gate. The six cases
are unit-tested hermetically in
`.just/tests/test_send_to_surface.py::test_wyvern_degradation_cases_fall_back_and_still_send`,
and are now also registered as a Phase AQ live-evidence harness —
`scripts/phase-aq/run_aq5_wyvern_degradation_evidence.py` — in
`.github/workflows/phase-aq-evidence.yml`'s `EVIDENCE_DIR_BY_SCRIPT` map,
the same way the AQ1.9 restart matrix and AQ2.5 queue-delivery-trigger
harnesses are listed. That job produces the six-case transcript under
`docs/plans/phase-aq/evidence/AQ5/` (`wyvern-degradation-<host>.{json,md}`)
on every CI run touching `scripts/phase-aq/**`; no atm build or test lane
requires Wyvern to be installed.

Verdict: PASS for the automated six-case matrix (unit test + registered CI
harness); the harness's own live-run transcript is produced by the next CI
pass on this branch, not fabricated here.

## Deliverable 3 — Wyvern `pick-member.html` (upstream, implemented and wired in)

Upstream PR: [`randlee/wyvern#140`](https://github.com/randlee/wyvern/pull/140)
(references atm-core issue #139) — adds
`examples/wizards/atm-pick-member/pages/pick-member.html` (roster grouped by
team, `idle`/`dead` rows rendered `disabled`, unrecognized `schema_version`
rejected) and the Playwright L2 contract test
`tests/l2/wizard-atm-pick-member.spec.ts`, run locally against the real
`wyvern` binary on `develop` (2 passed) before opening the PR.

Building it surfaced a real, minimal integration-shape gap: Wyvern has no
`--picker <path>` flag, and the terminal stdout is the full `WizardResult`
envelope (`.data` holds `PickerOutput`), not a bare `PickerOutput` object.
This is **implemented, not left as a gap**: `scripts/send-to/atm-send-to.sh`
and `atm-send-to.ps1` generate a `wizard.json` (`config` = `PickerInput`)
into `$ATM_TEMP` scratch, invoke `wyvern <wizard.json> --ui-root <dir>` (the
real, working invocation), and unwrap `.data` from the resulting
`WizardResult` via the new `picker.py --unwrap-wizard-result` mode. The
bounded-deadline probe (`probe_wyvern.py`, 1.5s) and every degradation case
(absent, below-pin, unparsable version, hanging `--version`, missing page
asset, unknown `PickerOutput.schema_version`) are unchanged and still gate
before any wizard.json is generated. The full shape is documented in
[`wyvern-pick-member-contract.md`](fixtures/wyvern-pick-member-contract.md).

Verified against a real `wyvern#140` build (commit
`958b5102e977f30f812213d5ae08c1420828bead`), not just the hermetic stub
fixtures: `scripts/send-to/atm-send-to.sh` was run unmodified end to end
against the real binary, with a roster fixture matching
`picker-input-v1.json` (active/idle/dead), driven headlessly over Wyvern's
own `WYVERN_VIEWER=none`/`WYVERN_DIALOG_URL_FILE` contract via the same
`/api/wizard/state`+`/api/wizard/finish` HTTP endpoints the page's own JS
uses. Full transcript (LOCAL, not a CI artifact — CI never installs Wyvern):
[`evidence/AQ5/wyvern-real-invocation-local.md`](evidence/AQ5/wyvern-real-invocation-local.md).
The hermetic stub fixtures in `.just/tests/test_send_to_surface.py` and
`scripts/phase-aq/run_aq5_wyvern_degradation_evidence.py` were updated to
emit the real `WizardResult`-with-`.data` shape (not the old bare
`PickerOutput`) so those automated suites prove the real contract.

Verdict: PASS — implemented and verified against a real `wyvern#140` build,
not an open invocation-shape gap. `AQ6`'s release-preflight pin bump remains
the process that keeps `WYVERN_PIN` current as Wyvern releases; that part
was already in scope for AQ6 and is unaffected by this fix.

## Manual and deferred register

| Item | Verdict / owner |
| --- | --- |
| `AQ5-gui-e2e` — Finder (macOS, first), Explorer SendTo (Windows), Nautilus (Ubuntu): real delivery to a live agent, transcript + screenshot, cold-start number | FOLLOW-UP — owner: Rand, macOS Finder first. **This requires a human and is never fabricated.** Steps: (1) install the shell entry per `scripts/send-to/README.md` for the target OS; (2) start `atm-daemon` and a receiving team member so the roster has at least one `active` member; (3) in the file manager, select one or more files, invoke the installed Send-To entry (Finder Quick Action / Explorer SendTo / Nautilus Scripts), pick the receiving member in the native picker, add a note, confirm; (4) on the receiving side, `atm read` to confirm the message + landed attachment path arrived; (5) record the wall-clock cold-start (invocation to picker interactive) alongside the transcript; (6) drop the terminal transcript (`atm read` output) and a screenshot of the picker dialog under `docs/plans/phase-aq/evidence/AQ5/gui-e2e/<os>/`, named `<os>-transcript.txt` and `<os>-picker.png`, and update this row's verdict to PASS with a link once committed; repeat per OS (Windows/Ubuntu QA operators) |
| Phase 2 drafting/chat/attachments metadata/note source | DEFERRED by PRD |
| Team-addressing polish, Share Extension, Win11 MSIX, KDE service menu | DEFERRED by AQ5 scope |
| Queued-attachment TTL interaction | OPEN — owner: QA; accepted follow-up slot |
