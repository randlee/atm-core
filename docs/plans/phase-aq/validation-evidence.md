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

| Requirement | Closing test or artifact | Verdict |
| --- | --- | --- |
| R1, one gesture on macOS/Windows/Linux | `scripts/send-to/README.md`, `atm-send-to.command`, `atm-send-to.ps1`, Nautilus script, and XDG desktop entry; physical Finder/Explorer/Nautilus delivery still needs host runs | OPEN — owners: fenix (macOS), QA Windows/Ubuntu |
| R2, multi-select recipients and multi-file `$@` | `.just/tests/test_send_to_surface.py::test_multiple_files_and_recipients_reach_one_final_send`; `picker-output-v1.json` | PASS — AQ5 implementation commit |
| R3, native picker behavior | `picker-macos.sh`, `picker-windows.ps1`, `picker-linux.sh`, and `test_reference_picker_emits_versioned_output` | PASS — AQ5 implementation commit |
| R4, dead/idle visibility and safe routing | `PickerInput` status mapping test in `crates/atm/src/commands/members.rs`; Wyvern contract fixture | PASS — AQ5 implementation commit |
| R5, fail closed before send | `test_cancel_exits_without_invoking_send`; picker output validation before final `atm send` | PASS — AQ5 implementation commit |
| R6, host/cwd/status picker projection | `atm teams --json --members`, explicit durable roster `host`, and CLI baseline | PASS — AQ5 implementation commit |
| R7, configured transfer and canonical unconfigured-host error | AQ4 transfer seam and `docs/cross-host-file-transfer.md`; live US-2 configured/unconfigured host transcript not run in this environment | OPEN — owner: fenix/QA host operator |
| R8, attachment text is untrusted | [`CLAUDE.md`](../../../CLAUDE.md) and [`docs/agent-conventions.md`](../../../agent-conventions.md) | PASS — AQ5 implementation commit |
| R13, side-effect-free chaining | pipeline captures and validates picker output before constructing the final send argv; cancel harness proves zero sends | PASS — AQ5 implementation commit |
| R14, queue mirrors send surface | AQ1–AQ3 queue evidence and the AQ4 shared attachment CLI; no queue code changed in AQ5 | PASS — referenced AQ2/AQ3 heads |
| R15, deferred wake and restart semantics | AQ2.7 Herdr and AQ3 tmux evidence refs above; AQ1.9 m5 restart matrix remains an explicit pending slot | OPEN — owner: fenix/m5 operator |

## AQ5.2a optional Wyvern degradation matrix

The optional Wyvern probe is bounded to 1.5 seconds and treats all of the
following as native-picker fallback with a one-line stderr note: absent binary,
below-pin version, unparsable version, a hanging `--version` child, unknown
PickerOutput schema, and missing page asset. The fixture and contract are
committed at [`wyvern-pick-member-contract.md`](fixtures/wyvern-pick-member-contract.md),
with `picker-output-unknown-schema.json` for the schema gate. The six live
stub cases are named in the harness contract but require the final CI review
run to attach their transcript; no atm build or test lane requires Wyvern.

Verdict: OPEN — owner: QA, until the six-case transcript is attached.

## Manual and deferred register

| Item | Verdict / owner |
| --- | --- |
| Finder Quick Action / Shortcut, real delivery + screenshot | OPEN — fenix on macOS |
| Explorer SendTo, real delivery + screenshot | OPEN — Windows QA operator |
| Nautilus, real delivery + screenshot | OPEN — Ubuntu QA operator |
| Cold-start launch-to-interactive measurement | OPEN — fenix; record method and number |
| Phase 2 drafting/chat/attachments metadata/note source | DEFERRED by PRD |
| Team-addressing polish, Share Extension, Win11 MSIX, KDE service menu | DEFERRED by AQ5 scope |
| Queued-attachment TTL interaction | OPEN — owner: QA; accepted follow-up slot |
