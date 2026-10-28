# Anchors - phase-current

Constraints captured during planning. Each anchor is a testable statement written by the orchestrator and approved by the user; the user's verbatim words are kept beneath it as provenance. Every anchor is judged at the planning gate and again at the development gate. Edit `anchors.json`, not this file; this file is rendered.

Intent: `doc/plan/phase-current/intent.md`

| ID | Status | Summary | Stated |
| --- | --- | --- | --- |
| ANC-001 | proposed | Business logic in one Python file | 2026-10-27 |
| ANC-002 | proposed | Business-logic file organized by transition type | 2026-10-27 |

## ANC-001

Anchor: The atm-bd-orchestration business logic must be in a single Python file.

> The business logic is to be in a single small python file which is organized by transition type and enums w/ exhaustive checks (that will compile error if not handled)

Said by Rand, planning session 2026-10-27 (fenix), 2026-10-27. Status: proposed. Version 1.

Summary: Business logic in one Python file

Scope: adds or changes atm-bd-orchestration close hooks, sanity-stack-link, assignment-gates.py, close-vars.py, close_fields.py or console dispatch code

Deterministic check to pair with the development gate: grep for bead/task/stack transition conditions outside the business-logic file finds none

## ANC-002

Anchor: The atm-bd-orchestration transition business-logic file is organized into one section per transition type.

> The business logic is to be in a single small python file which is organized by transition type and enums w/ exhaustive checks (that will compile error if not handled)

Said by Rand, planning session 2026-10-27 (fenix), 2026-10-27. Status: proposed. Version 1.

Summary: Business-logic file organized by transition type

Scope: adds or changes atm-bd-orchestration close hooks, sanity-stack-link, assignment-gates.py, close-vars.py, close_fields.py or console dispatch code

Deterministic check to pair with the development gate: the business-logic file has one top-level section per transition-type enum member
