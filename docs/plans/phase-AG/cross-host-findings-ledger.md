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
| `AG-FIND-005` | `AG-VAL-003` / `AG-VAL-004` live reruns and corrective review after AG.10 | `AG-VAL-016` / `AG-VAL-017` / `AG-VAL-019` / `AG-VAL-020` / `AG-VAL-021A..021F` / `AG-VAL-022A..022F` | Ordinary `atm send` lacked a first-class remote-target contract and dispatch branch, so a syntactically "remote-looking" send could take the local mailbox path instead of the daemon peer-transport path. | `blocking` | `PRODUCT-BUG` | `shared` | `corrective-implementation-landed` | AG.11 lands the typed remote-target contract, production `remote_host` branch, and composition-root-owned `DaemonCrossHostDelivery`. The remaining required scope is proof: localhost full-function same-host validation, self-IP same-host validation, automated integration coverage, other-Mac smoke, and Windows/macOS smoke. A follow-on two-layer fix was required for the cross-host ack round-trip: (1) the ack path never routed replies back to the origin host, and (2) the SQLite `StorageEnvelope` write projection dropped `origin_host`. Fix landed via commits `1df61a4d` (persist inbound peer `origin_host`, thread `peer_origin` through `RequestDispatcher::dispatch`, extend ack `ReplyTarget` with `Option<RemoteTargetHost>`, route ack reply via new `AckReplyDeliveryPort` boundary backed by `cross_host_delivery.deliver_remote`) and `2e8cc4ca` (persist `origin_host` through the SQLite writer `StorageEnvelope` projection; no migration, field lives in the `envelope_json` blob). | `AG-VAL-022D VALIDATED 2026-07-17 (Windows/macOS ack round-trip) via commits 1df61a4d + 2e8cc4ca; AG-VAL-022A/B/C/E/F PASS; localhost/self-IP/other-Mac revalidation complete` | This finding remains phase-blocking until the post-AG.11 validation ladder is complete, but the implementation gap itself is no longer unimplemented on the corrective branch. The Windows/macOS ack round-trip (AG-VAL-022D) is now validated after the two-layer ack-routing + origin_host-persistence fix. |
| `AG-FIND-006` | `AG-VAL-022F` | `AG-VAL-022F` | Cross-host send to a downed peer surfaces as a LOCAL `failed to read daemon response frame` (os error 35) instead of a clear remote-unreachable / retry-budget-exhausted classification. Recovery send after daemon restart correctly returns "sent", so delivery semantics are unaffected; only the error-quality classification for the unreachable-peer case is imprecise. | `non-blocking` | `PRODUCT-BUG` | `shared` | `open` | Improve unreachable-peer error classification so a send to a downed cross-host peer reports a clear remote-unreachable / retry-budget-exhausted outcome rather than a local response-frame read failure. Quality-only; does not block cross-host functional closure. | `TBD` | Observed during AG-VAL-022F on the 2026-07-17 clean-room Windows/macOS host-env run; evidence ~/atm-xhost-evidence. |
