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
- transition rule for `AA.2` through `AA.4`:
  - this boundary record freezes the intended end-state edges early so code and
    sprint scope can be built toward them
  - the existing `atm-rusqlite` boundary TOMLs remain the authoritative lint
    policy until `AA.5` relocks those files to remove `atm-daemon` from their
    allowlists
  - `AA.5` is the sprint that makes every machine-readable boundary record
    agree again on the forbidden `atm-daemon -> atm-rusqlite` edge
