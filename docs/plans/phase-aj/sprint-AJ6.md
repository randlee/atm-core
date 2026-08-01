---
id: AJ.6
title: Snapshot Projection And Boundary Guard
status: planned
branch: feature/pAJ-s6-snapshot-boundary-guard
worktree: ../atm-core-worktrees/feature/pAJ-s6-snapshot-boundary-guard
target: integrate/phase-AJ
---

# Sprint AJ.6 — Snapshot Projection And Boundary Guard

## Goal

Implement the production snapshot and roster projection for current runtime
observation, plus its narrow source-use guard. AJ.6 does not reconcile
governing documents, reclassify boundary records, or close the phase; AJ.7 owns
those closure actions after this feature merges.

## Hard Dependencies

- AJ.1 through AJ.5 merged forward into this branch
- `integrate/phase-AJ` at the Phase AJ entry-gate SHA
- `docs/plans/phase-aj/plan-phase-aj.md`
- `docs/plans/phase-aj/phase-aj-research.md`

## Dependency Relation

- `must_follow` AJ.5 because this sprint exposes AJ.5's converged cache and
  heartbeat semantics.
- AJ.7 `must_follow`s AJ.6 because contract closeout must review this merged
  public surface, tests, and boundary guard rather than an intended shape.
- No AJ sprint is `parallel_safe`. Merge AJ.5 → AJ.6 before every dev/fix
  round; AJ.6's PR completes after AJ.5's PR. Development may start after the
  AJ.5 development commit is pushed and does not wait for parent QA.

## Exact Targets

- `crates/atm-core/src/protocol.rs` (`RuntimeMemberObservation` and additive
  `RuntimeStatusSnapshot.members`)
- `crates/atm-daemon/src/runtime_status_cache.rs` (`snapshot_for_members()`)
- `crates/atm/src/commands/members.rs` (human/JSON projection)
- `.just/tests/test_runtime_observation_boundary.py` (narrow source-use guard)

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
  roster member, retaining `Unknown` in structured output so counts and state
  remain unambiguous. `atm members` filters default observation from human
  output.
- Snapshot and `atm members` render source/timestamp only beside defined state
  or session values; default `Unknown`/absent-session remains hidden.
- `state_changed_at` is rendered only beside a defined non-`Unknown` state.
  Human roster output renders relative age from that timestamp (for example,
  `Idle — 30m`); structured output retains the absolute timestamp. Fixtures use
  a fixed clock, never wall-clock sleep.
- A human session display is its first 12 Unicode scalar values, followed by
  `…` only when longer; no human table may expose the full value.

## Deliverables

- `RuntimeStatusSnapshot` contains optional current session/pid for every
  cached member; absent values are omitted on the wire.
- Snapshot evolution is additive: existing fields, `member_counts`, and
  `CLI_SCHEMA_VERSION` remain intact; an older reader may ignore `members`.
- An end-to-end test exercises UDS send/read, heartbeat with absent session,
  and TCP-loopback ack. It proves one member retains its original session when
  a later trusted command omits it.
- Roster fixtures prove JSON preserves raw pid/session while human output
  shortens a session id, displays defined state age from a fixed clock, and
  omits default observation.
- `.just/tests/test_runtime_observation_boundary.py` is a narrow source-use
  guard: it allows DTO/caller construction, local request construction,
  HTTPS Write/Receive stripping, local dispatcher forwarding,
  `runtime_status_cache.rs` merge/snapshot, and roster projection; it rejects
  peer delivery, post-write routing, nudge, retry, admission, notification,
  and policy modules.
- The guard includes required-positive checks for `ActivityObservation`, both
  request DTO fields, `AckRequest` conversion, HTTPS Write/Receive stripping,
  one local dispatch merge, and snapshot projection. It fails at the Phase AJ
  entry baseline, preventing shape-only closure.

## Required Validation

- `cargo build -p atm-core -p atm-daemon -p atm`
- `cargo clippy -p atm-core -p atm-daemon -p atm --all-targets -- -D warnings`
- `cargo test -p atm-core -p atm-daemon -p atm`
- Run `.just/tests/test_runtime_observation_boundary.py` through its normal
  lint test registration.
- Run the UDS/TCP/heartbeat end-to-end test on macOS and Linux CI.
- Compatibility fixtures prove a new reader accepts a pre-AJ
  `RuntimeStatusSnapshot` without `members` and an older reader ignores the
  additive field.
- `git diff --check`

## Acceptance Criteria

- `atm members` fixture tests prove default observations are omitted and a
  defined state/session/pid is rendered for the correct roster member.
- The source-use guard passes only with the required AJ implementation present
  and rejects policy consumers of observation fields.
- AJ.6 must_follow AJ.5 under the merge-forward and PR-completion rule in the
  phase plan. AJ.7 must not begin a closeout/fix round without first merging
  AJ.6 into its branch, and AJ.7's PR cannot complete before AJ.6's PR merges.
