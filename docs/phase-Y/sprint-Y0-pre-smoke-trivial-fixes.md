---
id: Y.0
title: Sprint Y.0 — Pre-Smoke Trivial Fixes
status: complete
branch: feature/pY-trivial-fixes
worktree: ../atm-core-worktrees/feature/pY-trivial-fixes
target: develop
---

# Sprint Y.0 — Pre-Smoke Trivial Fixes

## Goal

- land the small pre-`Y.1` cleanup items on `develop` before the heavier
  daemon-release smoke and dogfood work starts

## Delivered

- replaced the raw `command: "atm"` literal in
  `crates/atm/src/observability.rs` with the shared `ATM_SERVICE_NAME`
  constant
- removed the redundant dual read/ack state derivation in
  `crates/atm-core/src/ack/mod.rs` so the `atm ack` validation path derives
  only acknowledgement state
- removed the stale `pending-ack override` phrase from `docs/architecture.md`
  so the documented clear pipeline matches the current two-axis eligibility
  model
- reviewed GitHub issues `#78` and `#83` and sent the requested design
  proposals to `team-lead` before any implementation work for those issues
- landed the approved `#78` follow-up safely against the current release
  contract:
  - added explicit regression coverage that Claude-style JSONL ingress still
    reads correctly
  - added explicit regression coverage that the current ATM compatibility write
    path continues to rewrite back to the existing array-shaped file format on
    first mutation
  - documented that `locked_read_modify_write(...)` reads JSONL ingress but
    rewrites through the ATM-owned compatibility projection path
- recorded that `#83` (`atm help`) remains deferred to Phase `Y` Sprint `Y.1`
  and is not part of this trivial-fixes branch

## Validation

- `cargo build --workspace`
- `cargo clippy --workspace -- -D warnings`
- `cargo test -p agent-team-mail-core mailbox:: -- --nocapture`
- `git diff --check`
