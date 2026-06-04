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
- `atm-runtime` must own the concrete `ConfigDoctor` implementation used by
  the direct local doctor path.
- `atm-runtime` must support an `atm -> atm-runtime` dependency edge for the
  direct local doctor path while forbidding any direct `atm -> atm-rusqlite`
  dependency.
- `atm-runtime` must support an `atm-daemon -> atm-runtime` dependency edge
  for runtime bundle assembly while preserving the forbidden
  `atm-daemon -> atm-rusqlite` edge.
- `atm-runtime` must fail closed during `RuntimeBundle` assembly. If any
  component cannot be constructed, including the replay-store component needed
  by `REQ-DAEMON-RUNTIME-005`, daemon startup must fail before entering
  serving state.
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
