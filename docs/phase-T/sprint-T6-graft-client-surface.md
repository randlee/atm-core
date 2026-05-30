# Sprint T.6 Graft Client Surface

**Branch**: `integrate/phase-T`
**Base**: `integrate/phase-T @ 75d341b`
**PR target**: `integrate/phase-T`
**Status**: Planning

## Goal

Define the small embeddable daemon-client surface needed by `atm-graft`
without turning `atm-graft` into a second runtime stack or forcing a Rust
dependency on `atm-daemon`.

## Preconditions

- `T.2` through `T.5` must be merged first so the daemon/runtime baseline is
  stable before the graft-facing public client surface is locked

## Deliverables

- name the concrete `atm-core` graft-facing traits:
  - `AtmGraftClient`
  - `GraftSessionPort`
- add the public `atm-core` client-side models needed by embedded consumers
  for:
  - `send`
  - `read`
  - `ack`
  - graft registration / unregistration
  - nudge drain / fetch
- make the public client-facing types typed and explicit rather than
  CLI-composition-only
- ensure `AtmGraftClient` owns the unary daemon request surface and
  `GraftSessionPort` owns the session registration / receive-loop contract so
  T.8 does not re-mint parallel trait names
- keep the surface small enough that `atm-graft` behaves mostly like an ATM
  client embedded inside an agent
- ensure the retained CLI can consume the same client surface where practical
  rather than diverging into a parallel contract
- keep concrete daemon runtime details private to `atm-daemon`
- update the public protocol/interface documentation for the graft-facing
  client surface

## Key File Targets

- `crates/atm-core/src/*`
- `crates/atm/src/*`
- `crates/atm-daemon/src/*` only where public client-surface ownership must be
  moved out or narrowed
- `docs/atm-core/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-daemon/protocol-icd.md`
- `docs/atm-graft/architecture.md`
- `docs/atm-graft/requirements.md`

## Acceptance Criteria

- `atm-graft` can depend on `atm-core` and the documented daemon protocol
  surface without depending on `atm-daemon` as a crate
- the new public client-side request / response / event surface is typed and
  explicit
- concrete daemon runtime/socket details remain outside the `atm-graft` public
  boundary
- the graft-facing surface is small and embeddable rather than mirroring the
  entire CLI

## Required Validation

- `cargo fmt --all --check`
- `just lint`
- targeted `cargo test` for any new `atm-core` client-surface modules
- targeted `cargo test` for any `atm` CLI wiring updated to consume the same
  client surface

## QA Pointers

- `req-qa` must verify the named client surface exists in code, not only in
  planning docs
- `arch-qa` should verify no `atm-daemon` crate dependency leaks into the
  public graft boundary
- hardening review should focus on keeping the surface minimal and typed

## Dependencies

- depends on `T.2` through `T.5` merging first
- `T.7` depends on this sprint defining the client-side contract
