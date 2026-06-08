# `atm-runtime` Architecture

## Role

`atm-runtime` is the concrete composition root introduced by `Phase AA`.

It sits between:
- storage-neutral interfaces in `atm-core`
- backend implementation crates such as `atm-storage-rusqlite`
- top-level callers such as the CLI and `atm-daemon`

## Ownership

`atm-runtime` owns:
- concrete production assembly of runtime/store dependencies
- installation of the active `MailStore`, `TaskStore`, and `RosterStore`
  implementations
- installation of subsystem doctor implementations
- the concrete `ConfigDoctor` implementation and its direct local doctor-path
  assembly
- installation of the active `RemoteReplayStore`
- SQLite-specific observability injection into SQLite-owned code
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
    pub task_store: Arc<dyn TaskStore>,
    pub doctor_ports: RuntimeDoctorPorts,
    pub remote_replay_store: Arc<dyn RemoteReplayStore>,
    pub storage_finalizer: Arc<dyn RuntimeStorageFinalizer>,
}
```

`atm-daemon` must consume only this injected storage-neutral assembly and must
not construct backend storage objects directly.

The retained replay contract intentionally keeps the richer runtime-owned
shape. `RemoteReplayStateRecord` still carries `(team, agent, message_key)`,
peer endpoint, request envelope, expiry, attempt counters, and
`last_error: Option<AtmErrorCode>` so replay resume can retry, deduplicate,
and age out retained requests without reconstructing those fields from opaque
payload bytes.

The direct CLI doctor path is allowed to depend on `atm-runtime` for
`RuntimeDoctorPorts` and local runtime assembly. `atm doctor` now assembles
its local config/store doctor path here and must not depend directly on
`atm-storage-rusqlite`.

## Boundary Rule

`atm-runtime` is the legal Phase AA location for concrete SQLite assembly.
That authorization does not extend to `atm-daemon`.

## Startup Rule

Any `RuntimeAssembly` failure is fail-closed. The daemon must not enter
serving state if any required runtime component, including replay-store
construction, fails.

Shutdown-time storage finalization follows the same ownership split:
- `atm-runtime` injects a storage-neutral `RuntimeStorageFinalizer`
- daemon runtime code uses that seam instead of talking directly to SQLite

Config inspection follows the same seam discipline:
- `atm-runtime` owns the direct `ConfigDoctor`
- daemon startup captures one validated `current_dir` and passes it into
  runtime assembly so the direct doctor path and startup config load inspect
  the same workspace root
