# Sprint V.1 — Runtime Test Isolation Lint

```yaml
plan_type: sprint_plan
phase: V
sprint: "V.1"
status: planned
worktree: TBD
branch: TBD
estimated_scope: S
```

## Goal

Add a hard lint gate that forbids process-global mutable test seams in daemon
runtime and transport code.

## Scope

- build on the existing lint-framework direction from `arch-inj` on
  `feature/pQ-lint-tools`
- detect and reject runtime or transport test hooks that use process-global
  mutable state
- scope enforcement first to the daemon/runtime/transport line where the Phase U
  findings recurred
- document approved alternatives:
  instance-owned seams, constructor injection, and explicit test doubles

## Acceptance Criteria

- a lint or equivalent hard gate exists for global mutable runtime test seams
- the gate is documented with examples of forbidden and allowed patterns
- the daemon/runtime code path can be checked without relying on manual review
  only
- no new production behavior is introduced

## Out Of Scope

- broad test-style cleanup outside runtime/transport
- converting every historical test seam in one sprint if the lint can land
  first with a bounded remediation list
- non-daemon workspace lint work unrelated to runtime seam isolation
