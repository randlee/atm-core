---
id: AJ.5
title: HTTP Heartbeat Session State
status: planned
branch: feature/pAJ-s5-heartbeat-session
worktree: ../atm-core-worktrees/feature/pAJ-s5-heartbeat-session
target: integrate/phase-AJ
---

# Sprint AJ.5 — HTTP Heartbeat Session State

## Goal

Extend the heartbeat ingestion path so `POST /v1/atm/heartbeat` records
`session_id` into `RuntimeStatusCache` under the same non-overwrite rule
used by the local dispatch path.

## Hard Dependencies

- AJ.1 and AJ.4 merged forward into this branch
- `integrate/phase-AJ` at the Phase AJ entry-gate SHA
- `docs/plans/phase-aj/plan-phase-aj.md`
- `docs/plans/phase-aj/phase-aj-research.md`
- `crates/atm-core/src/api.rs` baseline (`HEARTBEAT_PATH` unchanged)
- `crates/atm-daemon/src/runtime_health.rs` baseline (`record_heartbeat`)

## Exact Targets

- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-daemon/src/runtime_status_cache.rs` (only if AJ.4 did not
  already add the `session_id` parameter to `record_heartbeat`)

## Interfaces To Add Or Modify

- `record_heartbeat()` accepts the `session_id` field from
  `TeamMemberHeartbeatRequest` and applies it to the cache by calling
  `touch_member(identity, session_id, Some(pid))` — the exact same write
  path used by the local dispatch side. The non-overwrite gating for
  `session_id` lives in `touch_member` only; `record_heartbeat` does not
  duplicate the Some/None logic inline. Note the pid argument is always
  `Some(...)` here because `TeamMemberHeartbeatRequest.pid` is a
  required `u32` and heartbeat is the canonical liveness authority —
  see "Pid Overwrite Policy" in `plan-phase-aj.md`.
- `HeartbeatActivity` semantics are unchanged
- `TeamMemberHeartbeatResponse` echoes the post-update cached
  `session_id` per the authoritative response-semantics contract stated
  in `sprint-AJ1.md` (Interfaces To Add Or Modify). AJ.5 implements
  that contract; it does not redefine it.
- No new HTTP route, no new handler — this sprint extends the existing
  heartbeat handler only

## Deliverables

- A heartbeat with `session_id: Some(...)` updates the cached value
- A heartbeat with `session_id: None` leaves the cached value untouched
- A heartbeat with `session_id: None` against an empty cache leaves the
  cached value as `None`
- Missing optional heartbeat telemetry must not reset known state/session or
  their provenance; only `reset_member_observation` may do so.
- Only a `SessionEnded` heartbeat may set `Offline`; missing telemetry and
  reset-to-default never mean `Offline`.
- The heartbeat response carries the post-update cached `session_id`
- A local dispatch (UDS or TCP) carrying `Some` followed by a heartbeat
  carrying `None` leaves the dispatch-supplied value visible in
  subsequent heartbeats (proves convergence on one cache entry per
  roster member across all three ingestion paths)
- No behavioral branching on `session_id` anywhere in the heartbeat path
- A heartbeat state/session change records `Heartbeat` provenance; `None`
  session input preserves both prior value and prior provenance.

## Required Validation

- `cargo build --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test -p atm-daemon`
- New integration test `heartbeat_session_id_round_trip`:
  - POST heartbeat with `session_id: Some("s-1")` → response shows
    `Some("s-1")`
  - POST heartbeat with `session_id: None` → response still shows
    `Some("s-1")`
  - POST heartbeat with `session_id: Some("s-2")` → response shows
    `Some("s-2")`
- New integration test `heartbeat_and_local_dispatch_converge_on_cache`
  exercises a UDS send followed by an HTTP heartbeat against the same
  identity; repeat with TCP send to confirm parity
- `rg -n "session_id" crates/atm-daemon/src/runtime_health.rs` shows
  the new flow
- `git diff --check`

## Acceptance Criteria

- heartbeat merges telemetry without adding a business decision; its response
  returns the post-update cached session while preserving existing fields.
- AJ.5 must_follow AJ.4 under the merge-forward and PR-completion rule in the
  phase plan.
