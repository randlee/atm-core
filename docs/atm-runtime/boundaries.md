# `atm-runtime` Boundary Inventory

> **Phase AI supersession notice:** ADR-036 retires runtime-owned replay and
> finalizer persistence seams. `atm-runtime` assembles storage traits only; it
> is not a SQLite service boundary.

Canonical machine-readable boundary source:
- [../../boundaries/atm-runtime/runtime-composition.toml](../../boundaries/atm-runtime/runtime-composition.toml)

## RuntimeAssembly

Purpose:
- own concrete runtime/store composition for the CLI direct-doctor path and
  daemon startup without letting either caller depend directly on
  `atm-storage-rusqlite`

Public assembly surface:
- `RuntimeAssembly`
- `RuntimeAssemblyInputs`
- `assemble_sqlite_runtime(...)`
- `assemble_default_runtime(...)`

Allowed dependents:
- `atm`
- `atm-daemon`
- `atm-daemon-bootstrap`
- `atm-runtime-test-support`

Allowed dependencies:
- `atm-core`
- `atm-storage`
- `atm-storage-rusqlite`

Forbidden edges:
- `atm-daemon -> atm-storage-rusqlite`
- `atm -> atm-storage-rusqlite`
- `atm-runtime -> atm-daemon`

Notes:
- `atm-runtime` is composition-only
- direct local `ConfigDoctor` ownership lives here
- direct local `atm doctor` assembly lives here
- `StorageBackends<M, R>` is the approved composition seam for concrete shared
  backend handles
- `RuntimeDoctorPorts` replaces the older runtime-bundle doctor grouping
- daemon startup remains fail-closed on any `RuntimeAssembly` construction
  error
