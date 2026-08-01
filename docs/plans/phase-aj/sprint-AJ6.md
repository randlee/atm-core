---
id: AJ.6
title: Snapshot Surface And Integration Validation
status: planned
branch: feature/pAJ-s6-snapshot-and-validation
worktree: ../atm-core-worktrees/feature/pAJ-s6-snapshot-and-validation
target: integrate/phase-AJ
---

# Sprint AJ.6 — Snapshot Surface And Integration Validation

## Goal

Surface `session_id` on `RuntimeStatusSnapshot` so external consumers can
read it, and run the full-workspace validation that closes Phase AJ.

## Hard Dependencies

- AJ.1 through AJ.5 merged forward into this branch
- `integrate/phase-AJ` at the Phase AJ entry-gate SHA
- `docs/plans/phase-aj/plan-phase-aj.md`
- `docs/plans/phase-aj/phase-aj-research.md`

## Exact Targets

- `crates/atm-core/src/protocol.rs` (`RuntimeStatusSnapshot` per-member
  entry)
- `crates/atm-daemon/src/runtime_status_cache.rs` (`snapshot()` builder)
- `crates/atm/src/commands/members.rs` and its existing output projection
- `docs/plans/phase-aj/plan-phase-aj.md` (exit-criteria checkboxes)
- Sprint frontmatter status flips on AJ.1 through AJ.6

## Interfaces To Add Or Modify

- Per-member struct carried by `RuntimeStatusSnapshot` gains
  `pub session_id: Option<SessionId>` with
  `#[serde(default, skip_serializing_if = "Option::is_none")]`
- `RuntimeStatusCache::snapshot()` populates the new field from the
  cached entry state
- No pid surface on the snapshot in this phase — pid stays internal to
  the cache for now (deferral recorded in plan-phase-aj.md)
- Snapshot and `atm members` render source/timestamp only beside defined state
  or session values; default `Unknown`/absent-session remains hidden.
- `state_changed_at` is rendered only beside a defined non-`Unknown` state.
- `atm members` displays a member's observed `state` and `session_id` only when
  state is not `Unknown` or session is `Some`. Default `Unknown` with no
  session adds no field, line, or placeholder to existing roster output.

## Deliverables

- `RuntimeStatusSnapshot` includes `session_id` for every member that
  has one cached; members without a cached value omit the field on the
  wire
- Snapshot wire format remains backward compatible — older consumers
  ignore the new field
- Full end-to-end integration test exercising:
  1. CLI `atm send` over UDS with `ATM_SESSION_ID=s-end` and `ATM_PID=4242`
  2. CLI `atm read` over UDS with the same env
  3. HTTP heartbeat with `session_id: None`
  4. Snapshot read shows `session_id: Some("s-end")` for the identity
  5. CLI `atm ack` over TCP loopback with `ATM_SESSION_ID` unset
  6. Snapshot still shows `Some("s-end")` (non-overwrite rule holds
     across mixed paths and mixed transports)
- Static check that no source file in `crates/atm-core`,
  `crates/atm-daemon`, or `crates/atm` contains a behavioral branch on
  `session_id.is_some()` / `pid.is_some()` outside the cache-write
  helpers (grep audit recorded in the sprint close-out comment)

## Required Validation

- `cargo build --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo test -p atm-daemon snapshot_session_id`
- End-to-end test above passes on macOS and Linux CI
- Grep audit:
  - `rg -n "session_id\.is_some|pid\.is_some" crates/atm-core crates/atm-daemon crates/atm`
    returns hits only inside `runtime_status_cache.rs` write helpers
  - any other hit is a defect and must be removed before close
- Transport audit: `rg -n "touch_member" crates/atm-daemon/src/` shows
  the call site inside `runtime_health.rs` only — no per-transport touch
  sites under `local_ipc_transport/` or in `local_tcp_transport.rs`
- All six sprint docs have `status: complete` in frontmatter
- `plan-phase-aj.md` exit criteria all checked off
- `git diff --check`

## Acceptance Criteria

- snapshot evolution is additive: retain `member_counts`, all current fields,
  and `CLI_SCHEMA_VERSION`; do not surface pid.
- `atm members` fixture tests prove default observations are omitted and a
  defined state/session is rendered for the correct roster member.
- AJ.6 must_follow AJ.5 under the merge-forward and PR-completion rule in the
  phase plan.
