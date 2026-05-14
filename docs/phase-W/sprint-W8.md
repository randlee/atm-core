---
id: W.8
title: Phase W Closeout Gaps
status: complete
branch: feature/pW-s8-phase-w-closeout
worktree: ../atm-core-worktrees/feature/pW-s8-phase-w-closeout
---

# Sprint W.8 — Phase W Closeout Gaps

## Goals

- close the remaining plan-audit gaps left after the original `W.8`
  implementation landed
- make the Phase `W` sequence, deliverables, and project-plan status block
  reflect the actual SQLite error-contract follow-through on this branch
- finish the typed SQLite subsystem identity closeout without reverting back to
  a raw retained-log subsystem string

## Implementation Notes

- `docs/plan-phase-W.md` now records `W.8` in the authoritative sprint
  sequence and deliverables list
- `docs/project-plan.md` section 27 now carries the `W.8` pending-merge status
  entry for `feature/pW-s8-phase-w-closeout` / PR `#277`
- `docs/project-plan.md` execution shape now includes both:
  - `W.7` Phase `W` carry-forward triage loop and phase closeout record
  - `W.8` phase closeout: typed SQLite subsystem identity and ATM error
    inventory correction
- this sprint closes the gap called out in
  `docs/phase-W/sprint-W1.md` lines 75-78:
  `DaemonSubsystem::Sqlite` was added and
  `emit_subsystem_event(...)` was tightened from `&'static str` subsystem ids
  to typed `DaemonSubsystem` on the retained-log path
- `SubsystemObservability::event(...)` remains an internal literal-based
  convenience builder; `W.8` documents that distinction rather than widening
  the typed boundary unnecessarily
- `TestDaemonObservability` now documents the `Mutex<Vec<String>>` +
  `Condvar` pairing so the shared lock discipline is explicit in test support

## Acceptance Criteria

- `docs/phase-W/sprint-W8.md` exists with goals, implementation notes, and
  acceptance criteria
- `docs/plan-phase-W.md` includes `W.8` in the authoritative sprint sequence
  and deliverables list
- `docs/project-plan.md` section 27 records the merged `W.7` status and the
  pending `W.8` status
- `docs/project-plan.md` section 27 execution shape includes both `W.7` and
  `W.8`
- the shared ATM error inventory includes
  `ATM_WARNING_SQLITE_HEALTH_DEGRADED` for degraded SQLite readiness
- the SQLite retained-log emission path uses typed
  `DaemonSubsystem::Sqlite`, not a raw subsystem string literal
- `SubsystemObservability::event(...)` documents why it remains an internal
  literal-based convenience builder distinct from the typed
  `emit_subsystem_event(...)` boundary
- `TestDaemonObservability` documents the `Mutex` + `Condvar` rationale for
  recorded retained-log messages
- `cargo build --workspace` passes
- `cargo test --workspace` passes
- `cargo clippy --workspace -- -D warnings` passes
- `cargo fmt --all --check` passes
- `git diff --check` passes
