# Phase AA Issues Inventory

## Goal

Track the known Phase AA issue set so planning, QA, and phase closeout use one
authoritative inventory rather than ad hoc reviewer memory.

AA.0 freezes this inventory as the starting point for the execution line.

## Open Architectural Issues

| ID | Status | Summary | Planned Closure |
| --- | --- | --- | --- |
| `AA-ISSUE-001` | `closed` | `atm-daemon` currently carries a direct `atm-rusqlite` dependency and concrete SQLite references. | `AA.2` through `AA.5` |
| `AA-ISSUE-002` | `closed` | daemon runtime health currently owns SQLite-specific readiness and status fields. | `AA.3` |
| `AA-ISSUE-003` | `closed` | daemon still carries SQLite observability, replay-store, and test-support leakage. | `AA.4` |
| `AA-ISSUE-004` | `closed` | repository enforcement currently trusts boundary TOML as final authority without an independent guard for policy widening. | `AA.5` |
| `AA-ISSUE-005` | `closed` | the repo still misclassifies the current Claude JSON-array inbox file shape as legacy in runtime/docs/smoke wording even though real `team-lead -> quality-mgr` traffic uses that current Claude path. | `AA.9` |
| `AA-ISSUE-006` | `closed` | schema docs, Pydantic models, and live historical samples are not frozen together as one current Claude Code inbox contract. | `AA.8` |
| `AA-ISSUE-007` | `closed` | active docs/tests no longer present historical ATM-owned inbox JSON variants as the primary contract; 1.2 now treats them as read-compatible derivatives only while still accepting legal additive inputs on read. | `AA.10` |
| `AA-ISSUE-008` | `closed` | pre-production SQLite identity compatibility scaffolding such as `legacy_message_id` no longer remains on the active runtime line; surviving mentions are historical inventory only. | `AA.11` |
| `AA-ISSUE-009` | `open` | one malformed Claude inbox fragment can still hide unrelated valid messages or surface an opaque parser failure instead of a degraded salvage result. | `AA.12` |

## Inventory Rules

- New Phase AA findings must be added here before they are considered accepted
  planning scope.
- A finding may move to `closed` only when the owning sprint closes and the
  readiness record references the accepted closure evidence.
- Smoke QA follow-up findings stay out of Phase AA planning scope until
  `team-lead` routes the concrete finding ids / verdict summary into this file.
- This file is the authoritative issues inventory for Phase AA.
