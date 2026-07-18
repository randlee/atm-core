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
| `AG-FIND-001` | `AG-VAL-011` | `AG-VAL-011` | Cross-host transport requirement still implies full TCP/TLS security closure, but the current `1.3.1` implementation now uses rustls plus pinned SHA256 peer-certificate fingerprints rather than full PKI chain / expiry validation. | `blocking` | `PRODUCT-BUG` | `shared` | `open` | Phase AG now owns the security closure in two steps: AG.8 plans/reconciles the transport-security direction and AG.10 owns the secured-transport implementation. Until the repository either accepts the pinned-fingerprint model via ADR-032 or layers full chain / expiry validation on top, any release verdict must explicitly exclude full transport-security closure. | `TBD` | Localhost secured transport is now present, but this finding stays open until the documented requirement language and validation posture fully align with the implemented rustls pinned-fingerprint trust model. |
| `AG-FIND-004` | `AG-VAL-003` / `AG-VAL-004` / `AG-VAL-005` / `AG-VAL-006` / `AG-VAL-007` | `AG-VAL-003..007` | Cross-host functional closure is blocked because the product lacks a durable control plane for interface selection/binding, inbound host authorization, and operator-visible diagnostics. Early AG execution also exposed that loopback-bypass design issues belong under this same gap rather than as a standalone track. | `blocking` | `PRODUCT-BUG` | `shared` | `open` | Land the missing control-plane product surface: SQLite-backed interface configuration rows, SQLite-backed deny-by-default exact-host allowlist rows, CLI commands to manage both, daemon enforcement against those tables, retained loopback self-test support, and `atm doctor` projection for the resulting state. Only then rerun real host-pair validation. | `TBD` | This is the real closure line for the old unauthenticated peer-auth gap and for the loopback-bypass design smell discovered during PR #556. Firewall/VPN/routing issues remain separate integration findings once this product surface exists. |
| `AG-FIND-005` | `AG-VAL-003` / `AG-VAL-004` live reruns and corrective review after AG.10 | `AG-VAL-016` / `AG-VAL-017` / `AG-VAL-019` / `AG-VAL-020` / `AG-VAL-021A..021F` / `AG-VAL-022A..022F` | Ordinary `atm send` lacked a first-class remote-target contract and dispatch branch, so a syntactically "remote-looking" send could take the local mailbox path instead of the daemon peer-transport path. | `blocking` | `PRODUCT-BUG` | `shared` | `corrective-implementation-landed` | AG.11 lands the typed remote-target contract, production `remote_host` branch, and composition-root-owned `DaemonCrossHostDelivery`. The remaining required scope is proof: localhost full-function same-host validation, self-IP same-host validation, automated integration coverage, other-Mac smoke, and Windows/macOS smoke. | `AG.11 implementation landed; AG.12-AG.16 revalidation pending` | This finding remains phase-blocking until the post-AG.11 validation ladder is complete, but the implementation gap itself is no longer unimplemented on the corrective branch. |
| `AG-FIND-006` | `AG-VAL-022F` | `AG-VAL-022F` | Cross-host send to a downed peer surfaces as a LOCAL `failed to read daemon response frame` (os error 35) instead of a clear remote-unreachable / retry-budget-exhausted classification. Recovery send after daemon restart correctly returns "sent", so delivery semantics are unaffected; only the error-quality classification for the unreachable-peer case is imprecise. | `non-blocking` | `PRODUCT-BUG` | `shared` | `open` | Improve unreachable-peer error classification so a send to a downed cross-host peer reports a clear remote-unreachable / retry-budget-exhausted outcome rather than a local response-frame read failure, instead of recovery text that points at the LOCAL daemon/socket/`ATM_DAEMON_BIN` as if the local daemon were misconfigured. Quality-only; does not block cross-host functional closure and is independent of the ack-routing implementation mechanism. | `TBD` | Observed during AG-VAL-022F on a 2026-07-17 clean-room Windows/macOS host-env run and reconfirmed on baseline `328993e1`. Consolidated here from the duplicate tracking entry previously logged as `AG-FIND-006` on branch `feature/pAG-ack-reconcile`; do not duplicate further. Implementation-agnostic classification quality gap that persists regardless of which cross-host ack-routing mechanism the baseline carries. |
| `AG-FIND-007` | `feature/pAG-crosshost-hardening` corrective review | `AG-FIND-005` (address/send-target parsing) | `.` was allowed in agent/team name validation, but `.` is the reserved delimiter in the `<agent>@<team>.<host>` inline remote-target syntax, so a legal team name such as `dev.qa` was uncreatable-safe / unusable for cross-host sends: `atm send "dev-win@dev.qa" --host 192.168.1.146 ...` fails with `cannot combine inline remote host syntax with --host` because `.qa` is parsed as an inline host segment. Confirmed on baseline `328993e1`. | `blocking` | `PRODUCT-BUG` | `shared` | `corrective-implementation-landed` | Forbid `.` in agent and team name validation (`crates/atm-core/src/address.rs::validate_path_segment`, `crates/atm-storage/src/validation.rs::validate_path_segment`), keeping `-` and `_` allowed, so `.` is unambiguously reserved for the team/host delimiter in cross-host addressing. Landed on `feature/pAG-crosshost-hardening`. | `cargo test --workspace` green post-fix; unit coverage added for dotted team/agent rejection and hyphen/underscore acceptance | This closes the dotted-target addressing ambiguity gap identified during cross-host hardening review; no further product change required beyond the validation fix already landed. |
