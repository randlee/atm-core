# `atm-runtime` Architecture

> **Phase AI supersession notice:** ADR-036 reduces this crate to thin
> backend-neutral composition. The older replay-store, runtime finalizer, and
> SQLite-observability bridge text below is historical and must not be extended.

## Role

`atm-runtime` is the concrete composition root introduced by `Phase AA`.

It sits between:
- storage-neutral interfaces in `atm-core`
- backend implementation crates such as `atm-storage-rusqlite`
- top-level callers such as the CLI and `atm-daemon`

## Ownership

`atm-runtime` owns:
- concrete production assembly of runtime/store dependencies
- installation of the active `MailStore` and `RosterStore` implementations
- installation of subsystem doctor implementations
- the concrete `ConfigDoctor` implementation and its direct local doctor-path
  assembly
- no public SQLite observability bridge above `atm-storage-rusqlite`
- the temporary legacy compile bridge from `atm-storage::{MessageStore,RosterStore}`
  into `atm_core::boundary::{MailStore,RosterStore}` during AC.4 cutover

`atm-runtime` does not own:
- daemon request routing
- daemon lifecycle control
- CLI command rendering
- backend-specific store logic that belongs inside `atm-storage-rusqlite`

## Required Seam

The minimum landed Phase AC seam is a composition-root assembly with explicit
shared-storage handles and doctor ports:

```rust
pub struct StorageBackends<M: MessageStore, R: RosterStore> {
    pub messages: M,
    pub rosters: R,
}
pub struct RuntimeAssembly {
    pub service_runtime: LocalServiceRuntime,
    pub storage_backends: StorageBackends<Arc<dyn MessageStore>, Arc<dyn RosterStore>>,
    pub mail_store: Arc<dyn MailStore>,
    pub roster_store: Arc<dyn RosterStore>,
    pub doctor_ports: RuntimeDoctorPorts,
}
```

`atm-daemon` must consume only this injected storage-neutral assembly and must
not construct backend storage objects directly.

The direct CLI doctor path is allowed to depend on `atm-runtime` for
`RuntimeDoctorPorts` and local runtime assembly. `atm doctor` now assembles
its local config/store doctor path here and must not depend directly on
`atm-storage-rusqlite`.

## Boundary Rule

`atm-runtime` is the legal Phase AA location for concrete SQLite assembly.
That authorization does not extend to `atm-daemon`.

## Startup Rule

Any `RuntimeAssembly` failure is fail-closed. The daemon must not enter
serving state if required runtime storage construction fails.

Config inspection follows the same seam discipline:
- `atm-runtime` owns the direct `ConfigDoctor`
- daemon startup captures one validated `current_dir` and passes it into
  runtime assembly so the direct doctor path and startup config load inspect
  the same workspace root
