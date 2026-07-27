# ADR-036 — Storage Boundary And Composition Topology

| Field | Value |
| --- | --- |
| ID | ADR-036 |
| Status | Accepted |
| Scope | Repository-wide |
| Relates to | ADR-018, ADR-032, ADR-034, Phase AI |

## Decision

Only `atm-storage-rusqlite` may import `rusqlite`, own SQL/schema behavior, or
return a concrete SQLite backend. `atm-storage` owns backend-neutral domain
types and storage traits. `atm-core`, `atm-daemon`, `atm-graft`, HTTP adapters,
and HTTPS adapters operate only on those traits.

`atm-runtime` is a thin composition crate. It may assemble trait objects from a
selected backend, but it may not expose concrete SQLite types, add a
SQLite-specific runtime service, or define a second persistence trait for a
daemon feature. In particular, `RemoteReplayStore`,
`RuntimeStorageFinalizer`, `SqliteRemoteReplayStore`, and their composition
fields are retired.

The executable composition root selects the backend through one
backend-neutral factory/assembly input. A new backend therefore implements the
same storage traits and is selected at composition; daemon, CLI, graft, and
transport source remain unchanged.

Storage-owned domain contracts include canonical-message lookup by exact ULID
and the bounded peer/direction/age query required by ADR-038. These are
backend-neutral query methods, not SQLite tables or a daemon-owned persistence
trait. Transport adapters receive canonical records only through those traits.

## Consequences

The prior runtime indirection is not a justification for SQLite-backed daemon
state. Phase AI deletes it while preserving normal storage shutdown through a
storage-owned lifecycle method if one remains necessary. Architecture checks
reject `rusqlite` or `atm-storage-rusqlite` dependencies outside the concrete
backend and approved composition construction site, and reject any new
daemon-specific persistence trait.
