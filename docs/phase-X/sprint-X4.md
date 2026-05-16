---
id: X.4
title: Replay Contract And IPC Helper Consolidation
status: complete
branch: feature/pXb-s4-replay-and-ipc-consolidation
worktree: ../atm-core-worktrees/feature/pXb-s4-replay-and-ipc-consolidation
target: integrate/phase-Xb
---

# Sprint X.4 — Replay Persistence Contract, Peer Transport, And Same-Host IPC Helpers

## Modification

- This sprint is a restart replay on `feature/pXb-s4-replay-and-ipc-consolidation`.
- Prior Phase `X` already completed the main `X.4` implementation and one
  follow-up fix round:
  - `df124a8605596337b0395e0db2b9cbfb7a404226`
    - `feat: complete phase X replay and ipc consolidation`
  - `3f8338b04181ce0591f2ec98e4c75981cb700bf1`
    - `fix: close phase X4 follow-up findings`
- Replay this sprint by cherry-picking or selectively reapplying those audited
  commits; do not treat `integrate/phase-X` as the authoritative merge base.
- QA must validate the entire `X.4` sprint on `pXb-s4`, including replay
  contract behavior and same-host parity, not only the replayed delta.

## Remaining Restart Work

- replay or selectively re-implement the audited `df124a8...` and
  `3f8338b...` changes onto `feature/pXb-s4-replay-and-ipc-consolidation`
- reconcile any restart-line drift in replay-contract enforcement, peer
  transport decomposition, and same-host helper ownership
- confirm the restarted branch satisfies every `X.4` transport, replay, and
  parity acceptance criterion on the new line
- run full `X.4` QA on `pXb-s4`

## Goal

- make replay persistence startup behavior explicit and enforceable
- decompose the oversized peer-transport paths
- consolidate shared same-host IPC helpers onto one daemon-client-owned line

## Hard Dependencies

- `X.0` merged on `develop`
- `X.1` complete because this sprint touches `composition.rs` after the mailbox
  runtime cutover
- `X.2` complete because the same-host helper and replay-path consolidation
  should land after command-path simplification removes legacy mailbox branches
- `X.3` complete because same-host parity and replay behavior should be fixed
  against the unified daemon-runtime truth model

## Exact Targets

- `crates/atm-daemon/src/composition.rs`
- `crates/atm-daemon/src/peer_transport.rs`
- `crates/atm-daemon-client/src/lib.rs`
- `crates/atm/src/composition.rs`
- `crates/atm-graft/src/lib.rs`
- `crates/atm-graft/src/runtime.rs`
- `crates/atm-graft/src/transport.rs`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-daemon-client/boundaries.md`
- `boundaries/atm-daemon-client/daemon-bootstrap.toml`

## Required Work

- document the replay-store startup contract as either fail-closed or explicitly
  allowed reduced-capability startup
- make `composition.rs` enforce that contract instead of leaving
  `replay_store = None` implicit
- refactor `send_to_endpoint(...)` below `80` lines
- refactor `send_once(...)` below `80` lines
- consolidate same-host `try_connect(...)`, `exchange(...)`, and
  `unexpected_response(...)` into the shared daemon-client line
- consolidate duplicate daemon-unavailable and unexpected-response behavior
  across `atm` and `atm-graft`
- update `docs/atm-daemon-client/boundaries.md` so the boundary contract
  explicitly allows daemon-client ownership of those shared same-host helpers
- update `boundaries/atm-daemon-client/daemon-bootstrap.toml` so the
  machine-readable boundary contract reflects the same helper ownership

## Acceptance Criteria

- one replay-persistence startup contract is documented in product and
  daemon-local docs
- daemon startup behavior in `composition.rs` matches the documented contract
- `send_to_endpoint(...)` is under `80` lines
- `send_once(...)` is under `80` lines
- `rg -n "fn try_connect\\(|fn exchange\\(|fn unexpected_response\\(" crates/atm crates/atm-graft crates/atm-daemon-client`
  finds one shared helper definition per helper name
- CLI and graft same-host paths share the same daemon-unavailable and
  unexpected-response behavior
- peer transport preserves the shared ATM error/recovery contract after
  `send_to_endpoint(...)` and `send_once(...)` refactor

## Delivered

- documented replay persistence startup as a fail-closed daemon dependency in:
  - `docs/requirements.md`
  - `docs/architecture.md`
  - `docs/atm-daemon/requirements.md`
  - `docs/atm-daemon/architecture.md`
- changed daemon composition to fail startup when the replay store cannot be
  assembled, instead of degrading to `replay_store = None`
- added direct coverage for the fail-closed replay-store startup contract in
  `crates/atm-daemon/src/composition.rs` tests
- refactored `send_to_endpoint(...)` to `43` lines and `send_once(...)` to
  `19` lines while preserving the shared peer-transport ATM error and recovery
  contract
- collapsed same-host helper ownership so the only remaining
  `try_connect(...)`, `exchange(...)`, and `unexpected_response(...)`
  definitions live in `crates/atm-daemon-client/src/lib.rs`
- updated CLI and graft same-host client wrappers to call the shared
  daemon-client helper line without keeping duplicate helper definitions
- moved peer transport retry-budget resolution up into daemon composition via
  `ConfigIngress` so the runtime no longer self-loads workspace config or
  silently defaults the retry budget after a config-load failure

Implementation result:
- the X.4 acceptance criteria are satisfied on
  `feature/pXb-s4-replay-and-ipc-consolidation`
- the machine-readable and prose daemon-client boundary contracts already
  matched the intended helper ownership on this branch baseline, so no extra
  boundary-doc delta was required in this sprint

## Required Validation

- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`
