# Phase AC Crate Graph And Migration Map

## Goal

Freeze the target storage crate graph and the ownership transitions required to
reach it.

This is an `AC.0` planning collateral artifact used by `AC.2`, `AC.3`, and
`AC.4`.

## Target Graph

Approved target graph:

```text
atm-storage
atm-storage-claude -> atm-storage
atm-storage-rusqlite -> atm-storage
atm-daemon-client -> atm-storage
atm-core -> atm-storage
```

Future extension:

```text
atm-storage-sqlserver -> atm-storage
```

Phase `AC.7` compile proof:

```text
atm-storage-sqlserver-proof -> atm-storage
```

## Forbidden Graph

Forbidden edges and shapes:

- `atm-storage-* -> atm-core`
- `atm-daemon-client -> atm-storage-rusqlite`
- `atm-daemon-client -> atm-storage-claude`
- `atm-storage` owning RPC request / response envelope families
- `atm-core` owning concrete backend file / SQLite mechanics above the storage
  seam
- Claude storage treated as compatibility-only while SQLite is treated as
  "real" storage

## Migration Ownership

### AC.1

Creates:

- `crates/atm-storage`

Owns:

- shared semantic traits
- shared canonical domain structs
- notification trait

### AC.2

Creates:

- `crates/atm-storage-claude`

Moves / converges:

- Claude inbox read / write / salvage / rewrite / lock behavior
- Claude storage-specific implementation details below the trait line

### AC.3

Converges:

- SQLite backend against `atm-storage`

Required outcome:

- concrete SQLite backend no longer depends on `atm-core`

### AC.4

Moves:

- `atm-core` consumers onto `atm-storage` traits

Required outcome:

- daemon/runtime/core stop reaching through backend-shaped seams

### AC.5

Owns:

- `RpcEnvelope` ownership at `atm-daemon-client`
- `atm-daemon-client -> atm-storage` as the permitted canonical-domain edge
- explicit prohibition of `atm-daemon-client -> atm-storage-rusqlite` and
  `atm-daemon-client -> atm-storage-claude`

## Required Use In Later Sprints

- `AC.2` uses this map to decide what belongs in `atm-storage-claude`
- `AC.3` uses this map to decide what belongs in `atm-storage-rusqlite`
- `AC.4` uses this map to decide what gets removed from `atm-core`
- `AC.6` uses this map as the final backend-leakage delete checklist
- `AC.7` uses this map to prove a future SQL Server backend can remain a peer
  backend under `atm-storage` rather than reopening `atm-core` coupling
