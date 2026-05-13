# Sprint V.3 — Recovery Context Hardening

```yaml
plan_type: sprint_plan
phase: V
sprint: "V.3"
status: planned
worktree: TBD
branch: TBD
estimated_scope: M
```

## Goal

Make daemon-unavailable and adjacent runtime failure paths consistently carry
actionable recovery guidance.

## Scope

- define the daemon/client/runtime error classes that must carry explicit
  recovery guidance
- add a checklist or lint strategy for `.with_recovery()` coverage on the
  required paths
- prioritize daemon-unavailable, socket-connect, daemon-start, and local IPC
  runtime failures
- document when recovery text is mandatory versus optional

## Acceptance Criteria

- the required daemon-unavailable error paths are explicitly enumerated
- `.with_recovery()` coverage is checked through a documented checklist, lint,
  or both
- required recovery text is specific and actionable rather than generic
- the resulting rule set is documented for future daemon and client work

## Out Of Scope

- redesigning the full ATM error model
- rewriting unrelated error messages with no daemon/runtime relevance
- adding new user-facing features beyond better failure guidance
