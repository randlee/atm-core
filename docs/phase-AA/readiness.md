# Phase AA Readiness

## Goal

Track the accepted closure state for the daemon simplification line that
removes concrete SQLite knowledge from `atm-daemon`.

Authoritative supporting inventory:
- `docs/phase-AA/issues.md`

## Sprint Status

| Sprint | Status | Branch | Worktree | Closure Gate |
| --- | --- | --- | --- | --- |
| `AA.0` | `planned` | `feature/pAA-s0-daemon-architecture-restatement` | `../atm-core-worktrees/feature/pAA-s0-daemon-architecture-restatement` | daemon role, doctor aggregation model, and state-machine inventory documented and accepted |
| `AA.1` | `planned` | `feature/pAA-s1-subsystem-doctor-traits` | `../atm-core-worktrees/feature/pAA-s1-subsystem-doctor-traits` | subsystem-owned capability/doctor traits and shared diagnostic DTOs land in the governing docs and code |
| `AA.2` | `planned` | `feature/pAA-s2-atm-runtime-composition-transfer` | `../atm-core-worktrees/feature/pAA-s2-atm-runtime-composition-transfer` | `atm-runtime` owns concrete SQLite/runtime assembly; `atm-daemon` stops constructing SQLite boundaries |
| `AA.3` | `planned` | `feature/pAA-s3-direct-doctor-and-runtime-health-split` | `../atm-core-worktrees/feature/pAA-s3-direct-doctor-and-runtime-health-split` | `atm doctor` regains direct local store diagnostics; daemon aggregates injected subsystem reports plus daemon-owned runtime state without backend-specific diagnosis logic |
| `AA.4` | `planned` | `feature/pAA-s4-delete-daemon-sqlite-leaks` | `../atm-core-worktrees/feature/pAA-s4-delete-daemon-sqlite-leaks` | remaining SQLite observability, replay, and test-support leaks are removed from `atm-daemon` |
| `AA.5` | `planned` | `feature/pAA-s5-boundary-relock-and-permanent-enforcement` | `../atm-core-worktrees/feature/pAA-s5-boundary-relock-and-permanent-enforcement` | daemon-to-SQLite edge is forbidden again and a second enforcement layer exists beyond TOML lint |

## Phase Exit Criteria

`Phase AA` is not complete until all of the following are true:

- `crates/atm-daemon/Cargo.toml` has no direct `atm-rusqlite` dependency
- no `atm_rusqlite::*` references remain in daemon production code
- direct local store diagnostics exist behind subsystem-owned doctor traits
- storage behavior is expressed through small behavior-named capability traits,
  not backend-shaped interfaces
- daemon doctor aggregation does not inspect SQLite internals directly and only
  compares subsystem reports at the aggregate level
- the boundary TOMLs forbid the daemon-to-SQLite edge again
- a repository-enforced dependency-boundary test or equivalent guard fails
  whenever that edge reappears
- a `boundary-guard` QA agent reviews both plans and phase-ending reviews
  and flags boundary-policy widening before closure
- `docs/phase-AA/issues.md` has no open issue whose planned closure sprint is
  still incomplete
