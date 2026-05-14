---
id: W.5
title: Doctor Projection
status: completed
branch: feature/pW-s5-doctor-projection
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pW-s5-doctor-projection
---

# Sprint W.5 — Doctor Projection Gap

## Goal

- project daemon bootstrap traceability into the shared `atm doctor` report so
  same-host daemon connect, launch-gate, and auto-start outcomes are visible
  outside retained-log inspection

## Design Decision

- use the `DoctorReport` projection path, not `RuntimeStatusSnapshot`
- rationale:
  - bootstrap traceability is CLI-owned same-host bootstrap state, not
    daemon-owned runtime state
  - `CliComposition::bootstrap(...)` already owns `BootstrapTraceability`
  - attaching the snapshot to `DoctorReport` preserves one shared doctor
    surface without adding a second daemon-side cache or forcing daemon runtime
    status to represent pre-daemon bootstrap work

## Files Changed

- `crates/atm-core/src/doctor/report.rs`
- `crates/atm-core/src/doctor/mod.rs`
- `crates/atm-daemon-client/src/lib.rs`
- `crates/atm/src/composition.rs`
- `crates/atm/src/output.rs`
- `crates/atm/src/commands/doctor.rs`
- `crates/atm-daemon/src/test_support.rs`

## Acceptance Criteria

- `DoctorReport` carries a typed bootstrap trace projection
- `CliComposition::bootstrap(...)` captures bootstrap trace outcomes after
  successful daemon availability
- `CliComposition::doctor(...)` attaches the captured bootstrap trace to the
  returned report
- human-readable `atm doctor` output prints daemon connect, launch-gate, and
  auto-start outcomes plus any recorded detail strings
- `cargo build --workspace` passes
- `cargo test --workspace` passes
- `cargo clippy --workspace -- -D warnings` passes
- `grep -rn 'use sc_observability' crates/atm-daemon/` returns `0` results
