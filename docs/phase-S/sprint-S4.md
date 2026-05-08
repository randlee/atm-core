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
- `REQ-DAEMON-TEST-003`
- `REQ-DAEMON-TEST-004`
- `REQ-DAEMON-PLATFORM-001`
- `REQ-DAEMON-PLATFORM-002`

## Required Work

1. Remove any remaining non-Unix same-host `daemon_unavailable(...)` stubs.
2. Add or tighten lint/review guards that reject:
   - fixed-sleep daemon stabilization
   - new broad `#[cfg(unix)]` gating outside portability adapters
   - Unix-only same-host functionality in production paths
3. Reconcile all docs, ADRs, and machine-readable boundary records with the
   landed cross-platform daemon design.
4. Run a final coverage audit proving the same shared same-host infrastructure
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

## Required Validation

- `just lint`
- workspace tests
- cross-platform CI coverage for same-host daemon functionality
