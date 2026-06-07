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

- `docs/adr/ADR-017-storage-contract-reset-and-backend-interchangeability.md`

## Open Architectural Issues

| ID | Status | Summary | Planned Closure |
| --- | --- | --- | --- |
| `AC-ISSUE-001` | `open` | the current storage surface is over-modeled with request/response-per-operation wrappers instead of a small semantic contract. | `AC.1` |
| `AC-ISSUE-002` | `open` | message, roster, and task records are duplicated across RPC, storage, and internal boundaries instead of using canonical shared structs. | `AC.1` and `AC.5` |
| `AC-ISSUE-003` | `open` | Claude inbox storage is not currently treated as a first-class storage backend even though ATM 1.0 used it as effective storage. | `AC.2` |
| `AC-ISSUE-004` | `open` | the concrete SQLite backend has become the implicit home of business logic, causing backend-specific seams and logic leakage upward. | `AC.3` and `AC.4` |
| `AC-ISSUE-005` | `open` | notifications are not frozen as a separate post-commit trait, which leaves write and event semantics underspecified across backends. | `AC.1` and `AC.3` |
| `AC-ISSUE-006` | `open` | the current crate graph does not keep future SQL Server support easy because backend crates still orbit `atm-core` too closely. | `AC.3` and `AC.6` |

## Inventory Rules

- `AC.0` is complete when this file and `ADR-017` are accepted as the Phase AC
  baseline.
- New Phase `AC` findings must be added here before they are accepted planning
  scope.
- An issue moves to `closed` only when the owning sprint closes and the
  readiness record references the closure evidence.
- This file is authoritative for the storage-contract reset line.
