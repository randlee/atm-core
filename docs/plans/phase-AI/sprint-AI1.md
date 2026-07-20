---
title: AI.1 daemon baseline reset
status: proposed
branch: feature/pAI-1-daemon-preag-reset
worktree: ../atm-core-worktrees/feature/pAI-1-daemon-preag-reset
target: integrate/phase-AI
---

# AI.1 — daemon baseline reset

## Deliverables

1. Preserve and validate the clean deletion baseline already landed on
   `feature/pAI-1-daemon-preag-reset` from `integrate/phase-AI`; do not revive
   the superseded reset branch or copy its AG plans/generated gate churn.
2. Delete the retired peer/replay subsystem: `crates/atm-daemon/src/peer_transport.rs`,
   `crates/atm-runtime/src/replay_store.rs`, their composition wiring, schema
   DDL, error codes, and documentation claims.
3. Retain and prove only singleton ownership, current local IPC, request
   dispatch, SQLite storage, and post-send emission as the starting daemon.
4. Reconcile primary daemon/core requirements with the smaller baseline. The
   HTTP/UDS design is planned, not implemented in AI.1.

## Acceptance criteria

- `rg` finds no compiled production reference to peer transport, replay store,
  remote replay state, deferred receipt, or remote retry queue.
- Fresh-schema tests contain no retired cross-host table. Existing database
  upgrade behavior is documented and does not alter unrelated user data.
- Singleton/local daemon tests, `just lint`, and `just test` pass.
- The AI.1 architecture check proves the deletion structurally and reports
  retained daemon modules explicitly.

## Required validation

`cargo test -p atm-daemon -p atm-runtime`; `just lint`; `just test`; AI.1
deletion gate; local daemon doctor/send/read/ack smoke.
