# Phase T Task List

## 1. Goal

Close the production-readiness gaps left open at the end of Phase S without
re-opening the completed daemon-baseline work unnecessarily.

Phase S delivered the cross-platform daemon/runtime baseline and the first
round of runtime hardening, but the final integrated review still found:
- missing S.15 SQLite writer-lane architecture
- missing immutable message-row enforcement on the hot mailbox write path
- missing real Windows same-host runtime proof for daemon singleton and local
  IPC behavior
- remaining daemon bounded-state and shutdown-contract gaps
- an integration-gate cleanup bucket on `integrate/phase-S`

Phase T turns those residuals into explicit follow-up sprints with narrower
acceptance gates than the overloaded S.14 / S.15 hardening branches.

Planning baseline:
- `docs/project-plan.md` §25 (Phase S) and §26 (Phase T)
- `integrate/phase-S` review baseline: `bdac03c`
- Phase T planning worktree:
  `/Users/randlee/Documents/github/atm-core-worktrees/integrate/phase-T`

## 2. Scope

Phase T is limited to:
- closing the open integration-gate findings already triaged on
  `integrate/phase-S`
- implementing the real SQLite write-worker design promised by S.15
- enforcing immutable message-row semantics on the hot mailbox write path
- proving real Windows same-host runtime parity for daemon singleton and local
  IPC behavior
- finishing the remaining daemon bounded-state and shutdown-contract hardening

Out of scope:
- new CLI features
- new peer-to-peer protocol features
- re-architecting the daemon beyond the named residual gaps
- broad SQLite tuning not required to land the single-writer and immutable-row
  correctness work

## 3. Why Phase T Exists

The final Phase S review and integration gate exposed a process problem:
- the major residual gaps were documented in the sprint docs and ADRs
- some of those planned deliverables were still not present in the integrated
  code
- the follow-up work needs tighter sprint boundaries so QA can verify concrete
  presence, not just directionally similar hardening

Phase T therefore splits the residual work into:
- one `integrate/phase-S` cleanup sprint (`T.1`)
- four focused follow-up sprints on `integrate/phase-T` (`T.2`–`T.5`)

Forward-integration rule:
- `integrate/phase-T` was branched from `integrate/phase-S` at `bdac03c`
- `T.1` lands on `integrate/phase-S` and reaches `integrate/phase-T` only by
  forward-merge from `develop` after the `integrate/phase-S` gate PR merges
- `T.2`–`T.5` must not open implementation PRs until that forward-merge brings
  the accepted `T.1` fixes into `integrate/phase-T`

## 4. Dependency Graph

| Sprint | Purpose | Base | Depends on | Can run in parallel with |
| --- | --- | --- | --- | --- |
| `T.1` | integrate/phase-S gate patch cleanup | `integrate/phase-S @ bdac03c` | none | `T.2`–`T.5` planning only |
| `T.2` | SQLite single-writer lane | `integrate/phase-T @ bdac03c` | none | `T.4`, `T.5` |
| `T.3` | Immutable message rows + probe removal | `integrate/phase-T @ bdac03c` | `T.2` merge | `T.4`, `T.5` |
| `T.4` | Windows same-host runtime parity | `integrate/phase-T @ bdac03c` | none | `T.2`, `T.5` |
| `T.5` | Remaining daemon hardening | `integrate/phase-T @ bdac03c` | none | `T.2`, `T.4` |

Execution rule:
- `T.1` closes the integration-gate residuals already tracked on
  `integrate/phase-S`
- `T.2` and `T.3` are a correctness pair; do not mark the SQLite write path
  production-ready until both land
- `T.4` and `T.5` may execute independently, but both must land before daemon
  runtime promotion is considered complete

## 5. Planned Sprint Sequence

### T.1 Integration Gate Patch Sprint

Goal:
- close the open `INTG-*` findings already triaged on `integrate/phase-S`
  without broadening scope into new architecture work

Artifacts:
- [`docs/phase-T/sprint-T1-integration-gate.md`](./phase-T/sprint-T1-integration-gate.md)

### T.2 SQLite Single-Writer Lane

Goal:
- land the real crate-private `atm-rusqlite` single-writer lane promised by
  S.15 and `ADR-ATM-RUSQLITE-002`

Artifacts:
- [`docs/phase-T/sprint-T2-sqlite-writer.md`](./phase-T/sprint-T2-sqlite-writer.md)

### T.3 Immutable Message Rows

Goal:
- finish the hot mailbox write-path contract by removing mutable-row conflict
  updates and removing the pre-write probe

Artifacts:
- [`docs/phase-T/sprint-T3-immutable-rows.md`](./phase-T/sprint-T3-immutable-rows.md)

### T.4 Windows Runtime Parity

Goal:
- replace compile-only confidence with real Windows same-host runtime proof for
  daemon singleton, lifecycle, local IPC, and retained-log startup/shutdown

Artifacts:
- [`docs/phase-T/sprint-T4-windows-runtime.md`](./phase-T/sprint-T4-windows-runtime.md)

### T.5 Remaining Hardening

Goal:
- close the remaining daemon bounded-state and shutdown-contract follow-up
  items left open after S.14 / S.15

Artifacts:
- [`docs/phase-T/sprint-T5-hardening.md`](./phase-T/sprint-T5-hardening.md)

## 6. Cross-Sprint Acceptance

Phase T is complete only when:
- `T.1` closes the remaining open `INTG-*` findings on `integrate/phase-S`
- `T.2` lands a real crate-private writer lane used by the hot write path
- `T.3` enforces immutable message-row semantics and removes the probe from the
  hot path
- `T.4` proves real Windows same-host daemon parity through runtime tests, not
  only `xwin` compile checks
- `T.5` closes the remaining bounded-state and shutdown-contract gaps and
  aligns docs with code

## 7. Additional Follow-Up Watchlist

These items are expected to be resolved inside the named T sprints rather than
as a separate `T.6`, but they must still be tracked explicitly during QA:
- `NotificationRuntime::shutdown()` bounded join semantics
- pre-connect terminate checks in peer transport shutdown windows
- orphaned shutdown-helper observability
- `SHUTDOWN_FINALIZER_THREADS` poison and lifetime hardening
- reconcile fingerprint-state bounds beyond key-count-only caps
