---
id: AJ.5
title: HTTP Heartbeat Session State
status: complete
implementation_status: complete
phase_closeout: pending-parent-pr-merges
branch: feature/pAJ-s5-heartbeat-session
worktree: ../atm-core-worktrees/feature/pAJ-s5-heartbeat-session
target: integrate/phase-aj
---

# Sprint AJ.5 — HTTP Heartbeat Session State

## Goal

Extend the heartbeat ingestion path so `POST /v1/atm/heartbeat` records
`session_id` into `RuntimeStatusCache` under the same non-overwrite rule
used by the local dispatch path.

Re-run AJ.4's `runtime_health.rs` production-line checkpoint after merge. If
the heartbeat addition would exceed the 1,000-line ceiling, extract the
existing heartbeat adapter into
`crates/atm-daemon/src/runtime_health/heartbeat.rs` before adding behavior;
never waive or exclude the line-count rule.

## Hard Dependencies

- AJ.1 through AJ.4 merged forward into this branch
- `integrate/phase-ai-31-33 @ 150391ecdf2e003185bff7d78427cd21509a7981`
- Completed Phase-AI reconciliation gate; `integrate/phase-aj` was cut from
  the recorded post-merge `develop` SHA before AJ.1 and AJ.5 begin
- `docs/plans/phase-aj/plan-phase-aj.md`
- `docs/plans/phase-aj/phase-aj-research.md`
- `crates/atm-core/src/api.rs` baseline (`HEARTBEAT_PATH` unchanged)
- `crates/atm-daemon/src/runtime_health.rs` baseline (`record_heartbeat`)

## Dependency Relation

- `must_follow` AJ.4 because heartbeat calls its only cache merge function and
  consumes its merge outcome.
- No AJ sprint is `parallel_safe`: AJ.6 exposes this converged heartbeat/cache
  result. AJ.5 begins immediately after AJ.4 → AJ.5 merge-forward; it does not
  wait for AJ.4 QA. Repeat that merge before every AJ.5 dev/fix round; AJ.5's
  PR completes after AJ.4's PR merges.
- On AJ.5 development-head push, AJ.6 begins immediately by merging
  AJ.5 → AJ.6; AJ.6 must complete that merge before any dev/fix round and does
  not wait for AJ.5 QA.

## Exact Targets

- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-daemon/src/runtime_health/heartbeat.rs` (split contingency when
  the LOC checkpoint requires it)
- `crates/atm-daemon/src/runtime_status_cache.rs`
- `crates/atm-daemon/src/tests.rs`
- `docs/atm-daemon/boundaries.md`

## Interfaces To Add Or Modify

- `record_heartbeat()` accepts `session_id` from
  `TeamMemberHeartbeatRequest` and calls AJ.4's shared
  `merge_observation(..., RuntimeObservationSource::Heartbeat, ...)`. The
  helper owns `Some`/`None` merge rules; `record_heartbeat` only maps the
  heartbeat activity to its lifecycle state. Its required pid becomes the
  current pid — see "Pid Overwrite Policy" in `plan-phase-aj.md`.
- `TeamMemberHeartbeatResponse.pid_changed` retains its existing wire field
  and means only replacement of a prior defined pid by a different defined
  pid. Initial pid observation is recorded and audited but is not a
  replacement. The value comes from AJ.4's `ObservationMergeOutcome`, never
  from a process-liveness guard.
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
  their provenance.
- Only a `SessionEnded` heartbeat may set `Offline`; missing telemetry and
  any default/absent value never mean `Offline`.
- Heartbeat activity transitions `ActiveToolUse` → `Active`, `Idle` → `Idle`,
  and `SessionEnded` → `Offline`; each transition records heartbeat provenance.
  `SessionEnded` retains the last known session id when its request omits one.
- Contract test fixtures represent hook emission exactly: startup/active uses
  `ActiveToolUse`, idle uses `Idle`, and stop uses `SessionEnded`. This sprint
  does not modify hook-side code.
- A changed heartbeat pid/session emits retained structured evidence, updates
  the current observation, and does not reject the heartbeat, change lifecycle
  state, degrade readiness, or trigger a cache policy. Doctor aggregation is
  explicitly future scope.
- The heartbeat response carries the post-update cached `session_id`
- A local dispatch (UDS or TCP) carrying `Some` followed by a heartbeat
  carrying `None` leaves the dispatch-supplied value visible in
  subsequent heartbeats (proves convergence on one cache entry per
  roster member across all three ingestion paths)
- No behavioral branching on `session_id` anywhere in the heartbeat path
- A heartbeat state/session change records `Heartbeat` provenance; `None`
  session input preserves both prior value and prior provenance.
- A heartbeat session/pid mutation uses the same required one-event audit
  contract as local ingress.
- Update `docs/atm-daemon/boundaries.md` for downstream alerting consumers:
  removing conflict-driven `Degraded` readiness is intentional because it was
  policy, not health; the replacement is the non-authoritative retained
  `runtime_observation_metadata_changed` event, not a new readiness signal or
  doctor aggregate.

## Paths To Delete

- In `crates/atm-daemon/src/runtime_health.rs`, delete `record_heartbeat`'s
  `process_is_alive` guard, `record_identity_conflict` call, and only its
  `return Err(AtmError::identity_conflict(...))` statement. The shared
  `AtmError::identity_conflict()` constructor remains for unrelated
  identity-resolution callers outside this sprint.
- In `crates/atm-daemon/src/runtime_status_cache.rs`, delete
  `RuntimeStatusCache::record_identity_conflict` and
  `record_identity_conflict_for_test`.
- In `runtime_status_cache.rs`, remove the `IdentityConflict` projection
  special case and its `conflict_count`-driven `Degraded` readiness branch and
  detail string `admin takeover or dead-pid retry` from
  `finish_runtime_snapshot`; remove `IdentityConflict` eviction prioritization
  from `evict_status_cache_entry_if_needed`; and remove its special projection
  handling from `build_runtime_snapshot_all` and
  `build_runtime_snapshot_scoped`. Keep an exhaustive match arm that buckets a
  deserialized-only `IdentityConflict` as `unknown_members`, preserving wire
  backwards compatibility.
- Remove or adapt the retired-behavior tests: the live-pid conflict rejection
  test, `heartbeat_retries_identity_conflict_after_old_pid_dies`,
  `identity_conflict_insert_evicts_oldest_conflict_when_cache_is_full`, and
  `doctor_projects_degraded_runtime_when_member_identity_conflicts_exist`.

The replacement is the ordinary `merge_observation` result and retained audit
evidence; no ATM routing, retry, admission, delivery, nudge, or
session-rejection behavior may consume a PID/session conflict.

## Required Validation

- `cargo build --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test -p atm-daemon`
- `just lint` proves `runtime_health.rs` remains at or below the production
  line ceiling after the checkpoint/split
- New integration test `heartbeat_session_id_round_trip`:
  - POST heartbeat with `session_id: Some("s-1")` → response shows
    `Some("s-1")`
  - POST heartbeat with `session_id: None` → response still shows
    `Some("s-1")`
  - POST heartbeat with `session_id: Some("s-2")` → response shows
    `Some("s-2")`
- New regression test `session_ended_preserves_last_known_session`:
  `ActiveToolUse` with `Some("s-1")`, then `SessionEnded` with `None`, yields
  `Offline` plus `Some("s-1")`.
- New integration test `heartbeat_and_local_dispatch_converge_on_cache`
  exercises a UDS send followed by an HTTP heartbeat against the same
  identity; repeat with TCP send to confirm parity
- Regression test: a prior pid plus a heartbeat with a different pid
  succeeds, updates the current pid, retains one change event, and never emits
  `IdentityConflict`.
- `rg -n "merge_observation|record_identity_conflict|process_is_alive" crates/atm-daemon/src/runtime_health.rs crates/atm-daemon/src/runtime_status_cache.rs`
  shows the shared merge flow and no live-pid conflict producer or guard
- `rg -n "record_identity_conflict_for_test|AtmErrorCode::IdentityConflict" crates/atm-daemon/src/tests.rs`
  returns no matches
- `rg -n "conflict_count|admin takeover or dead-pid retry" crates/atm-daemon/src/runtime_status_cache.rs`
  returns no matches
- `git diff --check`

## Acceptance Criteria

- heartbeat merges telemetry without adding a business decision; its response
  returns the post-update cached session while preserving existing fields.
- AJ.5 must_follow AJ.4 under the merge-forward and PR-completion rule in the
  phase plan.
