# Phase S.4 — Cross-Platform Hardening And Release Closeout

```yaml
plan_type: sprint_plan
phase: S
sprint: "S.4"
status: complete
estimated_scope: M
```

## Goal

Close the Phase S line by proving same-host daemon parity on supported
operating systems and tightening the guardrails that prevent regression to
Unix-only behavior.

## Governing Requirements

- `REQ-P-PLATFORM-001`
- `REQ-P-PLATFORM-002`
- `REQ-P-TEST-001`
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
- `docs/adr/ADR-008-no-flaky-test-policy-and-mechanical-enforcement.md`

## Governing ICD Sections

- `docs/atm-daemon/protocol-icd.md §6` packet kind registry
- `docs/atm-daemon/protocol-icd.md §8` exchange rules
- `docs/atm-daemon/protocol-icd.md §10` timeout and failure semantics
- `docs/atm-daemon/protocol-icd.md §14` test and reuse rules

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
   `ATM_WINDOWS_CLIPPY_SCOPE=cross-platform-only` guardrail. This is now
   enforced directly in `Justfile` and `.github/workflows/ci.yml`.
4. Reconcile all docs, ADRs, and machine-readable boundary records with the
   landed cross-platform daemon design.
4.1 Verify the shipped implementation still matches the packet inventory and
   failure contract documented in the protocol ICD.
5. Run a final coverage audit proving the same shared same-host infrastructure
   is used on Unix and Windows. The landed audit points to:
   - `crates/atm-daemon/src/tests.rs::local_ipc_runtime_round_trips_doctor_requests_on_shared_transport`
   - `crates/atm-daemon/src/tests.rs` host-ownership tests
   - `crates/atm-daemon/src/lifecycle_control.rs` Windows lifecycle tests
   - `crates/atm/src/composition.rs` CLI launch-gate and same-host client tests

## Acceptance Criteria

- same-host daemon functionality is supported and test-covered on macOS,
  Linux, and Windows
- every allowed OS-specific implementation difference is documented in the
  architecture and boundary inventory
- no remaining production path depends on Unix-only host APIs outside the
  owned portability adapters
- the test suite forbids flaky timing-based stabilization for same-host daemon
  coverage and forbids unbounded waits in same-host daemon coverage
- the shutdown drain, WAL checkpoint, and singleton release sequence remains
  ordered and bounded after S.4 hardening
- Windows CI and local `just lint` both run full workspace clippy without the
  temporary daemon exclusion
- `just lint` includes the dedicated `same-host-portability` guard that rejects
  broad Unix-only host-shell gating and non-Unix same-host `daemon_unavailable`
  stubs in production adapter code

## Required Validation

- `just lint`
- `cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings`
- workspace tests
- cross-platform CI coverage for same-host daemon functionality

## QA-1 Closeout Notes

- Windows legacy endpoint preparation is intentionally a no-op in
  `crates/atm-daemon/src/local_ipc_transport.rs` because no filesystem socket path exists to
  create or unlink.
- Phase S records the remaining accepted production polling exceptions in
  `docs/plans/phase-S/plan-phase-S.md §4.1`:
  - Windows lifecycle-control wake propagation
  - CLI daemon auto-start endpoint publication wait
  - retained-observability shutdown flush deadline
