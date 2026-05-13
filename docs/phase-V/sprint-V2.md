# Sprint V.2 — Workspace `#[path]` Lint

```yaml
plan_type: sprint_plan
phase: V
sprint: "V.2"
status: planned
worktree: TBD
branch: TBD
estimated_scope: S
```

## Goal

Add a hard workspace lint that forbids production cross-crate `#[path]`
imports.

## Scope

- build on the existing lint-framework direction from `arch-inj` on
  `feature/pQ-lint-tools`
- define the exact forbidden pattern:
  production code reaching into another crate through `#[path]` instead of a
  real crate boundary
- allow only tightly documented exceptions if they are test-only and
  intentionally scoped
- document the approved alternatives:
  real crate dependencies, extracted shared modules, or explicit boundary
  traits

## Acceptance Criteria

- a lint or equivalent hard gate exists for production cross-crate `#[path]`
  imports
- the rule distinguishes production code from approved test-only exceptions
- the enforcement rule is documented with concrete examples
- no new production behavior is introduced

## Out Of Scope

- general module layout cleanup unrelated to `#[path]`
- replacing every historical test-only `#[path]` use if the production gate
  lands first
- creating new helper crates unless a real shared boundary is justified
