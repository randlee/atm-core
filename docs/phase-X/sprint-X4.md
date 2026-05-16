---
id: X.4
title: Replay Contract And IPC Helper Consolidation
status: replayed
branch: feature/pXb-s4-replay-and-ipc-consolidation
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pXb-s4-replay-and-ipc-consolidation
target: integrate/phase-Xb
---

# Sprint X.4 — Replay Persistence Contract, Peer Transport, And Same-Host IPC Helpers

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

## Replay Status

- replayed on `feature/pXb-s4-replay-and-ipc-consolidation` at `f10c9ec`
- replayed from prior Phase `X` salvage commits:
  - `df124a8`
  - `3f8338b`
- `quality-mgr` alignment review already confirmed the replayed branch matches
  the intended sprint deliverables at a non-critical level

## Already Complete On Restart Branch

- the replay-store startup contract changes are replayed onto the restart line
- same-host helper consolidation onto `atm-daemon-client` is replayed onto the
  restart branch
- the peer-transport support changes from the prior Phase `X` follow-up pass
  are already present on this branch

## Remaining Restart Work

- split
  `crates/atm-daemon-client/src/lib.rs:ensure_daemon_available_with_lock_path_impl`
  below the RULE-002 `80`-line limit
- move peer-transport retry-budget loading out of
  `PeerClientTransport::new_with_observability(...)` direct
  `load_workspace_config(...)` calls and back behind the daemon-owned
  `ConfigIngress` boundary or explicit composition-time injection
- stop silently falling back to the default remote retry budget when
  peer-transport config loading fails; either:
  - fail explicitly during daemon composition, or
  - document and emit a typed degraded contract that matches requirements and
    architecture
- run full sprint QA and validation on
  `feature/pXb-s4-replay-and-ipc-consolidation`
- if a later closeout sprint still needs manifest or dependency cleanup tied to
  this replay, keep that cleanup on `X.5` unless `X.4` cannot validate
  independently without it

## Exact Targets

- `crates/atm-daemon/src/composition.rs`
- `crates/atm-daemon/src/peer_transport.rs`
- `crates/atm-daemon/src/boundary_adapters.rs`
- `crates/atm-daemon/src/direct_boundaries.rs`
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
- keep peer-transport configuration ownership aligned with
  `DaemonConfigIngressAdapter`; peer transport must not become a second config
  loader
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
- remote retry-budget loading uses one daemon-owned configuration path and does
  not silently erase config-load failures with an implicit default
- `send_to_endpoint(...)` is under `80` lines
- `send_once(...)` is under `80` lines
- `rg -n "fn try_connect\\(|fn exchange\\(|fn unexpected_response\\(" crates/atm crates/atm-graft crates/atm-daemon-client`
  finds one shared helper definition per helper name
- CLI and graft same-host paths share the same daemon-unavailable and
  unexpected-response behavior
- peer transport preserves the shared ATM error/recovery contract after
  `send_to_endpoint(...)` and `send_once(...)` refactor

## Required Validation

- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`
