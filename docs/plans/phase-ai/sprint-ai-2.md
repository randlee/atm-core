---
title: AI.2 storage boundary and composition topology
status: proposed
branch: feature/pAI-s2-storage-topology
worktree: ../atm-core-worktrees/feature/pAI-s2-storage-topology
target: integrate/phase-AI
---

# AI.2 — storage boundary and composition topology

## Deliverables

1. Delete `RemoteReplayStore`, `RuntimeStorageFinalizer`,
   `SqliteRemoteReplayStore`, SQLite replay/finalizer composition fields, and
   their storage schema/error/documentation residue.
2. Make `atm-runtime` a thin trait-object assembly layer with no public SQLite
   type, SQLite observability bridge, or daemon-specific persistence contract.
3. Establish one backend-neutral factory/assembly input so a backend is selected
   at composition without daemon, CLI, graft, or transport backend access.

## Contract

```rust
pub trait StorageFactory: Send + Sync {
    fn open(&self, scope: &HostRuntimeScope) -> Result<Box<dyn MessageStore>, AtmError>;
}
```

`MessageStore` and its related storage traits are the only persistence types
visible outside the selected backend. `atm-storage-rusqlite` alone owns
`rusqlite`, SQL, schema, and migrations.

## Acceptance criteria

- `rg` finds no runtime replay/finalizer type, schema, or SQLite-specific
  daemon service.
- Only the concrete backend owns rusqlite/schema references; daemon and
  transport dependency graphs contain no backend edge.
- The selected backend is visible only as storage traits after composition.
- Existing storage shutdown and doctor behavior remains covered without a
  runtime SQLite escape hatch.

## Required validation

`cargo test -p atm-storage -p atm-runtime -p atm-daemon`; dependency and
architecture checks; `just lint`; `just test`.
