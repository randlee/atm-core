# `atm-runtime` Boundary Inventory

Canonical machine-readable boundary source:
- [../../boundaries/atm-runtime/runtime-composition.toml](../../boundaries/atm-runtime/runtime-composition.toml)

## RuntimeComposition

Purpose:
- own concrete runtime/store composition for the CLI direct-doctor path and
  daemon startup without letting either caller depend directly on
  `atm-rusqlite`

Allowed dependents:
- `atm`
- `atm-daemon`

Allowed dependencies:
- `atm-core`
- `atm-rusqlite`

Forbidden edges:
- `atm-daemon -> atm-rusqlite`
- `atm -> atm-rusqlite`
- `atm-runtime -> atm-daemon`

Notes:
- `atm-runtime` is composition-only
- direct local `ConfigDoctor` ownership lives here
- daemon startup remains fail-closed on any `RuntimeBundle` construction error
