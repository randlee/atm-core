---
id: AJ.2
title: CallerContext Env Resolution
status: complete
branch: feature/pAJ-s2-caller-context-env
worktree: ../atm-core-worktrees/feature/pAJ-s2-caller-context-env
target: integrate/phase-aj
---

# Sprint AJ.2 — CallerContext Env Resolution

## Goal

Add one optional environment-attested `ActivityObservation` to
`CallerContext` so AJ.3 can carry caller-side observation without treating
command arguments as trusted telemetry.

## Hard Dependencies

- AJ.1 merged forward into this branch
- `integrate/phase-ai-31-33 @ 150391ecdf2e003185bff7d78427cd21509a7981`
- Completed Phase-AI reconciliation gate; `integrate/phase-aj` was cut from
  the recorded post-merge `develop` SHA before AJ.1 and AJ.2 begin
- `docs/plans/phase-aj/plan-phase-aj.md`
- `docs/plans/phase-aj/phase-aj-research.md`
- `crates/atm-core/src/caller_context.rs` baseline

## Dependency Relation

- `must_follow` AJ.1 because `ActivityObservation` uses its `SessionId` type.
- No AJ sprint is `parallel_safe`: AJ.3 consumes this caller contract and later
  sprints consume its wire result. AJ.2 begins immediately after AJ.1 → AJ.2
  merge-forward; it does not wait for AJ.1 QA. Repeat that merge before every
  AJ.2 dev/fix round; AJ.2's PR completes after AJ.1's PR merges.
- On AJ.2 development-head push, AJ.3 begins immediately by merging
  AJ.2 → AJ.3; AJ.3 must complete that merge before any dev/fix round and does
  not wait for AJ.2 QA.

## Exact Targets

- `crates/atm-core/src/caller_context.rs`
- `crates/atm-core/src/protocol.rs`
- `.just/lint-config.toml` (the environment-reader boundary allowlist)

## Interfaces To Add Or Modify

- Add `ActivityObservation`, deriving `Debug`, `Clone`, `Serialize`,
  `Deserialize`, `PartialEq`, and `Eq`:
  ```rust
  pub struct ActivityObservation {
      pub team: TeamName,
      pub member: AgentName,
      #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "deserialize_optional_session_id")]
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
  `ATM_SESSION_ID` via `std::env::var_os`; missing, non-Unicode, blank, or
  over-256-byte input is `None`. It calls `SessionId::new` and emits only the
  permitted informational suppression diagnostic for invalid optional
  telemetry.
- New public function
  `pub fn read_cli_pid_from_env() -> Option<u32>` — reads `ATM_PID`;
  missing, non-Unicode, parse failure, or empty input yields `None`. Validation rules: zero
  and negative values are rejected (yield `None`); the valid range is
  `1..=u32::MAX`. (`pid: 0` is a real PID on some systems but is never
  a legitimate caller-supplied observational value here.) If tighter
  platform-specific validation is ever needed (e.g. checking the
  process actually exists), it belongs in a later phase, not in this
  resolver.
- Add `ATM_SESSION_ID` and `ATM_PID` to
  `.just/lint-config.toml`'s `env_var_boundary.forbidden_env_vars`, and add
  only `read_cli_session_id_from_env` and `read_cli_pid_from_env` to the
  boundary-reader allowlist. The comment must state that these are the sole
  `atm-core` readers because they construct transient, environment-attested
  caller metadata; all other reads remain lint violations.
- `ActivityObservation.pid` is optional local caller metadata only. Its
  required-heartbeat counterpart remains `TeamMemberHeartbeatRequest.pid` in
  `crates/atm-core/src/protocol.rs`; the overwrite asymmetry is implemented
  only by AJ.4's `merge_observation`, never by either env resolver.
- Add one shared, non-fallible
  `activity_observation_for_resolved_caller(&AgentName, &TeamName) -> Option<ActivityObservation>`.
  It reads raw environment identity/team with `var_os`, parses them locally,
  compares them to the already resolved caller, and returns `None` for absent,
  non-Unicode, malformed, args-only, or mismatched identity/team. It must not
  call a fallible caller resolver or turn optional telemetry into an `AtmError`.
  When identity/team match, it returns `Some` even if session and pid are both
  absent: the attested command still establishes `Active` later.
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
- A resolver test sets malformed/non-Unicode metadata where the platform test
  helper permits it and proves telemetry becomes `None` without adding a new
  caller-resolution error.
- No behavioral branching on the presence of this DTO anywhere in
  `atm-core`
- The environment-boundary lint has positive tests for both new approved
  readers and a negative test proving a direct `ATM_SESSION_ID` or `ATM_PID`
  read outside those functions fails.

## Acceptance Criteria

- observation is present only when environment identity/team are present and
  match the resolved command identity/team; args-only or mismatch is `None`
  without altering command behavior. The comparison is write-side telemetry
  attestation only; it does not replace existing command validation.
- AJ.2 must_follow AJ.1: begin immediately after AJ.1 → AJ.2 merge-forward,
  repeat it before every dev/fix round, do not wait for AJ.1 QA, and complete
  AJ.2's PR only after AJ.1's PR merges.

## Required Validation

- `cargo build -p agent-team-mail-core`
- `cargo clippy -p agent-team-mail-core --all-targets -- -D warnings`
- `cargo test -p agent-team-mail-core caller_context`
- New unit tests use `temp-env` (or an equivalent per-command environment
  fixture) and never mutate process-global environment through
  `std::env::set_var`, avoiding parallel-test pollution
- `rg -n "ATM_SESSION_ID|ATM_PID" crates/atm-core/src/caller_context.rs`
  shows both readers
- `git diff --check`
