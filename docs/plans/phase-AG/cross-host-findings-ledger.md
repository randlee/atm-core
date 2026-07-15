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
| `AG-FIND-001` | `AG-VAL-011` | `AG-VAL-011` | Cross-host transport requirement still documents TCP/TLS, but the current `1.3.1` implementation uses plain `TcpStream` with no TLS crate in the workspace. | `blocking` | `PRODUCT-BUG` | `shared` | `open` | Track transport-security gap separately from AG functional validation; do not expand AG dev scope to implement TLS in this phase. | `TBD` | Any AG release-usable verdict must explicitly exclude TLS / transport-security coverage while this finding remains open. |
| `AG-FIND-002` | `AG.1 Windows setup` | `AG-VAL-001` | The AG.1 clean-room runbook listed disposable `ATM_HOME`, `ATM_CONFIG_HOME`, and `ATM_LOG_DIR`, but release binaries still resolve durable SQLite state and singleton runtime from the OS-account `.atm` root; a normal Windows operator account cannot satisfy the no-live-`~/.atm` Lane A rule with the documented environment variables alone. | `blocking` | `SETUP-GAP` | `shared` | `open` | Team-lead/arch-ctm must choose and document an approved clean-room isolation method, either a disposable OS account/container/VM or a supported release-binary durable-state override, before accepting `AG-VAL-001` as clean-room evidence. | `TBD` | Windows probe showed command health can pass only after touching `C:\Users\rand.lee\.atm\db\mail.db`; the probe rows were removed immediately after discovery. |
