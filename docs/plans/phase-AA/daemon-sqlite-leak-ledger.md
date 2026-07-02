# Phase AA Daemon SQLite Leak Ledger

## Purpose

Freeze the exact daemon-side SQLite leak inventory and the Phase AA repair
decision now, so later sprints perform mechanical deletion or movement rather
than rediscovering ownership.

## Frozen Leak Decisions

| File | Current leak | Phase AA decision |
| --- | --- | --- |
| `crates/atm-daemon/src/sqlite_observability.rs` | daemon-owned SQLite observability adapter | `delete` |
| `crates/atm-daemon/src/runtime_health_test_support.rs` | daemon-local SQLite test assembly | `delete` |
| `crates/atm-daemon/src/lib.rs` | `SqliteRemoteReplayStore`, `RemoteReplayStateRecord` re-export, direct SQLite observability type use | `keep-and-rewrite` to remove the replay wrapper and all direct `atm_rusqlite` types while preserving the daemon entry surface |
| `crates/atm-daemon/src/composition.rs` | `SqliteBoundaryAssembly::new*`, SQLite observability injection, concrete production boundary construction | `move` concrete assembly to `atm-runtime`; keep daemon composition storage-neutral |
| `crates/atm-daemon/src/runtime_health.rs` | direct `SqliteBoundaryAssembly`, direct roster-store use, WAL checkpoint call, SQLite readiness probing | `keep-and-rewrite` to injected subsystem reports plus daemon-owned runtime state only |
| `crates/atm-daemon/src/runtime_status_cache.rs` | `sqlite_ready`, `sqlite_detail`, `mark_sqlite_unavailable*` | `keep-and-rewrite` to daemon runtime fields only, with SQLite-named status removed |
| `crates/atm-daemon/src/peer_transport.rs` | daemon-owned replay DTO/trait coupled to SQLite-owned record type | `keep-and-rewrite` to a storage-neutral replay DTO/trait owned outside daemon and `atm-rusqlite` |
| `crates/atm-daemon/src/tests.rs` | direct `assemble_boundary(...)` use | `keep-and-rewrite` to composition/subsystem fixtures with no daemon-local SQLite assembly |
| `crates/atm-daemon/src/tests_advisory.rs` | direct `assemble_boundary(...)` use | `keep-and-rewrite` to composition/subsystem fixtures with no daemon-local SQLite assembly |

## Concrete Type Decisions

### `SqliteBoundaryAssembly`

Current location of daemon use:
- `crates/atm-daemon/src/composition.rs`
- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-daemon/src/lib.rs`

Phase AA decision:
- no production use remains in `atm-daemon`
- concrete construction moves to `atm-runtime`

### `RemoteReplayStateRecord`

Current location:
- `crates/atm-rusqlite/src/boundary_assembly.rs`
- re-exported from `crates/atm-daemon/src/lib.rs`

Phase AA decision:
- move to a storage-neutral boundary owned outside both `atm-daemon` and
  `atm-rusqlite`, expected in `atm-core`

### Store/roster health

Current location of daemon probing:
- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-daemon/src/runtime_status_cache.rs`

Phase AA decision:
- deep health belongs to subsystem doctor traits
- daemon aggregates subsystem reports and daemon-owned runtime state only

## WAL Checkpoint Ownership Record

Frozen references that must leave daemon-owned SQLite control flow by `AA.3`:
- `docs/atm-daemon/architecture.md §3.1.2` step `5` documents the current
  graceful-shutdown WAL checkpoint step; the daemon-side implementation leak is
  the direct checkpoint call in `crates/atm-daemon/src/runtime_health.rs`, and
  the Phase AA decision remains `keep-and-rewrite` until that call is removed
- `docs/atm-daemon/architecture.md §3.6` documents the crash-recovery rule that
  graceful-shutdown WAL checkpoint is best-effort only; this is a retained
  contract reference, not permission for daemon-owned SQLite control flow after
  `AA.3`

## Review Rule

If implementation work proposes:
- “keep this SQLite helper in the daemon for convenience”
- “re-export this SQLite type from daemon temporarily”
- “leave sqlite_ready in runtime status for now”

the proposal is out of plan and should be rejected unless the Phase AA docs are
first updated with explicit user-approved justification.
