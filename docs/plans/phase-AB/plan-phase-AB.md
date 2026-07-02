---
title: Phase AB Plan
status: complete
branch: plan/phase-AB
worktree: ../atm-core-worktrees/feature/phase-AA-cross-host-smoke-plan
---

# Phase AB Plan

## Goal

Validate cross-host ATM messaging between Windows and macOS on real binaries
after the Phase Z single-host release-readiness line is complete.

Phase `AB` owns executable cross-host smoke coverage that was intentionally left
outside the same-host daemon and release-signoff line:

- Windows <-> macOS daemon/bootstrap interoperability on disposable state
- peer transport bring-up across hosts
- durable send/read/ack behavior across hosts
- degraded notification behavior after durable cross-host delivery
- retry-visible observability during cross-host interruption and restart
- copied-state revalidation only after the clean-room lane passes

## Baseline

- planning branch: `plan/phase-AB`
- follow-up planning branch for post-`AB.1` fixes, if needed:
  - `plan/phase-AB-fix-planning`
- prerequisite accepted line:
  - `Phase Z` complete on `develop`
  - Windows same-host build/test parity restored on the accepted baseline
- execution integration branch:
  - `integrate/phase-AB`

## Phase Entry Criteria

`Phase AB` does not begin until:

- `Phase Z` remains closed with the release-ready verdict already recorded
- the accepted `develop` baseline includes the `AC.8` thin-client bootstrap
  dependency relock
- the accepted `develop` baseline passes the same-host workspace test suite on:
  - Windows
  - macOS
- the Windows release-binary daemon client path is proven healthy for:
  - `doctor --json`
  - `list --json`
  - `clear --json`
  - `send --json`
  - `read --all --json`
- both participating hosts can run disposable ATM homes/config roots without
  touching live host state
- the operator has confirmed any host firewall / local-network prompts before
  the active smoke pass begins

## Scope Rules

Phase `AB` is a smoke and interoperability line, not a new transport-design
phase.

Phase `AB` may:

- add smoke fixtures, checklists, reports, and targeted cross-host bug fixes
- harden existing cross-host delivery/bootstrap code when the bug is required to
  make the smoke matrix pass
- add explicit operator guidance for cross-host setup and recovery
- add notes that cross-host daemon operations must not block the async runtime
  without explicit justification
- rely on the existing ATM message payload size limits; `Phase AB` does not add
  a separate cross-host payload limit layer

Phase `AB` must not:

- redesign the peer transport contract without a separate ADR/planning line
- absorb unrelated same-host daemon refactors
- use live host state as the first validation lane
- treat notification-only degradation as a durable-delivery failure

## Validation Lanes

### Lane A: Disposable Clean-Room Cross-Host Smoke

Purpose:

- prove the full Windows/macOS cross-host flow on synthetic state only
- keep failures attributable to config/bootstrap/transport/runtime boundaries

Required shape:

- one disposable `ATM_HOME` per host
- one disposable `ATM_CONFIG_HOME` per host
- explicit `ATM_LOG_DIR` per host
- explicit cross-host transport configuration per host
- cross-host transport configuration is validated before any send is attempted;
  misconfiguration must fail fast with a clear error instead of silently
  hanging or surfacing a misleading downstream failure
- no reads or writes against live `~/.claude` or `~/.atm`

### Lane B: Copied-State Cross-Host Revalidation

Purpose:

- prove cross-host delivery on a disposable copy of realistic retained state

Entry condition:

- Lane A must already pass end to end

Required shape:

- disposable copy of each host's ATM/Claude state
- no writes against live host-scoped state
- exact capture of any repair/setup step needed before send/read/ack succeeds

## Sprint Sequence

### AB.1 Cross-Host Harness And Clean-Room Baseline

Purpose:

- freeze the Windows/macOS host-pair smoke checklist
- define disposable env/bootstrap rules for both hosts
- prove same-host release commands on both hosts under disposable state before
  any cross-host send is attempted

Execution branch:
- `feature/pAB-s1-cross-host-harness-and-clean-room-baseline`

Execution worktree:
- `../atm-core-worktrees/feature/pAB-s1-cross-host-harness-and-clean-room-baseline`

### AB.2 One-Way Cross-Host Delivery

Purpose:

- prove Windows -> macOS durable delivery on the disposable lane
- prove macOS -> Windows durable delivery on the disposable lane
- record exact transport/bootstrap evidence in retained logs on both hosts

Execution branch:
- `feature/pAB-s2-one-way-cross-host-delivery`

Execution worktree:
- `../atm-core-worktrees/feature/pAB-s2-one-way-cross-host-delivery`

### AB.3 Cross-Host Ack Round-Trip

Purpose:

- prove `--requires-ack` send plus reply-state mutation across hosts
- validate receiver read and sender reply visibility on the disposable lane

Execution branch:
- `feature/pAB-s3-cross-host-ack-round-trip`

Execution worktree:
- `../atm-core-worktrees/feature/pAB-s3-cross-host-ack-round-trip`

### AB.4 Degraded Notification And Retry-Visible Recovery

Purpose:

- prove durable cross-host delivery still succeeds when notification/hook paths
  degrade
- prove retry-visible observability during daemon restart or temporary peer
  unavailability
- require explicit bounded timeouts for cross-host send and retry operations so
  every smoke row completes within a defined wall-clock window
- capture evidence that in-flight work was drained or queued before a daemon
  restart is treated as complete

Execution branch:
- `feature/pAB-s4-degraded-notification-and-retry-visible-recovery`

Execution worktree:
- `../atm-core-worktrees/feature/pAB-s4-degraded-notification-and-retry-visible-recovery`

### AB.5 Copied-State Revalidation And Readiness Closeout

Purpose:

- rerun the approved subset on disposable copied state from both hosts
- capture any remaining operator setup or repair guidance
- record the phase readiness verdict

Execution branch:
- `feature/pAB-s5-copied-state-revalidation-and-readiness-closeout`

Execution worktree:
- `../atm-core-worktrees/feature/pAB-s5-copied-state-revalidation-and-readiness-closeout`

## Required Smoke Coverage

The frozen `AB.1` checklist must include at least:

- release-binary `doctor` on Windows and macOS individually
- Windows -> macOS send
- macOS -> Windows send
- cross-host `read` on the receiving side
- cross-host `ack` back to the original sender
- one degraded-notification case after durable cross-host send succeeds
- one retry-visible interruption/recovery case
- one copied-state lane after clean-room success

## Required Evidence Per Row

Every smoke row must capture:

- host pair and sender/receiver direction
- exact disposable env/config inputs
- command transcript on both hosts
- sender JSON result
- receiver read/ack JSON result
- `doctor --json` for the active daemon host when relevant
- `log snapshot --json` from both hosts when the row exercises daemon-backed
  behavior
- for restart or temporary-unavailability rows, evidence that shutdown drained
  or queued in-flight work before the restart completed

## Immediate Planning Outputs

- `docs/plans/phase-AB/plan-phase-AB.md`
- `docs/plans/phase-AB/cross-host-smoke-checklist.md`
- `docs/plans/phase-AB/cross-host-findings-ledger.md`
- `docs/plans/phase-AB/readiness.md`
- `docs/plans/phase-AB/sprint-AB1.md`
- `docs/plans/phase-AB/sprint-AB2.md`
- `docs/plans/phase-AB/sprint-AB3.md`
- `docs/plans/phase-AB/sprint-AB4.md`
- `docs/plans/phase-AB/sprint-AB5.md`

## Acceptance / Phase Entry Gate

- `Phase Z` must remain closed and accepted on `develop`
- `AB.1` must freeze the authoritative Windows/macOS checklist before later
  smoke/fix sprints widen execution
- `AB.5` must not begin until `AB.2` through `AB.4` are complete on the
  accepted `integrate/phase-AB` line
- the phase closeout verdict must remain `FAIL` until both:
  - disposable clean-room cross-host smoke passes
  - copied-state cross-host revalidation passes
