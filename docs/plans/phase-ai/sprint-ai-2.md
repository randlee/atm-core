---
title: AI.2 storage boundary and composition topology
status: complete
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
4. Extend the architecture boundary gate to parse every
   `boundaries/atm-core/*.toml` record. A record whose implementation source is
   absent must be `state = "retired"`, must not name live callers, and must be
   described as historical in `docs/atm-core/boundaries.md`.

## Contract

```rust
pub trait StorageFactory: Send + Sync {
    fn open(&self, durable_state_root: &Path) -> Result<StorageHandles, AtmError>;
}
```

`StorageHandles` contains only the canonical message, roster, and nudge
override traits. Those traits are the only persistence types visible outside
the selected backend. `atm-storage-rusqlite` alone owns `rusqlite`, SQL,
schema, and migrations. `atm-runtime` passes the durable-state path to the
factory but never imports a concrete backend crate.

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

## Non-closure

AI.2 removes storage topology and stale-record escape hatches only. It does
not add an HTTP listener, a peer listener, delivery persistence, or an
alternative storage trait for transport.
