# Phase AC Issues Inventory

## Goal

Track the accepted issue set for the storage-contract reset line so planning,
QA, and closure use one authoritative inventory.

## Baseline Evidence

The accepted `AC.0` baseline is:

- the shared storage-facing surface in `atm-core` is materially oversized:
  - roughly `13` traits
  - roughly `95` structs
  - roughly `3` enums
- the current implementation violates the original architecture that required:
  - generic RPC envelopes
  - canonical shared domain structs
  - interchangeable storage backends

The authoritative design reset is:

- `docs/adr/ADR-018-storage-contract-reset-and-backend-interchangeability.md`

## Open Architectural Issues

| ID | Status | Summary | Planned Closure |
| --- | --- | --- | --- |
| `AC-ISSUE-001` | `closed` | the current storage surface is over-modeled with request/response-per-operation wrappers instead of a small semantic contract. | `AC.1` |
| `AC-ISSUE-002` | `closed` | message and roster records were duplicated across RPC, storage, and internal boundaries instead of using canonical shared structs; speculative task-store records also existed and were deleted in `AC.6` instead of being promoted into the approved shared contract. | `AC.1`, `AC.5`, and `AC.6` |
| `AC-ISSUE-003` | `closed` | Claude inbox storage is not currently treated as a first-class storage backend even though ATM 1.0 used it as effective storage. | `AC.2` |
| `AC-ISSUE-004` | `closed` | the concrete SQLite backend had become the implicit home of business logic, causing backend-specific seams and logic leakage upward; `AC.6` completed the cleanup by removing the last `atm-storage`-level SQLite observability leakage and leaving that surface owned by `atm-storage-rusqlite`. | `AC.3`, `AC.4`, and `AC.6` |
| `AC-ISSUE-005` | `closed` | notifications are not frozen as a separate post-commit trait, which leaves write and event semantics underspecified across backends. | `AC.1` and `AC.3` |
| `AC-ISSUE-006` | `closed` | the current crate graph did not keep future SQL Server support easy because backend crates orbited `atm-core` too closely; Phase `AC` now proves peer backends can target `atm-storage` directly, including the compile-only SQL Server proof crate with no `atm-core` edge. | `AC.3` and `AC.7` |
| `AC-ISSUE-007` | `open` | `atm-graft` still carries an unconditional compile-time dependency on `atm-daemon-bootstrap`, which leaks `atm-runtime` and `atm-storage-rusqlite` into a thin client even though same-host daemon auto-start should remain only a thin-client convenience path. | `AC.8` |

## Inventory Rules

- `AC.0` is complete when this file and `ADR-018` are accepted as the Phase AC
  baseline.
- New Phase `AC` findings must be added here before they are accepted planning
  scope.
- An issue moves to `closed` only when the owning sprint closes and the
  readiness record references the closure evidence.
- This file is authoritative for the storage-contract reset line.
