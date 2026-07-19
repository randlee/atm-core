# Ruthless Boundary QA Design

`ruthless-boundary-qa` is a principle-first reviewer.

Its job is to:

- find active boundary violations
- find places where boundaries are wider than necessary
- find repeated leak patterns that need mechanical enforcement

It is not a historical incident reviewer and it is not a branch archaeology
agent.

## Review model

The reviewer should apply stable rules:

- one owner per decision
- one production path per behavior
- transport moves bytes only
- storage backends are replaceable
- state machines are minimal
- proof paths are not product paths

## Finding classes

- `boundary_violation`
  - active leak or forbidden dependency/logic crossing
- `boundary_tightening`
  - code works but the boundary is still wider than necessary
- `lint_gap`
  - repeated leak pattern has no machine guard
- `doc_gap`
  - rules, TOML policy, or ADR guidance are stale or incomplete

## Scope rule

Use stable architectural rules as the source of truth.

Do not anchor the review on:

- transient branch-specific file layouts
- historical line numbers
- incident narratives that will be obsolete after the next cleanup sprint
