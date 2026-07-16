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
| `AG-FIND-001` | `AG-VAL-011` | `AG-VAL-011` | Cross-host transport requirement still documents TCP/TLS, but the current `1.3.1` implementation uses plain `TcpStream` with no TLS crate in the workspace. | `blocking` | `PRODUCT-BUG` | `shared` | `open` | Phase AG now owns the security closure in two steps: AG.8 plans/reconciles the transport-security direction and AG.10 owns the secured-transport implementation. Until AG.10 passes, any release verdict must explicitly exclude transport-security closure. | `TBD` | Any AG release-usable verdict must explicitly exclude TLS / transport-security coverage while this finding remains open. |
| `AG-FIND-004` | `AG-VAL-003` / `AG-VAL-004` / `AG-VAL-005` / `AG-VAL-006` / `AG-VAL-007` | `AG-VAL-003..007` | Cross-host functional closure is blocked because the product lacks a durable control plane for interface selection/binding, inbound host authorization, and operator-visible diagnostics. Early AG execution also exposed that loopback-bypass design issues belong under this same gap rather than as a standalone track. | `blocking` | `PRODUCT-BUG` | `shared` | `open` | Land the missing control-plane product surface: SQLite-backed interface configuration rows, SQLite-backed deny-by-default exact-host allowlist rows, CLI commands to manage both, daemon enforcement against those tables, retained loopback self-test support, and `atm doctor` projection for the resulting state. Only then rerun real host-pair validation. | `TBD` | This is the real closure line for the old unauthenticated peer-auth gap and for the loopback-bypass design smell discovered during PR #556. Firewall/VPN/routing issues remain separate integration findings once this product surface exists. |
| `AG-FIND-005` | `AG-VAL-003` / `AG-VAL-004` live reruns and corrective review after AG.10 | `AG-VAL-016` / `AG-VAL-017` / `AG-VAL-019` / `AG-VAL-020` / `AG-VAL-021A..021F` / `AG-VAL-022A..022F` | Ordinary `atm send` still lacks a first-class remote-target contract and dispatch branch, so a syntactically "remote-looking" send can still take the local mailbox path instead of the daemon peer-transport path. | `blocking` | `PRODUCT-BUG` | `shared` | `open` | Add a typed remote-target contract to CLI parsing and `SendRequest`, normalize exactly two operator forms (`<agent>@<team>.<host>` and `<agent>@<team> --host <host>`), and route every non-empty `remote_host` through the cross-host delivery trait boundary. Then revalidate that full functionality works first on localhost loopback, then public-interface loopback, then another Mac, then Windows/macOS. | `TBD` | This finding is corrective follow-on scope discovered after AG.6-AG.10 review. It does not reopen those reviewed sprint docs; it adds the missing implementation and revalidation ladder after AG.10. |
