---
id: AD.26
title: Post-Send Boundary Wiring And Hook Accounting Repair
status: planned
branch: feature/pAD-s26-post-send-boundary-wiring-and-accounting
worktree: ../atm-core-worktrees/feature/pAD-s26-post-send-boundary-wiring-and-accounting
target: integrate/phase-AD
---

# Sprint AD.26 — Post-Send Boundary Wiring And Hook Accounting Repair

## Goal

- make the accepted post-send boundary real on the production send/ack path,
  and fix mixed-success hook accounting so successful emission is never hidden
  by another matching hook failure

## Hard Dependencies

- `AD.25` complete
- `docs/plans/phase-AD/plan-phase-AD.md`
- review provenance:
  - ATM message `01KX1P4D0SEZXWW90VW2F7FF27` from `quality-mgr`,
    `2026-07-08`, subject `PHASE-AD-END-QA FINAL VERDICT`
  - ATM message `01KX1MTJE596JE8SC2766V0Q10` from `arch-ctm`,
    `2026-07-08`, subject `PHASE-AD-END-REVIEW complete`

## Exact Targets

- `boundaries/atm-core/post-send-hook-emitter.toml`
- `boundaries/atm-core/graft-post-send-port.toml`
- `crates/atm-core/src/boundary/mod.rs`
- `crates/atm-core/src/send/hook.rs`
- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-daemon/src/composition.rs`
- `crates/atm-daemon/src/local_ipc_transport/request_worker.rs`
- `crates/atm-daemon/src/tests.rs`
- `crates/atm/src/commands/internal_nudge.rs`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/architecture.md`
- `docs/adr/ADR-019-direct-post-send-and-claude-json-retirement.md`
- `docs/project-plan.md`
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/plans/phase-AD/sprint-AD26.md`

## Interfaces To Add Or Modify

This sprint makes the architectural call explicitly:

- keep `PostSendHookEmitter`
- keep `GraftPostSendPort`
- wire both as live runtime seams
- delete the `std::process::Command` subprocess bypass from the accepted
  send/ack path

ADR-019 already fixes the accepted architecture to one direct post-persist
emitter seam with receiver-specific handoff staying capability-specific, so
wiring the existing boundaries is the correct closure and retiring them would
contradict the accepted Phase AD design rather than repairing implementation
drift.

The accepted accounting shape after this sprint is:

```rust
pub struct HookExecutionSummary {
    pub matched_rules: usize,
    pub succeeded_rules: usize,
    pub failed_rules: usize,
}

pub enum PostSendEmissionPath {
    ExternalHook,
    LocalTmux,
    GraftPort,
}

pub enum PostSendEmissionOutcome {
    NoCapability,
    Delivered {
        path: PostSendEmissionPath,
        hook_summary: HookExecutionSummary,
    },
    Failed {
        hook_summary: HookExecutionSummary,
        warning: WarningEntry,
    },
}

pub trait PostSendHookEmitter: sealed::Sealed + Send + Sync {
    fn emit_post_send(
        &self,
        event: &PostSendHookEvent,
        config: Option<&AtmConfig>,
        delivery_snapshot: &DeliveryRecipientSnapshot,
        graft_port: Option<&dyn GraftPostSendPort>,
    ) -> Result<PostSendEmissionOutcome, AtmError>;
}
```

Required runtime meaning after this sprint:

- graft-backed delivery attempts call `graft_port.deliver_post_send(...)`
  directly on the accepted send/ack runtime path
- local tmux-backed delivery stays behind the accepted emitter seam and does
  not use `std::process::Command` subprocess spawn from
  `crates/atm-core/src/send/hook.rs`
- matching external hook rules still execute in config order
- any successful matching external hook counts as real emission
- built-in fallback is attempted only when zero external hook rules matched
- sender-visible warning is appended only when a post-send-capable recipient
  saw total emission failure
- partial failures keep warnings and logs, but do not erase a successful
  emission outcome
- notification log append occurs on any real successful emission, even when a
  sibling matching hook also failed

`atm internal-nudge` may remain temporarily as a thin renderer/delivery helper,
but it is no longer allowed to be the production boundary bypass on the send
path. `AD.27` owns the remaining extraction cleanup around that helper.

## Paths To Delete

- unused `_graft_port` threading with no live call to
  `.deliver_post_send(...)`
- any `std::process::Command`-based post-send delivery bypass on the accepted
  send/ack runtime path
- boundary TOMLs or readiness criteria that claim a live emitter seam while the
  implementation still bypasses it
- mixed-success accounting that treats “matched with one success and one
  failure” as no emission

## Deliverables

- `PostSendHookEmitter` has at least one real implementation and one real
  production call site on the accepted send/ack path
- graft-backed delivery goes through `GraftPostSendPort`
- mixed-success hook execution is accounted as matched/succeeded/failed
  distinctly
- notification logging and sender warnings reflect real delivery outcome rather
  than the previous all-or-nothing warning vector shortcut
- boundary TOMLs, boundary inventory docs, readiness criteria, and runtime code
  all describe the same mechanism

## This Sprint Does Not Close

- explicit set/disable/clear lifecycle for template overrides
- upstream movement of template resolution out of `atm internal-nudge`
- the `atm-graft` timing race
- the phase-end smoke/service-hardening lane

## Acceptance Criteria

- `rg 'deliver_post_send\\(' crates` shows a live production call path, not
  only trait definition or tests
- `rg 'std::process::Command' crates/atm-core/src/send/hook.rs` returns no
  accepted send-path subprocess bypass
- targeted tests prove:
  - matched hook success + sibling hook failure still logs successful emission
  - total external-hook failure returns sender-visible warning
  - zero matching hooks still trigger the built-in path
  - graft-backed delivery goes through the graft port rather than subprocess
    bypass
- `boundaries/atm-core/post-send-hook-emitter.toml`,
  `boundaries/atm-core/graft-post-send-port.toml`,
  `docs/atm-core/boundaries.md`, `docs/plans/phase-AD/plan-phase-AD.md`, and
  `docs/adr/ADR-019-direct-post-send-and-claude-json-retirement.md` all match
  the accepted live mechanism

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- targeted post-send accounting regression tests
- targeted graft-port delivery regression tests
- `git diff --check`
