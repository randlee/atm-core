# Sprint S.11 — host_log_dir Override-First Order Fix

**Branch**: feature/pS-s11-log-dir-override-order  
**Base**: integrate/phase-S @ c61d090  
**PR target**: integrate/phase-S  
**Status**: Implementation

## Goal

Fix `host_log_dir()` in `crates/atm-core/src/home.rs` to check `ATM_LOG_DIR` before resolving the OS home directory. Currently the function resolves `home_dir()` first and only then checks the env override, breaking the override-first contract for headless/service environments where `HOME` may be absent or irrelevant.

## Required Work

### 1. Fix override-first order in host_log_dir()

`crates/atm-core/src/home.rs` — reorder the resolution logic:

1. Check `ATM_LOG_DIR` env var first
2. If set and valid: return it immediately (no home_dir() call needed)
3. Only call `home_dir()` when `ATM_LOG_DIR` is absent or empty

This matches the contract implied by ADR-011 and required by headless daemon deployments where `HOME` is not set.

ADR-011 now states this ordering explicitly: `ATM_LOG_DIR` is evaluated before
any OS home-directory resolution, and a valid override bypasses `home_dir()`
entirely.

### 2. Update/add tests

Verify the new order with tests:
- `ATM_LOG_DIR` set to valid absolute path → returned without calling home_dir()
- `ATM_LOG_DIR` unset → falls through to home_dir()-based resolution (existing behavior)
- `ATM_LOG_DIR` set to non-absolute path → rejected (existing validation preserved)
- Headless case: `HOME` unset + `ATM_LOG_DIR` set → succeeds

### 3. Windows cfg-gate hygiene

Any new test helpers that use `LocalEnvGuard` or env manipulation must follow the established pattern from home.rs: unix-only constructs gated with `#[cfg(unix)]`, cross-platform constructs ungated.

### 4. ADR-011 overlap-check scope

Overlap checks against `~/.atm/daemon/` and `~/.claude/` are out of scope for
S.11 when `ATM_LOG_DIR` is set. Enforcing those exclusions requires resolving
the OS home directory, which defeats the headless override-first contract this
sprint restores. ADR-011 records that accepted scope boundary explicitly.

## Acceptance Criteria

- `ATM_LOG_DIR` override checked before `home_dir()` resolution
- Headless/service environments with `ATM_LOG_DIR` set but no `HOME` work correctly
- All existing `host_log_dir` tests continue to pass
- `just lint` PASS
- `cargo test -p atm-core` PASS
- `cargo xwin check --workspace --target x86_64-pc-windows-msvc` PASS

CI note:
- PR `#224` was green at `34bf2eb`, satisfying the sprint's Windows `cargo xwin check` acceptance evidence for the pre-fix branch head.

## References

- `crates/atm-core/src/home.rs` — `host_log_dir()` implementation
- `docs/adr/ADR-011-host-scoped-retained-log-root.md` — override contract
- `docs/adr/ADR-011-host-scoped-retained-log-root.md` — override-first order and overlap-check scope amendment
- `docs/atm-daemon/requirements.md` — daemon observability requirements
- `docs/cross-platform-guidelines.md` — Windows cfg-gate patterns
