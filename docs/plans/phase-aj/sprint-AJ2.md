---
id: AJ.2
title: CallerContext Env Resolution
status: planned
branch: feature/pAJ-s2-caller-context-env
worktree: ../atm-core-worktrees/feature/pAJ-s2-caller-context-env
target: integrate/phase-AJ
---

# Sprint AJ.2 — CallerContext Env Resolution

## Goal

Add one optional environment-attested `ActivityObservation` to
`CallerContext` so AJ.3 can carry caller-side observation without treating
command arguments as trusted telemetry.

## Hard Dependencies

- AJ.1 merged forward into this branch
- `integrate/phase-AJ` at the Phase AJ entry-gate SHA
- `docs/plans/phase-aj/plan-phase-aj.md`
- `docs/plans/phase-aj/phase-aj-research.md`
- `crates/atm-core/src/caller_context.rs` baseline

## Dependency Relation

- `must_follow` AJ.1 because `ActivityObservation` uses its `SessionId` type.
- No AJ sprint is `parallel_safe`: AJ.3 consumes this caller contract and later
  sprints consume its wire result. Merge AJ.1 → AJ.2 before every dev/fix round;
  AJ.2's PR completes after AJ.1's PR, but development need not wait for QA.

## Exact Targets

- `crates/atm-core/src/caller_context.rs`
- `crates/atm-core/src/protocol.rs`
- `crates/atm-core/src/lib.rs` (re-exports if needed)

## Interfaces To Add Or Modify

- Add `ActivityObservation`, deriving `Debug`, `Clone`, `Serialize`,
  `Deserialize`, `PartialEq`, and `Eq`:
  ```rust
  pub struct ActivityObservation {
      pub team: TeamName,
      pub member: AgentName,
      #[serde(default, skip_serializing_if = "Option::is_none")]
      pub session_id: Option<SessionId>,
      #[serde(default, skip_serializing_if = "Option::is_none")]
      pub pid: Option<u32>,
  }
  ```
  It is transient observation metadata, not command identity and not mail
  persistence data.
- `CallerContext` gains `pub activity_observation: Option<ActivityObservation>`.
- New public function
  `pub fn read_cli_session_id_from_env() -> Option<SessionId>` — reads
  `ATM_SESSION_ID`; empty string is treated as `None`; no validation of
  contents
- New public function
  `pub fn read_cli_pid_from_env() -> Option<u32>` — reads `ATM_PID`;
  parse failure or empty string yields `None`. Validation rules: zero
  and negative values are rejected (yield `None`); the valid range is
  `1..=u32::MAX`. (`pid: 0` is a real PID on some systems but is never
  a legitimate caller-supplied observational value here.) If tighter
  platform-specific validation is ever needed (e.g. checking the
  process actually exists), it belongs in a later phase, not in this
  resolver.
- Add one shared, non-fallible
  `activity_observation_for_resolved_caller(&AgentName, &TeamName) -> Option<ActivityObservation>`.
  It reads only environment identity/team, compares them to the already
  resolved caller, and returns `None` for absent, malformed, args-only, or
  mismatched identity/team. It must not turn optional telemetry into an
  `AtmError`. When identity/team match, it returns `Some` even if session and
  pid are both absent: the attested command still establishes `Active` later.
- `resolve_cli_inspection_caller_context()` constructs the optional observation
  from the shared resolver; mutation resolution inherits it through its
  existing delegation to inspection resolution. AJ.3's CLI callers copy that
  field; graft calls the same helper once against `PyGraftSession`'s resolved
  caller. Neither adds a per-command identity override.

## Deliverables

- `CallerContext` carries one `Some(ActivityObservation)` only when parsed
  `ATM_IDENTITY` and `ATM_TEAM` match the resolved command identity/team.
  Args-only and mismatched invocations retain command behavior and yield `None`;
  an `info!` anomaly is allowed.
- Env resolvers are pure — no process-state probing, no `/proc` reads, and no
  fallback to `std::process::id()` in any AJ caller path.
- Unit tests cover: env unset → `None`; env empty → `None`; env set to a
  valid value → `Some(...)`; `ATM_PID` set to non-numeric → `None`; matching
  command/environment creates one observation; args-only and mismatch create
  none while preserving the command's existing result/error contract.
- Unit tests also prove matching identity/team with no session/pid produces
  `Some(ActivityObservation { session_id: None, pid: None, .. })`; absence of
  metadata must not suppress the state observation.
- No behavioral branching on the presence of this DTO anywhere in
  `atm-core`

## Acceptance Criteria

- observation is present only when environment identity/team are present and
  match the resolved command identity/team; args-only or mismatch is `None`
  without altering command behavior. The comparison is write-side telemetry
  attestation only; it does not replace existing command validation.
- AJ.2 must_follow AJ.1: merge AJ.1 → AJ.2 before every dev/fix round; AJ.2
  PR completes after AJ.1 PR merges, while AJ.2 development need not await QA.

## Required Validation

- `cargo build -p atm-core`
- `cargo clippy -p atm-core --all-targets -- -D warnings`
- `cargo test -p atm-core caller_context`
- New unit tests use `temp-env` or serialized `std::env::set_var` to
  avoid cross-test pollution
- `rg -n "ATM_SESSION_ID|ATM_PID" crates/atm-core/src/caller_context.rs`
  shows both readers
- `git diff --check`
