# `atm-runtime` Architecture

## Role

`atm-runtime` is the concrete composition root introduced by `Phase AA`.

It sits between:
- storage-neutral interfaces in `atm-core`
- backend implementation crates such as `atm-rusqlite`
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

`atm-runtime` does not own:
- daemon request routing
- daemon lifecycle control
- CLI command rendering
- backend-specific store logic that belongs inside `atm-rusqlite`

## Required Seam

The minimum Phase AA seam is a storage-neutral runtime bundle:

```rust
pub struct RuntimeBundle {
    pub mail_store: Arc<dyn MailStore>,
    pub task_store: Arc<dyn TaskStore>,
    pub roster_store: Arc<dyn RosterStore>,
    pub mail_store_doctor: Arc<dyn MailStoreDoctor>,
    pub task_store_doctor: Arc<dyn TaskStoreDoctor>,
    pub roster_store_doctor: Arc<dyn RosterStoreDoctor>,
    pub config_doctor: Arc<dyn ConfigDoctor>,
    pub remote_replay_store: Arc<dyn RemoteReplayStore>,
}
```

`atm-daemon` must consume only this kind of injected storage-neutral bundle and
must not construct `SqliteBoundaryAssembly` directly.

The direct CLI doctor path is allowed to depend on `atm-runtime` for this
bundle/doctor assembly. It must not depend directly on `atm-rusqlite`.

## Boundary Rule

`atm-runtime` is the legal Phase AA location for concrete SQLite assembly.
That authorization does not extend to `atm-daemon`.

## Startup Rule

Any `RuntimeBundle` assembly failure is fail-closed. The daemon must not enter
serving state if any required runtime component, including replay-store
construction, fails.
