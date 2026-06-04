# Phase AA Issues Inventory

## Goal

Track the known Phase AA issue set so planning, QA, and phase closeout use one
authoritative inventory rather than ad hoc reviewer memory.

## Open Architectural Issues

| ID | Status | Summary | Planned Closure |
| --- | --- | --- | --- |
| `AA-ISSUE-001` | `planned` | `atm-daemon` currently carries a direct `atm-rusqlite` dependency and concrete SQLite references. | `AA.2` through `AA.5` |
| `AA-ISSUE-002` | `planned` | daemon runtime health currently owns SQLite-specific readiness and status fields. | `AA.3` |
| `AA-ISSUE-003` | `planned` | daemon still carries SQLite observability, replay-store, and test-support leakage. | `AA.4` |
| `AA-ISSUE-004` | `planned` | repository enforcement currently trusts boundary TOML as final authority without an independent guard for policy widening. | `AA.5` |

## Inventory Rules

- New Phase AA findings must be added here before they are considered accepted
  planning scope.
- A finding may move to `closed` only when the owning sprint closes and the
  readiness record references the accepted closure evidence.
- This file is the authoritative issues inventory for Phase AA.
