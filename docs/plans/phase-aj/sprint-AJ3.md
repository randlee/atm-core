---
id: AJ.3
title: CLI Wire Payload Integration
status: planned
branch: feature/pAJ-s3-cli-wire-payload
worktree: ../atm-core-worktrees/feature/pAJ-s3-cli-wire-payload
target: integrate/phase-AJ
---

# Sprint AJ.3 — CLI Wire Payload Integration

## Goal

Extend `WriteRequest` and `ReadQuery` with optional `session_id` / `pid`
and have `atm send`, `atm read`, and `atm ack` populate them from
`CallerContext` so the daemon sees caller observational state on every
local dispatch (UDS or TCP — both share the same wire structs).

## Hard Dependencies

- AJ.1 and AJ.2 merged forward into this branch
- `integrate/phase-ai-31-33` baseline
- `docs/plans/phase-aj/plan-phase-aj.md`
- `docs/plans/phase-aj/phase-aj-research.md`
- `crates/atm-core/src/send/mod.rs` baseline
- `crates/atm-core/src/read/mod.rs` baseline

## Exact Targets

- `crates/atm-core/src/send/mod.rs`
- `crates/atm-core/src/read/mod.rs`
- `crates/atm/src/commands/send.rs`
- `crates/atm/src/commands/read.rs`
- `crates/atm/src/commands/ack.rs`

No changes to `crates/atm-daemon/src/local_ipc_transport/request_worker.rs`
or `crates/atm-daemon/src/local_tcp_transport.rs` — the unified HTTP
framing passes the JSON body through unchanged.

## Interfaces To Add Or Modify

- `WriteRequest` gains:
  - `pub session_id: Option<SessionId>` with
    `#[serde(default, skip_serializing_if = "Option::is_none")]`
  - `pub pid: Option<u32>` with
    `#[serde(default, skip_serializing_if = "Option::is_none")]`
- `ReadQuery` gains the same two fields with the same serde attributes
- `SendCommand::build_request()` populates both fields from
  `CallerContext`
- The `read` command populates both fields from `CallerContext` when
  constructing `ReadQuery`
- The `ack` command populates both fields from `CallerContext` when
  constructing its write request

## Deliverables

- All three CLI commands transmit `session_id` and `pid` whenever the
  caller's env provided them
- All three CLI commands omit the fields entirely on the wire when env
  was unset (no `null` literals, no empty strings)
- Wire payloads remain readable by older daemon builds — the new fields
  are strictly additive
- Payloads flow identically over UDS and TCP because both transports use
  `HttpFrameReader` and dispatch through the same `ApiRouter`
- No CLI flag is added in this sprint; values come from env only
- No code path inspects `session_id`/`pid` to alter CLI behavior

## Required Validation

- `cargo build --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test -p atm-core -p atm`
- New integration test: serialize `WriteRequest` with `session_id: None`
  → JSON contains no `session_id` key; with `Some(...)` → JSON contains
  the value
- Same shape of test for `ReadQuery`
- Manual smoke over UDS: `ATM_SESSION_ID=smoke-1 ATM_PID=$$ atm send ...`
  against a dev daemon and confirm receipt in daemon trace log
- Manual smoke over TCP loopback: same env against a daemon started with
  TCP enabled; confirm identical receipt in trace log (UDS/TCP parity)
- `rg -n "session_id|pid" crates/atm-core/src/send/mod.rs crates/atm-core/src/read/mod.rs`
  shows the new fields with the required serde attributes
- `git diff --check`
