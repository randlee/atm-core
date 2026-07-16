# Phase AG Cross-Host Findings Ledger

## Purpose

Record verified cross-host validation findings for the `1.3.1` release line.

## Record Schema

Each finding entry must record:

- `finding_id`
- `discovered_in`
- `linked_row_id`
- `summary`
- `severity`
- `classification`
- `owner`
- `status`
- `required_fix_scope`
- `revalidation_result`
- `notes`

## Classification Enum

Use exactly one of:

- `SETUP-GAP`
- `ENV-MISTAKE`
- `PRODUCT-BUG`
- `EXTERNAL-BLOCKER`

See `plan-phase-AG.md` for the sole authoritative definition. This ledger only
consumes that enum.

## Owner Enum

Use exactly one of:

- `team-lead`
- `arch-ctm`
- `quality-mgr`
- `windows-operator`
- `macos-operator`
- `shared`

## Findings

| Finding ID | Discovered In | Linked Row ID | Summary | Severity | Classification | Owner | Status | Required Fix Scope | Revalidation Result | Notes |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `AG-FIND-001` | `AG-VAL-011` | `AG-VAL-011` | Cross-host transport requirements document TCP/TLS. The AG.10 development line now implements TLS-backed secure mode, trusted-peer approval, and doctor visibility, but secure LAN/VPN host-pair evidence has not yet been retained on real hardware. | `blocking` | `PRODUCT-BUG` | `shared` | `open` | AG.8 owned the wording/ADR/implementation-plan reconciliation; AG.10 now owns the landed secure transport implementation plus local secure smoke/integration rows. Keep the finding open until `AG-VAL-013` and `AG-VAL-015` pass on real hardware and the final release verdict can claim transport-security closure honestly. | `PARTIAL` | Local secure loopback (`AG-VAL-012`) and local rejection proof (`AG-VAL-014`) now exist and close the same-host portions of the secure transport surface. They do not replace the required LAN/VPN reruns. |
| `AG-FIND-002` | `AG.1 Windows setup` | `AG-VAL-001` | The AG.1 clean-room runbook listed disposable `ATM_HOME`, `ATM_CONFIG_HOME`, and `ATM_LOG_DIR`, but release binaries resolve durable SQLite state and singleton runtime from the OS-account `.atm` root. | `medium` | `SETUP-GAP` | `shared` | `closed` | Windows operator/team-lead accepted using the Windows host account environment for AG.1 because no installed/running ATM service exists on this computer; future strict clean-room claims still need a disposable OS account/container/VM or release-binary durable-state override. | `PASS` | Windows host-env rerun at `2026-07-15T05:39Z` passed `AG-VAL-001` using branch-built release binaries; this closes the immediate AG.1 Windows setup blocker without making a strict no-live-OS-account-state clean-room claim. |
| `AG-FIND-003` | `AG-VAL-002` | `AG-VAL-002` | The AG.1 clean-room env contract does not isolate the daemon/runtime surface on `1.3.1`: `ATM_HOME` / `ATM_CONFIG_HOME` move team/config discovery, but `atm doctor` still connects to the one host-global daemon, host-global SQLite store, and host-global retained log sink. | `important` | `SETUP-GAP` | `arch-ctm` | `open` | Update the AG runbook/evidence contract to treat same-host validation as host-singleton validation; if future phases require true disposable daemon isolation, open separate product work for a supported runtime-root override instead of launching a second daemon. | `TBD` | Observed on macOS with temp `ATM_HOME`, temp `ATM_CONFIG_HOME`, and temp `ATM_LOG_DIR`: doctor reported `daemon_connect=connected`, `daemon_auto_start=skipped`, `owner_pid=7633`, and `active_log_path=/Users/randlee/.atm/logs/atm.log.jsonl` while also reading temp-team config under `/tmp/.../.claude/teams/atm-dev`. |
| `AG-FIND-004` | `AG-VAL-003` / `AG-VAL-004` / `AG-VAL-005` / `AG-VAL-006` / `AG-VAL-007` | `AG-VAL-003..007` | Historical AG blocker: cross-host functional closure originally failed because the product lacked a durable control plane for interface selection/binding, inbound host authorization, and operator-visible diagnostics. | `blocking` | `PRODUCT-BUG` | `shared` | `open` | The required product surface was sequenced into AG.4 durable interface rows, AG.5 deny-by-default exact-host rows plus pre-dispatch enforcement, and AG.6 doctor projection. Keep the finding open until that line is accepted and the live AG.7 host-pair rerun confirms the product surface is the one being exercised in practice. | `TBD` | This finding is about the historical product-surface gap, not transport security. AG.7's local harness work proves the peer-listener request path on the current branch; the remaining open work is live host-pair evidence on the intended product surface. Firewall/VPN/routing issues remain separate integration findings if they surface during the rerun. |
| `AG-FIND-005` | `AG-VAL-004` | `AG-VAL-004` | The live daemon send path still never invokes peer transport, so a cross-host send can report success while writing the payload into the local daemon sink instead of handing it to the remote daemon. | `blocking` | `PRODUCT-BUG` | `arch-ctm` | `open` | Hard release blocker. Close only through the AG.11-AG.17 corrective chain: wire `RequestEnvelope::Send(...)` through the peer client for cross-host delivery, then rerun the first real Windows/macOS send in both directions before any release-usable claim. | `FAIL` | Live evidence at `2026-07-15T16:10Z`: macOS patched daemon bound `192.168.128.82:43101`, restarted with `ATM_DAEMON_PEER_ADDR=10.10.100.98:43101`, and `ATM_TEAM=ag-clean-room ATM_IDENTITY=arch-ctm atm send windows-operator "AG-VAL-004 macOS->Windows attempt-1" --json` returned `message_id=01KXK8PV31QMXR3FPJRXRQNSJZ`. But the message was written locally into `/Users/randlee/.atm/daemon/non_claude_outbound.jsonl` instead of producing peer-transport evidence. Code root cause: `crates/atm-daemon/src/runtime_health.rs` dispatches `RequestEnvelope::Send(...)` via `send_mail_with_runtime_and_post_send_emitter(..., &self.service_runtime, ...)`, while `crates/atm-daemon/src/composition.rs` only assembles `peer_transport_runtime` for listener/replay lifecycle and never injects it into the live send dispatcher. |
