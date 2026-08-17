# Phase S.6 — Daemon Post-Mortem Runtime Remediation

```yaml
plan_type: sprint_plan
phase: S
sprint: "S.6"
status: planned
estimated_scope: M
```

## Goal

Close the remaining daemon/runtime post-mortem fixes left open after S.4 so
the cross-platform host line is no longer carrying known shutdown, wakeup, or
endpoint-preparation defects.

## Governing Requirements

- `REQ-P-PLATFORM-002`
- `REQ-P-TEST-001`
- `REQ-DAEMON-RUNTIME-003`
- `REQ-DAEMON-TRANSPORT-004`
- `REQ-DAEMON-SIGNAL-001`
- `REQ-DAEMON-TEST-004`

## Governing ADRs

- `docs/adr/ADR-003-test-fidelity-and-daemon-isolation.md`
- `docs/adr/ADR-007-supported-platform-parity.md`
- `docs/adr/ADR-008-no-flaky-test-policy-and-mechanical-enforcement.md`

## Hard Dependencies

- S.4 cross-platform parity and closeout work is merged
- S.5 planning hardening is merged
- no new Phase S daemon remediation item may bypass this sprint once it is
  accepted as a live post-mortem fix

## Required Work

1. Close `RSH-001` in `crates/atm-daemon/src/composition.rs`.
1.1 Fix `shutdown_background_lanes` so partial shutdown cannot strand
    background lanes after an earlier lane fails.
1.2 Audit all call sites in:
   - `RuntimeComposition::begin_shutdown`
   - `RuntimeComposition::finalize_shutdown`
   - `RuntimeComposition::start`

2. Close `RSH-014` in `crates/atm-daemon/src/lifecycle_control.rs`.
2.1 Fix the Unix EOF wake path so lifecycle-control shutdown propagation does
    not silently miss the notify step.
2.2 Prove the wake path through bounded Unix-focused coverage.

3. Close `WIN-001` in the Windows daemon graceful-shutdown path.
3.1 Restore the missing Windows graceful-shutdown behavior in the daemon
    shutdown-signal / lifecycle-control line.
3.2 Tighten the daemon shutdown-signal tests in `crates/atm-daemon/src/tests.rs`
    and any Windows lifecycle-control tests so the regression is covered.

4. Close `ATM-QA-S4-001` in
   `crates/atm-daemon/src/local_ipc_transport.rs::prepare_local_ipc_endpoint`.
4.1 Replace the silent non-Unix `Ok(())` path with explicit documented
    behavior that matches the retired endpoint-preparation contract.
4.2 Keep the adapter behavior platform-neutral above the owned local-IPC
    implementation layer.

5. Re-run the daemon post-mortem audit after the fixes land.
5.1 Update any affected product or crate-local docs if the runtime behavior
    contract changes while fixing these defects.
5.2 Keep `FTQ-001` out of scope here; it remains a lint/analyzer deferral.

## Required Code Targets

- `crates/atm-daemon/src/composition.rs`
- `crates/atm-daemon/src/lifecycle_control.rs`
- `crates/atm-daemon/src/local_ipc_transport.rs`
- `crates/atm-daemon/src/tests.rs`

## Acceptance Criteria

- `RSH-001`, `RSH-014`, `WIN-001`, and `ATM-QA-S4-001` are closed on the
  active branch
- shutdown sequencing remains ordered and bounded after partial-lane failure
- the Unix lifecycle-control EOF path always performs the documented wake
  notification
- the Windows graceful-shutdown path is covered again and no longer regresses
- local-IPC endpoint preparation no longer hides non-Unix behavior behind a
  silent success path

## Required Validation

- `just lint`
- `cargo test -p atm-daemon`
- `cargo xwin check --workspace --target x86_64-pc-windows-msvc`
- `cargo xwin clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings`
