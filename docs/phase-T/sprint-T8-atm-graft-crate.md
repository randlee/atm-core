# Sprint T.8 ATM-Graft Crate

**Branch**: `integrate/phase-T`
**Base**: `integrate/phase-T @ 75d341b`
**PR target**: `integrate/phase-T`
**Status**: Planning

## Goal

Add `atm-graft` as a thin embedded ATM client crate for Rust host agents.

## Preconditions

- `T.6` and `T.7` must merge first so the crate is built on the accepted
  graft-facing client surface and daemon-owned runtime features

## Deliverables

- add the `atm-graft` crate
- add minimal `[atm.graft]` activation support in ATM-owned config loading
- add `GraftSession` as the concrete implementation of the `atm_core`
  `GraftSessionPort` trait
- expose a small public API limited to:
  - `send`
  - `read`
  - `ack`
  - session lifecycle
  - host-facing automatic nudge injection integration
- keep the crate runtime-neutral at its core
- add any first convenience runtime adapter only if the host integration proves
  it necessary

## Key File Targets

- `crates/atm-graft/*`
- `crates/atm-core/src/config/*`
- `docs/atm-graft/architecture.md`
- `docs/atm-graft/requirements.md`
- `docs/plan-atm-graft.md`

## Acceptance Criteria

- `atm-graft` builds as a thin embedded client crate with no `atm-daemon` Rust
  dependency and no direct SQLite / inbox JSONL access
- graft mode is inert when `.atm.toml` is absent and active only through the
  ATM-owned config rules
- `GraftSession` lifecycle is explicit and test-covered
- embedded mode automatically injects nudges into host context between tool
  calls via one live receive task/thread
- the public API remains intentionally small and does not mirror the full CLI
- the host-facing bridge is sufficient for between-tool-call insertion without
  forcing host-specific tool-loop logic into the crate
- no production acceptance path relies on `tmux send-keys`, shell-hook polling,
  or other external terminal automation

## Required Validation

- `cargo fmt --all --check`
- `just lint`
- `cargo test -p atm-graft`
- targeted `cargo test` for `atm-core` config loading that exercises
  `[atm.graft].enabled`
- `cargo xwin check --workspace --target x86_64-pc-windows-msvc`

## QA Pointers

- `req-qa` must verify that the crate exists, the config activation exists, and
  the named public API is actually present
- `arch-qa` should verify the crate stays thin and does not absorb daemon
  business logic or direct I/O ownership
- hardening review should focus on API minimalism, session lifecycle, and host
  integration neutrality

## Dependencies

- depends on `T.6` and `T.7`
- depends transitively on `T.2` through `T.5` through the `T.6` baseline gate
