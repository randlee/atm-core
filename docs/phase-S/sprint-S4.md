# Phase S.4 — Cross-Platform Hardening And Release Closeout

```yaml
plan_type: sprint_plan
phase: S
sprint: "S.4"
status: planned
estimated_scope: M
```

## Goal

Close the Phase S line by proving same-host daemon parity on supported
operating systems and tightening the guardrails that prevent regression to
Unix-only behavior.

## Governing Requirements

- `REQ-P-PLATFORM-001`
- `REQ-P-PLATFORM-002`
- `REQ-DAEMON-RUNTIME-001`
- `REQ-DAEMON-RUNTIME-002`
- `REQ-DAEMON-RUNTIME-003`
- `REQ-DAEMON-TEST-003`
- `REQ-DAEMON-TEST-004`
- `REQ-DAEMON-PLATFORM-001`
- `REQ-DAEMON-PLATFORM-002`

## Governing ADRs

- `docs/adr/ADR-003-test-fidelity-and-daemon-isolation.md`
- `docs/adr/ADR-007-supported-platform-parity.md`

## Hard Dependencies

- S.2 Windows local IPC implementation is complete
- S.3 lifecycle-control and host-ownership parity work is complete
- no remaining unsupported-path same-host daemon stubs are treated as
  acceptable release state

## Required Work

1. Remove any remaining non-Unix same-host `daemon_unavailable(...)` stubs.
2. Add or tighten lint/review guards that reject:
   - fixed-sleep daemon stabilization
   - new broad `#[cfg(unix)]` gating outside portability adapters
   - Unix-only same-host functionality in production paths
3. Re-enable full Windows `cargo clippy --workspace --all-targets -- -D warnings`
   in both GitHub CI and `just lint` by removing the temporary
   `ATM_WINDOWS_CLIPPY_SCOPE=cross-platform-only` guardrail.
4. Reconcile all docs, ADRs, and machine-readable boundary records with the
   landed cross-platform daemon design.
5. Run a final coverage audit proving the same shared same-host infrastructure
   is used on Unix and Windows.

## Acceptance Criteria

- same-host daemon functionality is supported and test-covered on macOS,
  Linux, and Windows
- every allowed OS-specific implementation difference is documented in the
  architecture and boundary inventory
- no remaining production path depends on Unix-only host APIs outside the
  owned portability adapters
- the test suite forbids flaky timing-based stabilization for same-host daemon
  coverage
- the shutdown drain, WAL checkpoint, and singleton release sequence remains
  ordered and bounded after S.4 hardening
- Windows CI and local `just lint` both run full workspace clippy without the
  temporary daemon exclusion

## Required Validation

- `just lint`
- `cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings`
- workspace tests
- cross-platform CI coverage for same-host daemon functionality
