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
| `AC.2` | `planned` | `feature/pAC-s2-atm-storage-claude-extraction` | `../atm-core-worktrees/feature/pAC-s2-atm-storage-claude-extraction` | Claude inbox storage is extracted behind the shared traits without widening the contract to file-format specifics |
| `AC.3` | `planned` | `feature/pAC-s3-sqlite-backend-convergence` | `../atm-core-worktrees/feature/pAC-s3-sqlite-backend-convergence` | the SQLite backend implements the same approved mail/roster contract, emits notifications only after durable write success, and no longer depends on `atm-core` |
| `AC.4` | `planned` | `feature/pAC-s4-atm-core-storage-boundary-adoption` | `../atm-core-worktrees/feature/pAC-s4-atm-core-storage-boundary-adoption` | `atm-core`, runtime, and daemon paths depend on storage traits rather than concrete backend seams |
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
