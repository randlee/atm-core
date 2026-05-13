---
id: W.1
title: Daemon Emit Failure Visibility
status: planned
branch: TBD
worktree: TBD
---

# Sprint W.1 — Emit Silent Discard Fix

## Goals

- replace daemon-side silent `emit()` / `emit_event()` discards with an
  explicit fallback rule
- ensure observability sink failure is visible instead of disappearing behind
  `let _ = ...`
- restore the existing reporting contract where sink degradation must be
  diagnosable through the ATM surfaces that already exist
- keep the fix on the shared observability path; this sprint must not invent
  subsystem-specific fallback/reporting implementations
- define how sink degradation becomes visible through:
  - concise ATM CLI failure output when it affects command execution
  - `atm doctor` degraded-health diagnostics when the sink is impaired but the
    command path still succeeds

## Hard Dependencies

- none on `W.2`, `W.3`, or `W.4`
- shared observability trait and doctor/runtime-health paths remain the only
  allowed reporting surfaces

## Required Work

- replace every daemon-side silent `emit()` / `emit_event()` discard in the
  required path inventory
- define the common fallback behavior once and apply it consistently
- define how sustained sink degradation becomes visible through doctor or
  runtime health instead of being silently dropped
- explicitly document whether any emit failure is allowed to remain
  non-blocking and why
- identify the shared daemon surfaces that own the fallback and degraded-health
  projection so no subsystem grows its own reporting side channel

## Acceptance Criteria

- every `let _ = ...emit(...)` and `let _ = ...emit_event(...)` call in the
  current path inventory is replaced; no daemon-side silent-discard callsite
  remains in the listed files
- the final rule replaces silent discard with
  `if let Err(error) = ... { tracing::warn!(...) }` or a stricter approved
  policy; silent loss is forbidden
- the fix uses shared observability/reporting behavior rather than
  subsystem-specific custom fallback pipelines
- the sprint defines which emit failures remain non-blocking warnings versus
  which failures must mark runtime health degraded
- the sprint names how doctor will surface observability impairment
- the sprint verifies whether any ATM CLI commands already surface an adequate
  warning/failure and names the exact regressions that must be restored where
  they do not
- the sprint names the shared runtime-health / doctor projection points that
  carry observability impairment across CLI, graft, and peer-triggered
  diagnostics
- every file/function inventory item listed below is treated as required scope,
  not as an optional review target
- req-qa can verify from the sprint doc alone that daemon-client tracing is
  intentionally owned by `W.2`, not omitted from Phase W

## Implementation Notes

Observed silent-discard inventory on `origin/integrate/phase-V`:
- `crates/atm-daemon/src/advisory_runtime.rs` — 6 call sites
- `crates/atm-daemon/src/composition.rs` — 11 call sites
- `crates/atm-daemon/src/host_ownership.rs` — 4 call sites
- `crates/atm-daemon/src/lifecycle_control.rs` — 5 call sites
- `crates/atm-daemon/src/local_ipc_transport.rs` — 3 call sites
- `crates/atm-daemon/src/notification_runtime.rs` — 8 call sites
- `crates/atm-daemon/src/peer_transport.rs` — 6 call sites
- `crates/atm-daemon/src/reconcile_runtime.rs` — 9 call sites
- `crates/atm-daemon/src/runtime_health.rs` — 5 call sites
- `crates/atm-daemon/src/runtime_status_cache.rs` — 3 call sites
- `crates/atm-daemon/src/watch_runtime.rs` — 8 call sites

Total currently identified daemon-side silent-discard call sites:
- `68`

Current path inventory:
- `crates/atm-daemon/src/advisory_runtime.rs`
  - startup/registration emit paths
  - session-activation success/failure emit paths
  - advisory drain / receive loop event emission
- `crates/atm-daemon/src/composition.rs`
  - startup transition events
  - lane start/stop events
  - rollback / shutdown / finalize events
- `crates/atm-daemon/src/host_ownership.rs`
  - owner acquire / stale owner / release events
- `crates/atm-daemon/src/lifecycle_control.rs`
  - worker install / join / wake / teardown events
- `crates/atm-daemon/src/local_ipc_transport.rs`
  - listener start / accept failure / connection-cap events
- `crates/atm-daemon/src/notification_runtime.rs`
  - notification worker start / timeout / shutdown / dispatch events
- `crates/atm-daemon/src/peer_transport.rs`
  - remote delivery success / retry / failure events
- `crates/atm-daemon/src/reconcile_runtime.rs`
  - queue admission / completion / timeout / worker events
- `crates/atm-daemon/src/runtime_health.rs`
  - health projection / advisory state / sqlite checkpoint events
- `crates/atm-daemon/src/runtime_status_cache.rs`
  - sqlite degraded / status replacement events
- `crates/atm-daemon/src/watch_runtime.rs`
  - watch batch delivery / timeout / shutdown events

Required execution shape:
- replace each silent discard with an explicit fallback block
- standardize fallback fields:
  - subsystem
  - action
  - original emit error code/message
  - whether the command/runtime path continued
- add or update doctor/runtime-health surfaces so sustained observability sink
  failure becomes diagnosable after the fact

Files in scope:
- all files listed in the inventory above
- `crates/atm-daemon/src/daemon_observability.rs`
- `crates/atm-daemon/src/daemon_runtime_observability.rs`
- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-daemon/src/runtime_status_cache.rs`
- `crates/atm-daemon/src/tests.rs`
- `crates/atm-daemon/src/test_observability.rs`

Shared reporting paths that must be reused:
- `crates/atm-daemon/src/daemon_observability.rs`
- `crates/atm-daemon/src/daemon_runtime_observability.rs`
- `crates/atm-core/src/doctor/mod.rs`
- `crates/atm/src/commands/doctor.rs`
- `crates/atm/src/output.rs`
- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-daemon/src/runtime_status_cache.rs`
- `crates/atm-core/src/error.rs`

Current main baseline to preserve:
- degraded daemon conditions should continue to be surfaced through the shared
  ATM error / doctor paths rather than hidden behind daemon-only warnings
- this sprint must verify whether any CLI-facing command currently reports
  observability impairment directly or only through doctor/runtime health, and
  must preserve that behavior while removing silent discard

Critical issue classes covered directly by this sprint:
- observability sink degradation
- daemon runtime incidents whose evidence would otherwise be silently dropped

## Out of Scope

- daemon-client connection tracing
- SQLite subsystem observability
- replay persistence recovery text outside the sink-failure policy

## Required Validation

Plan-auditable now:
- inventory completeness against the listed daemon files
- ownership split between `W.1` and `W.2`
- explicit shared-path rule and non-silent fallback requirement

Implementation validation later:
- grep or lint proof that no listed daemon-side silent-discard callsite remains
- tests showing doctor/runtime-health can surface observability impairment
- proof that the final fallback/reporting path is shared rather than
  subsystem-specific
