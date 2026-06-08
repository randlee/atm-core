# Phase AC Readiness

## Goal

Track accepted closure for the storage-contract reset line that restores:

- generic RPC envelope
- canonical shared domain structs
- interchangeable storage backends
- Claude storage as a first-class backend

Authoritative supporting inventory:
- `docs/plans/phase-AC/issues.md`

## Sprint Status

| Sprint | Status | Branch | Worktree | Closure Gate |
| --- | --- | --- | --- | --- |
| `AC.0` | `complete` | `plan/phase-AC` | `../atm-core-worktrees/plan/phase-AC` | ADR-018 and the Phase AC issue inventory freeze the original design reset and the target crate graph before execution sprint planning continues |
| `AC.1` | `complete` | `feature/pAC-s1-atm-storage-contract-and-canonical-types` | `../atm-core-worktrees/feature/pAC-s1-atm-storage-contract-and-canonical-types` | `crates/atm-storage` landed with a small audited message/roster contract, canonical shared message/roster structs, separate notification traits, and an `atm-core` compile bridge |
| `AC.2` | `complete` | `feature/pAC-s2-atm-storage-claude-extraction` | `../atm-core-worktrees/feature/pAC-s2-atm-storage-claude-extraction` | `crates/atm-storage-claude` landed as a first-class Claude backend, implements `MessageStore` and `RosterStore` without depending on `atm-core`, and leaves only generic source/projection seam names in `atm-core` during the later consumer-cutover window |
| `AC.3` | `complete` | `feature/pAC-s3-sqlite-backend-convergence` | `../atm-core-worktrees/feature/pAC-s3-sqlite-backend-convergence` | `crates/atm-storage-rusqlite` is the landed backend identity, mail/roster persistence implements the shared `atm-storage` contract without `atm-core`, speculative task/assembly surfaces are deleted from the active backend boundary, and notification semantics remain post-commit only |
| `AC.4` | `complete` | `feature/pAC-s4-atm-core-storage-boundary-adoption` | `../atm-core-worktrees/feature/pAC-s4-atm-core-storage-boundary-adoption` | `atm-core`, runtime, and daemon paths depend on storage traits rather than concrete backend seams; `RuntimeBundle` is removed in favor of `RuntimeDoctorPorts` + `StorageBackends<M,R>` and runtime/daemon no longer own raw SQLite access above `atm-storage-rusqlite` |
| `AC.5` | `planned` | `feature/pAC-s5-rpc-envelope-and-domain-type-unification` | `../atm-core-worktrees/feature/pAC-s5-rpc-envelope-and-domain-type-unification` | RPC uses one generic envelope and canonical body types instead of per-message transport clones |
| `AC.6` | `planned` | `feature/pAC-s6-cleanup-and-deletion-closeout` | `../atm-core-worktrees/feature/pAC-s6-cleanup-and-deletion-closeout` | obsolete wrappers are deleted and backend leakage is closed against the final AC ledger |
| `AC.7` | `planned` | `feature/pAC-s7-sqlserver-readiness-proof` | `../atm-core-worktrees/feature/pAC-s7-sqlserver-readiness-proof` | the final contract is explicitly proven suitable for a future SQL Server backend without another storage-architecture reset |

## Phase Exit Criteria

Phase `AC` is not complete until all of the following are true:

- `crates/atm-storage` exists and remains a small audited storage contract crate
- `docs/adr/ADR-018-storage-contract-reset-and-backend-interchangeability.md` remains the accepted governing reset for Phase `AC`
- the shared storage surface has no request/response-per-operation DTO families
- the shared storage contract has `MessageStore`, `RosterStore`, and a
  separate notification trait
- the shared contract adds no more than four capability traits without a new ADR
- canonical shared `Message` and `Roster*` structs are used at both the RPC
  body layer and the storage layer
- Claude storage and the SQLite backend implement the same approved core
  storage traits
- the concrete SQLite backend does not depend on `atm-core`
- notifications happen only after durable write success
- daemon/runtime/core no longer carry concrete storage logic above the approved composition seam
- the repo has no remaining message-shaped RPC/storage/domain clone families that contradict the generic RPC envelope model
- speculative task-store code is not treated as approved Phase `AC`
  compatibility surface
- `docs/plans/phase-AC/issues.md` has no open issue whose owning sprint is still incomplete
