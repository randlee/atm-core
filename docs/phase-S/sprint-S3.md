# Phase S.3 — Windows Runtime Control And Host Ownership

```yaml
plan_type: sprint_plan
phase: S
sprint: "S.3"
status: complete
estimated_scope: L
```

## Goal

Implement Windows lifecycle control and host ownership under the extracted
portability boundaries while preserving the same singleton, reload, and
shutdown semantics on every supported operating system.

## Governing Requirements

- `REQ-P-PLATFORM-001`
- `REQ-P-PLATFORM-002`
- `REQ-P-RUNTIME-002`
- `REQ-P-RUNTIME-003`
- `REQ-DAEMON-RUNTIME-001`
- `REQ-DAEMON-RUNTIME-003`
- `REQ-DAEMON-RUNTIME-005`
- `REQ-DAEMON-SIGNAL-001`
- `REQ-DAEMON-PLATFORM-001`
- `REQ-DAEMON-PLATFORM-002`
- `REQ-DAEMON-TRANSPORT-008`
- `REQ-DAEMON-TEST-003`
- `REQ-DAEMON-TEST-004`
- `REQ-CORE-BOUNDARY-001`

## Governing ADRs

- `docs/adr/ADR-002-host-wide-daemon-singleton.md`
- `docs/adr/ADR-007-supported-platform-parity.md`

## Governing ICD Sections

- `docs/atm-daemon/protocol-icd.md §8` exchange rules
- `docs/atm-daemon/protocol-icd.md §10` timeout and failure semantics
- `docs/atm-daemon/protocol-icd.md §12` delivery and outcome semantics

## Hard Dependencies

- S.1 boundary extraction is complete
- S.2 Windows local IPC implementation is complete
- PID liveness semantics remain unchanged in Phase S unless a later ADR
  explicitly reopens that seam

## Exact Code Targets

- `crates/atm-daemon/src/lifecycle_control.rs`
  - `LifecycleControlSourceAdapter::install`
  - `LifecycleControlSourceAdapter::new_for_test`
- `crates/atm-daemon/src/host_ownership.rs`
  - `host_runtime_lock_path`
  - `host_runtime_lock_path_from_home`
  - `write_owner_record`
  - `recorded_owner_pid`
  - `HostOwnershipAdapter::{acquire, acquire_at}`
  - `HostOwnershipGuard::drop`
  - `open_lock_file`
  - `recover_stale_owner_lock`
- `crates/atm-daemon/src/composition.rs`
  - `RuntimeComposition::begin_shutdown`
  - `RuntimeComposition::finalize_shutdown`
  - `RuntimeComposition::start`

## Required Work

1. Replace the Unix-only lifecycle-control implementation with a
   platform-neutral contract plus Unix and Windows adapters.
2. Replace Unix-shaped host-ownership mechanics with one cross-platform
   contract that preserves identical admission, stale-owner recovery, and
   teardown semantics.
2.1 Use stable permanent host-wide lock-file paths under `~/.atm/daemon/`:
   - `launch.lock`
   - `owner.lock`
2.2 Use one whole-file exclusive-lock contract on those paths rather than
   lock-file creation/deletion as the ownership signal.
2.3 Store current owner metadata in documented `pid[:token]` form in the held
   lock-file contents.
3. Prove ordered release semantics on Windows as well as Unix.
4. Preserve one bounded graceful-shutdown and reload model across supported
   operating systems.
4.1 Preserve the same externally visible protocol behavior while lifecycle
   control and host ownership internals differ by OS.
4.2 Map Windows `SIGBREAK` / `CTRL_BREAK_EVENT` to the same bounded reload
    trigger that Unix receives through `SIGHUP`, while terminate events remain
    the shared graceful-shutdown path.
5. Keep lifecycle-control and host-ownership tests aligned with the shared
   parity contract from ADR-007; platform-specific tests may cover adapter
   internals only.

## Acceptance Criteria

- Windows provides real lifecycle-control behavior for shutdown and reload
- Windows singleton ownership is real and bounded
- Windows and Unix both use the same stable `launch.lock` / `owner.lock`
  ownership model
- teardown ordering matches the documented singleton and runtime contract on
  both platform families
- crash-recovery remains non-regressive: stale-owner recovery and replay-facing
  runtime admission still preserve `REQ-DAEMON-RUNTIME-005` semantics on Unix
  and Windows after the host-ownership refactor
- no same-host daemon code above the adapter line branches directly on Unix
  signal or file-locking APIs
- Windows and Unix lifecycle-control / host-ownership tests prove the same
  externally visible contract, with platform-specific assertions limited to the
  adapter internals

## Required Validation

- `just lint`
- workspace tests
- targeted host-ownership and lifecycle-control tests on Unix and Windows
- shared-harness parity review against ADR-007 before sprint closeout
