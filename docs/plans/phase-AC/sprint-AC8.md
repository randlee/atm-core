# AC.8 Thin-Client Same-Host Bootstrap Dependency Relock

```yaml
plan_type: sprint_plan
phase: AC
sprint: AC.8
worktree: ../atm-core-worktrees/feature/pAC-s8-thin-client-bootstrap-dependency-relock
branch: feature/pAC-s8-thin-client-bootstrap-dependency-relock
status: planned
estimated_scope: small
```

## Goal

Remove the unconditional `atm-graft -> atm-daemon-bootstrap` compile-time edge
while preserving the standard same-host thin-client convenience path:

- resolve the canonical daemon endpoint and daemon binary from the normal ATM
  environment/config path
- auto-start the daemon when launch conditions are met
- keep the RPC surface unchanged

## Scope Summary

This sprint is a narrow structural extraction. It does not redesign daemon
policy, graft session lifecycle, or the RPC contract.

Production-ready commitment:
- every listed deliverable must land with the real dependency leak removed and
  with the standard convenience autostart behavior still intact for thin
  clients

Primary closure rule:
- `AC.8` closes only when `atm-graft` no longer links `atm-daemon-bootstrap`
  transitively, yet both `atm` and `atm-graft` still use the same thin-client
  same-host bootstrap helper seam

## Bootstrap Seam Contract

`AC.8` must land one explicit shared thin-client helper seam in
`crates/atm-daemon-client/src/lib.rs`:

```rust
pub fn resolve_daemon_local_ipc_endpoint() -> Result<DaemonLocalIpcEndpoint, AtmError>;
pub fn resolve_daemon_bin(current_host_label: &str) -> Result<DaemonBinaryPath, AtmError>;
```

Required `AC.8` consumer call sites:

- `crates/atm/src/composition.rs`
- `crates/atm-graft/src/lib.rs`

Required `AC.8` behavior at those call sites:

- both clients resolve the canonical same-host endpoint through
  `resolve_daemon_local_ipc_endpoint()`
- both clients resolve the daemon binary through
  `resolve_daemon_bin(current_host_label)`
- both clients construct `DaemonSupervisor` from those resolved values
- both clients keep the supervised convenience auto-start path
- neither client depends on `atm-daemon-bootstrap` to reach that path

Compatibility-shim rule:

- `AC.8` does not use a compatibility-shim closure mode
- helper ownership and thin-client call-site migration close in the same sprint
- `atm` and `atm-graft` must import the shared seam directly from
  `atm-daemon-client`

## Governing Sources

- `docs/plans/phase-AC/plan-phase-AC.md`
- `docs/plans/phase-AC/readiness.md`
- `docs/plans/phase-AC/issues.md`
- `docs/atm-graft/architecture.md`
- `docs/atm-graft/boundaries.md`
- `docs/atm-daemon-client/boundaries.md`
- `boundaries/atm-graft/shared-client-consumer.toml`
- `boundaries/atm-daemon-client/daemon-bootstrap.toml`

## Prerequisites

- accepted post-`AC.7` baseline on `develop`

## Out Of Scope

- no RPC envelope changes
- no daemon/runtime/storage backend redesign
- no removal of convenience autostart from `atm` or `atm-graft`
- no version-lock requirement between `atm-graft` and the primary `atm`
  install beyond RPC compatibility
- no broader `atm-graft` API redesign beyond the helper-seam migration and
  dependency relock documented here

## Deliverables

- `atm-daemon-client` owns the standard same-host daemon endpoint and daemon
  binary resolution helpers used by thin clients
- `atm` and `atm-graft` both consume those shared thin-client helpers for the
  convenience bootstrap path
- `boundaries/atm-daemon-client/daemon-bootstrap.toml` and
  `boundaries/atm-graft/shared-client-consumer.toml` are updated to encode the
  relocked thin-client ownership and dependency rules enforced by
  `python3 .just/lint_boundaries.py`
- `crates/atm-graft/Cargo.toml` no longer depends on `atm-daemon-bootstrap`
- `atm-graft` no longer transitively pulls `atm-runtime`,
  `atm-storage-rusqlite`, `rusqlite`, or `libsqlite3-sys`
- the endpoint/bin helper ownership no longer remains in
  `atm-daemon-bootstrap`

## Acceptance Criteria

- `cargo tree -p atm-graft -e normal --prefix none` does not include
  `atm-daemon-bootstrap`
- `cargo tree -p atm-graft -e normal --prefix none` does not include
  `atm-runtime`
- `cargo tree -p atm-graft -e normal --prefix none` does not include
  `atm-storage-rusqlite`
- `cargo tree -p atm-graft -e normal --prefix none` does not include
  `rusqlite`
- `cargo tree -p atm-graft -e normal --prefix none` does not include
  `libsqlite3-sys`
- `GraftClient::connect()` still resolves the standard same-host daemon
  endpoint/binary and still attempts daemon auto-start through
  `DaemonSupervisor`
- CLI bootstrap still uses the same shared thin-client helper seam
- `crates/atm/src/composition.rs` and `crates/atm-graft/src/lib.rs` both call
  the shared `atm-daemon-client` helper seam rather than
  `atm-daemon-bootstrap`
- `atm-daemon-bootstrap` no longer owns the endpoint/bin helper seam
- the RPC surface exposed by `atm-daemon-client` and consumed by `atm-graft`
  remains unchanged
- the machine-readable boundary records for the shared thin-client consumer and
  daemon-bootstrap seam pass `python3 .just/lint_boundaries.py` with the
  relocked dependency policy encoded directly in the TOMLs

## Required Validation

- `cargo test -p atm-daemon-client`
- `cargo test -p atm-graft`
- `cargo test -p atm`
- `cargo tree -p atm-graft -e normal --prefix none`
- `cargo tree -p atm-daemon-client -e normal --prefix none`
- `python3 .just/lint_boundaries.py`
- `git diff --check`
- `rg -n "resolve_daemon_local_ipc_endpoint|resolve_daemon_bin" crates/atm-daemon-client/src/lib.rs`
- `rg -n "atm_daemon_client::.*resolve_daemon|resolve_daemon_local_ipc_endpoint|resolve_daemon_bin" crates/atm/src/composition.rs crates/atm-graft/src/lib.rs`
- `! rg -n "atm_daemon_bootstrap::.*resolve_daemon|resolve_daemon_local_ipc_endpoint|resolve_daemon_bin" crates/atm/src/composition.rs crates/atm-graft/src/lib.rs`
- `! rg -n "pub fn resolve_daemon_local_ipc_endpoint|pub fn resolve_daemon_bin" crates/atm-daemon-bootstrap/src/lib.rs`
- `! rg -n "atm-daemon-bootstrap" crates/atm-graft/Cargo.toml`
- `! cargo tree -p atm-graft -e normal --prefix none | rg "rusqlite|libsqlite3-sys"`

## Required Document Updates

- `docs/plans/phase-AC/sprint-AC8.md`
- `docs/plans/phase-AC/plan-phase-AC.md`
- `docs/plans/phase-AC/readiness.md`
- `docs/plans/phase-AC/issues.md`
- `boundaries/atm-daemon-client/daemon-bootstrap.toml`
- `boundaries/atm-graft/shared-client-consumer.toml`
- `docs/atm-daemon-client/boundaries.md`
- `docs/atm-graft/boundaries.md`
- `docs/atm-graft/architecture.md`
- `docs/atm-graft/requirements.md`
- `docs/atm/architecture.md`
- `docs/atm/requirements.md`

## Risks And Watchouts

- moving the helper seam must not reintroduce runtime/storage knowledge into
  thin clients
- preserving convenience autostart must not be mistaken for restoring a daemon
  composition dependency
- if `atm-daemon-client` depends on `atm-core` for canonical ATM
  environment/endpoint resolution, that new edge must stay free of
  `atm-runtime` and concrete storage backend leakage
- the sprint must not claim closure by leaving the helper seam effectively
  owned by `atm-daemon-bootstrap` under a renamed or delegated path

## Review Notes

- `AC.8` is a post-closeout follow-on sprint on the completed `Phase AC`
  crate-graph cleanup line
- this sprint exists because thin-client bootstrap convenience is acceptable,
  but the prior `atm-graft -> atm-daemon-bootstrap -> atm-runtime ->
  atm-storage-rusqlite` edge violates the intended thin-client boundary
