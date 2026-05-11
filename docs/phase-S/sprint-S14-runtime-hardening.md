# Sprint S.14 Runtime Hardening

**Branch**: `feature/pS-s14-impl`  
**Base**: `feature/pS-s13-ipc-hardening`  
**PR target**: `integrate/phase-S`  
**Status**: Implementation

## Goal

Implement the S.14 runtime-hardening plan from
`docs/phase-S/sprint-S14-runtime-plan.md` and close the remaining daemon
runtime shutdown, bounded-state, observability, and anti-flake gaps after S.13.

## Scope

This sprint hardens:
- lifecycle wake-worker ownership and teardown
- reconcile/watch shutdown failure semantics
- runtime status-cache bounded retention
- retained-observability flush and write-path behavior
- daemon runtime test cleanup and anti-flake compliance

## Required Code Targets

- `crates/atm-daemon/src/lifecycle_control.rs`
- `crates/atm-daemon/src/reconcile_runtime.rs`
- `crates/atm-daemon/src/watch_runtime.rs`
- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-daemon/src/daemon_observability.rs`
- `crates/atm-daemon/src/test_support.rs`
- `crates/atm-daemon/src/tests.rs`

## Closeout Requirements

- keep bounded shutdown timeouts observable as typed failures
- keep retained runtime state actually bounded in memory
- keep remaining accepted detach/open-per-write exceptions documented inline
- keep daemon runtime tests compliant with `docs/plan-phase-S.md` §4.1
- merge-forward accepted S.14 fixes into `feature/pS-s15-rusqlite-hardening`

## Validation

- `cargo test -p atm-daemon`
- `cargo xwin check --workspace --target x86_64-pc-windows-msvc`
- `just lint`
- `cargo fmt --all --check`

## References

- `docs/phase-S/sprint-S14.md`
- `docs/phase-S/sprint-S14-runtime-plan.md`
- `docs/plan-phase-S.md`
- `docs/atm-daemon/architecture.md`
