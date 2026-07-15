---
id: AG.1
title: Cross-Host Setup Contract And Channel Bring-Up
status: in_progress
branch: feature/pAG-s1-macos-execution
worktree: ../atm-core-worktrees/feature/pAG-s1-macos-execution
target: develop
---

# Sprint AG.1 — Cross-Host Setup Contract And Channel Bring-Up

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.1
worktree: ../atm-core-worktrees/feature/pAG-s1-macos-execution
branch: feature/pAG-s1-macos-execution
status: in_progress
estimated_scope: medium
```

## Goal

Document an operational Windows/macOS clean-room setup contract and use it to
attempt the first live cross-host daemon-to-daemon channel.

## Deliverables

- `cross-host-setup-runbook.md`
- frozen clean-room env contract for both hosts
- checklist rows `AG-VAL-001` and `AG-VAL-002`
- transport-security requirement disposition row `AG-VAL-011`
- exact first-live-channel validation order
- exact evidence contract for setup and bring-up failures
- one AG.1-only first-live-channel viability attempt that can open a finding
  but does not formally close `AG-VAL-003` or later rows
- production inbound peer-listener fix for `AG-FIND-004`, landed in-sprint
  after the viability attempt exposed a real daemon-to-daemon bring-up defect

## Required Validation

- `docs/plans/phase-AG/cross-host-smoke-checklist.md`
  - `AG-VAL-001`
  - `AG-VAL-002`
  - `AG-VAL-011`
  - AG.1 viability may exercise `AG-VAL-003` or `AG-VAL-005`, but does not
    formally close them
- peer-listener deliverable
  - `cargo test -p atm-daemon peer_transport -- --nocapture`
  - `cargo test -p atm-daemon -- --nocapture`
  - evidence artifact: retained `docs/plans/phase-AG/reports/macos-report.md`
    entry naming PR #551 / the validation commit used for the AG.1 rerun

## Ownership

- execution owner: `arch-ctm`
- host operators: `windows-operator`, `macos-operator`
- verification owner: `quality-mgr`

## Acceptance Criteria

- the runbook is concrete enough that both hosts can execute without guessing
- the first live channel attempt has a defined pass/fail evidence contract
- setup ambiguity is classified as a finding instead of being hand-waved away
- the peer-listener fix is production-ready only if the inbound listener
  starts, reload/rebind degradation is queryable in doctor/runtime status, and
  the bounded peer transport tests above pass on the sprint branch

## PeerServerTransport contract

```rust
pub(super) struct PeerServerTransport {
    listen_addr: Mutex<Option<SocketAddr>>,
    observability: SubsystemObservability,
    state: Mutex<Option<PeerServerHandle>>,
    status_cache: RuntimeStatusCache,
}

impl PeerServerTransport {
    pub(super) fn new(
        listen_addr: Option<SocketAddr>,
        observability: SubsystemObservability,
        status_cache: RuntimeStatusCache,
    ) -> Self;
    pub(super) fn start(&self, dispatcher: Arc<dyn RequestDispatcher + Send + Sync>)
        -> Result<(), AtmError>;
    pub(super) fn shutdown(&self) -> Result<(), AtmError>;
    pub(super) fn reload(
        &self,
        listen_addr: Option<SocketAddr>,
        dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
    ) -> Result<(), AtmError>;
}
```

Notes:

- `listen_addr` comes from `daemon.peer_listen_addr`
- `reload(...)` must preserve one bounded runtime view: if rebind fails, doctor
  surfaces degraded listener state until a later successful rebind clears it
