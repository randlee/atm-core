# Boundary Enforcement Model

Related ADR:
- [`./adr/ADR-004-structured-boundary-definitions.md`](./adr/ADR-004-structured-boundary-definitions.md)

Related requirements:
- [`./requirements.md`](./requirements.md)

## Purpose

This document records the planned warn/error enforcement model for
inventory-parity boundary checks in `sc-lint-boundary`.

The goal is:

- hard gate architectural drift
- preserve visibility for planned future work
- avoid using indefinite warning states as a suppression system

## Problem

The current boundary tooling validates and enforces many runtime and structural
rules, but it does not yet act as a completion guard for "documented boundary
item exists in code".

That gap leaves room for two bad outcomes:

- documented boundary items that never land in code
- ad hoc exceptions for future-sprint items that should remain visible and
  traceable

## Core Requirement

Boundary lint should compare the structured boundary inventory against the code
graph and classify missing documented items by planning status:

- missing and planned in a future sprint: `WARN`
- missing and unplanned: `ERROR`
- missing and overdue: `ERROR`

This model applies at item-key granularity, not just per boundary record. One
boundary may therefore contain:

- satisfied items
- warning-level planned gaps
- error-level unscheduled or overdue gaps

## Inventory-Parity Scope

Initial parity checks should verify that the following documented items resolve
in code:

- `implementation.module`
- `implementation.type`
- `public.trait`
- `composition.root`

Later extensions may include other required architecture module families once
they are represented in structured form.

## Required Planning Inputs

The warning model is only valid when the planning metadata is complete enough to
evaluate mechanically.

Each planned item must include:

- stable item key
- owning boundary id
- scheduled sprint
- tracking id
- escalation condition

If any of those are missing or malformed, the linter should treat the item as
an error, not a warning.

The item key must be derived mechanically from the canonical boundary record
path, not invented as freeform prose. Recommended shape:

- `<boundary_id>.<section>.<field>[.<subfield>]`

Examples:

- `BOUNDARY-MailStore.public.trait`
- `BOUNDARY-MailStore.implementation.type`
- `BOUNDARY-MailStore.composition.root`

## Finding Classes

### Error

Use an error when:

- the documented item is missing from code
- and there is no structured planning mapping for that item

This is immediate architectural drift and should fail the lint.

### Warning

Use a warning when:

- the documented item is missing from code
- and it is explicitly scheduled for a future sprint

This warning should remain visible in lint output and logs, not be silently
suppressed.

### Warning auto-escalation

Warnings must automatically escalate to errors when the planned delivery point
is current or past and the item still does not resolve.

This prevents:

- permanent warning debt
- stale plan references becoming de facto suppressions

## Structured Planning Mapping

Warning eligibility must come from structured data, not freeform prose.

Each planned-but-missing item should map to:

- item key
- scheduled sprint
- tracking id
- escalation condition

The mapping must be machine-readable so the linter can:

- decide warn vs error deterministically
- explain the reason for warning
- auto-escalate at the correct point

Recommended source:

- TOML-backed planning metadata checked into the repo

Not acceptable as the long-term source:

- prose-only notes
- inline comments in Markdown boundary docs
- freeform allowlists

## Recommended Data Shape

The enforcement model should assume TOML-backed boundary records and TOML-backed
planning metadata in:

- `boundaries/planning.toml`

Example direction:

```toml
[planning]
current_sprint = "S.14"

[planned_items."BOUNDARY-MailStore.implementation.type"]
scheduled_sprint = "S.14"
tracking_id = "SCB-CHANGE-1234"
expires_when = "sprint_current_or_past"
```

This shape is illustrative, not final. The `S.N` notation is only an example;
quarter-based labels such as `Q.3` or release-phase labels such as `R.7` are
equally valid if the repository defines a machine-parsable sprint ordering.

The important point is that:

- the item key is stable
- the scheduling metadata is structured
- the linter can evaluate it without parsing prose

## Sprint Evaluation Rule

The linter must have one deterministic source for "current sprint" when it
evaluates overdue warnings.

Recommended source:

- `boundaries/planning.toml`
- specifically `[planning].current_sprint`

The source must be explicit and testable. It must not rely on a human manually
interpreting the current sprint at review time.

If the current-sprint source is missing, malformed, or cannot be parsed, the
linter must classify planned-but-missing items as errors rather than warnings.

Sprint comparison must use parsed sprint ordering, not lexical string
comparison. For example, `S.10` must compare greater than `S.9`.

## Finding Content

Both warnings and errors should include:

- stable rule id
- exact missing item
- owning document / boundary record
- scheduled sprint, when present
- tracking id, when present

This makes the output useful to:

- QA
- planning reviewers
- future implementation sprints

Required rule families:

- `SCB-INVENTORY-001`
  - missing documented item with no valid planning mapping
- `SCB-INVENTORY-002`
  - missing documented item scheduled for a future sprint
- `SCB-INVENTORY-003`
  - missing documented item whose planned sprint is current or past

## Rule Behavior

Recommended behavior:

1. Parse the structured boundary inventory.
2. Build or load the code graph.
3. Resolve each required documented item into the graph.
4. If the item exists:
   - pass silently
5. If the item does not exist:
   - check structured planning metadata
   - classify as warning or error
6. If the item is planned but overdue:
   - promote warning to error

## Dual-Loader Behavior During TOML Migration

During the Markdown + TOML transition, the parity model should operate on the
shared internal boundary model, but the planning/enforcement feature itself
should still be TOML-first.

Default behavior should be:

- TOML planning metadata is authoritative
- `boundaries/planning.toml` is the default authoritative planning-metadata
  file
- duplicate boundary definitions across sources are errors unless explicitly in
  an equivalence-test migration mode
- duplicate item keys in the planning metadata are errors

The equivalence-test migration mode should be test-only and disabled in normal
developer lint runs and CI.

## Testing Requirements

At minimum, the implementation should ship with:

- positive test: documented item resolves in code
- warning test: documented item missing but scheduled in a future sprint
- overdue test: documented item missing and scheduled in current/past sprint
- error test: documented item missing with no planning mapping
- malformed-planning test:
  - missing sprint
  - missing tracking id
  - invalid item key
  - item key that points at no known boundary path
- malformed-current-sprint-source test
- sprint-ordering test:
  - `S.9` vs `S.10`
  - current sprint vs future sprint
- duplicate-planning-entry test
- mixed-boundary test where one record contains pass/warn/error items together
- dual-loader test where TOML and Markdown coexist without duplicate authority
- duplicate-source test where the same boundary id appears in both sources and
  fails
- equivalence-test-mode test showing duplicate-source migration checks are only
  legal in explicit migration fixtures, not normal lint mode

## Relationship To TOML Migration

This model should be implemented against TOML-backed boundary data.

The model is possible with Markdown compatibility during transition, but it
should not be designed around Markdown parsing as the long-term source.

New boundary-enforcement features in this area should be TOML-first.

## Recommendation

Implement the warn/error model as the next boundary-enforcement planning item,
with these constraints:

- structured mapping only
- no freeform warning exceptions
- overdue warnings must escalate automatically
- implementation should land on TOML-backed boundary data rather than extending
  Markdown-only parsing further
