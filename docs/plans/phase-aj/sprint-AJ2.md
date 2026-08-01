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

Extend `CallerContext` with optional `session_id` and `pid` fields resolved
from `ATM_SESSION_ID` / `ATM_PID` env vars so the CLI commands in AJ.3 have
a single source of caller-side observational state.

## Hard Dependencies

- AJ.1 merged forward into this branch
- `integrate/phase-AJ` at the Phase AJ entry-gate SHA
- `docs/plans/phase-aj/plan-phase-aj.md`
- `docs/plans/phase-aj/phase-aj-research.md`
- `crates/atm-core/src/caller_context.rs` baseline

## Exact Targets

- `crates/atm-core/src/caller_context.rs`
- `crates/atm-core/src/lib.rs` (re-exports if needed)

## Interfaces To Add Or Modify

- `CallerContext` gains:
  - `pub session_id: Option<SessionId>`
  - `pub pid: Option<u32>`
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
- `resolve_cli_send_caller_context()`,
  `resolve_cli_read_caller_context()`, and
  `resolve_cli_ack_caller_context()` populate the new fields by calling
  the two resolvers above (no per-command overrides in this sprint)

## Deliverables

- `CallerContext` carries both new optional fields through every existing
  resolver
- Env resolvers are pure — no process-state probing, no `/proc` reads, and no
  fallback to `std::process::id()` in any AJ caller path.
- Unit tests cover: env unset → `None`; env empty → `None`; env set to a
  valid value → `Some(...)`; `ATM_PID` set to non-numeric → `None`
- No behavioral branching on the presence of these fields anywhere in
  `atm-core`

## Acceptance Criteria

- observation is present only when environment identity/team are present and
  match the resolved command identity/team; args-only or mismatch is `None`
  without altering command behavior.
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
