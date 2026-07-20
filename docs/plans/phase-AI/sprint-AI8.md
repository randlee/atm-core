---
title: AI.8 cross-host control plane
status: proposed
branch: feature/pAI-s8-crosshost-control-plane
worktree: ../atm-core-worktrees/feature/pAI-s8-crosshost-control-plane
target: integrate/phase-AI
---

# AI.8 — cross-host control plane

## Deliverables

1. Add storage-trait-backed SQLite records for enabled HTTPS interfaces, local
   certificate identity, and exact trusted peers (host identity + pinned
   fingerprint).
2. Add CLI lifecycle commands to list/manage interfaces, initialize/show the
   local certificate, and explicitly add/replace/revoke trusted peers.
3. Surface safe configured/bound/trust state in `atm doctor`.
4. Forbid environment-controlled peer address, bind address, or trust state.

## Acceptance criteria

- No enabled interface means no HTTPS listener.
- A peer record cannot be added or fingerprint replaced without explicit
  confirmation.
- Configuration is behind the storage trait; HTTP/HTTPS adapters do not use
  rusqlite types.
- Doctor never exposes private key material.

## Required validation

Storage migration/trait tests; CLI integration tests; doctor redaction tests;
`just lint`; `just test`; configuration-boundary gate.
