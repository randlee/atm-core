---
id: AD.16
title: Thin Graft Receiver Reset
status: complete
branch: feature/pAD-s16-thin-graft-receiver-reset
worktree: ../atm-core-worktrees/feature/pAD-s16-thin-graft-receiver-reset
target: integrate/phase-AD
---

# Sprint AD.16 — Thin Graft Receiver Reset

## Goal

- reset `atm-graft` to a thin receiver implementation that no longer depends
  on daemon-owned advisory session protocol families

## Hard Dependencies

- `AD.15` complete
- `docs/plans/phase-AD/plan-phase-AD.md`

## Exact Targets

- `crates/atm-graft/src/lib.rs`
- `crates/atm-graft/src/runtime.rs`
- `crates/atm-graft/src/transport.rs`
- `crates/atm-graft/examples/smoke_same_host.rs`
- `docs/atm-graft/architecture.md`
- `docs/atm-graft/boundaries.md`
- `docs/atm-graft/requirements.md`

## Interfaces To Add Or Modify

The host injection seam remains receiver-owned:

```rust
pub trait HostNudgeInjector: Send + Sync {
    fn inject_nudge(&self, nudge: PostSendHookEvent) -> Result<(), AtmError>;
}
```

The accepted implementation rule after this sprint is:

- any receiver-side active/inactive state lives inside `atm-graft`
- no `atm-graft` public API depends on shared daemon advisory registration,
  fetch/drain, or stream DTOs
- if `atm-graft` needs a graft-local projection of `PostSendHookEvent`, that
  projection stays private to `atm-graft`
- if `atm-graft` still uses a same-host listener or receive task, that detail
  stays internal to `atm-graft` and must not reappear in `atm-core`,
  `atm-daemon`, or `atm-daemon-client`

## Paths To Delete

- `GraftSessionClient: AtmGraftClient + AdvisorySessionPort`
- `ActiveAdvisoryStream`
- registration/unregistration helpers that depend on daemon-owned advisory
  session protocol
- fetch/drain helpers that depend on daemon-owned advisory queue DTOs
- dedicated advisory-stream transport helpers in `crates/atm-graft/src/transport.rs`
- public `atm-graft` option/state fields that exist only to drive the deleted
  shared advisory session model
- dedicated advisory-stream and persistent-receive-thread requirements from
  `docs/atm-graft/architecture.md`, `docs/atm-graft/requirements.md`, and
  `docs/atm-graft/boundaries.md`

## Deliverables

- `atm-graft` no longer consumes shared advisory register/fetch/drain/stream
  packet families
- any remaining receiver-side runtime state is private to `atm-graft`
- host-facing injection remains capability-based and independent from daemon
  dispatcher/session ownership
- graft-local docs no longer prescribe daemon-owned session registration,
  daemon-owned queues, or a dedicated shared advisory-stream path

## This Sprint Does Not Close

- final smoke/readiness verification
- unrelated cross-host feature expansion

## Acceptance Criteria

- `atm-graft` builds and tests without depending on deleted shared advisory
  session/stream DTOs
- `atm-graft` public API exposes only the retained thin client and host
  injection concepts
- no daemon-facing graft code path requires session registration, fetch/drain,
  or dedicated advisory-stream protocol families
- `docs/atm-graft/architecture.md`, `docs/atm-graft/requirements.md`, and
  `docs/atm-graft/boundaries.md` describe only receiver-local runtime detail
  plus the retained thin shared client contract

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- `rg -n "AdvisorySessionPort|ActiveAdvisoryStream|Advisory(Register|Unregister|Fetch|Drain|Stream)" crates/atm-graft`
- `git diff --check`
