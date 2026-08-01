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

Carry optional `ActivityObservation` on `WriteRequest` and `ReadQuery`, then
have `atm send`, `atm read`, `atm ack`, and graft populate it from their
environment-derived context so the daemon sees caller observational state on every
local dispatch (UDS or TCP — both share the same wire structs).

## Hard Dependencies

- AJ.1 and AJ.2 merged forward into this branch
- `integrate/phase-AJ` at the Phase AJ entry-gate SHA
- `docs/plans/phase-aj/plan-phase-aj.md`
- `docs/plans/phase-aj/phase-aj-research.md`
- `crates/atm-core/src/send/mod.rs` baseline
- `crates/atm-core/src/read/mod.rs` baseline

## Dependency Relation

- `must_follow` AJ.2 because CLI and graft transmit its attestation DTO.
- No AJ sprint is `parallel_safe`: AJ.4 consumes these wire fields. Merge
  AJ.2 → AJ.3 before every dev/fix round; AJ.3's PR completes after AJ.2's PR,
  while development may start after AJ.2's development commit is pushed.

## Exact Targets

- `crates/atm-core/src/send/mod.rs`
- `crates/atm-core/src/read/mod.rs`
- `crates/atm/src/commands/send.rs`
- `crates/atm/src/commands/read.rs`
- `crates/atm/src/commands/ack.rs`
- `crates/atm-graft-python/src/lib.rs`
- `crates/atm-daemon/src/https_transport.rs` (remote-ingress stripping only)

No changes to `crates/atm-daemon/src/local_ipc_transport/request_worker.rs`
or `crates/atm-daemon/src/local_tcp_transport.rs` — the unified HTTP
framing passes the JSON body through unchanged.

## Interfaces To Add Or Modify

- `WriteRequest` and `ReadQuery` gain
  `pub activity_observation: Option<ActivityObservation>` with
  `#[serde(default, skip_serializing_if = "Option::is_none")]`.
  It is transient request metadata, consumed only by the trusted observation
  merge after successful local dispatch; it must not enter a `mail_messages`
  row or message payload.
- `SendCommand::build_request()` copies the optional observation from
  `CallerContext`.
- The `read` command copies it from `CallerContext` when
  constructing `ReadQuery`
- The `ack` command copies it from `CallerContext` when
  constructing its write request
- `PyGraftSession` retains its existing caller argument for normal graft
  behavior, but read/send/ack obtain observation only through AJ.2's shared
  resolver against that resolved caller. Args-only graft sessions and an
  environment mismatch serialize no observation; the Python address is never
  copied directly into telemetry.
- HTTPS peer ingress clears `activity_observation` before it reaches the
  shared router. Remote peer traffic can never update local observation.

## Deliverables

- CLI and graft transmit `activity_observation` only when a trusted environment
  identity/team attests the caller; otherwise the field is omitted entirely
  (no `null` literals, no empty strings)
- Wire payloads remain readable by older daemon builds — the new fields
  are strictly additive
- Payloads flow identically over UDS and TCP because both transports use
  `HttpFrameReader` and dispatch through the same `ApiRouter`
- No CLI flag is added in this sprint; values come from env only
- No code path inspects observation telemetry to alter CLI/graft behavior

## Required Validation

- `cargo build --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test -p atm-core -p atm -p atm-graft-python -p atm-daemon`
- New integration test: serialize `WriteRequest`/`ReadQuery` with observation
  `None` → JSON contains no `activity_observation`; with `Some(...)` → JSON
  contains the nested team/member/session/pid values
- Manual smoke over UDS: `ATM_SESSION_ID=smoke-1 ATM_PID=$$ atm send ...`
  against a dev daemon and confirm receipt in daemon trace log
- Manual smoke over TCP loopback: same env against a daemon started with
  TCP enabled; confirm identical receipt in trace log (UDS/TCP parity)
- Regression test: an inbound HTTPS `WriteRequest` with a forged observation
  reaches normal write handling but cannot update `RuntimeStatusCache`.
- Storage regression test: serializing the durable mail row/payload after a
  local write proves it contains no `activity_observation`, session id, or pid
  telemetry from the request DTO.
- `rg -n "activity_observation" crates/atm-core/src/send/mod.rs crates/atm-core/src/read/mod.rs crates/atm-daemon/src/https_transport.rs`
  shows the additive field and remote-ingress clearing boundary
- `git diff --check`

## Acceptance Criteria

- request-capture tests prove CLI and graft only transmit telemetry when env
  identity/team match any corresponding arguments; UDS/TCP use identical DTOs
  and remote HTTPS cannot carry observation into the cache.
- AJ.3 must_follow AJ.2 under the merge-forward and PR-completion rule in the
  phase plan.
