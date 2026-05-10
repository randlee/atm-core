# Sprint S.12 — Integration Gate Findings

**Branch**: feature/pS-s12-intg-gate-findings  
**Base**: integrate/phase-S @ c61d090  
**PR target**: integrate/phase-S  
**Status**: Implementation

## Goal

Resolve all 13 findings from the Phase-S integration gate review. Findings span flaky-test hardening (FTQ), missing recovery chains (RBP), and bounded-shutdown patterns (RSH) in `atm-daemon`.

## Findings

### Critical

**INTG-FTQ-001** `crates/atm-daemon/src/watch_runtime.rs:447`  
`request_for()` hardcodes `std::env::temp_dir().join("atm-watch-test")` as `home_dir` for all parallel test invocations → collision. Fix: allocate a per-invocation `TempDir` and pass its path.

### Important

**INTG-RSH-001** `crates/atm-daemon/src/reconcile_runtime.rs:235`  
`ReconcileRuntime::shutdown()` calls `handle.join()` unconditionally — same class as RSH-001/007 already fixed in S.9. Fix: bounded deadline with `drop(handle)` on timeout.

**INTG-RSH-002** `crates/atm-daemon/src/reconcile_runtime.rs:285`  
Waiter loop checks `completed` before `shutdown` flag — incorrect condition order. Fix: check `shutdown` first.

**INTG-RBP-001** `crates/atm-daemon/src/runtime_health.rs:763`  
Missing `.with_recovery()` on team-name parse error. Fix: add `.with_recovery("ensure team name is valid UTF-8")`.

**INTG-RBP-002** `crates/atm-daemon/src/runtime_health.rs:784`  
Missing `.with_recovery()` on serde_json parse error. Fix: add `.with_recovery("check atm message format")`.

**INTG-FTQ-005** `crates/atm-daemon/src/runtime_health.rs:44`  
Shared static `SHUTDOWN_FINALIZER_THREADS` accumulates across parallel tests. Fix: drain in test teardown or use per-test isolation.

### Minor

**INTG-FTQ-002** `crates/atm-daemon/src/watch_runtime.rs:582`  
Wall-clock assertion `elapsed < WATCH_SHUTDOWN_DEADLINE + 1s` → flaky on CI. Fix: remove the wall-clock assertion.

**INTG-FTQ-003** `crates/atm-daemon/src/watch_runtime.rs:600`  
50ms health timeout too tight for condvar spurious wakes. Fix: increase to ~500ms.

**INTG-FTQ-004** `crates/atm-daemon/src/reconcile_runtime.rs:532`  
TOCTOU race in coalescing test. Fix: remove the racy assertion or use explicit synchronization.

**INTG-FTQ-006** `crates/atm-core/src/home.rs`  
Drop-order issue in `host_log_dir_rejects_non_utf8_override` — env_lock released before `ATM_LOG_DIR` restored. Fix: reorder drops or use a single guard.

**INTG-FTQ-007** `crates/atm-daemon/src/reconcile_runtime.rs:784`  
Nondeterministic coalescing in duplicate notification test. Fix: use deterministic ordering or assert count only.

**INTG-RSH-003** `crates/atm-daemon/src/reconcile_runtime.rs`  
Pre-existing reconcile_runtime shutdown race — document intentional deferral if out of scope, otherwise apply same bounded-shutdown pattern.

**INTG-RSH-004** `crates/atm-daemon/src/reconcile_runtime.rs:471`  
Silent executor failures — no `tracing::warn!`. Fix: add warn! on executor task failure.

## Acceptance Criteria

- All 13 findings addressed (INTG-FTQ-001 critical first)
- `just lint` PASS
- `cargo test -p atm-daemon` PASS
- `cargo test -p atm-core` PASS (FTQ-006 is in atm-core)
- `cargo xwin check --workspace --target x86_64-pc-windows-msvc` PASS
- No new flaky tests introduced

## References

- `.triage/phase-S/findings/INTG-FTQ-{001..007}.ttl` — canonical finding records
- `.triage/phase-S/findings/INTG-RBP-{001..002}.ttl`
- `.triage/phase-S/findings/INTG-RSH-{001..004}.ttl`
- `docs/cross-platform-guidelines.md` — Windows cfg-gate patterns
- integrate/phase-S @ c61d090
