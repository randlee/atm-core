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

## Validation

- `cargo build --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`
