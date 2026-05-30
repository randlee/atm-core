# `atm-runtime` Requirements

## Goal

Define the concrete composition-root requirements for the `atm-runtime` crate
introduced by `Phase AA`.

## Scope

`atm-runtime` exists only to own concrete runtime/store assembly that must not
live in `atm-daemon`.

## Requirements

- `atm-runtime` must be the only legal home for concrete production assembly
  of SQLite-backed runtime/store components after `AA.2`.
- `atm-runtime` must construct and inject:
  - `SqliteBoundaryAssembly`
  - SQLite-backed `MailStore`
  - SQLite-backed `TaskStore`
  - SQLite-backed `RosterStore`
  - SQLite-backed `RemoteReplayStore`
- `atm-runtime` must expose storage-neutral runtime inputs to callers through
  the `atm-core` trait surfaces frozen by `Phase AA`.
- `atm-runtime` must not own:
  - CLI parsing/rendering
  - daemon transport
  - daemon lifecycle state machines
  - backend-specific logic that belongs inside `atm-rusqlite`
- `atm-runtime` must remain composition-only. If logic is not composition, it
  belongs in another crate.

## Doctor Ownership Rule

- `atm-runtime` may wire together subsystem doctors for caller use.
- `atm-runtime` must not become a second implementation home for deep
  SQLite-specific diagnosis; that remains inside `atm-rusqlite`.
