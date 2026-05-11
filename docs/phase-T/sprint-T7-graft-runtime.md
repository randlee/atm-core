# Sprint T.7 Graft Runtime

**Branch**: `integrate/phase-T`
**Base**: `integrate/phase-T @ 1232244`
**PR target**: `develop`
**Status**: Planning

## Goal

Add the small daemon-side runtime features embedded and hook/poll graft
consumers actually need: registration, bounded nudge queueing, and drain/fetch
access.

## Preconditions

- `T.6` must merge first so the daemon-side registration and drain surfaces are
  implementing an already-defined public graft client contract

## Deliverables

- add graft registration / unregistration handling in the daemon/runtime line
- add a daemon-owned bounded pending-nudge queue
- add a typed drain/fetch API for embedded and hook/poll consumers
- add explicit backpressure and queue-overflow behavior with structured error
  identity and observability
- add the hook-facing `atm` command surface that consumes the same daemon API
  rather than creating a separate binary
- update the daemon protocol/interface docs for registration, drain/fetch, and
  nudge payload boundaries

## Key File Targets

- `crates/atm-daemon/src/*`
- `crates/atm-core/src/*`
- `crates/atm/src/commands/*`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/protocol-icd.md`
- `docs/atm-graft/architecture.md`
- `docs/atm-graft/requirements.md`

## Acceptance Criteria

- registration and unregistration paths exist and are test-covered
- pending nudge queue ownership is daemon-side, not graft-side
- the queue is bounded and its overflow/backpressure behavior is explicit
- embedded consumers and hook/poll consumers use the same daemon-owned drain
  contract
- the hook-facing path is on the `atm` CLI surface, not on a separate
  `atm-graft` executable

## Required Validation

- `cargo fmt --all --check`
- `just lint`
- `cargo test -p atm-daemon`
- targeted `cargo test` for the hook-facing `atm` nudge-drain command surface
- `cargo xwin check --workspace --target x86_64-pc-windows-msvc`

## QA Pointers

- `req-qa` must verify queue/drain deliverables are present in code and not
  just described in docs
- `arch-qa` should verify daemon queue ownership and the absence of duplicate
  host-side queue logic
- hardening review should focus on boundedness, shutdown, and backpressure

## Dependencies

- depends on `T.6` defining the public graft client contract
- `T.8` depends on this sprint merging first
