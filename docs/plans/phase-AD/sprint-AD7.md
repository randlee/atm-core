---
id: AD.7
title: Local Tmux Post-Send Emitter
status: planned
branch: feature/pAD-s7-local-tmux-post-send-emitter
worktree: ../atm-core-worktrees/feature/pAD-s7-local-tmux-post-send-emitter
target: integrate/phase-AD
---

# Sprint AD.7 — Local Tmux Post-Send Emitter

## Goal

- implement the local tmux-backed post-send emitter

## Hard Dependencies

- `AD.6` complete
- `AD.5` complete
- `docs/plans/phase-AD/plan-phase-AD.md`

## Exact Targets

- `crates/atm-core/src/send/mod.rs`
- `crates/atm-core/src/ack/mod.rs`
- `crates/atm-core/src/send/hook.rs`
- `crates/atm-core/src/service_runtime.rs`
- `crates/atm-core/src/team_admin.rs`
- `crates/atm-core/src/boundary/store.rs`
- `scripts/atm-nudge.sh`

## Interfaces To Add Or Modify

```rust
pub struct LocalTmuxPostSendEmitter { /* owned dependencies */ }

impl PostSendHookEmitter for LocalTmuxPostSendEmitter {
    fn emit(&self, event: &PostSendHookEvent) -> Result<(), AtmError>;
}
```

```rust
fn emit(&self, event: &PostSendHookEvent) -> Result<(), AtmError> {
    let pane_id = event.recipient_pane_id.ok_or_else(missing_pane_error)?;
    self.tmux_nudge.send(&pane_id, event)?;
    Ok(())
}
```

- modify local emission so the emitter consumes authoritative
  `recipient_pane_id` from roster/store state
- modify send/ack warning paths so pane-missing and tmux-send failures become
  sender-visible warnings with structured logs
- modify any surviving shell adapter so it consumes the provided pane id rather
  than rediscovering pane routing from repo-local config

## Obsolescence Instructions

- any local nudge helper that rediscovers pane routing from `.atm.toml`,
  `config.json`, or cwd-dependent lookup becomes obsolete in this sprint
- if `scripts/atm-nudge.sh` survives as an execution helper, mark the old
  discovery path obsolete and permit only payload-driven pane targeting

## Deliverables

- local tmux-backed recipients receive post-send emission through the approved
  local emitter path
- pane-not-found and local emission failures are logged and returned as
  sender-visible warnings

## Required Work

- map local recipients with post-send capability onto the tmux-backed emitter
- use authoritative SQLite roster pane metadata for emission
- fail cleanly and visibly when pane metadata is missing or invalid

## Error And Warning Contract

The local tmux emitter must use the shared `AD.6` post-send taxonomy exactly:

- `PostSendPaneMissing` / `ATM_POST_SEND_PANE_MISSING`
  - cause: `recipient_pane_id` is absent for a recipient that requires local
    tmux emission
  - sender surface: warning after successful persistence
  - recovery: repair the roster row with
    `atm teams update-member --team <team> --member <member> --tmux-pane-id <pane>`
- `PostSendTmuxSendFailed` / `ATM_POST_SEND_TMUX_SEND_FAILED`
  - cause: tmux rejected the pane id or the send operation failed
  - sender surface: warning after successful persistence
  - recovery: verify the pane still exists and repair changed pane metadata
    through `atm teams update-member` when the pane id is stale

## This Sprint Does Not Close

- graft-backed emission
- roster drift repair
- Claude inbox nudge deletion

## Acceptance Criteria

- successful local post-send emission returns no warning
- missing or invalid pane state returns a sender-visible warning
- emission failure is logged with enough context to diagnose sender, recipient,
  and pane ownership
- the accepted local emitter does not require repo-local `.atm.toml` lookup to
  resolve the live target pane

## Required Validation

- targeted local-emitter tests
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- `git diff --check`
