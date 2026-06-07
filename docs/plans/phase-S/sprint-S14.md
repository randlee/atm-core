# Sprint S.14 — Daemon Runtime Hardening Plan

**Branch**: feature/pS-s14-runtime-hardening  
**Base**: integrate/phase-S @ 77badd5  
**PR target**: integrate/phase-S  
**Status**: Planning

## Goal

Produce the implementation plan for the next `atm-daemon` runtime-hardening
pass. S.14 is the follow-on sprint after S.13 IPC hardening: it closes the
remaining shutdown-ownership, bounded-state, doctor-projection, and retained
observability gaps that are still open across lifecycle control, reconcile,
watch, runtime health, and daemon observability.

S.0-S.4 remain fully closed on the Phase S line, including the temporary
Windows lint-guardrail removal recorded in `docs/plans/phase-S/plan-phase-S.md` §6.

`docs/plans/phase-S/sprint-S14-runtime-plan.md` is the authoritative design document
for this sprint.

## Required Work

### 1. Write the authoritative runtime plan

Add `docs/plans/phase-S/sprint-S14-runtime-plan.md` covering all S.14 inventory
findings:
- `S14-001` through `S14-011`
- exact file:line target for each finding
- concrete fix approach for each finding
- regression-test or documentation closeout for each finding

### 2. Keep the sprint brief aligned with the runtime plan

This sprint brief must stay consistent with the authoritative runtime plan and
the active daemon architecture:
- no implementation code in the planning sprint
- no reopening of S.13 transport-boundary decisions
- no storage-layer redesign mixed into daemon-runtime hardening
- no daemon-spawn test exceptions

### 3. Record the implementation scope for the follow-on fix sprint

The follow-on implementation worktree should target:
- `crates/atm-daemon/src/lifecycle_control.rs`
- `crates/atm-daemon/src/reconcile_runtime.rs`
- `crates/atm-daemon/src/watch_runtime.rs`
- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-daemon/src/daemon_observability.rs`
- targeted daemon runtime tests
- targeted daemon architecture documentation updates

## Acceptance Criteria

- `docs/plans/phase-S/sprint-S14.md` designates
  `docs/plans/phase-S/sprint-S14-runtime-plan.md` as authoritative
- `docs/plans/phase-S/sprint-S14-runtime-plan.md` covers all 11 S.14 findings with
  concrete fix approach and exact file:line target
- no implementation code is included in the planning sprint
- `just lint` PASS

## References

- `docs/plans/phase-S/sprint-S14-runtime-plan.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`
- `crates/atm-daemon/src/lifecycle_control.rs`
- `crates/atm-daemon/src/reconcile_runtime.rs`
- `crates/atm-daemon/src/watch_runtime.rs`
- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-daemon/src/daemon_observability.rs`
- `TASK-1224-PLAN`
