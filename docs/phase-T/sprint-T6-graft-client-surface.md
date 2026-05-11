# Sprint T.6 Graft Client Surface

**Branch**: `integrate/phase-T`
**Base**: `integrate/phase-T @ 1232244`
**PR target**: `develop`
**Status**: Planning

## Goal

Define the small embeddable daemon-client surface needed by `atm-graft`
without turning `atm-graft` into a second runtime stack or forcing a Rust
dependency on `atm-daemon`.

## Deliverables

- add the public `atm-core` client-side models needed by embedded consumers
  for:
  - `send`
  - `read`
  - `ack`
  - graft registration / unregistration
  - nudge drain / fetch
- make the public client-facing types typed and explicit rather than
  CLI-composition-only
- keep the surface small enough that `atm-graft` behaves mostly like an ATM
  client embedded inside an agent
- ensure the retained CLI can consume the same client surface where practical
  rather than diverging into a parallel contract
- keep concrete daemon runtime details private to `atm-daemon`

## Key File Targets

- `crates/atm-core/src/*`
- `crates/atm/src/*`
- `crates/atm-daemon/src/*` only where public client-surface ownership must be
  moved out or narrowed
- `docs/atm-core/architecture.md`
- `docs/atm-core/requirements.md`
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

## QA Pointers

- `req-qa` must verify the named client surface exists in code, not only in
  planning docs
- `arch-qa` should verify no `atm-daemon` crate dependency leaks into the
  public graft boundary
- hardening review should focus on keeping the surface minimal and typed

## Dependencies

- should begin only after `T.2`-`T.5` are stable enough that the daemon/runtime
  baseline is not thrashing
- `T.7` depends on this sprint defining the client-side contract
