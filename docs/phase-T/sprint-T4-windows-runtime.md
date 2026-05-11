# Sprint T.4 Windows Runtime Parity

**Branch**: `integrate/phase-T`
**Base**: `integrate/phase-T @ bdac03c`
**PR target**: `develop`
**Status**: Planning

## Goal

Replace compile-only Windows confidence with real same-host daemon runtime proof
for local IPC, lifecycle control, singleton ownership, and retained-log
startup/shutdown.

## Deliverables

- add real Windows runtime coverage for:
  - same-host local IPC request/response on the real transport
  - daemon singleton admission and rejection
  - lifecycle terminate / bounded shutdown behavior
  - retained-log bootstrap and orderly shutdown on Windows
- audit and remove compile-only Windows test guards and cfg-gated runtime skips
  in the named daemon test files, replacing them with real runtime coverage
  rather than layering runtime tests on top of lingering compile-only stubs:
  - `crates/atm-daemon/src/tests.rs`
  - `crates/atm-daemon/src/local_ipc_transport.rs`
  - `crates/atm-daemon/src/test_observability.rs`
  - `crates/atm-daemon/src/test_support.rs`
- keep Unix parity tests intact while making the Windows runtime path a
  first-class execution lane
- document any remaining accepted Windows-specific runtime exception explicitly
  in daemon architecture docs rather than hiding it in test comments

## Key File Targets

- `crates/atm-daemon/src/tests.rs`
- `crates/atm-daemon/src/local_ipc_transport.rs`
- `crates/atm-daemon/src/lifecycle_control.rs`
- `crates/atm-daemon/src/host_ownership.rs`
- `crates/atm-daemon/src/test_support.rs`
- `docs/atm-daemon/architecture.md`
- `docs/requirements.md`
- `docs/plan-phase-S.md`

## Acceptance Criteria

- Windows parity is proven by runtime tests, not only by `cargo xwin check`
  (`REQ-P-PLATFORM-001`, `REQ-P-PLATFORM-002`)
- the same-host local IPC path, singleton path, and lifecycle shutdown path all
  have Windows runtime coverage
- retained-log startup/shutdown is exercised on Windows through the real daemon
  flow
- compile-only Windows runtime stubs in the named test files are removed or
  explicitly reduced to non-runtime helper scope
- any remaining platform exception is documented with explicit rationale

## QA Pointers

- `req-qa` must reject compile-only substitutes for runtime-proof deliverables
- `flaky-test-qa` should review the new Windows runtime coverage for bounded
  waits and anti-sleep discipline
- CI planning should include a Windows runtime lane, not only a Windows compile
  lane

## Dependencies

- may run independently of `T.2` / `T.3`
- should complete before any claim that daemon singleton and IPC behavior are
  production-ready on Windows
