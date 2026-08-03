---
id: AJ.6
title: Runtime Observation Snapshot Projection
status: planned
branch: feature/pAJ-s6-runtime-observation-snapshot
worktree: ../atm-core-worktrees/feature/pAJ-s6-runtime-observation-snapshot
target: integrate/phase-AJ
---

# Sprint AJ.6 — Runtime Observation Snapshot Projection

## Goal

Implement the production `RuntimeStatusSnapshot` and `atm members` projection
for current runtime observation. This sprint owns only the user-visible
snapshot feature and its direct tests; AJ.7 owns the source-use guard.

## Hard Dependencies

- AJ.1 through AJ.5 merged forward into this branch
- `integrate/phase-ai-31-33 @ 150391ecdf2e003185bff7d78427cd21509a7981`
- Completed Phase-AI reconciliation gate; `integrate/phase-AJ` was cut from
  the recorded post-merge `develop` SHA before AJ.1 and AJ.6 begin
- `docs/plans/phase-aj/plan-phase-aj.md`
- `docs/plans/phase-aj/phase-aj-research.md`

## Dependency Relation

- `must_follow` AJ.5 because this sprint exposes AJ.5's converged cache and
  heartbeat semantics.
- AJ.7 `must_follow`s AJ.6 because its static guard inspects this implemented
  snapshot and roster projection.
- AJ.6 begins immediately after AJ.5 → AJ.6 merge-forward; it does not wait
  for AJ.5 QA. Repeat that merge before every AJ.6 dev/fix round. AJ.6's PR
  completes only after AJ.5's PR merges. No AJ pair is `parallel_safe`.
- On AJ.6 development-head push, AJ.7 begins immediately by merging
  AJ.6 → AJ.7; AJ.7 must complete that merge before any dev/fix round and does
  not wait for AJ.6 QA.

## Exact Targets

- `crates/atm-core/src/protocol.rs` (`RuntimeMemberObservation` and additive
  `RuntimeStatusSnapshot.members`)
- `crates/atm-daemon/src/runtime_status_cache.rs` (`snapshot_for_members()`)
- `crates/atm/src/commands/members.rs` (human/JSON projection)

## Interfaces To Add Or Modify

- Add public `RuntimeMemberObservation`, deriving `Debug`, `Clone`,
  `Serialize`, `Deserialize`, `PartialEq`, and `Eq`:
  ```rust
  pub struct RuntimeMemberObservation {
      pub team: TeamName,
      pub member: AgentName,
      pub state: RuntimeMemberState,
      pub session_id: Option<SessionId>,
      pub pid: Option<u32>,
      pub last_active_at: Option<IsoTimestamp>,
      pub state_changed_by: Option<RuntimeObservationSource>,
      pub state_changed_at: Option<IsoTimestamp>,
      pub session_changed_by: Option<RuntimeObservationSource>,
      pub session_changed_at: Option<IsoTimestamp>,
  }
  ```
  Optional fields use `#[serde(default, skip_serializing_if =
  "Option::is_none")]`; structured output contains raw session id and pid.
- `RuntimeStatusSnapshot` gains `members: Vec<RuntimeMemberObservation>` with
  `#[serde(default, skip_serializing_if = "Vec::is_empty")]`.
- `RuntimeStatusCache::snapshot_for_members()` returns one observation for each
  roster member, retaining `Unknown` in structured output. `atm members`
  omits default observation from human output.
- `crates/atm/src/commands/members.rs` owns one private
  `short_session_id_for_human(&SessionId)` helper. Human output renders state
  age only from `state_changed_at`, then pid and that helper's first 12 Unicode
  scalar values plus `…` only when longer; JSON retains raw values and absolute
  timestamps. Fixtures use a fixed clock.

## Deliverables

- Optional current session/pid are projected in snapshot members; existing
  snapshot fields, `member_counts`, and `CLI_SCHEMA_VERSION` remain intact.
- A UDS send/read, heartbeat-without-session, and TCP-loopback ack integration
  test proves the same member retains its known session when a later trusted
  command omits session metadata.
- Roster fixtures prove raw JSON, short human session display, state age, and
  omission of default `Unknown` observation.
- Compatibility fixtures prove a new reader accepts a pre-AJ snapshot without
  `members`; an older reader ignores the additive field.

## Required Validation

- `cargo build -p atm-core -p atm-daemon -p atm`
- `cargo clippy -p atm-core -p atm-daemon -p atm --all-targets -- -D warnings`
- `cargo test -p atm-core -p atm-daemon -p atm`
- Run the UDS/TCP/heartbeat integration test on macOS and Linux CI.
- `git diff --check`

## Acceptance Criteria

- `atm members` fixture tests prove the defined observation renders for the
  correct member and default observation renders nowhere in the human table.
- AJ.6 is production-ready only for its snapshot/roster feature; it does not
  claim source-use enforcement, boundary-record reclassification, governing
  document reconciliation, or phase closeout.
- AJ.6 must_follow AJ.5 under the merge-forward and PR-completion rule above.
